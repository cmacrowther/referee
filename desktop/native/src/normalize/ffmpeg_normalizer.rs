use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use crate::graph::{LatencyMode, SharedNormalizationPlan};
use crate::runtime::{
    FrameTransport, Normalizer, PipelineStageId, PipelineStageReadinessPolicy, ProcessSpec,
    StageRuntimeContext, StderrMode, StdinMode, StdoutMode,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegNormalizerCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub output_transport: FrameTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegNormalizer {
    program: PathBuf,
    output_transport: FrameTransport,
    pixel_format: Option<String>,
    output_resolution: Option<(u32, u32)>,
    denoise: bool,
}

impl FfmpegNormalizer {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            output_transport: FrameTransport::StdoutPipe,
            pixel_format: Some("yuv420p".to_string()),
            output_resolution: None,
            denoise: false,
        }
    }

    pub fn with_output_transport(mut self, output_transport: FrameTransport) -> Self {
        self.output_transport = output_transport;
        self
    }

    pub fn with_pixel_format(mut self, pixel_format: Option<impl Into<String>>) -> Self {
        self.pixel_format = pixel_format.map(Into::into);
        self
    }

    pub fn with_output_resolution(mut self, resolution: Option<(u32, u32)>) -> Self {
        self.output_resolution = resolution;
        self
    }

    pub fn with_denoise(mut self, denoise: bool) -> Self {
        self.denoise = denoise;
        self
    }

    #[allow(dead_code)]
    pub fn output_transport(&self) -> FrameTransport {
        self.output_transport.clone()
    }

    pub fn build_command(
        &self,
        context: &StageRuntimeContext,
    ) -> io::Result<FfmpegNormalizerCommand> {
        self.validate_context(context)?;
        let normalization_plan = self.planned_normalization(context);
        let shared_preprocess_plan = context
            .execution_plan
            .to_intermediate_plan()
            .shared_preprocess_plan();

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
                args.push(build_source_headers(&context.source.runtime_headers));
            }
            args.extend(["-i".to_string(), context.source.runtime_url.clone()]);
        }

        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-vf".to_string(),
            {
                let base = match self.output_resolution {
                    Some((tw, th)) => format!(
                        "crop=if(gt(iw*{th}\\,ih*{tw})\\,trunc(ih*{tw}/{th}/2)*2\\,iw):if(gt(iw*{th}\\,ih*{tw})\\,ih\\,trunc(iw*{th}/{tw}/2)*2),setpts=PTS-STARTPTS"
                    ),
                    None => "setpts=PTS-STARTPTS".to_string(),
                };
                if self.denoise {
                    format!("{},hqdn3d", base)
                } else {
                    base
                }
            },
            "-fps_mode".to_string(),
            "passthrough".to_string(),
            "-c:v".to_string(),
            "rawvideo".to_string(),
        ]);

        if normalization_plan.requires_stage() {
            // Keep the first stage limited to shared ingress normalization for now.
        }

        if let Some(pixel_format) = &self.pixel_format {
            args.push("-pix_fmt".to_string());
            args.push(pixel_format.clone());
        }

        let aresample_async = match (
            context.source.relay.is_some(),
            context.execution_plan.latency_mode,
        ) {
            (true, LatencyMode::Balanced) => 1u32,
            _ => 0u32,
        };

        args.extend([
            "-af".to_string(),
            format!(
                "asetpts=PTS-STARTPTS,aresample=async={}:first_pts=0",
                aresample_async
            ),
            "-c:a".to_string(),
            "pcm_s16le".to_string(),
            "-flush_packets".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "nut".to_string(),
            self.output_sink()?,
        ]);

        if shared_preprocess_plan.requires_stage()
            || shared_preprocess_plan.has_temporary_executor_fallbacks()
            || shared_preprocess_plan.has_unimplemented_portable_operations()
        {
            // Keep this stage normalize-only until downstream preprocess renders
            // portable resize/interpolation/HDR/shader work.
        }

        Ok(FfmpegNormalizerCommand {
            program: self.program.clone(),
            args,
            output_transport: self.output_transport.clone(),
        })
    }

    fn planned_normalization(&self, context: &StageRuntimeContext) -> SharedNormalizationPlan {
        context
            .execution_plan
            .to_intermediate_plan()
            .normalization_plan()
    }

    fn validate_context(&self, context: &StageRuntimeContext) -> io::Result<()> {
        if context.transport.input != FrameTransport::SourcePull {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FfmpegNormalizer requires {:?} input transport, got {:?}.",
                    FrameTransport::SourcePull,
                    context.transport.input
                ),
            ));
        }

        let expected_output = self.output_transport.clone();
        if context.transport.output != expected_output {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FfmpegNormalizer requires {:?} output transport, got {:?}.",
                    expected_output, context.transport.output
                ),
            ));
        }

        Ok(())
    }

    fn output_sink(&self) -> io::Result<String> {
        match &self.output_transport {
            FrameTransport::StdoutPipe => Ok("pipe:1".to_string()),
            FrameTransport::NamedPipe(path) => Ok(path.to_string_lossy().to_string()),
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "FFmpeg local-socket output transport at {:?} is a future transport placeholder.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg normalizer cannot emit to {:?}; expected an explicit inter-stage transport.",
                    transport
                ),
            )),
        }
    }
}

