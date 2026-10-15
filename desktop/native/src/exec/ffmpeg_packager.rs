use std::io;
use std::path::{Path, PathBuf};

use crate::runtime::{
    FrameTransport, Packager, PipelineStageId, ProcessSpec, StageRuntimeContext,
    HLS_SEGMENT_FILENAME_PATTERN,
};

use super::build_ffmpeg_hls_output_args;

/// Staged-path packager: copies the executor's MPEG-TS into the session's
/// HLS playlist and segments without re-encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegHlsPackager {
    program: PathBuf,
    segment_duration_secs: u32,
}

impl FfmpegHlsPackager {
    pub fn new(program: impl Into<PathBuf>, segment_duration_secs: u32) -> Self {
        Self {
            program: program.into(),
            segment_duration_secs,
        }
    }

    pub fn build_command(&self, runtime: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let stage = PipelineStageId::Packager;
        let input_url = runtime.transport.input.ffmpeg_input_url(stage)?;
        let (playlist_path, segment_dir) = hls_output_target(&runtime.transport.output)?;

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
            input_url.to_string(),
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-sn".to_string(),
            "-dn".to_string(),
            "-c".to_string(),
            "copy".to_string(),
        ];
        // Segment names are relative so the playlist references them by name;
        // the process runs inside the session directory.
        args.extend(build_ffmpeg_hls_output_args(
            self.segment_duration_secs,
            HLS_SEGMENT_FILENAME_PATTERN,
            &playlist_path.to_string_lossy(),
        ));

        Ok(ProcessSpec::new(
            stage,
            &self.program,
            args,
            runtime.transport.clone(),
            "ffmpeg-hls-packager",
        )
        .with_current_dir(Some(segment_dir.to_path_buf())))
    }
}

/// The playlist path and segment directory of an HLS output transport.
/// They must match, because segment URIs in the playlist are bare file names.
fn hls_output_target(transport: &FrameTransport) -> io::Result<(&Path, &Path)> {
    let FrameTransport::HlsOutput {
        playlist_path,
        segment_dir,
    } = transport
    else {
        return Err(transport.unsupported(PipelineStageId::Packager, "output"));
    };

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

    Ok((playlist_path, segment_dir))
}

impl Packager for FfmpegHlsPackager {
    /// Spawning the process only means the muxer is alive. Session startup
    /// readiness is gated separately on the final playlist file.
    fn build_packager_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        self.build_command(context)
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegHlsPackager;
    use crate::graph::{ExecutionPlan, ExecutorKind, LatencyMode, VideoOp};
    use crate::runtime::{
        FrameTransport, Packager, PipelineStageReadinessPolicy, SessionOutputPaths,
        StageRuntimeContext, TransportConfig,
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

    fn session_hls_output() -> FrameTransport {
        FrameTransport::HlsOutput {
            playlist_path: PathBuf::from("session").join("index.m3u8"),
            segment_dir: PathBuf::from("session"),
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
        let packager = FfmpegHlsPackager::new("ffmpeg", 1);
        let command = packager
            .build_command(&context(FrameTransport::StdoutPipe, session_hls_output()))
            .unwrap();

        assert_eq!(command.current_dir, Some(PathBuf::from("session")));
        assert!(has_subsequence(
            &command.args,
            &["-fflags", "+discardcorrupt+nobuffer"]
        ));
        assert!(has_subsequence(
            &command.args,
            &["-f", "mpegts", "-i", "pipe:0"]
        ));
        assert!(has_subsequence(&command.args, &["-c", "copy"]));
        assert!(has_subsequence(&command.args, &["-hls_time", "1"]));
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
        let packager = FfmpegHlsPackager::new("ffmpeg", 1);
        let spec = packager
            .build_packager_spec(&context(FrameTransport::StdoutPipe, session_hls_output()))
            .unwrap();

        assert_eq!(spec.stage, crate::runtime::PipelineStageId::Packager);
        assert_eq!(spec.log_label, "ffmpeg-hls-packager");
        assert_eq!(
            spec.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnSpawn
        );
    }

    #[test]
    fn rejects_non_pipe_input() {
        let packager = FfmpegHlsPackager::new("ffmpeg", 1);
        let error = packager
            .build_command(&context(FrameTransport::SourcePull, session_hls_output()))
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_output_dirs_that_would_produce_wrong_playlist_uris() {
        let packager = FfmpegHlsPackager::new("ffmpeg", 1);
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
