use std::io;
use std::path::PathBuf;

use crate::runtime::{
    FrameTransport, Normalizer, PipelineStageId, ProcessSpec, StageRuntimeContext, StderrMode,
};
use crate::source::ffmpeg_header_block;

const LIVE_RECONNECT_DELAY_MAX_SECONDS: u32 = 2;
const LIVE_RW_TIMEOUT_MICROS: u64 = 15_000_000;
/// Matches the Rigaya executors' buffered ingest (`--input-analyze 4`,
/// `--input-probesize 1000000`).
const ANALYZE_DURATION_MICROS: u32 = 4_000_000;
const PROBESIZE_BYTES: u32 = 1_000_000;
/// Source timestamp jumps longer than this many seconds are cut out of the
/// timeline instead of passed downstream. NVEncC's `--avsync forcecfr` fills
/// any forward gap with repeats of the last frame (up to 5 minutes' worth),
/// so a live HLS source that skips expired segments or restarts its
/// timestamps froze the picture for as long as the gap. Shorter gaps are
/// left for forcecfr to smooth over.
const TIMESTAMP_JUMP_CUT_SECONDS: &str = "2";

/// Ingest for executors that decode the source themselves: stream-copies the
/// source's first video and audio streams to MPEG-TS on stdout.
///
/// The FFmpeg CLI removes timestamp jumps above `-dts_delta_threshold` from
/// formats with discontinuous timestamps, which includes HLS. It applies one
/// offset to every stream, so audio stays aligned with video. Its warnings
/// ("timestamp discontinuity", "skipping N segments ahead") also reach the
/// log, which NVEncC's own HLS reader keeps to itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegSourceRemuxer {
    program: PathBuf,
}

impl FfmpegSourceRemuxer {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn build_command(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        let stage = PipelineStageId::Normalizer;
        if context.transport.input != FrameTransport::SourcePull {
            return Err(context.transport.input.unsupported(stage, "input"));
        }
        if context.transport.output != FrameTransport::RemuxedSourcePipe {
            return Err(context.transport.output.unsupported(stage, "output"));
        }
        let output_url = context.transport.output.ffmpeg_output_url(stage)?;

        let mut args = vec![
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-nostdin".to_string(),
            "-rw_timeout".to_string(),
            LIVE_RW_TIMEOUT_MICROS.to_string(),
            "-reconnect".to_string(),
            "1".to_string(),
            "-reconnect_streamed".to_string(),
            "1".to_string(),
            "-reconnect_on_network_error".to_string(),
            "1".to_string(),
            "-reconnect_delay_max".to_string(),
            LIVE_RECONNECT_DELAY_MAX_SECONDS.to_string(),
            "-analyzeduration".to_string(),
            ANALYZE_DURATION_MICROS.to_string(),
            "-probesize".to_string(),
            PROBESIZE_BYTES.to_string(),
            // The input proxy rewrites every segment to `/v1/input/{id}?url=…`,
            // which has no media extension, and FFmpeg's HLS demuxer rejects
            // such segments by default ("is not in allowed_segment_extensions").
            "-extension_picky".to_string(),
            "0".to_string(),
        ];
        if !context.source.runtime_headers.is_empty() {
            args.push("-headers".to_string());
            args.push(ffmpeg_header_block(None, &context.source.runtime_headers));
        }
        args.extend([
            "-i".to_string(),
            context.source.runtime_url.clone(),
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0?".to_string(),
            "-c".to_string(),
            "copy".to_string(),
            "-dts_delta_threshold".to_string(),
            TIMESTAMP_JUMP_CUT_SECONDS.to_string(),
            "-flush_packets".to_string(),
            "1".to_string(),
            "-f".to_string(),
            "mpegts".to_string(),
            output_url.to_string(),
        ]);

        Ok(ProcessSpec::new(
            stage,
            &self.program,
            args,
            context.transport.clone(),
            "ffmpeg-remux",
        )
        .with_stderr_mode(StderrMode::Filtered {
            suppress: &["Invalid timestamps stream="],
        }))
    }
}

impl Normalizer for FfmpegSourceRemuxer {
    fn build_normalizer_spec(&self, context: &StageRuntimeContext) -> io::Result<ProcessSpec> {
        self.build_command(context)
    }
}

