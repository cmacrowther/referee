//! The per-session task: launch an attempt, supervise it, and decide whether
//! to retry.

use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::graph::{ExecutionPlan, ExecutorKind};
use crate::runtime::{PipelineStageId, PipelineSupervisorOutcome, PipelineSupervisorStopReason};

use super::launch::{PipelineLaunch, SessionPipeline, MAX_PIPELINE_START_ATTEMPTS};
use super::session::{
    finalize_pipeline_session, remove_session_from_pipeline, PipelineCompletionSignal, SessionMap,
};

/// Starts a session's pipeline on a background task and returns immediately.
///
/// The task launches the stages (staged when the plan needs a preprocess
/// stage, single-process otherwise, with NVEncC reading HLS sources through
/// the source remuxer), supervises them against client
/// heartbeats, retries known transient startup failures once, and finally
/// removes the session and its directory and signals `completion`.
pub fn start_pipeline(
    launch: PipelineLaunch,
    sessions: &SessionMap,
    completion: PipelineCompletionSignal,
    heartbeat_rx: mpsc::Receiver<()>,
) {
    let pipeline = SessionPipeline::prepare(launch);
    let sessions = sessions.clone();
    tokio::spawn(run_session(pipeline, sessions, completion, heartbeat_rx));
}

/// What to do after an attempt ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptResolution {
    /// The session was stopped, timed out, or ended; tear it down.
    Finish,
    /// Retry with the same stages (encoder transient failure).
    Retry,
    /// Retry on the session's plan without RIFE.
    RetryWithoutRife,
    /// Retry with the executor pulling the source instead of the remuxer.
    RetryWithDirectIngest,
}

async fn run_session(
    mut pipeline: SessionPipeline,
    sessions: SessionMap,
    completion: PipelineCompletionSignal,
    mut heartbeat_rx: mpsc::Receiver<()>,
) {
    let session_id = pipeline.session_id().to_string();
    let backend_label = pipeline.backend_label().to_string();
    let mut attempt_number = 1u32;
    let mut allow_staged_launch = pipeline.should_attempt_staged_launch();

    loop {
        let launch = match pipeline
            .launch_attempt(attempt_number, &mut allow_staged_launch)
            .await
        {
            Ok(launch) => launch,
            Err(error) => {
                error!(
                    "[Pipeline:{}]: Failed to spawn encoder for session {}: {}",
                    backend_label, session_id, error
                );
                remove_session_from_pipeline(&session_id, &sessions);
                break;
            }
        };

        let session_registered = match sessions.get_mut(&session_id) {
            Some(mut session) => {
                session.update_local_startup_attempt(
                    attempt_number,
                    launch.startup_mode.as_str().to_string(),
                );
                true
            }
            None => false,
        };
        if !session_registered {
            warn!(
                "[Pipeline:{}]: Session {} disappeared before process could be registered; aborting.",
                backend_label, session_id
            );
            // The heartbeat sender went with the session, so the supervisor
            // stops the stages immediately.
            let _ = launch.supervisor.run(&mut heartbeat_rx).await;
            break;
        }

        let outcome = launch.supervisor.run(&mut heartbeat_rx).await;
        let resolution = resolve_attempt(
            &outcome,
            &sessions,
            &session_id,
            &backend_label,
            pipeline.launch.execution_plan.executor,
            attempt_number,
            pipeline.uses_rife(),
            pipeline.remuxes_source_ingest(),
        );

        match resolution {
            AttemptResolution::Finish => break,
            AttemptResolution::RetryWithDirectIngest => {
                warn!(
                    "[Pipeline:{}]: Remuxed source ingest failed before producing output for session {}; retrying with the encoder pulling the source.",
                    backend_label, session_id
                );
                pipeline.use_direct_source_ingest();
            }
            AttemptResolution::Retry => {
                if pipeline.switch_to_universal_fallback() {
                    let plan = &pipeline.launch.execution_plan;
                    warn!(
                        "[Pipeline:{}]: VCEEncC failed before producing output for session {}; retrying on the Universal FFmpeg executor ({}).",
                        backend_label,
                        session_id,
                        plan.framegen_label().unwrap_or("no frame generation"),
                    );
                    sync_session_plan(&sessions, &session_id, plan);
                    allow_staged_launch = pipeline.should_attempt_staged_launch();
                } else {
                    warn!(
                        "[Pipeline:{}]: encoder failed before producing output for session {}; retrying with fallback configuration.",
                        backend_label, session_id
                    );
                }
            }
            AttemptResolution::RetryWithoutRife => {
                if !pipeline.switch_to_rife_fallback() {
                    error!(
                        "[Pipeline:{}]: rife-worker failed for session {} and no fallback plan exists.",
                        backend_label, session_id
                    );
                    remove_session_from_pipeline(&session_id, &sessions);
                    break;
                }
                let plan = &pipeline.launch.execution_plan;
                warn!(
                    "[Pipeline:{}]: rife-worker failed for session {} (attempt {}/{}); retrying with {}.",
                    backend_label,
                    session_id,
                    attempt_number,
                    MAX_PIPELINE_START_ATTEMPTS,
                    plan.framegen_label().unwrap_or("no frame generation"),
                );
                sync_session_plan(&sessions, &session_id, plan);
                allow_staged_launch = pipeline.should_attempt_staged_launch();
            }
        }

        clear_session_output(&pipeline.launch.session_dir).await;
        if let Some(mut session) = sessions.get_mut(&session_id) {
            session.set_retrying_startup(true);
        }
        attempt_number += 1;
    }

    finalize_pipeline_session(
        &session_id,
        &sessions,
        &pipeline.launch.session_dir,
        &completion,
    )
    .await;
}