fn build_source_headers(headers: &HashMap<String, String>) -> String {
    let mut header_str = String::new();
    for (key, value) in headers {
        header_str.push_str(&format!("{}: {}\r\n", key, value));
    }
    header_str
}

impl Normalizer for FfmpegNormalizer {
    fn build_normalizer_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let command = self.build_command(context)?;
        let stdout = match &command.output_transport {
            FrameTransport::StdoutPipe => StdoutMode::Transport(FrameTransport::StdoutPipe),
            FrameTransport::NamedPipe(_) | FrameTransport::LocalSocket(_) => StdoutMode::Null,
            transport => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "FFmpeg normalizer cannot map {:?} to a process stdout mode.",
                        transport
                    ),
                ))
            }
        };

        Ok(ProcessSpec {
            stage: PipelineStageId::Normalizer,
            program: command.program,
            args: command.args,
            transport: context.transport.clone(),
            stdin: StdinMode::Null,
            stdout,
            stderr_piped: true,
            current_dir: None,
            env: Vec::new(),
            readiness_policy: PipelineStageReadinessPolicy::ReadyOnSpawn,
            log_label: "ffmpeg-normalizer".to_string(),
            stderr_mode: StderrMode::Filtered {
                suppress: &["Invalid timestamps stream="],
            },
            kill_on_drop: true,
            hidden_window: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegNormalizer;
    use crate::graph::{
        ExecutionPlan, ExecutorKind, LatencyMode, SharedNormalizationOperation, VideoOp,
    };
    use crate::runtime::{
        FrameTransport, Normalizer, SessionOutputPaths, StageRuntimeContext, TransportConfig,
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
    fn builds_low_latency_nut_pipe_command_for_relayed_live_input() {
        let normalizer = FfmpegNormalizer::new("ffmpeg");
        let context = context(LatencyMode::Low, FrameTransport::StdoutPipe);
        let normalization_plan = normalizer.planned_normalization(&context);
        let command = normalizer.build_command(&context).unwrap();

        assert!(normalization_plan.requires_stage());
        assert_eq!(normalization_plan.operations.len(), 1);
        assert!(matches!(
            &normalization_plan.operations[0],
            SharedNormalizationOperation::NormalizeInput(_)
        ));
        assert_eq!(command.program, PathBuf::from("ffmpeg"));
        assert_eq!(command.output_transport, FrameTransport::StdoutPipe);
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
    fn balanced_latency_relaxes_low_delay_flags_and_supports_named_pipe_output() {
        let normalizer = FfmpegNormalizer::new("ffmpeg").with_output_transport(
            FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1")),
        );
        let command = normalizer
            .build_command(&context(
                LatencyMode::Balanced,
                FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1")),
            ))
            .unwrap();
        let spec = normalizer
            .build_normalizer_spec(&context(
                LatencyMode::Balanced,
                FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1")),
            ))
            .unwrap();

        assert!(!has_subsequence(
            &command.args,
            &["-fflags", "+genpts+nobuffer"]
        ));
        assert!(!has_subsequence(&command.args, &["-flags", "low_delay"]));
        assert!(!command.args.iter().any(|a| a == "-re"));
        assert!(has_subsequence(&command.args, &["-fflags", "+genpts"]));
        assert!(has_subsequence(
            &command.args,
            &["-f", "nut", r"\\.\pipe\referee-normalized-session-1"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["-af", "asetpts=PTS-STARTPTS,aresample=async=1:first_pts=0"]
        ));
        assert_eq!(
            spec.transport.output,
            FrameTransport::NamedPipe(PathBuf::from(r"\\.\pipe\referee-normalized-session-1"))
        );
        assert_eq!(spec.stdout, crate::runtime::StdoutMode::Null);
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
        assert_eq!(command.output_transport, FrameTransport::StdoutPipe);
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
