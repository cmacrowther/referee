use std::io;
use std::path::PathBuf;

use crate::graph::LatencyMode;
use crate::runtime::{
    FrameTransport, Normalizer, PipelineStageId, ProcessSpec, StageRuntimeContext, StderrMode,
};
use crate::source::ffmpeg_header_block;

const LIVE_RECONNECT_DELAY_MAX_SECONDS: u32 = 2;
const LIVE_RW_TIMEOUT_MICROS: u64 = 15_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveInputProfile {
    /// Input demuxer thread queue depth. Smaller values reduce in-pipe buffering
    /// and therefore end-to-end latency; larger values improve tolerance for
    /// bursty network delivery.
    thread_queue_size: u32,
    analyze_duration_micros: u32,
    probesize_bytes: u32,
    ffmpeg_fflags: &'static str,
}

impl LiveInputProfile {
    fn for_latency_mode(latency_mode: LatencyMode) -> Self {
        match latency_mode {
            LatencyMode::UltraLow => Self {
                thread_queue_size: 128,
                analyze_duration_micros: 1_500_000,
                probesize_bytes: 2_000_000,
                // Keep latency low, but avoid overly aggressive timestamp/DTS
                // manipulation on fragile HLS/MPEG-TS sources.
                ffmpeg_fflags: "+genpts+nobuffer",
            },
            LatencyMode::Low => Self {
                thread_queue_size: 256,
                analyze_duration_micros: 2_000_000,
                probesize_bytes: 3_000_000,
                ffmpeg_fflags: "+genpts+nobuffer",
            },
            LatencyMode::Balanced => Self {
                thread_queue_size: 512,
                analyze_duration_micros: 4_000_000,
                probesize_bytes: 6_000_000,
                ffmpeg_fflags: "+genpts",
            },
        }
    }
}

/// Staged-path ingest: pulls the source and emits muxed NUT (rawvideo +
/// PCM audio) on stdout for the preprocess or executor stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegNormalizer {
    program: PathBuf,
    pixel_format: String,
    output_resolution: Option<(u32, u32)>,
    denoise: bool,
    preserve_stream_offsets: bool,
}

