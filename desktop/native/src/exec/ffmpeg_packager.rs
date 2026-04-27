use std::io;
use std::path::{Path, PathBuf};

use crate::runtime::{
    FrameTransport, Packager, PipelineStageId, PipelineStageReadinessPolicy, ProcessSpec,
    StageRuntimeContext, StderrMode, StdinMode, StdoutMode,
};

const DEFAULT_SEGMENT_FILENAME_PATTERN: &str = "segment_%06d.ts";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegHlsPackagerContext {
    pub segment_duration_secs: u32,
    pub list_size: u32,
    pub delete_segments: bool,
    pub segment_filename_pattern: String,
    /// Duration of the first HLS segment in seconds. Defaults to `0` so the
    /// packager writes the first segment as soon as frames arrive rather than
    /// waiting a full `segment_duration_secs` before emitting any output.
    /// This reduces start-up latency by up to one segment duration.
    pub hls_init_time_secs: u32,
}

impl Default for FfmpegHlsPackagerContext {
    fn default() -> Self {
        Self {
            segment_duration_secs: 1,
            list_size: 8,
            delete_segments: true,
            segment_filename_pattern: DEFAULT_SEGMENT_FILENAME_PATTERN.to_string(),
            hls_init_time_secs: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegHlsPackagerCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub stdin: StdinMode,
    pub current_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegHlsPackager {
    program: PathBuf,
    context: FfmpegHlsPackagerContext,
}

struct HlsPackagerOutputTarget {
    playlist_path: PathBuf,
    segment_dir: PathBuf,
}

impl FfmpegHlsPackager {
    pub fn new(program: impl Into<PathBuf>, context: FfmpegHlsPackagerContext) -> Self {
        Self {
            program: program.into(),
            context,
        }
    }

    pub fn build_command(
        &self,
        runtime: &StageRuntimeContext,
    ) -> io::Result<FfmpegHlsPackagerCommand> {
        let (input_arg, stdin) = self.resolve_input_transport(&runtime.transport.input)?;
        let output = self.resolve_output_transport(&runtime.transport.output)?;

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
            "-fflags".to_string(),
            "+discardcorrupt+nobuffer".to_string(),
            "-f".to_string(),
            "mpegts".to_string(),
            "-i".to_string(),
            input_arg,
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-c".to_string(),
            "copy".to_string(),
            "-f".to_string(),
            "hls".to_string(),
            "-hls_time".to_string(),
            self.context.segment_duration_secs.to_string(),
            "-hls_init_time".to_string(),
            self.context.hls_init_time_secs.to_string(),
            "-hls_list_size".to_string(),
            self.context.list_size.to_string(),
            "-hls_segment_type".to_string(),
            "mpegts".to_string(),
            "-hls_segment_filename".to_string(),
            self.context.segment_filename_pattern.clone(),
        ];

        if self.context.delete_segments {
            args.push("-hls_flags".to_string());
            args.push("delete_segments".to_string());
        }

        args.push(output.playlist_path.to_string_lossy().to_string());

        Ok(FfmpegHlsPackagerCommand {
            program: self.program.clone(),
            args,
            stdin,
            current_dir: Some(output.segment_dir),
        })
    }

    fn resolve_input_transport(
        &self,
        transport: &FrameTransport,
    ) -> io::Result<(String, StdinMode)> {
        match transport {
            FrameTransport::StdoutPipe => Ok((
                "pipe:0".to_string(),
                StdinMode::Transport(FrameTransport::StdoutPipe),
            )),
            FrameTransport::NamedPipe(path) => {
                Ok((path.to_string_lossy().to_string(), StdinMode::Null))
            }
            FrameTransport::LocalSocket(path) => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "FFmpeg HLS packager does not yet support local-socket input at {:?}.",
                    path
                ),
            )),
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg HLS packager cannot consume {:?} as its input transport.",
                    transport
                ),
            )),
        }
    }

    fn resolve_output_transport(
        &self,
        transport: &FrameTransport,
    ) -> io::Result<HlsPackagerOutputTarget> {
        match transport {
            FrameTransport::HlsOutput {
                playlist_path,
                segment_dir,
            } => {
                let playlist_dir = playlist_path.parent().unwrap_or(Path::new("."));
                if playlist_dir != segment_dir {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!(
                            "FFmpeg HLS packager currently expects playlist {:?} and segments {:?} to share the same directory.",
                            playlist_path, segment_dir
                        ),
                    ));
                }

                Ok(HlsPackagerOutputTarget {
                    playlist_path: playlist_path.clone(),
                    segment_dir: segment_dir.clone(),
                })
            }
            transport => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "FFmpeg HLS packager requires {:?} output transport, got {:?}.",
                    FrameTransport::HlsOutput {
                        playlist_path: PathBuf::from("index.m3u8"),
                        segment_dir: PathBuf::from("."),
                    },
                    transport
                ),
            )),
        }
    }
}

