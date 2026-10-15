//! Executor implementations and shared encoder plumbing.
//!
//! The intended executor surface is deliberately small:
//! - `NvidiaSpecializedExecutor` is the premium Windows + NVIDIA path backed by
//!   NVEncC/Rigaya for backend-native NGX-VSR, FRUC, and TrueHDR behavior.
//! - `AmdExecutor` is its AMD counterpart backed by VCEEncC/Rigaya: AMF FSR
//!   resize, AMF FRC frame doubling, and libplacebo HDR. Retries fall back to
//!   the Universal path.
//! - `UniversalExecutor` is the cross-platform FFmpeg/libplacebo path. On Linux,
//!   NVIDIA routes through FFmpeg NVENC and AMD routes through FFmpeg VAAPI.
//! - `FfmpegHlsPackager` remains separate from encode/processing executors so
//!   staged pipelines can package a transport stream independently.
//!
//! The helpers in this module hold argument conventions shared by more than
//! one executor: source ingest and its buffering, aspect crop, HDR signaling,
//! live pacing, Rigaya output muxing, FFmpeg HLS muxing, and segment-aligned
//! GOP length.

mod amd;
mod family;
mod ffmpeg_packager;
pub(crate) mod libplacebo_filters;
mod nvidia;
mod universal;

use std::collections::HashMap;
use std::io;

use crate::graph::{
    parse_resolution, ColorPrimariesIntent, HdrPlan, MatrixCoefficientsIntent, OutputBitDepth,
    TransferCharacteristicIntent,
};
use crate::runtime::{
    FrameTransport, PipelineStageId, StageRuntimeContext, StartupMode, HLS_PLAYLIST_SIZE,
};
use crate::source::{ffmpeg_header_block, BROWSER_USER_AGENT};

// Standalone packager stage used by staged pipelines after executor output.
pub use ffmpeg_packager::FfmpegHlsPackager;

// Specialized AMD executor surface.
pub use amd::{detect_vceenc_amf_fsr11, AmdExecutor, AmdExecutorContext};

// Capability parsing is only part of startup/planning; do not treat it as an
// executor surface.
pub(crate) use nvidia::parse_nvenc_capabilities;

// Runtime executor selection keeps graph-level executor kinds intact while
// resolving the vendor-specific implementation.
pub(crate) use family::{select_runtime_executor_target, RuntimeExecutorTarget};

// Specialized Windows + NVIDIA executor surface.
pub use nvidia::{NvidiaSpecializedExecutor, NvidiaSpecializedExecutorContext};

// Universal backend selection is internal runtime wiring for FFmpeg encoders.
pub(crate) use universal::{select_universal_encode_backend, RuntimePlatform};

// Cross-platform FFmpeg/libplacebo executor surface.
pub use universal::{UniversalExecutor, UniversalExecutorContext};

const RETRY_INPUT_ANALYZE_SECONDS: u32 = 4;
/// Output frame rate assumed for GOP length when neither interpolation nor the
/// source metadata gives one.
pub(crate) const UNKNOWN_SOURCE_FPS_FALLBACK: f64 = 30.0;
/// Headroom applied to the `--max-procfps` pacing cap. Rigaya's speed limiter
/// (`CProcSpeedControl`) resets its reference clock whenever it falls behind,
/// so time lost to an input or GPU stall is never made up at a cap of exactly
/// 1.0x. Without headroom that lag accumulates until the live HLS demuxer falls
/// off the source playlist window and skips segments.
const PROCESSING_FPS_CAP_HEADROOM: f64 = 1.05;
const RETRY_INPUT_PROBESIZE_BYTES: u32 = 1_000_000;
const HQDN3D_EQUIVALENT_CONVOLUTION3D: &str = "ythresh=4,cthresh=3,t_ythresh=6,t_cthresh=4.5";

/// Where an executor reads its frames from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderInput {
    /// Single-process launch: the executor pulls the source itself.
    SourceUrl {
        url: String,
        extra_headers: HashMap<String, String>,
    },
    /// Staged launch: muxed NUT from the previous stage on stdin.
    StdinPipe,
}

impl EncoderInput {
    pub(crate) fn is_source_url(&self) -> bool {
        matches!(self, Self::SourceUrl { .. })
    }
}

pub(crate) fn resolve_encoder_input(context: &StageRuntimeContext) -> io::Result<EncoderInput> {
    match &context.transport.input {
        FrameTransport::SourcePull => Ok(EncoderInput::SourceUrl {
            url: context.source.runtime_url.clone(),
            extra_headers: context.source.runtime_headers.clone(),
        }),
        FrameTransport::StdoutPipe => Ok(EncoderInput::StdinPipe),
        other => Err(other.unsupported(PipelineStageId::Executor, "input")),
    }
}

