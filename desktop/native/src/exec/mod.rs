//! Executor implementations and shared input plumbing.
//!
//! The intended executor surface is deliberately small:
//! - `NvidiaSpecializedExecutor` is the premium Windows + NVIDIA path backed by
//!   NVEncC/Rigaya for backend-native NGX-VSR, FRUC, and TrueHDR behavior.
//! - `AmdExecutor` is the AMD-native seam. It currently carries honest
//!   VCEEncC/native-upscale capability state while delegating safe rendering to
//!   the Universal path until a standalone VCEEncC renderer is extracted.
//! - `UniversalExecutor` is the cross-platform FFmpeg/libplacebo path. On Linux,
//!   NVIDIA routes through FFmpeg NVENC and AMD routes through FFmpeg VAAPI.
//! - `FfmpegHlsPackager` remains separate from encode/processing executors so
//!   staged pipelines can package a transport stream independently.
//!
//! Transitional compatibility code may still call the shared input resolver and
//! Rigaya-compatible input-argument helper, but backend-specific processing and
//! encode policy should stay in the dedicated executor modules.

mod amd;
mod family;
mod ffmpeg_packager;
pub(crate) mod libplacebo_filters;
mod nvidia;
mod universal;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crate::runtime::{FrameTransport, StageRuntimeContext, StdinMode};

// Standalone packager stage used by staged pipelines after executor output.
pub use ffmpeg_packager::{FfmpegHlsPackager, FfmpegHlsPackagerContext};

// AMD executor surface and raw native capability modeling.
#[allow(unused_imports)]
pub use amd::{AmdExecutor, AmdExecutorCapabilities, AmdExecutorContext, AmdNativeUpscaleSupport};

// Capability parsing is only part of startup/planning; do not treat it as an
// executor surface.
pub(crate) use nvidia::parse_nvenc_capabilities;

// Runtime family selection keeps graph-level executor kinds intact while
// allowing the runtime to grow NVIDIA / AMD / Universal executor surfaces.
pub(crate) use family::{
    select_runtime_executor_target, ExecutorFamily, RuntimeExecutorFamilyContext,
    RuntimeExecutorTarget,
};

// Specialized Windows + NVIDIA executor surface.
pub use nvidia::{NvidiaSpecializedExecutor, NvidiaSpecializedExecutorContext};

// Universal backend selection is internal runtime wiring for FFmpeg encoders.
pub(crate) use universal::{
    select_universal_encode_backend, RuntimePlatform, UniversalBackendSelectionContext,
};

// Cross-platform FFmpeg/libplacebo executor surface.
pub use universal::{UniversalExecutor, UniversalExecutorContext};

pub(crate) const REQUEST_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
const RETRY_INPUT_ANALYZE_SECONDS: u32 = 4;
const RETRY_INPUT_PROBESIZE_BYTES: u32 = 1_000_000;
const HQDN3D_EQUIVALENT_CONVOLUTION3D: &str = "ythresh=4,cthresh=3,t_ythresh=6,t_cthresh=4.5";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncoderInput {
    SourceUrl {
        url: String,
        extra_headers: HashMap<String, String>,
    },
    StdinPipe,
    NamedPipe(PathBuf),
}

pub(crate) fn resolve_encoder_input(
    context: &StageRuntimeContext,
) -> io::Result<(EncoderInput, StdinMode)> {
    match &context.transport.input {
        FrameTransport::SourcePull => Ok((
            EncoderInput::SourceUrl {
                url: context.source.runtime_url.clone(),
                extra_headers: context.source.runtime_headers.clone(),
            },
            StdinMode::Null,
        )),
        FrameTransport::StdoutPipe => Ok((
            EncoderInput::StdinPipe,
            StdinMode::Transport(FrameTransport::StdoutPipe),
        )),
        FrameTransport::NamedPipe(path) => {
            Ok((EncoderInput::NamedPipe(path.clone()), StdinMode::Null))
        }
        FrameTransport::LocalSocket(path) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "local socket executor input at {:?} is declared but not wired yet",
                path
            ),
        )),
        transport => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "executor cannot consume {:?} as its input transport",
                transport
            ),
        )),
    }
}

pub(crate) fn build_rigaya_input_args(input: &EncoderInput, low_latency: bool) -> Vec<String> {
    // Shared ownership stops at source ingest and retry/probe policy for
    // Rigaya-compatible CLIs. FFmpeg executors intentionally build their own
    // input args because the option grammar is different.
    match input {
        EncoderInput::SourceUrl { url, extra_headers } => {
            let headers = build_input_headers(extra_headers);
            let mut input_args = vec![
                "-i".to_string(),
                url.clone(),
                "--input-option".to_string(),
                format!("headers:{}", headers),
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
            }

            if !low_latency {
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
        EncoderInput::NamedPipe(path) => vec![
            "--avsw".to_string(),
            "-i".to_string(),
            path.to_string_lossy().to_string(),
        ],
    }
}

pub(crate) fn append_rigaya_denoise_args(args: &mut Vec<String>) {
    // Approximate FFmpeg hqdn3d's default spatial/temporal strengths with the
    // GPU-side Rigaya convolution3d filter on native source-pull paths.
    args.push("--vpp-convolution3d".to_string());
    args.push(HQDN3D_EQUIVALENT_CONVOLUTION3D.to_string());
}

fn build_input_headers(extra_headers: &HashMap<String, String>) -> String {
    let mut header_str = format!("User-Agent: {}\r\n", REQUEST_USER_AGENT);
    for (key, value) in extra_headers {
        header_str.push_str(&format!("{}: {}\r\n", key, value));
    }
    header_str
}

#[cfg(test)]
mod tests {
    use super::{build_rigaya_input_args, EncoderInput};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn source_url_input_keeps_reconnect_and_low_latency_flags() {
        let args = build_rigaya_input_args(
            &EncoderInput::SourceUrl {
                url: "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
                extra_headers: HashMap::new(),
            },
            true,
        );

        assert!(args.windows(2).any(|window| {
            window
                == [
                    "-i",
                    "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8",
                ]
        }));
        assert!(args
            .windows(2)
            .any(|window| window == ["--input-option", "reconnect:1"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--input-option", "fflags:nobuffer"]));
    }

    #[test]
    fn source_url_input_adds_probe_flags_when_not_low_latency() {
        let args = build_rigaya_input_args(
            &EncoderInput::SourceUrl {
                url: "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
                extra_headers: HashMap::new(),
            },
            false,
        );

        assert!(!args
            .windows(2)
            .any(|window| window == ["--input-option", "fflags:nobuffer"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--input-analyze", "4"]));
        assert!(args
            .windows(2)
            .any(|window| window == ["--input-probesize", "1000000"]));
    }

    #[test]
    fn named_pipe_input_stays_explicit_without_http_flags() {
        let args = build_rigaya_input_args(
            &EncoderInput::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1")),
            false,
        );

        assert!(args
            .windows(3)
            .any(|window| window == ["--avsw", "-i", r"\\.\pipe\referee-normalized-session-1"]));
        assert!(!args.iter().any(|arg| arg == "--input-option"));
        assert!(!args.iter().any(|arg| arg == "--input-analyze"));
    }

    #[test]
    fn stdin_pipe_input_uses_avsw_reader_for_muxed_streams() {
        let args = build_rigaya_input_args(&EncoderInput::StdinPipe, false);

        assert!(args
            .windows(3)
            .any(|window| window == ["--avsw", "-i", "-"]));
        assert!(!args.iter().any(|arg| arg == "--input-option"));
    }
}