/// Reports a replanned session's executor and frame generation in status.
fn sync_session_plan(sessions: &SessionMap, session_id: &str, plan: &ExecutionPlan) {
    if let Some(mut session) = sessions.get_mut(session_id) {
        session.info.framegen_enabled = plan.has_executable_interpolation();
        session.info.framegen_engine = plan.framegen_label().map(String::from);
        session.info.executor = plan.executor;
        session.info.target_fps = plan.output_frame_rate(session.info.source_fps);
    }
}

/// Logs how an attempt ended and decides whether to retry. Removes the
/// session from the map when the pipeline gives up on it.
fn resolve_attempt(
    outcome: &PipelineSupervisorOutcome,
    sessions: &SessionMap,
    session_id: &str,
    backend_label: &str,
    executor: ExecutorKind,
    attempt_number: u32,
    uses_rife: bool,
    remuxed_ingest: bool,
) -> AttemptResolution {
    let (failed_stage, exit_code) = match outcome.stop_reason {
        PipelineSupervisorStopReason::HeartbeatChannelClosed => {
            return AttemptResolution::Finish;
        }
        PipelineSupervisorStopReason::OrphanTimeout { waiting_on } => {
            warn!(
                "[Pipeline:{}]: Session {} timed out waiting for the first {} heartbeat.",
                backend_label, session_id, waiting_on
            );
            remove_session_from_pipeline(session_id, sessions);
            return AttemptResolution::Finish;
        }
        PipelineSupervisorStopReason::InactivityTimeout { waiting_on } => {
            warn!(
                "[Pipeline:{}]: Session {} timed out due to {} inactivity after the first heartbeat.",
                backend_label, session_id, waiting_on
            );
            remove_session_from_pipeline(session_id, sessions);
            return AttemptResolution::Finish;
        }
        PipelineSupervisorStopReason::StageExited { stage, exit_code } => (stage, exit_code),
    };

    let report = outcome.stage_report(failed_stage);
    let stderr_tail = report
        .map(|report| report.stderr_tail.as_str())
        .unwrap_or_default();
    info!(
        "[Pipeline:{}]: {} stage exited \u{2014} session={} attempt={} exit_code={:?}",
        backend_label, failed_stage, session_id, attempt_number, exit_code,
    );
    debug!(
        "[Pipeline:{}]: Stage exit detail \u{2014} state={:?} readiness={:?} first_heartbeat={}",
        backend_label,
        report.map(|report| report.state),
        report.map(|report| report.readiness),
        outcome.first_heartbeat_seen
    );
    if exit_code != Some(0) && !stderr_tail.is_empty() {
        error!(
            "[Pipeline:{}:{}]: Last stderr for session {} (attempt {}):\n{}",
            backend_label,
            failed_stage,
            session_id,
            attempt_number,
            stderr_tail.trim()
        );
    }

    let Some(startup_complete) = sessions
        .get(session_id)
        .map(|session| session.info.startup_complete)
    else {
        info!(
            "[Pipeline:{}]: Session {} no longer exists; stopping pipeline loop.",
            backend_label, session_id
        );
        return AttemptResolution::Finish;
    };

    let failed_before_startup =
        exit_code != Some(0) && !startup_complete && attempt_number < MAX_PIPELINE_START_ATTEMPTS;
    if failed_before_startup && remuxed_ingest && is_ingest_failure(failed_stage, stderr_tail) {
        return AttemptResolution::RetryWithDirectIngest;
    }
    // The AMD executor's retries run the Universal FFmpeg fallback, so any
    // VCEEncC startup failure (AMF FRC or FSR unavailable on this GPU or
    // driver, for one) is worth one more attempt, whatever its message.
    let amd_fallback_available = executor == ExecutorKind::AmdSpecialized && attempt_number == 1;
    if failed_before_startup
        && failed_stage == PipelineStageId::Executor
        && (amd_fallback_available
            || ((executor == ExecutorKind::NvidiaSpecialized || backend_label == "vceenc")
                && is_retryable_failure(stderr_tail)))
    {
        return AttemptResolution::Retry;
    }
    if failed_before_startup && uses_rife && failed_stage == PipelineStageId::Preprocess {
        return AttemptResolution::RetryWithoutRife;
    }

    if exit_code != Some(0) {
        debug!(
            "[Pipeline:{}]: No retry \u{2014} session={} stage={} attempt={}/{} startup={} retryable_output={}",
            backend_label,
            session_id,
            failed_stage,
            attempt_number,
            MAX_PIPELINE_START_ATTEMPTS,
            startup_complete,
            is_retryable_failure(stderr_tail),
        );
    }
    match (startup_complete, exit_code) {
        (true, Some(0)) => info!(
            "[Pipeline:{}]: Stream ended \u{2014} session={} (clean exit)",
            backend_label, session_id,
        ),
        (true, _) => warn!(
            "[Pipeline:{}]: Stream ended \u{2014} session={} exit_code={:?}",
            backend_label, session_id, exit_code,
        ),
        (false, _) => warn!(
            "[Pipeline:{}]: Stream ended before startup \u{2014} session={} exit_code={:?}",
            backend_label, session_id, exit_code,
        ),
    }
    remove_session_from_pipeline(session_id, sessions);
    AttemptResolution::Finish
}

