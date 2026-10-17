//! Turning a planned session into running stages: executor construction,
//! stage contexts, and single-process or staged launches.

use std::io;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::deps::detect_ffmpeg_filter;
use crate::exec::{
    detect_vceenc_amf_fsr11, select_runtime_executor_target, select_universal_encode_backend,
    AmdExecutor, AmdExecutorContext, FfmpegHlsPackager, NvidiaSpecializedExecutor,
    NvidiaSpecializedExecutorContext, RuntimeExecutorTarget, RuntimePlatform, UniversalExecutor,
    UniversalExecutorContext,
};
use crate::graph::{parse_resolution, ExecutionPlan, ExecutorKind, IntermediateExecutionPlan};
use crate::normalize::{FfmpegNormalizer, FfmpegSourceRemuxer};
use crate::preprocess::{FfmpegPreprocessor, StreamingRifePreprocessor};
use crate::runtime::{
    BackendExecutor, FrameTransport, Normalizer, Packager, PipelineStageSpawn, PipelineSupervisor,
    PipelineSupervisorTimeouts, Preprocessor, ProcessSpec, StageRuntimeContext, StartupMode,
};
use crate::source::SourceDescriptor;

use super::{detect_encoder_capabilities, EncodingProfile};

pub(super) const MAX_PIPELINE_START_ATTEMPTS: u32 = 2;
/// CAS (FidelityFX Contrast Adaptive Sharpening) strength applied at the end of every
/// executor's filter chain. Range [0.0, 1.0]; the sweet spot is roughly 0.4–0.7.
const CAS_DEFAULT_STRENGTH: f32 = 0.5;
/// How long to wait without a heartbeat before killing a session that has already checked in.
pub(crate) const HEARTBEAT_TIMEOUT_MS: u64 = 15_000;
/// How long to wait for the very first heartbeat before treating the session as orphaned.
/// This needs to be long enough to cover GPU startup + packager playlist
/// appearance + client round-trip.
pub(crate) const ORPHAN_SESSION_TIMEOUT_MS: u64 = 300_000;
/// File name of the session playlist inside the session directory.
const SESSION_PLAYLIST_FILENAME: &str = "index.m3u8";

/// Encoder and helper binaries resolved for this machine.
#[derive(Debug, Clone)]
pub struct RuntimeBackend {
    pub gpu_vendor: String,
    /// `"nvenc"`, `"vceenc"`, or `"universal"` (FFmpeg as the encoder).
    pub encoder_backend: String,
    pub encoder_path: PathBuf,
    pub ffmpeg_path: PathBuf,
    pub rife_worker_path: Option<PathBuf>,
    pub rife_model_path: Option<PathBuf>,
}

impl RuntimeBackend {
    /// Whether `rife-worker` and a RIFE model were both resolved.
    pub fn has_rife(&self) -> bool {
        self.rife_worker_path.is_some() && self.rife_model_path.is_some()
    }
}

/// Everything needed to launch and supervise one local session.
#[derive(Debug, Clone)]
pub struct PipelineLaunch {
    pub session_id: String,
    pub source: SourceDescriptor,
    pub execution_plan: ExecutionPlan,
    /// The session planned without RIFE, used when `rife-worker` fails before
    /// startup completes. `None` unless `execution_plan` uses RIFE.
    pub rife_fallback_plan: Option<ExecutionPlan>,
    pub output_resolution: String,
    pub profile: EncodingProfile,
    pub session_dir: PathBuf,
    pub backend: RuntimeBackend,
}

impl PipelineLaunch {
    /// The session playlist. Session startup is complete once it exists and is
    /// non-empty.
    pub fn playlist_path(&self) -> PathBuf {
        session_playlist_path(&self.session_dir)
    }
}

fn session_playlist_path(session_dir: &Path) -> PathBuf {
    session_dir.join(SESSION_PLAYLIST_FILENAME)
}

/// Parameters for the `rife-worker` preprocessor.
#[derive(Debug, Clone)]
struct StreamingRifeParams {
    rife_worker_path: PathBuf,
    ffmpeg_path: PathBuf,
    model_path: PathBuf,
}

impl StreamingRifeParams {
    /// Present only when both the worker binary and a model were resolved.
    fn from_backend(backend: &RuntimeBackend) -> Option<Self> {
        Some(Self {
            rife_worker_path: backend.rife_worker_path.clone()?,
            ffmpeg_path: backend.ffmpeg_path.clone(),
            model_path: backend.rife_model_path.clone()?,
        })
    }
}

