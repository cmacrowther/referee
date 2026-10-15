use crate::graph::ExecutorKind;

/// The concrete executor implementation selected for a pipeline run.
///
/// This is a *runtime* concept, distinct from [`ExecutorKind`], which is a
/// *graph-planning* concept: the planner emits `ExecutorKind` for the desired
/// execution strategy, and this names the implementation that renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeExecutorTarget {
    /// NVEncC with NGX-VSR, FRUC, NVIDIA Framegen, and TrueHDR.
    Nvidia,
    /// VCEEncC with AMF FSR, AMF FRC, and libplacebo HDR; Universal FFmpeg on
    /// retries.
    Amd,
    /// FFmpeg + libplacebo with the platform's encoder.
    Universal,
}

/// Selects the runtime executor for a planned executor kind. `Cpu` is never
/// produced by executor selection; it renders through the Universal executor,
/// whose encoder falls back to libx265 when no GPU vendor matches.
pub(crate) fn select_runtime_executor_target(executor_kind: ExecutorKind) -> RuntimeExecutorTarget {
    match executor_kind {
        ExecutorKind::NvidiaSpecialized => RuntimeExecutorTarget::Nvidia,
        ExecutorKind::AmdSpecialized => RuntimeExecutorTarget::Amd,
        ExecutorKind::Universal | ExecutorKind::Cpu => RuntimeExecutorTarget::Universal,
    }
}

#[cfg(test)]
mod tests {
    use super::{select_runtime_executor_target, RuntimeExecutorTarget};
    use crate::graph::ExecutorKind;

    #[test]
    fn specialized_plans_route_to_their_vendor_executor() {
        assert_eq!(
            select_runtime_executor_target(ExecutorKind::NvidiaSpecialized),
            RuntimeExecutorTarget::Nvidia
        );
        assert_eq!(
            select_runtime_executor_target(ExecutorKind::AmdSpecialized),
            RuntimeExecutorTarget::Amd
        );
    }

    #[test]
    fn universal_and_cpu_plans_render_through_ffmpeg_on_any_vendor() {
        assert_eq!(
            select_runtime_executor_target(ExecutorKind::Universal),
            RuntimeExecutorTarget::Universal
        );
        assert_eq!(
            select_runtime_executor_target(ExecutorKind::Cpu),
            RuntimeExecutorTarget::Universal
        );
    }
}