/// Known transient encoder startup failures that are safe to retry with the
/// buffered (or, for AMD, Universal FFmpeg) configuration.
pub(super) fn is_retryable_failure(log_output: &str) -> bool {
    let lower = log_output.to_lowercase();
    lower.contains("input video info not parsed yet")
        || lower.contains("failed to initialize file reader")
        // AMF encoder hard-failure on Linux (e.g. RDNA 4 / RADV).  VCEEncC starts
        // up, prints its configuration, then the AMF HEVC engine fails before any
        // output is produced.  The second attempt routes through the universal
        // (VAAPI) fallback via AmdExecutor, so this is safe to retry.
        || lower.contains("break in task amfenc")
}

/// Whether a remuxed launch failed in its ingest: the remuxer itself, or the
/// executor failing to read what it was fed. The retry pulls the source
/// directly, which is how these sessions launched before the remuxer and is
/// also the right retry for a transient source failure.
pub(super) fn is_ingest_failure(failed_stage: PipelineStageId, stderr_tail: &str) -> bool {
    match failed_stage {
        PipelineStageId::Normalizer => true,
        PipelineStageId::Executor => is_retryable_failure(stderr_tail),
        PipelineStageId::Preprocess | PipelineStageId::Packager => false,
    }
}

/// Deletes every file in the session directory before a retry, so stale
/// segments and a partial playlist cannot satisfy the readiness check.
/// Errors on individual files are ignored.
async fn clear_session_output(session_dir: &std::path::Path) {
    if let Ok(mut entries) = tokio::fs::read_dir(session_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
}