/// A launched attempt: its supervisor and the ingest profile it used.
pub(super) struct PreparedLaunch {
    pub(super) supervisor: PipelineSupervisor,
    pub(super) startup_mode: StartupMode,
}

/// One session's launch inputs, resolved once and reused across attempts.
pub(super) struct SessionPipeline {
    pub(super) launch: PipelineLaunch,
    executor: Box<dyn BackendExecutor>,
    denoise: bool,
    /// The plan has preprocessor-owned work, so staged launches include a
    /// preprocess stage.
    preprocess_stage: bool,
    /// Feed the executor an FFmpeg stream-copy of the source instead of
    /// letting it pull the source (see `should_remux_source_ingest`).
    remux_source_ingest: bool,
    streaming_rife: Option<StreamingRifeParams>,
}

/// NVEncC's avsw reader stages NV12 frames directly into CUDA memory; YUV420P
/// (YV12) would force an extra plane reorder on every frame before upload.
/// Other backends use the widest-compatible YUV420P.
fn staged_pixel_format(encoder_backend: &str) -> &'static str {
    if encoder_backend == "nvenc" {
        "nv12"
    } else {
        "yuv420p"
    }
}

impl SessionPipeline {
    /// Probes capabilities (cached per binary) and builds the runtime executor.
    pub(super) fn prepare(launch: PipelineLaunch) -> Self {
        let backend = &launch.backend;
        let streaming_rife = StreamingRifeParams::from_backend(backend);
        let capabilities =
            detect_encoder_capabilities(&backend.encoder_path, &backend.encoder_backend)
                .with_rife(streaming_rife.is_some());
        let denoise = detect_ffmpeg_filter(&backend.ffmpeg_path, "hqdn3d");
        info!(
            "[Pipeline:{}]: Capabilities \u{2014} vpp-resize={}, fruc={}, native-double={}, truehdr={}, rife={}, hqdn3d={}",
            backend.encoder_backend,
            capabilities.has_vpp_resize,
            capabilities.has_fruc,
            capabilities.has_native_double,
            capabilities.has_truehdr,
            capabilities.has_rife,
            denoise,
        );

        let intermediate_plan = launch.execution_plan.to_intermediate_plan();
        let preprocess_stage = intermediate_plan.shared_preprocess_plan().requires_stage();
        let remux_source_ingest =
            should_remux_source_ingest(&launch.source, &launch.execution_plan);
        let executor = build_runtime_executor(RuntimeExecutorBuildContext {
            intermediate_plan: &intermediate_plan,
            backend,
            profile: &launch.profile,
            denoise,
        });

        Self {
            launch,
            executor,
            denoise,
            preprocess_stage,
            remux_source_ingest,
            streaming_rife,
        }
    }

    pub(super) fn session_id(&self) -> &str {
        &self.launch.session_id
    }

    pub(super) fn backend_label(&self) -> &str {
        &self.launch.backend.encoder_backend
    }

    /// Whether this session interpolates with `rife-worker`.
    pub(super) fn uses_rife(&self) -> bool {
        self.launch.execution_plan.uses_rife()
    }

    /// Re-prepares the session on its plan without RIFE, for a retry after
    /// `rife-worker` failed to start. Returns `false` when there is no such
    /// plan.
    pub(super) fn switch_to_rife_fallback(&mut self) -> bool {
        let Some(fallback_plan) = self.launch.rife_fallback_plan.take() else {
            return false;
        };
        let mut launch = self.launch.clone();
        launch.execution_plan = fallback_plan;
        *self = Self::prepare(launch);
        true
    }

    /// Moves an AMD session onto the Universal FFmpeg executor for the
    /// remaining attempts after VCEEncC fails to start. Native-only work (AMF
    /// FRC) is dropped (`ExecutionPlan::for_universal_fallback`). Returns
    /// whether the plan changed.
    pub(super) fn switch_to_universal_fallback(&mut self) -> bool {
        if self.launch.execution_plan.executor != ExecutorKind::AmdSpecialized {
            return false;
        }
        let mut launch = self.launch.clone();
        launch.execution_plan = launch.execution_plan.for_universal_fallback();
        launch.rife_fallback_plan = launch
            .rife_fallback_plan
            .as_ref()
            .map(ExecutionPlan::for_universal_fallback);
        *self = Self::prepare(launch);
        true
    }