/// Input args for the Rigaya CLIs (NVEncC, VCEEncC). FFmpeg executors build
/// their own because the option grammar differs.
pub(crate) fn build_rigaya_input_args(input: &EncoderInput, low_latency: bool) -> Vec<String> {
    match input {
        EncoderInput::SourceUrl { url, extra_headers } => {
            let mut input_args = vec![
                "-i".to_string(),
                url.clone(),
                "--input-option".to_string(),
                format!(
                    "headers:{}",
                    ffmpeg_header_block(Some(BROWSER_USER_AGENT), extra_headers)
                ),
                "--input-option".to_string(),
                "reconnect:1".to_string(),
                "--input-option".to_string(),
                "reconnect_on_network_error:1".to_string(),
                "--input-option".to_string(),
                "reconnect_delay_max:5".to_string(),
                "--input-option".to_string(),
                "http_persistent:0".to_string(),
                "--input-option".to_string(),
                "http_multiple:0".to_string(),
                "--input-option".to_string(),
                "extension_picky:0".to_string(),
            ];

            if low_latency {
                input_args.push("--input-option".to_string());
                input_args.push("fflags:nobuffer".to_string());
            } else {
                input_args.push("--input-analyze".to_string());
                input_args.push(RETRY_INPUT_ANALYZE_SECONDS.to_string());
                input_args.push("--input-probesize".to_string());
                input_args.push(RETRY_INPUT_PROBESIZE_BYTES.to_string());
            }

            input_args
        }
        // Staged FFmpeg handoff uses a muxed NUT stream carrying both rawvideo
        // and audio; force Rigaya onto the avformat/libav reader so the pipe
        // is probed as a containerized stream instead of a video-only raw pipe.
        EncoderInput::StdinPipe => vec!["--avsw".to_string(), "-i".to_string(), "-".to_string()],
    }
}

pub(crate) fn append_rigaya_denoise_args(args: &mut Vec<String>) {
    // Approximate FFmpeg hqdn3d's default spatial/temporal strengths with the
    // GPU-side Rigaya convolution3d filter on native source-pull paths.
    args.push("--vpp-convolution3d".to_string());
    args.push(HQDN3D_EQUIVALENT_CONVOLUTION3D.to_string());
}

/// Inline HLS muxing options for a Rigaya encoder writing the session
/// playlist itself (single-process launches).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RigayaInlineHls {
    pub hls_time: u32,
    /// Pass `--strict-gop` so segment cuts always land on a keyframe.
    pub strict_gop: bool,
    /// Keep evicted segments on disk for another playlist window so slower
    /// players such as VLC that drift behind the live edge can still fetch them.
    pub retain_evicted_segments: bool,
}

/// Output args shared by NVEncC and VCEEncC: AAC audio plus either inline HLS
/// (single-process) or MPEG-TS on stdout for the staged packager.
pub(crate) fn build_rigaya_output_args(
    output: &FrameTransport,
    inline_hls: RigayaInlineHls,
) -> io::Result<Vec<String>> {
    let mut args = vec!["--audio-codec".to_string(), "aac".to_string()];
    match output {
        FrameTransport::HlsOutput { playlist_path, .. } => {
            args.extend(["--format".to_string(), "hls".to_string()]);
            if inline_hls.strict_gop {
                args.push("--strict-gop".to_string());
            }
            args.extend([
                "--mux-option".to_string(),
                format!("hls_time:{}", inline_hls.hls_time),
                "--mux-option".to_string(),
                format!("hls_list_size:{}", HLS_PLAYLIST_SIZE),
            ]);
            if inline_hls.retain_evicted_segments {
                args.extend([
                    "--mux-option".to_string(),
                    format!("hls_delete_threshold:{}", HLS_PLAYLIST_SIZE),
                ]);
            }
            args.extend([
                "--mux-option".to_string(),
                "hls_flags:delete_segments".to_string(),
                "-o".to_string(),
                playlist_path.to_string_lossy().to_string(),
            ]);
        }
        FrameTransport::StdoutPipe => args.extend([
            "--format".to_string(),
            "mpegts".to_string(),
            "-o".to_string(),
            "-".to_string(),
        ]),
        other => return Err(other.unsupported(PipelineStageId::Executor, "output")),
    }
    Ok(args)
}

/// FFmpeg HLS muxer args shared by the Universal executor (inline HLS) and
/// the staged packager. `segment_filename` may be relative to the process
/// working directory.
pub(crate) fn build_ffmpeg_hls_output_args(
    hls_time: u32,
    segment_filename: &str,
    playlist_path: &str,
) -> Vec<String> {
    vec![
        "-f".to_string(),
        "hls".to_string(),
        "-hls_time".to_string(),
        hls_time.to_string(),
        "-hls_list_size".to_string(),
        HLS_PLAYLIST_SIZE.to_string(),
        "-hls_segment_type".to_string(),
        "mpegts".to_string(),
        "-hls_flags".to_string(),
        "delete_segments".to_string(),
        "-hls_segment_filename".to_string(),
        segment_filename.to_string(),
        playlist_path.to_string(),
    ]
}