#[cfg(test)]
mod tests {
    use super::FfmpegSourceRemuxer;
    use crate::graph::{ExecutionPlan, ExecutorKind, LatencyMode, VideoOp};
    use crate::runtime::{
        FrameTransport, Normalizer, PipelineStageId, PipelineStageReadinessPolicy,
        SessionOutputPaths, StageRuntimeContext, StderrMode, TransportConfig,
    };
    use crate::source::{
        RelayedSource, SourceClassification, SourceDescriptor, SourceKind, SourceTransport,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    const RELAY_URL: &str =
        "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8";

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
                runtime_url: RELAY_URL.to_string(),
                runtime_headers: HashMap::new(),
                session_headers: HashMap::new(),
                relay: Some(RelayedSource {
                    original_url: "https://example.com/live/master.m3u8".to_string(),
                    relay_url: RELAY_URL.to_string(),
                    headers: HashMap::new(),
                    metadata: None,
                }),
                metadata: None,
            },
            session_paths: SessionOutputPaths {
                session_dir: PathBuf::from("session"),
                packager_playlist_path: PathBuf::from("session\\index.m3u8"),
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
    fn stream_copies_the_source_to_mpegts_and_cuts_timestamp_jumps() {
        let spec = FfmpegSourceRemuxer::new("ffmpeg")
            .build_normalizer_spec(&context(
                FrameTransport::SourcePull,
                FrameTransport::RemuxedSourcePipe,
            ))
            .unwrap();

        assert_eq!(spec.stage, PipelineStageId::Normalizer);
        assert_eq!(spec.program, PathBuf::from("ffmpeg"));
        assert!(has_subsequence(&spec.args, &["-i", RELAY_URL]));
        assert!(has_subsequence(
            &spec.args,
            &["-map", "0:v:0", "-map", "0:a:0?", "-c", "copy"]
        ));
        assert!(has_subsequence(&spec.args, &["-dts_delta_threshold", "2"]));
        assert!(has_subsequence(&spec.args, &["-f", "mpegts", "pipe:1"]));
        assert!(has_subsequence(&spec.args, &["-reconnect", "1"]));
        assert!(has_subsequence(&spec.args, &["-rw_timeout", "15000000"]));
        // Relayed segment URLs have no media extension.
        let picky = spec
            .args
            .windows(2)
            .position(|pair| pair == ["-extension_picky", "0"])
            .expect("-extension_picky 0");
        assert!(picky < spec.args.iter().position(|a| a == "-i").unwrap());
        // `-copyts` would switch FFmpeg's discontinuity correction off.
        assert!(!spec.args.iter().any(|arg| arg == "-copyts"));
        assert!(!spec.args.iter().any(|arg| arg == "-headers"));
        assert_eq!(
            spec.readiness_policy,
            PipelineStageReadinessPolicy::ReadyOnSpawn
        );
        assert!(matches!(spec.stderr_mode, StderrMode::Filtered { .. }));
    }

    #[test]
    fn forwards_runtime_headers_before_the_input() {
        let mut context = context(
            FrameTransport::SourcePull,
            FrameTransport::RemuxedSourcePipe,
        );
        context.source.runtime_headers =
            HashMap::from([("Referer".to_string(), "https://site/".to_string())]);
        let spec = FfmpegSourceRemuxer::new("ffmpeg")
            .build_command(&context)
            .unwrap();

        let headers = spec.args.iter().position(|a| a == "-headers").unwrap();
        let input = spec.args.iter().position(|a| a == "-i").unwrap();
        assert!(headers < input);
        assert!(spec.args[headers + 1].contains("Referer: https://site/"));
    }

    #[test]
    fn requires_source_pull_into_the_remuxed_source_pipe() {
        let remuxer = FfmpegSourceRemuxer::new("ffmpeg");
        assert!(remuxer
            .build_command(&context(
                FrameTransport::SourcePull,
                FrameTransport::StdoutPipe
            ))
            .is_err());
        assert!(remuxer
            .build_command(&context(
                FrameTransport::StdoutPipe,
                FrameTransport::RemuxedSourcePipe
            ))
            .is_err());
    }
}