    pub(super) fn should_attempt_staged_launch(&self) -> bool {
        should_attempt_staged_launch(&self.launch.source, &self.launch.execution_plan)
    }

    /// Whether single-process launches feed the executor through the source
    /// remuxer.
    pub(super) fn remuxes_source_ingest(&self) -> bool {
        self.remux_source_ingest
    }

    /// Makes later single-process launches pull the source in the executor,
    /// after the remuxed ingest failed to start.
    pub(super) fn use_direct_source_ingest(&mut self) {
        self.remux_source_ingest = false;
    }

    /// Launches one attempt. A staged launch that fails to build or spawn
    /// falls back to single-process, and `allow_staged` is cleared so later
    /// attempts go straight there. A remuxed single-process launch that
    /// fails to build or spawn falls back to the executor pulling the source,
    /// for this attempt and later ones.
    pub(super) async fn launch_attempt(
        &mut self,
        attempt_number: u32,
        allow_staged: &mut bool,
    ) -> io::Result<PreparedLaunch> {
        if *allow_staged {
            match self.launch_staged(attempt_number).await {
                Ok(launch) => return Ok(launch),
                Err(error) => {
                    warn!(
                        "[Pipeline:{}]: Failed to start staged pipeline for session {}: {}; falling back to legacy single-process path.",
                        self.backend_label(),
                        self.session_id(),
                        error
                    );
                    *allow_staged = false;
                }
            }
        } else if self.preprocess_stage {
            // Make the loss visible in logs rather than a silent quality
            // regression.
            warn!(
                "[Pipeline:{}]: Session {} using single-process legacy path; \
                 preprocess-owned work (interpolation, resize, HDR) will be skipped.",
                self.backend_label(),
                self.session_id()
            );
        }

        if self.remux_source_ingest {
            match self.launch_remuxed_single_process(attempt_number).await {
                Ok(launch) => return Ok(launch),
                Err(error) => {
                    warn!(
                        "[Pipeline:{}]: Failed to start remuxed source ingest for session {}: {}; falling back to the encoder pulling the source.",
                        self.backend_label(),
                        self.session_id(),
                        error
                    );
                    self.use_direct_source_ingest();
                }
            }
        }

        self.launch_single_process(attempt_number)
    }

    fn base_context(&self) -> StageRuntimeContext {
        StageRuntimeContext::single_process(
            &self.launch.session_id,
            &self.launch.execution_plan,
            &self.launch.source,
            &self.launch.playlist_path(),
        )
    }

    /// One executor process that ingests, processes, encodes, and muxes HLS.
    fn launch_single_process(&self, attempt_number: u32) -> io::Result<PreparedLaunch> {
        let spec = self
            .executor
            .build_executor_spec(&self.base_context(), attempt_number)?;

        info!(
            "[Pipeline:{}]: Launching {} startup path for session {} (attempt {}/{})",
            self.backend_label(),
            spec.startup_mode.as_str(),
            self.session_id(),
            attempt_number,
            MAX_PIPELINE_START_ATTEMPTS
        );
        debug!(
            "[Pipeline:{}]: Command: {}",
            self.backend_label(),
            spec.process.command_line()
        );

        let mut supervisor = PipelineSupervisor::new(self.session_id(), supervisor_timeouts());
        supervisor.add_stage(spec.process.spawn_stage()?.into_handle());

        Ok(PreparedLaunch {
            supervisor,
            startup_mode: spec.startup_mode,
        })
    }

    /// The single-process executor reading an FFmpeg stream-copy of the
    /// source on stdin: source remuxer → executor (inline HLS).
    async fn launch_remuxed_single_process(
        &self,
        attempt_number: u32,
    ) -> io::Result<PreparedLaunch> {
        let contexts = RemuxedIngestContexts::new(&self.base_context());
        let remuxer = FfmpegSourceRemuxer::new(&self.launch.backend.ffmpeg_path)
            .build_normalizer_spec(&contexts.remuxer)?;
        let executor_spec = self
            .executor
            .build_executor_spec(&contexts.executor, attempt_number)?;
        let startup_mode = executor_spec.startup_mode;
        let specs = [remuxer, executor_spec.process];

        info!(
            "[Pipeline:{}]: Launching {} remuxed-ingest path for session {} (attempt {}/{})",
            self.backend_label(),
            startup_mode.as_str(),
            self.session_id(),
            attempt_number,
            MAX_PIPELINE_START_ATTEMPTS
        );
        for spec in &specs {
            debug!(
                "[Pipeline:{}:{}]: Command: {}",
                self.backend_label(),
                spec.stage,
                spec.command_line()
            );
        }

        let supervisor = spawn_connected_stages(self.session_id(), &specs).await?;
        Ok(PreparedLaunch {
            supervisor,
            startup_mode,
        })
    }