/// Direct HLS source pulls always use buffered ingest in the Rigaya encoders.
/// Their no-buffer mode keeps the stream close to the live edge but discards
/// late packets, which shows up as visible timeline jumps under network jitter.
pub(crate) fn rigaya_startup_mode(
    context: &StageRuntimeContext,
    attempt_number: u32,
) -> StartupMode {
    if rigaya_prefers_buffered_ingest(context) {
        StartupMode::Buffered
    } else {
        StartupMode::for_attempt(attempt_number)
    }
}

pub(crate) fn rigaya_prefers_buffered_ingest(context: &StageRuntimeContext) -> bool {
    matches!(context.transport.input, FrameTransport::SourcePull)
        && context.source.classification.is_hls_like()
}

/// `--max-procfps` for a Rigaya encoder writing live HLS. The cap counts input
/// frames, before frame generation adds any, so it paces at the source rate
/// with headroom. `None` when the source rate is unknown: capping an unknown
/// 60 fps source at a guessed 30 would run the encode at half real time.
pub(crate) fn rigaya_processing_fps_cap(context: &StageRuntimeContext) -> Option<u32> {
    if !matches!(context.transport.output, FrameTransport::HlsOutput { .. }) {
        return None;
    }
    context
        .source_fps()
        .filter(|fps| *fps > 0.0)
        .map(|fps| (fps * PROCESSING_FPS_CAP_HEADROOM).ceil().max(1.0) as u32)
}

/// `--crop` arguments that center-crop a directly pulled source to the output
/// aspect ratio, so the resize does not stretch it. Empty when the source size
/// is unknown or already matches.
pub(crate) fn rigaya_aspect_crop_args(
    context: &StageRuntimeContext,
    output_resolution: Option<&str>,
) -> Vec<String> {
    match output_resolution.and_then(|resolution| derive_aspect_crop(context, resolution)) {
        Some((left, top, right, bottom)) => {
            vec![
                "--crop".to_string(),
                format!("{left},{top},{right},{bottom}"),
            ]
        }
        None => Vec::new(),
    }
}

fn derive_aspect_crop(
    context: &StageRuntimeContext,
    output_resolution: &str,
) -> Option<(u32, u32, u32, u32)> {
    let (target_w, target_h) = parse_resolution(output_resolution)?;
    let (source_w, source_h) = context.source.metadata.as_ref()?.dimensions()?;

    let source_ratio = source_w as u64 * target_h as u64;
    let target_ratio = source_h as u64 * target_w as u64;

    if source_ratio == target_ratio {
        return None;
    }

    if source_ratio > target_ratio {
        let crop_w = ((source_h as u64 * target_w as u64 / target_h as u64) as u32 / 2) * 2;
        if crop_w == 0 || crop_w >= source_w {
            return None;
        }
        let total_crop = source_w - crop_w;
        let left = total_crop / 2;
        let right = total_crop - left;
        Some((left, 0, right, 0))
    } else {
        let crop_h = ((source_w as u64 * target_h as u64 / target_w as u64) as u32 / 2) * 2;
        if crop_h == 0 || crop_h >= source_h {
            return None;
        }
        let total_crop = source_h - crop_h;
        let top = total_crop / 2;
        let bottom = total_crop - top;
        Some((0, top, 0, bottom))
    }
}

/// Adds the HDR color signaling the Rigaya encoders share (`--colorprim`,
/// `--transfer`, `--colormatrix`, `--colorrange`) to `args`, skipping pairs
/// already present. Returns whether the output is 10-bit.
pub(crate) fn push_rigaya_hdr_color_args(args: &mut Vec<String>, hdr: &HdrPlan) -> bool {
    let mut push_pair = |key: &str, value: &str| {
        if !args
            .windows(2)
            .any(|window| window[0] == key && window[1] == value)
        {
            args.push(key.to_string());
            args.push(value.to_string());
        }
    };
    if matches!(hdr.color_primaries, ColorPrimariesIntent::Bt2020) {
        push_pair("--colorprim", "bt2020");
    }
    if matches!(hdr.transfer, TransferCharacteristicIntent::Smpte2084) {
        push_pair("--transfer", "smpte2084");
    }
    if matches!(hdr.matrix, MatrixCoefficientsIntent::Bt2020Nc) {
        push_pair("--colormatrix", "bt2020nc");
    }
    if hdr.metadata.is_some() {
        push_pair("--colorrange", "limited");
    }
    hdr.output_bit_depth == OutputBitDepth::Bit10
}