impl FfmpegNormalizer {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            pixel_format: "yuv420p".to_string(),
            output_resolution: None,
            denoise: false,
            preserve_stream_offsets: false,
        }
    }

    pub fn with_pixel_format(mut self, pixel_format: impl Into<String>) -> Self {
        self.pixel_format = pixel_format.into();
        self
    }

    /// Crops to the aspect ratio of this output resolution.
    pub fn with_output_resolution(mut self, resolution: Option<(u32, u32)>) -> Self {
        self.output_resolution = resolution;
        self
    }

    pub fn with_denoise(mut self, denoise: bool) -> Self {
        self.denoise = denoise;
        self
    }

    /// Keeps the source's offset between its audio and video, shifting both
    /// by the earliest stream's start instead of rebasing each stream to its
    /// own first timestamp. Audio is still padded to start at 0. The
    /// `rife-worker` stage relies on this to keep the streams in sync.
    pub fn with_stream_offsets_preserved(mut self, preserve: bool) -> Self {
        self.preserve_stream_offsets = preserve;
        self
    }

    pub fn build_command(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let stage = PipelineStageId::Normalizer;
        if context.transport.input != FrameTransport::SourcePull {
            return Err(context.transport.input.unsupported(stage, "input"));
        }
        let output_url = context.transport.output.ffmpeg_output_url(stage)?;

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
        ];

        if let Some(relay) = &context.source.relay {
            // HLS live source: use the relay URL with reconnect support, but do
            // not force file-style realtime pacing (-re) or low_delay decode flags.
            // Those can make fragile live HLS/MPEG-TS inputs more brittle.
            let live_profile =
                LiveInputProfile::for_latency_mode(context.execution_plan.latency_mode);

            args.extend([
                "-thread_queue_size".to_string(),
                live_profile.thread_queue_size.to_string(),
                "-rw_timeout".to_string(),
                LIVE_RW_TIMEOUT_MICROS.to_string(),
                "-analyzeduration".to_string(),
                live_profile.analyze_duration_micros.to_string(),
                "-probesize".to_string(),
                live_profile.probesize_bytes.to_string(),
                "-fflags".to_string(),
                live_profile.ffmpeg_fflags.to_string(),
                "-reconnect".to_string(),
                "1".to_string(),
                "-reconnect_streamed".to_string(),
                "1".to_string(),
                "-reconnect_on_network_error".to_string(),
                "1".to_string(),
                "-reconnect_delay_max".to_string(),
                LIVE_RECONNECT_DELAY_MAX_SECONDS.to_string(),
                "-i".to_string(),
                relay.relay_url.clone(),
            ]);
        } else {
            if !context.source.runtime_headers.is_empty() {
                args.push("-headers".to_string());
                args.push(ffmpeg_header_block(None, &context.source.runtime_headers));
            }
            args.extend(["-i".to_string(), context.source.runtime_url.clone()]);
        }

        // Rebasing each stream to its own start drops the source's A/V offset;
        // without it FFmpeg shifts both by the input's shared start time.
        let rebase_streams = !self.preserve_stream_offsets;
        let mut video_filters = Vec::new();
        if let Some((tw, th)) = self.output_resolution {
            video_filters.push(format!(
                "crop=if(gt(iw*{th}\\,ih*{tw})\\,trunc(ih*{tw}/{th}/2)*2\\,iw):if(gt(iw*{th}\\,ih*{tw})\\,ih\\,trunc(iw*{th}/{tw}/2)*2)"
            ));
        }
        if rebase_streams {
            video_filters.push("setpts=PTS-STARTPTS".to_string());
        }
        let mut video_filter = video_filters.join(",");
        if self.denoise {
            if !video_filter.is_empty() {
                video_filter.push(',');
            }
            video_filter.push_str("hqdn3d");
        }
        if video_filter.is_empty() {
            video_filter.push_str("null");
        }

        let aresample_async = match (
            context.source.relay.is_some(),
            context.execution_plan.latency_mode,
        ) {
            (true, LatencyMode::Balanced) => 1u32,
            _ => 0u32,
        };

        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-vf".to_string(),
            video_filter,
            "-fps_mode".to_string(),
            "passthrough".to_string(),
            "-c:v".to_string(),
            "rawvideo".to_string(),
            "-pix_fmt".to_string(),
            self.pixel_format.clone(),
            "-af".to_string(),
            format!(
                "{}aresample=async={}:first_pts=0",
                if rebase_streams {
                    "asetpts=PTS-STARTPTS,"
                } else {
                    ""
                },
                aresample_async
            ),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            "-flush_packets".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            output_url.to_string(),
        ]);

        Ok(ProcessSpec::new(
            stage,
            &self.program,
            args,
            context.transport.clone(),
            "ffmpeg-normalizer",
        )
        .with_stderr_mode(StderrMode::Filtered {
            suppress: &["Invalid timestamps stream="],
        }))
    }
}

impl Normalizer for FfmpegNormalizer {
    fn build_normalizer_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        self.build_command(context)
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegNormalizer;
    use crate::graph::{
        ExecutionPlan, ExecutorKind, LatencyMode, SharedNormalizationOperation, VideoOp,
    };
    use crate::runtime::{
        FrameTransport, Normalizer, PipelineStageReadinessPolicy, SessionOutputPaths,
        StageRuntimeContext, StderrMode, TransportConfig,
    };
    use crate::source::{
        RelayedSource, SourceClassification, SourceDescriptor, SourceKind, SourceTransport,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn context(latency_mode: LatencyMode, output: FrameTransport) -> StageRuntimeContext {
        let relay_url =
            "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8";

        StageRuntimeContext {
            session_id: "session-1".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::NvidiaSpecialized,
                latency_mode,
                requires_local_hls_relay: true,
                video_ops: vec![VideoOp::NormalizeInput],
            },
            source: SourceDescriptor {
                classification: SourceClassification {
                    transport: SourceTransport::RemoteHttp,
                    kind: SourceKind::Hls,
                },
                original_url: "https://example.com/live/master.m3u8".to_string(),
                runtime_url: relay_url.to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: Some(RelayedSource {
                    original_url: "https://example.com/live/master.m3u8".to_string(),
                    relay_url: relay_url.to_string(),
                    headers: HashMap::new(),
                    metadata: None,
                }),
                metadata: None,
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session\\index.m3u8"),
            },
            transport: TransportConfig {
                input: FrameTransport::SourcePull,
                output,
            },
        }
    }