    /// Normalizer → [preprocess] → executor → packager, bridged over pipes.
    async fn launch_staged(&self, attempt_number: u32) -> io::Result<PreparedLaunch> {
        let launch = &self.launch;
        let ffmpeg_path = &launch.backend.ffmpeg_path;
        let contexts = StagedPipelineContexts::new(&self.base_context(), self.preprocess_stage);
        let pixel_format = staged_pixel_format(self.backend_label());
        let uses_rife = self.uses_rife();

        let normalizer = FfmpegNormalizer::new(ffmpeg_path)
            .with_pixel_format(pixel_format)
            .with_output_resolution(parse_resolution(&launch.output_resolution))
            .with_denoise(self.denoise)
            .with_stream_offsets_preserved(uses_rife);

        let mut specs = vec![normalizer.build_normalizer_spec(&contexts.normalizer)?];
        if let Some(context) = &contexts.preprocess {
            let streaming_rife = self.streaming_rife.as_ref().filter(|_| uses_rife);
            specs.push(build_shared_preprocess_spec(
                ffmpeg_path,
                streaming_rife,
                pixel_format,
                context,
            )?);
        }
        let executor_spec = self
            .executor
            .build_executor_spec(&contexts.executor, attempt_number)?;
        let startup_mode = executor_spec.startup_mode;
        specs.push(executor_spec.process);
        specs.push(
            FfmpegHlsPackager::new(ffmpeg_path, launch.profile.hls_time)
                .build_packager_spec(&contexts.packager)?,
        );

        info!(
            "[Pipeline:{}]: Launching {} staged path for session {} (attempt {}/{})",
            self.backend_label(),
            startup_mode.as_str(),
            self.session_id(),
            attempt_number,
            MAX_PIPELINE_START_ATTEMPTS
        );
        for spec in &specs {
            debug!(
                "[Pipeline:{}:{}]: Command: {}",
                self.backend_label(),
                spec.stage,
                spec.command_line()
            );
        }

        let supervisor = spawn_connected_stages(self.session_id(), &specs).await?;
        Ok(PreparedLaunch {
            supervisor,
            startup_mode,
        })
    }
}

/// Spawns `specs` in order and bridges each stage's stdout into the next
/// stage's stdin. On any failure, already-spawned stages are stopped in LIFO
/// order.
async fn spawn_connected_stages(
    session_id: &str,
    specs: &[ProcessSpec],
) -> io::Result<PipelineSupervisor> {
    let mut stage_spawns = Vec::with_capacity(specs.len());
    for spec in specs {
        match spec.spawn_stage() {
            Ok(spawn) => stage_spawns.push(spawn),
            Err(error) => {
                stop_spawned_stages(stage_spawns).await;
                return Err(error);
            }
        }
    }

    let mut supervisor = PipelineSupervisor::new(session_id, supervisor_timeouts());
    for index in 1..stage_spawns.len() {
        let (upstream, downstream) = stage_spawns.split_at_mut(index);
        if let Err(error) =
            supervisor.connect_pipe_stages(&mut upstream[index - 1], &mut downstream[0])
        {
            stop_spawned_stages(stage_spawns).await;
            return Err(error);
        }
    }
    for spawn in stage_spawns {
        supervisor.add_stage_spawn(spawn);
    }
    Ok(supervisor)
}

async fn stop_spawned_stages(mut stage_spawns: Vec<PipelineStageSpawn>) {
    while let Some(spawn) = stage_spawns.pop() {
        spawn.into_handle().stop_now().await;
    }
}

fn supervisor_timeouts() -> PipelineSupervisorTimeouts {
    PipelineSupervisorTimeouts {
        inactivity_timeout: std::time::Duration::from_millis(HEARTBEAT_TIMEOUT_MS),
        orphan_timeout: std::time::Duration::from_millis(ORPHAN_SESSION_TIMEOUT_MS),
    }
}