/// Keyframe interval that lines GOPs up with HLS segment boundaries, so every
/// segment can be cut on a keyframe.
pub(crate) fn segment_aligned_gop(output_fps: f64, hls_time: u32) -> u32 {
    (output_fps * hls_time as f64).round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{
        build_ffmpeg_hls_output_args, build_rigaya_input_args, build_rigaya_output_args,
        segment_aligned_gop, EncoderInput, RigayaInlineHls,
    };
    use crate::runtime::FrameTransport;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

    fn source_input() -> EncoderInput {
        EncoderInput::SourceUrl {
            url: "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
            extra_headers: HashMap::new(),
        }
    }

    #[test]
    fn source_url_input_keeps_reconnect_and_low_latency_flags() {
        let args = build_rigaya_input_args(&source_input(), true);

        assert!(has_subsequence(
            &args,
            &[
                "-i",
                "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8",
            ]
        ));
        assert!(has_subsequence(&args, &["--input-option", "reconnect:1"]));
        assert!(has_subsequence(
            &args,
            &["--input-option", "fflags:nobuffer"]
        ));
    }

    #[test]
    fn source_url_input_adds_probe_flags_when_not_low_latency() {
        let args = build_rigaya_input_args(&source_input(), false);

        assert!(!has_subsequence(
            &args,
            &["--input-option", "fflags:nobuffer"]
        ));
        assert!(has_subsequence(&args, &["--input-analyze", "4"]));
        assert!(has_subsequence(&args, &["--input-probesize", "1000000"]));
    }

    #[test]
    fn stdin_pipe_input_uses_avsw_reader_for_muxed_streams() {
        let args = build_rigaya_input_args(&EncoderInput::StdinPipe, false);

        assert!(has_subsequence(&args, &["--avsw", "-i", "-"]));
        assert!(!args.iter().any(|arg| arg == "--input-option"));
    }

    #[test]
    fn rigaya_inline_hls_output_honors_strict_gop_and_retention_options() {
        let output = FrameTransport::HlsOutput {
            playlist_path: PathBuf::from("session/index.m3u8"),
            segment_dir: PathBuf::from("session"),
        };
        let nvidia = build_rigaya_output_args(
            &output,
            RigayaInlineHls {
                hls_time: 1,
                strict_gop: true,
                retain_evicted_segments: true,
            },
        )
        .unwrap();
        assert!(has_subsequence(
            &nvidia,
            &[
                "--audio-codec",
                "aac",
                "--format",
                "hls",
                "--strict-gop",
                "--mux-option",
                "hls_time:1",
                "--mux-option",
                "hls_list_size:8",
                "--mux-option",
                "hls_delete_threshold:8",
                "--mux-option",
                "hls_flags:delete_segments",
            ]
        ));

        let amd = build_rigaya_output_args(
            &output,
            RigayaInlineHls {
                hls_time: 2,
                strict_gop: false,
                retain_evicted_segments: false,
            },
        )
        .unwrap();
        assert!(!amd.iter().any(|arg| arg == "--strict-gop"));
        assert!(!amd
            .iter()
            .any(|arg| arg.starts_with("hls_delete_threshold")));
        assert!(has_subsequence(&amd, &["--mux-option", "hls_time:2"]));
    }

    #[test]
    fn rigaya_pipe_output_emits_mpegts_on_stdout() {
        let args = build_rigaya_output_args(
            &FrameTransport::StdoutPipe,
            RigayaInlineHls {
                hls_time: 1,
                strict_gop: true,
                retain_evicted_segments: true,
            },
        )
        .unwrap();
        assert_eq!(
            args,
            ["--audio-codec", "aac", "--format", "mpegts", "-o", "-"]
        );
        assert!(build_rigaya_output_args(
            &FrameTransport::SourcePull,
            RigayaInlineHls {
                hls_time: 1,
                strict_gop: false,
                retain_evicted_segments: false,
            },
        )
        .is_err());
    }

    #[test]
    fn ffmpeg_hls_output_uses_shared_window_and_segment_flags() {
        let args = build_ffmpeg_hls_output_args(1, "segment_%06d.ts", "index.m3u8");
        assert!(has_subsequence(&args, &["-f", "hls", "-hls_time", "1"]));
        assert!(has_subsequence(&args, &["-hls_list_size", "8"]));
        assert!(has_subsequence(&args, &["-hls_flags", "delete_segments"]));
        assert_eq!(args.last().map(String::as_str), Some("index.m3u8"));
    }

    #[test]
    fn gop_length_tracks_output_fps_per_segment() {
        assert_eq!(segment_aligned_gop(59.94, 1), 60);
        assert_eq!(segment_aligned_gop(24.0, 2), 48);
        assert_eq!(segment_aligned_gop(0.1, 1), 1);
    }
}