    fn has_subsequence(args: &[String], expected: &[&str]) -> bool {
        args.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }

    #[test]
    fn preserving_stream_offsets_skips_per_stream_rebasing() {
        let spec = FfmpegNormalizer::new("ffmpeg")
            .with_stream_offsets_preserved(true)
            .build_normalizer_spec(&context(LatencyMode::Low, FrameTransport::StdoutPipe))
            .unwrap();

        assert!(has_subsequence(&spec.args, &["-vf", "null"]));
        assert!(has_subsequence(
            &spec.args,
            &["-af", "aresample=async=0:first_pts=0"]
        ));
        assert!(!spec.args.iter().any(|arg| arg.contains("STARTPTS")));

        let cropped = FfmpegNormalizer::new("ffmpeg")
            .with_stream_offsets_preserved(true)
            .with_output_resolution(Some((1920, 1080)))
            .with_denoise(true)
            .build_normalizer_spec(&context(LatencyMode::Low, FrameTransport::StdoutPipe))
            .unwrap();
        let video_filter = cropped
            .args
            .windows(2)
            .find(|pair| pair[0] == "-vf")
            .map(|pair| pair[1].clone())
            .unwrap();
        assert!(video_filter.starts_with("crop="));
        assert!(video_filter.ends_with(",hqdn3d"));
        assert!(!video_filter.contains("setpts"));
    }

    #[test]
    fn builds_low_latency_nut_pipe_command_for_relayed_live_input() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let context = context(LatencyMode::Low, FrameTransport::StdoutPipe);
        let normalization_plan = context
            .execution_plan
            .to_intermediate_plan()
            .normalization_plan();
        let command = normalizer.build_command(&context).unwrap();