/// Whether to try the staged (multi-process) launch.
///
/// - HLS sources stage only through the local input relay.
/// - `NvidiaSpecialized` stages only for RIFE. Otherwise direct NVEncC source
///   pull stays single-process: the staged handoff used to cause timeline
///   jumps and A/V drift. The RIFE path avoids that by keeping the source's
///   A/V offset in the normalizer and starting both of the worker's output
///   streams at 0.
/// - `Universal` and `AmdSpecialized` stage whenever the plan has
///   preprocessor-owned work (RIFE or `minterpolate`), which also makes them
///   reachable for non-HLS inputs.
/// - `Cpu` never stages.
pub(super) fn should_attempt_staged_launch(
    source_descriptor: &SourceDescriptor,
    execution_plan: &ExecutionPlan,
) -> bool {
    if source_descriptor.classification.is_hls_like() && source_descriptor.relay.is_none() {
        return false;
    }

    let requires_preprocess_stage = || {
        execution_plan
            .to_intermediate_plan()
            .shared_preprocess_plan()
            .requires_stage()
    };
    match execution_plan.executor {
        ExecutorKind::NvidiaSpecialized => {
            execution_plan.uses_rife() && requires_preprocess_stage()
        }
        ExecutorKind::Universal | ExecutorKind::AmdSpecialized => requires_preprocess_stage(),
        ExecutorKind::Cpu => false,
    }
}

/// Whether a single-process launch feeds the executor an FFmpeg stream-copy
/// of the source instead of letting it pull the source.
///
/// Only NVEncC on an input-relayed HLS source. NVEncC's `--avsync forcecfr`
/// fills a forward timestamp jump with repeats of the last frame, up to 5
/// minutes' worth. Live HLS sources jump when they skip expired segments or
/// restart their timestamps, so the picture froze for as long as the gap,
/// and encoding those repeats put the reader further behind the source's
/// playlist window, which caused the next skip. The remuxer cuts such jumps
/// out of the timeline. VCEEncC shares the forcecfr behavior but has not been
/// validated reading MPEG-TS from stdin.
pub(super) fn should_remux_source_ingest(
    source_descriptor: &SourceDescriptor,
    execution_plan: &ExecutionPlan,
) -> bool {
    source_descriptor.classification.is_hls_like()
        && source_descriptor.relay.is_some()
        && select_runtime_executor_target(execution_plan.executor) == RuntimeExecutorTarget::Nvidia
}

/// Stage contexts for a remuxed single-process launch: the remuxer pulls the
/// source, and the executor reads its MPEG-TS and writes the session's HLS.
pub(super) struct RemuxedIngestContexts {
    pub(super) remuxer: StageRuntimeContext,
    pub(super) executor: StageRuntimeContext,
}

impl RemuxedIngestContexts {
    pub(super) fn new(base: &StageRuntimeContext) -> Self {
        Self {
            remuxer: base.with_transport(
                FrameTransport::SourcePull,
                FrameTransport::RemuxedSourcePipe,
            ),
            executor: base.with_transport(
                FrameTransport::RemuxedSourcePipe,
                base.transport.output.clone(),
            ),
        }
    }
}

/// Stage contexts for a staged launch. Every hop is a process pipe; only the
/// packager writes the session's HLS output.
pub(super) struct StagedPipelineContexts {
    pub(super) normalizer: StageRuntimeContext,
    pub(super) preprocess: Option<StageRuntimeContext>,
    pub(super) executor: StageRuntimeContext,
    pub(super) packager: StageRuntimeContext,
}

impl StagedPipelineContexts {
    /// Derives the stage contexts from a single-process context, whose output
    /// is the session's HLS target.
    pub(super) fn new(base: &StageRuntimeContext, with_preprocess: bool) -> Self {
        let pipe = || FrameTransport::StdoutPipe;
        Self {
            normalizer: base.with_transport(FrameTransport::SourcePull, pipe()),
            preprocess: with_preprocess.then(|| base.with_transport(pipe(), pipe())),
            executor: base.with_transport(pipe(), pipe()),
            packager: base.with_transport(pipe(), base.transport.output.clone()),
        }
    }
}