impl Packager for FfmpegHlsPackager {
    fn build_packager_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let command = self.build_command(context)?;
        Ok(ProcessSpec {
            stage: PipelineStageId::Packager,
            program: command.program,
            args: command.args,
            transport: context.transport.clone(),
            stdin: command.stdin,
            stdout: StdoutMode::Null,
            stderr_piped: true,
            current_dir: command.current_dir,
            env: Vec::new(),
            // Spawning the process only means the muxer is alive. Session startup
            // readiness is still gated separately on the final playlist file.
            readiness_policy: PipelineStageReadinessPolicy::ReadyOnSpawn,
            log_label: "ffmpeg-hls-packager".to_string(),
            stderr_mode: StderrMode::Raw,
            kill_on_drop: true,
            hidden_window: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FfmpegHlsPackager, FfmpegHlsPackagerContext};
    use crate::graph::{ExecutionPlan, ExecutorKind, LatencyMode, VideoOp};
    use crate::runtime::{
        FrameTransport, Packager, SessionOutputPaths, StageRuntimeContext, StdinMode,
        TransportConfig,
    };
    use crate::source::{SourceClassification, SourceDescriptor, SourceKind, SourceTransport};
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;

    fn context(input: FrameTransport, output: FrameTransport) -> StageRuntimeContext {
        StageRuntimeContext {
            session_id: "session-1".to_string(),
            execution_plan: ExecutionPlan {
                executor: ExecutorKind::NvidiaSpecialized,
                latency_mode: LatencyMode::Low,
                requires_local_hls_relay: true,
                video_ops: vec![VideoOp::NormalizeInput],
            },
            source: SourceDescriptor {
                classification: SourceClassification {
                    transport: SourceTransport::RemoteHttp,
                    kind: SourceKind::Hls,
                },
                original_url: "https://example.com/live/master.m3u8".to_string(),
                runtime_url: "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8".to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: None,
                metadata: None,
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session").join("index.m3u8"),
            },
            transport: TransportConfig { input, output },
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
    fn build_command_maps_pipe_input_to_live_hls_output() {
        let packager = FfmpegHlsPackager::new(
            "ffmpeg",
            FfmpegHlsPackagerContext {
                segment_duration_secs: 1,
                list_size: 8,
                delete_segments: true,
                segment_filename_pattern: "segment_%06d.ts".to_string(),
                hls_init_time_secs: 0,
            },
        );
        let command = packager
            .build_command(&context(
                FrameTransport::StdoutPipe,
                FrameTransport::HlsOutput {
                    playlist_path: PathBuf::from("session").join("index.m3u8"),
                    segment_dir: PathBuf::from("session"),
                },
            ))
            .unwrap();

        assert_eq!(
            command.stdin,
            StdinMode::Transport(FrameTransport::StdoutPipe)
        );
        assert_eq!(command.current_dir, Some(PathBuf::from("session")));
        assert!(has_subsequence(
            &command.args,
            &["-fflags", "+discardcorrupt+nobuffer"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["-f", "mpegts", "-i", "pipe:0"]
        ));
        assert!(has_subsequence(&command.args, &["-i", "pipe:0"]));
        assert!(has_subsequence(&command.args, &["-c", "copy"]));
        assert!(has_subsequence(&command.args, &["-hls_time", "1"]));
        assert!(has_subsequence(&command.args, &["-hls_init_time", "0"]));
        assert!(has_subsequence(&command.args, &["-hls_list_size", "8"]));
        assert!(has_subsequence(
            &command.args,
            &["-hls_segment_filename", "segment_%06d.ts"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["-hls_flags", "delete_segments"]
        ));
        assert_eq!(
            command.args.last().map(String::as_str),
            Some(
                PathBuf::from("session")
                    .join("index.m3u8")
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn build_packager_spec_keeps_stage_identity() {
        let packager = FfmpegHlsPackager::new("ffmpeg", FfmpegHlsPackagerContext::default());
        let spec = packager
            .build_packager_spec(&context(
                FrameTransport::StdoutPipe,
                FrameTransport::HlsOutput {
                    playlist_path: PathBuf::from("session").join("index.m3u8"),
                    segment_dir: PathBuf::from("session"),
                },
            ))
            .unwrap();

        assert_eq!(spec.stage, crate::runtime::PipelineStageId::Packager);
        assert_eq!(spec.stdin, StdinMode::Transport(FrameTransport::StdoutPipe));
        assert_eq!(spec.stdout, crate::runtime::StdoutMode::Null);
    }

    #[test]
    fn rejects_output_dirs_that_would_produce_wrong_playlist_uris() {
        let packager = FfmpegHlsPackager::new("ffmpeg", FfmpegHlsPackagerContext::default());
        let error = packager
            .build_command(&context(
                FrameTransport::StdoutPipe,
                FrameTransport::HlsOutput {
                    playlist_path: PathBuf::from("session\\index.m3u8"),
                    segment_dir: PathBuf::from("session\\segments"),
                },
            ))
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(error.to_string().contains("share the same directory"));
    }
}