        assert!(normalization_plan.requires_stage());
        assert_eq!(normalization_plan.operations.len(), 1);
        assert!(matches!(
            &normalization_plan.operations[0],
            SharedNormalizationOperation::NormalizeInput(_)
        ));
        assert_eq!(command.program, PathBuf::from("ffmpeg"));
        assert_eq!(command.transport.output, FrameTransport::StdoutPipe);
        assert!(has_subsequence(
            &command.args,
            &["-fflags", "+genpts+nobuffer"]
        ));
        assert!(!has_subsequence(&command.args, &["-flags", "low_delay"]));
        assert!(!command.args.iter().any(|a| a == "-re"));
        assert!(has_subsequence(
            &command.args,
            &[
                "-i",
                "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8"
            ]
        ));
        assert!(has_subsequence(
            &command.args,
            &["-vf", "setpts=PTS-STARTPTS"]
        ));
        assert!(has_subsequence(&command.args, &["-c:v", "rawvideo"]));
        assert!(has_subsequence(&command.args, &["-pix_fmt", "yuv420p"]));
        assert!(has_subsequence(
            &command.args,
            &["-af", "asetpts=PTS-STARTPTS,aresample=async=0:first_pts=0"]
        ));
        assert!(has_subsequence(&command.args, &["-c:a", "pcm_s16le"]));
        assert!(has_subsequence(&command.args, &["-f", "nut", "pipe:1"]));
    }

    #[test]
    fn balanced_latency_relaxes_low_delay_flags() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let spec = normalizer
            .build_normalizer_spec(&context(LatencyMode::Balanced, FrameTransport::StdoutPipe))
            .unwrap();

        assert!(!has_subsequence(
            &spec.args,
            &["-fflags", "+genpts+nobuffer"]
        ));
        assert!(!has_subsequence(&spec.args, &["-flags", "low_delay"]));
        assert!(!spec.args.iter().any(|a| a == "-re"));
        assert!(has_subsequence(&spec.args, &["-fflags", "+genpts"]));
        assert!(has_subsequence(
            &spec.args,
            &["-af", "asetpts=PTS-STARTPTS,aresample=async=1:first_pts=0"]
        ));
        assert_eq!(
            spec.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnSpawn
        );
        assert!(matches!(spec.stderr_mode, StderrMode::Filtered { .. }));
    }

    #[test]
    fn rejects_transports_other_than_source_pull_to_pipe() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let mut hls_output = context(
            LatencyMode::Low,
            FrameTransport::HlsOutput {
                playlist_path: PathBuf::from("session\\index.m3u8"),
                segment_dir: PathBuf::from("session"),
            },
        );
        assert!(normalizer.build_command(&hls_output).is_err());

        hls_output.transport.output = FrameTransport::StdoutPipe;
        hls_output.transport.input = FrameTransport::StdoutPipe;
        assert!(normalizer.build_command(&hls_output).is_err());
    }

    #[test]
    fn appends_crop_and_denoise_to_the_video_filter() {
        let normalizer = FfmpegNormalizer::new("ffmpeg")
            .with_pixel_format("nv12")
            .with_output_resolution(Some((1920, 1080)))
            .with_denoise(true);
        let command = normalizer
            .build_command(&context(LatencyMode::Low, FrameTransport::StdoutPipe))
            .unwrap();

        let filter_index = command.args.iter().position(|a| a == "-vf").unwrap();
        let filter = &command.args[filter_index + 1];
        assert!(filter.starts_with("crop="));
        assert!(filter.ends_with(",setpts=PTS-STARTPTS,hqdn3d"));
        assert!(has_subsequence(&command.args, &["-pix_fmt", "nv12"]));
    }

    fn direct_source_context(url: &str, headers: HashMap<String, String>) -> StageRuntimeContext {
        use crate::source::SourceMetadata;

        StageRuntimeContext {
            session_id: "session-2".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::Universal,
                latency_mode: LatencyMode::Balanced,
                requires_local_hls_relay: false,
                video_ops: vec![VideoOp::NormalizeInput],
            },
            source: SourceDescriptor {
                classification: SourceClassification {
                    transport: SourceTransport::LocalPath,
                    kind: SourceKind::Other,
                },
                original_url: url.to_string(),
                runtime_url: url.to_string(),
                runtime_headers: headers,
                session_headers: HashMap::new(),
                relay: None,
                metadata: Some(SourceMetadata {
                    source_fps: Some(30.0),
                    ..SourceMetadata::default()
                }),
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session/index.m3u8"),
            },
            transport: TransportConfig {
                input: FrameTransport::SourcePull,
                output: FrameTransport::StdoutPipe,
            },
        }
    }

    #[test]
    fn builds_nut_pipe_command_for_direct_non_relay_source() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let ctx = direct_source_context("/media/video.mkv", HashMap::new());
        let command = normalizer.build_command(&ctx).unwrap();

        assert!(has_subsequence(&command.args, &["-i", "/media/video.mkv"]));
        assert!(!command.args.iter().any(|a| a == "-re"));
        assert!(!command.args.iter().any(|a| a == "-reconnect"));
        assert!(!command.args.iter().any(|a| a == "-rw_timeout"));
        assert!(!command.args.iter().any(|a| a == "-fflags"));
        assert!(has_subsequence(&command.args, &["-c:v", "rawvideo"]));
        assert!(has_subsequence(&command.args, &["-pix_fmt", "yuv420p"]));
        assert!(has_subsequence(&command.args, &["-c:a", "pcm_s16le"]));
        assert!(has_subsequence(&command.args, &["-f", "nut", "pipe:1"]));
    }

    #[test]
    fn direct_source_includes_runtime_headers_when_present() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let headers = HashMap::from([("Authorization".to_string(), "Bearer tok".to_string())]);
        let ctx = direct_source_context("https://example.com/video.mp4", headers);
        let command = normalizer.build_command(&ctx).unwrap();

        let header_idx = command.args.iter().position(|a| a == "-headers").unwrap();
        let input_idx = command.args.iter().position(|a| a == "-i").unwrap();
        assert!(header_idx < input_idx);
        let header_value = &command.args[header_idx + 1];
        assert!(header_value.contains("Authorization: Bearer tok"));
    }

    #[test]
    fn direct_source_omits_headers_arg_when_runtime_headers_are_empty() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let ctx = direct_source_context("https://example.com/video.mp4", HashMap::new());
        let command = normalizer.build_command(&ctx).unwrap();

        assert!(!command.args.iter().any(|a| a == "-headers"));
    }
}