/// Builds the preprocess stage: `rife-worker` when the plan interpolates
/// with RIFE (`streaming_rife` is set) and the worker can be built for this
/// stream, otherwise FFmpeg libplacebo + `minterpolate`.
fn build_shared_preprocess_spec(
    ffmpeg_path: &Path,
    streaming_rife: Option<&StreamingRifeParams>,
    pixel_format: &str,
    context: &StageRuntimeContext,
) -> io::Result<ProcessSpec> {
    if let Some(params) = streaming_rife {
        let preprocessor = StreamingRifePreprocessor::new(
            &params.rife_worker_path,
            &params.ffmpeg_path,
            &params.model_path,
        )
        .with_pixel_format(pixel_format);
        match preprocessor.build_preprocess_spec(context) {
            Ok(spec) => {
                info!(
                    "[Pipeline:{}]: Using rife-worker preprocess backend (worker={}, model={}).",
                    context.session_id,
                    params.rife_worker_path.display(),
                    params.model_path.display(),
                );
                return Ok(spec);
            }
            Err(error) => {
                warn!(
                    "[Pipeline:{}]: rife-worker preprocess could not be staged: {}. Falling back to FFmpeg minterpolate.",
                    context.session_id, error
                );
            }
        }
    }

    FfmpegPreprocessor::new(ffmpeg_path).build_preprocess_spec(context)
}

pub(super) struct RuntimeExecutorBuildContext<'a> {
    pub(super) intermediate_plan: &'a IntermediateExecutionPlan,
    pub(super) backend: &'a RuntimeBackend,
    pub(super) profile: &'a EncodingProfile,
    pub(super) denoise: bool,
}

/// Builds the runtime executor for the planned executor kind (see
/// `exec::select_runtime_executor_target`).
pub(super) fn build_runtime_executor(
    context: RuntimeExecutorBuildContext<'_>,
) -> Box<dyn BackendExecutor> {
    let backend = context.backend;
    match select_runtime_executor_target(context.intermediate_plan.executor) {
        // NVEncC / Rigaya for NGX-VSR, FRUC, NVIDIA Framegen, and TrueHDR.
        RuntimeExecutorTarget::Nvidia => Box::new(NvidiaSpecializedExecutor::new(
            NvidiaSpecializedExecutorContext {
                profile: context.profile.clone(),
                encoder_path: backend.encoder_path.clone(),
                intermediate_plan: context.intermediate_plan.clone(),
                denoise: context.denoise,
                cas_strength: CAS_DEFAULT_STRENGTH,
            },
        )),
        // VCEEncC / Rigaya for AMF FSR, AMF FRC, and libplacebo HDR.
        RuntimeExecutorTarget::Amd => Box::new(AmdExecutor::new(AmdExecutorContext {
            profile: context.profile.clone(),
            encoder_path: backend.encoder_path.clone(),
            encoder_backend: backend.encoder_backend.clone(),
            intermediate_plan: context.intermediate_plan.clone(),
            amf_fsr11: detect_vceenc_amf_fsr11(&backend.encoder_path),
            denoise: context.denoise,
            cas_strength: CAS_DEFAULT_STRENGTH,
        })),
        RuntimeExecutorTarget::Universal => {
            Box::new(UniversalExecutor::new(universal_executor_context(&context)))
        }
    }
}

/// The Universal FFmpeg executor's context. Shaders are looked up next to the
/// FFmpeg binary, under `shaders/anime4k` and `shaders/artcnn`.
fn universal_executor_context(
    context: &RuntimeExecutorBuildContext<'_>,
) -> UniversalExecutorContext {
    let ffmpeg_path = &context.backend.ffmpeg_path;
    let shaders_dir = |name: &str| ffmpeg_path.parent().map(|p| p.join("shaders").join(name));
    UniversalExecutorContext {
        profile: context.profile.clone(),
        ffmpeg_program: ffmpeg_path.clone(),
        intermediate_plan: context.intermediate_plan.clone(),
        encode_backend: select_universal_encode_backend(
            RuntimePlatform::current(),
            &context.backend.gpu_vendor,
        ),
        vaapi_device: None,
        anime4k_shaders_dir: shaders_dir("anime4k"),
        artcnn_shaders_dir: shaders_dir("artcnn"),
        denoise: context.denoise,
        cas_strength: CAS_DEFAULT_STRENGTH,
    }
}
