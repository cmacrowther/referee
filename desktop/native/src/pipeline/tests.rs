use super::capabilities::{
    parse_encoder_backend_capabilities, select_executor_kind_for_request, EncoderCapabilities,
    ExecutorPreference, ExecutorSelectionContext,
};
use super::launch::{
    build_runtime_executor, should_attempt_staged_launch, should_remux_source_ingest,
    RemuxedIngestContexts, RuntimeBackend, RuntimeExecutorBuildContext, StagedPipelineContexts,
};
use super::profile::{EncodingProfile, MAX_VSR_QUALITY};
use super::run::{is_ingest_failure, is_retryable_failure};
use super::session::{
    cleanup_session, new_session_map, PipelineCompletionSignal, Session, SessionInfo,
};
use crate::exec::RuntimePlatform;
use crate::graph::{
    BackendFamily, EncoderBinaryAvailability, ExecutionPlan, ExecutorKind, FeatureAvailability,
    GraphPlanner, HdrRequest, HdrSupport, HdrTransformKind, InterpolationDecision,
    InterpolationPlan, InterpolationRealization, InterpolationRequest, InterpolationSupport,
    LatencyMode, NativeInterpolationEngine, NativeUpscaleBackend, OpExecutionStage,
    PipelineRequest, ResizePlan, ResizeSupport, SelectedUpscalePath, UpscaleRequest, VideoOp,
};
use crate::runtime::FrameTransport;
use crate::runtime::StageRuntimeContext;
use crate::settings::RelayPeerMetadata;
use crate::source::{
    RelayedSource, SourceClassification, SourceContentKind, SourceDescriptor, SourceKind,
    SourceTransport,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

const FFMPEG_PROGRAM: &str = "ffmpeg";

fn runtime_backend(gpu_vendor: &str, encoder_backend: &str, encoder_path: &str) -> RuntimeBackend {
    RuntimeBackend {
        gpu_vendor: gpu_vendor.to_string(),
        encoder_backend: encoder_backend.to_string(),
        encoder_path: PathBuf::from(encoder_path),
        ffmpeg_path: PathBuf::from(FFMPEG_PROGRAM),
        rife_worker_path: None,
        rife_model_path: None,
    }
}

fn pipeline_request(
    upscale: UpscaleRequest,
    interpolation: InterpolationRequest,
    hdr: HdrRequest,
) -> PipelineRequest {
    PipelineRequest {
        source_transport: SourceTransport::RemoteHttp,
        source_kind: SourceKind::Hls,
        source_content_kind: SourceContentKind::Unknown,
        source_resolution: Some("1920x1080".to_string()),
        output_resolution: "3840x2160".to_string(),
        source_fps: Some(30.0),
        latency_mode: LatencyMode::Low,
        upscale,
        interpolation,
        hdr,
    }
}

fn unique_session_dir(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("referee-{}-{}", test_name, uuid::Uuid::new_v4()))
}

fn cleanup_test_session(
    session_dir: PathBuf,
    completion: PipelineCompletionSignal,
) -> (Session, mpsc::Receiver<()>) {
    let (heartbeat_tx, heartbeat_rx) = mpsc::channel(1);
    (
        Session::new_local(
            SessionInfo {
                id: "session-1".to_string(),
                source_url: "https://example.com/live/master.m3u8".to_string(),
                output_url: "http://127.0.0.1:14002/v1/tmp/session-1/index.m3u8".to_string(),
                app_name: None,
                stream_title: None,
                source_content_kind: Some(SourceContentKind::Animated),
                upscaler: Some("Anime4K2x".to_string()),
                source_resolution: Some("1920x1080".to_string()),
                output_resolution: "3840x2160".to_string(),
                source_fps: Some(30.0),
                target_fps: Some(30.0),
                framegen_enabled: false,
                framegen_engine: None,
                hdr_enabled: false,
                quality_level: 3,
                executor: ExecutorKind::Universal,
                encoder_backend: Some("universal".to_string()),
                startup_complete: true,
                retrying_startup: false,
                startup_stage: "ready".to_string(),
            },
            HashMap::new(),
            heartbeat_tx,
            session_dir,
            completion,
            1,
            "low-latency",
        ),
        heartbeat_rx,
    )
}

fn remote_cleanup_test_session() -> Session {
    Session::new_remote(
        SessionInfo {
            id: "session-1".to_string(),
            source_url: "https://example.com/live/master.m3u8".to_string(),
            output_url: "http://127.0.0.1:14002/v1/tmp/session-1/index.m3u8".to_string(),
            app_name: None,
            stream_title: None,
            source_content_kind: Some(SourceContentKind::Animated),
            upscaler: Some("Anime4K2x".to_string()),
            source_resolution: Some("1920x1080".to_string()),
            output_resolution: "3840x2160".to_string(),
            source_fps: Some(30.0),
            target_fps: Some(30.0),
            framegen_enabled: false,
            framegen_engine: None,
            hdr_enabled: false,
            quality_level: 3,
            executor: ExecutorKind::Universal,
            encoder_backend: Some("universal".to_string()),
            startup_complete: true,
            retrying_startup: false,
            startup_stage: "ready".to_string(),
        },
        "http://192.168.1.25:14002".to_string(),
        "remote-session-1".to_string(),
        "relay-secret".to_string(),
        RelayPeerMetadata {
            instance_id: Some("peer-1".to_string()),
            hostname: Some("media-box".to_string()),
            ip: Some("192.168.1.25".to_string()),
            version: Some("1.2.3".to_string()),
            platform: Some("linux".to_string()),
            gpu_ready: Some(true),
            gpu_vendor: Some("nvidia".to_string()),
            gpu_name: Some("RTX 4080".to_string()),
        },
        tokio::sync::mpsc::channel(1).0,
    )
}

#[tokio::test]
async fn cleanup_session_drops_heartbeat_and_waits_for_completion() {
    let sessions = new_session_map();
    let session_dir = unique_session_dir("cleanup-waits");
    tokio::fs::create_dir_all(&session_dir).await.unwrap();
    let completion = PipelineCompletionSignal::new();
    let (session, mut heartbeat_rx) = cleanup_test_session(session_dir.clone(), completion.clone());
    sessions.insert("session-1".to_string(), session);

    let cleanup = tokio::spawn({
        let sessions = sessions.clone();
        async move {
            cleanup_session("session-1", &sessions).await;
        }
    });

    assert!(heartbeat_rx.recv().await.is_none());
    assert!(!sessions.contains_key("session-1"));
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert!(!cleanup.is_finished());

    completion.mark_complete();
    tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn cleanup_session_returns_when_completion_was_already_marked() {
    let sessions = new_session_map();
    let session_dir = unique_session_dir("cleanup-already-complete");
    tokio::fs::create_dir_all(&session_dir).await.unwrap();
    let completion = PipelineCompletionSignal::new();
    completion.mark_complete();
    let (session, _heartbeat_rx) = cleanup_test_session(session_dir.clone(), completion);
    sessions.insert("session-1".to_string(), session);

    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        cleanup_session("session-1", &sessions),
    )
    .await
    .unwrap();

    assert!(!sessions.contains_key("session-1"));
    assert!(tokio::fs::metadata(&session_dir).await.is_err());
}

#[tokio::test]
async fn cleanup_session_removes_temp_dir_only_after_completion() {
    let sessions = new_session_map();
    let session_dir = unique_session_dir("cleanup-temp-dir");
    tokio::fs::create_dir_all(&session_dir).await.unwrap();
    tokio::fs::write(session_dir.join("index.m3u8"), "#EXTM3U")
        .await
        .unwrap();
    let completion = PipelineCompletionSignal::new();
    let (session, mut heartbeat_rx) = cleanup_test_session(session_dir.clone(), completion.clone());
    sessions.insert("session-1".to_string(), session);

    let cleanup = tokio::spawn({
        let sessions = sessions.clone();
        async move {
            cleanup_session("session-1", &sessions).await;
        }
    });

    assert!(heartbeat_rx.recv().await.is_none());
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert!(tokio::fs::metadata(&session_dir).await.is_ok());

    completion.mark_complete();
    tokio::time::timeout(std::time::Duration::from_secs(1), cleanup)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::fs::metadata(&session_dir).await.is_err());
}

#[tokio::test]
async fn cleanup_session_for_remote_backed_session_returns_without_waiting_for_local_runtime() {
    let sessions = new_session_map();
    sessions.insert("session-1".to_string(), remote_cleanup_test_session());

    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        cleanup_session("session-1", &sessions),
    )
    .await
    .unwrap();

    assert!(!sessions.contains_key("session-1"));
}

#[test]
fn executor_selection_uses_specialized_only_for_windows_nvidia_with_matching_feature_request() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::Off,
        HdrRequest::Off,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::NvidiaSpecialized
    );
}

#[test]
fn executor_selection_uses_specialized_for_windows_nvidia_hdr_or_framegen_benefits() {
    let caps = EncoderCapabilities {
        has_vpp_resize: false,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let framegen_request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::TargetFps(60),
        HdrRequest::Off,
    );
    let hdr_request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::Off,
        HdrRequest::TonemapToHdr10,
    );

    for request in [&framegen_request, &hdr_request] {
        assert_eq!(
            select_executor_kind_for_request(ExecutorSelectionContext {
                platform: RuntimePlatform::Windows,
                gpu_vendor: "nvidia",
                request,
                encoder_capabilities: &caps,
                executor_preference: ExecutorPreference::Auto,
            }),
            ExecutorKind::NvidiaSpecialized
        );
    }
}

#[test]
fn executor_selection_routes_non_beneficial_windows_nvidia_to_universal() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::Off,
        HdrRequest::Off,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::Universal
    );
}

#[test]
fn executor_selection_nvidia_ai_preference_forces_specialized_when_available() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::Off,
        HdrRequest::Off,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::NvidiaAi,
        }),
        ExecutorKind::NvidiaSpecialized
    );
}

#[test]
fn executor_selection_nvidia_ai_preference_falls_back_when_unavailable() {
    let caps = EncoderCapabilities {
        has_vpp_resize: false,
        has_fruc: false,
        has_truehdr: false,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::Off,
        HdrRequest::Off,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::NvidiaAi,
        }),
        ExecutorKind::Universal
    );
    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Linux,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &EncoderCapabilities {
                has_vpp_resize: true,
                has_fruc: true,
                has_truehdr: true,
                has_rife: false,
                has_native_double: false,
                has_hdr_tonemap: false,
            },
            executor_preference: ExecutorPreference::NvidiaAi,
        }),
        ExecutorKind::Universal
    );
}

#[test]
fn executor_selection_requires_requested_feature_to_match_specialized_capability() {
    let caps = EncoderCapabilities {
        has_vpp_resize: false,
        has_fruc: false,
        has_truehdr: false,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::TargetFps(60),
        HdrRequest::TonemapToHdr10,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::Universal
    );
}

#[test]
fn executor_selection_routes_linux_nvidia_to_universal_and_amd_with_vceenc_to_amd_specialized() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::TargetFps(60),
        HdrRequest::TonemapToHdr10,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Linux,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::Universal
    );
    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Linux,
            gpu_vendor: "amd",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::AmdSpecialized
    );
    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "amd",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Auto,
        }),
        ExecutorKind::AmdSpecialized
    );
    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "amd",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Universal,
        }),
        ExecutorKind::Universal
    );
    // Without VCEEncC (FFmpeg AMF encode only) AMD stays on Universal.
    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "amd",
            request: &request,
            encoder_capabilities: &EncoderCapabilities {
                has_vpp_resize: false,
                ..caps
            },
            executor_preference: ExecutorPreference::AmdAi,
        }),
        ExecutorKind::Universal
    );
}

#[test]
fn executor_selection_universal_preference_overrides_specialized_benefits() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::TargetFps(60),
        HdrRequest::TonemapToHdr10,
    );

    assert_eq!(
        select_executor_kind_for_request(ExecutorSelectionContext {
            platform: RuntimePlatform::Windows,
            gpu_vendor: "nvidia",
            request: &request,
            encoder_capabilities: &caps,
            executor_preference: ExecutorPreference::Universal,
        }),
        ExecutorKind::Universal
    );
}

#[test]
fn windows_nvidia_capabilities_translate_to_specialized_when_requested_feature_matches() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::TargetFps(60),
        HdrRequest::Off,
    );

    let translated = caps.to_backend_capabilities_for_request(
        RuntimePlatform::Windows,
        "nvidia",
        "nvenc",
        &request,
        ExecutorPreference::Auto,
    );

    assert_eq!(translated.executor, ExecutorKind::NvidiaSpecialized);
    assert_eq!(
        translated.resize,
        ResizeSupport::QualityRange {
            min_quality: 1,
            max_quality: MAX_VSR_QUALITY,
        }
    );
    assert_eq!(translated.interpolation, InterpolationSupport::Native);
    assert_eq!(
        translated.hdr,
        HdrSupport {
            passthrough_10_bit: false,
            tonemap_to_hdr10: true,
            inject_hdr10_metadata: false,
        }
    );
    assert_eq!(
        translated.upscale.selected_path,
        SelectedUpscalePath::NativeBackend
    );
    assert_eq!(
        translated.upscale.preferred_native_backend,
        Some(NativeUpscaleBackend::NvidiaNgxVsr)
    );
}

#[test]
fn linux_nvidia_capabilities_translate_to_universal_without_claiming_specialized_parity() {
    let caps = EncoderCapabilities {
        has_vpp_resize: false,
        has_fruc: true,
        has_truehdr: true,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Off,
        InterpolationRequest::TargetFps(60),
        HdrRequest::TonemapToHdr10,
    );

    let translated = caps.to_backend_capabilities_for_request(
        RuntimePlatform::Linux,
        "nvidia",
        "ffmpeg",
        &request,
        ExecutorPreference::Auto,
    );

    assert_eq!(translated.executor, ExecutorKind::Universal);
    assert_eq!(
        translated.resize,
        ResizeSupport::QualityRange {
            min_quality: 1,
            max_quality: MAX_VSR_QUALITY,
        }
    );
    assert_eq!(translated.interpolation, InterpolationSupport::Unsupported);
    assert_eq!(
        translated.hdr,
        HdrSupport {
            passthrough_10_bit: true,
            tonemap_to_hdr10: true,
            inject_hdr10_metadata: true,
        }
    );
    assert_eq!(
        translated.upscale.selected_path,
        SelectedUpscalePath::TemporaryUniversalCompatibility
    );
    assert_eq!(translated.upscale.preferred_native_backend, None);
}

/// VCEEncC with only `--vpp-resize` selects the AMD executor with AMF FSR as
/// its native upscaler; frame generation and HDR fall to portable stages.
#[test]
fn windows_amd_vceenc_with_vpp_resize_selects_amd_specialized_native_upscale() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: false,
        has_truehdr: false,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::Off,
        HdrRequest::Off,
    );

    let translated = caps.to_backend_capabilities_for_request(
        RuntimePlatform::Windows,
        "amd",
        "vceenc",
        &request,
        ExecutorPreference::Auto,
    );

    assert_eq!(translated.executor, ExecutorKind::AmdSpecialized);
    assert_eq!(
        translated.resize,
        ResizeSupport::QualityRange {
            min_quality: 1,
            max_quality: MAX_VSR_QUALITY,
        }
    );
    assert_eq!(translated.interpolation, InterpolationSupport::Unsupported);
    assert_eq!(translated.hdr, HdrSupport::default());
    assert_eq!(
        translated.upscale.selected_path,
        SelectedUpscalePath::NativeBackend
    );
    assert_eq!(
        translated.upscale.preferred_native_backend,
        Some(NativeUpscaleBackend::AmdVceEncResize)
    );
}

/// Constructs a `SourceDescriptor` representing an HLS source that is accessed via a local HTTP relay.
///
/// The descriptor contains both the original source URL and the relay runtime URL, and includes a
/// populated `relay` field with the same information.
///
/// # Examples
///
fn relayed_source_descriptor() -> SourceDescriptor {
    let relay_url =
        "http://127.0.0.1:14002/v1/input/session-1?url=https%3A%2F%2Fexample.com%2Flive%2Fmaster.m3u8";

    SourceDescriptor {
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
    }
}

fn encoding_profile() -> EncodingProfile {
    EncodingProfile {
        bitrate: 25_000,
        max_bitrate: 37_500,
        preset: "p4".to_string(),
        lookahead: 8,
        bframes: 3,
        hls_time: 1,
    }
}

fn executor_plan(executor: ExecutorKind) -> ExecutionPlan {
    ExecutionPlan {
        executor,
        latency_mode: LatencyMode::Low,
        requires_local_hls_relay: true,
        video_ops: vec![VideoOp::NormalizeInput],
    }
}

#[test]
fn generic_capability_parser_only_claims_backend_neutral_features() {
    let help = r#"
VCEEnc (x64) 8.25
   --vpp-resize <string>
   --vpp-fruc [<param1>=<value>]
   --vpp-ngx-truehdr
"#;

    let caps = EncoderCapabilities::from_backend_capability_profile(
        &parse_encoder_backend_capabilities(help, "vceenc"),
    );

    assert!(caps.has_vpp_resize);
    assert!(!caps.has_fruc);
    assert!(!caps.has_truehdr);
}

#[test]
fn backend_capability_parser_distinguishes_nvidia_amd_and_universal_families() {
    let nvenc = parse_encoder_backend_capabilities(
        r#"
Features
ngx: yes
nvof fruc: yes
   --vpp-resize <string>
   --vpp-ngx-truehdr
"#,
        "nvenc",
    );
    let vceenc = parse_encoder_backend_capabilities(
        r#"
VCEEnc (x64) 8.25
   --vpp-resize <string>
"#,
        "vceenc",
    );
    let universal = parse_encoder_backend_capabilities("", "ffmpeg");

    assert_eq!(nvenc.family, BackendFamily::Nvidia);
    assert_eq!(
        nvenc.binary_availability,
        EncoderBinaryAvailability::Available
    );
    assert_eq!(nvenc.resize, FeatureAvailability::Exact);
    assert_eq!(nvenc.interpolation, FeatureAvailability::Exact);
    assert_eq!(nvenc.hdr_transform, FeatureAvailability::Exact);
    assert_eq!(nvenc.metadata_injection, FeatureAvailability::Unavailable);

    assert_eq!(vceenc.family, BackendFamily::Amd);
    assert_eq!(vceenc.resize, FeatureAvailability::Approximate);
    assert_eq!(vceenc.interpolation, FeatureAvailability::Unavailable);
    assert_eq!(vceenc.hdr_transform, FeatureAvailability::Unavailable);

    assert_eq!(universal.family, BackendFamily::Universal);
    assert_eq!(universal.resize, FeatureAvailability::Exact);
    assert_eq!(universal.interpolation, FeatureAvailability::Unavailable);
    assert_eq!(universal.hdr_transform, FeatureAvailability::Exact);
    assert_eq!(universal.metadata_injection, FeatureAvailability::Exact);
}

/// VCEEncC with AMF FRC and libplacebo tonemapping matches the NVIDIA
/// executor's native coverage: FSR upscale, 2x frame doubling, and HDR10
/// tonemapping with executor-injected metadata.
#[test]
fn windows_amd_vceenc_with_frc_and_tonemapping_plans_native_double_and_hdr() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: false,
        has_truehdr: false,
        has_rife: false,
        has_native_double: true,
        has_hdr_tonemap: true,
    };
    let request = PipelineRequest {
        source_fps: Some(30.0),
        ..pipeline_request(
            UpscaleRequest::Quality(3),
            InterpolationRequest::NativeDouble,
            HdrRequest::TonemapToHdr10,
        )
    };

    let translated = caps.to_backend_capabilities_for_request(
        RuntimePlatform::Windows,
        "amd",
        "vceenc",
        &request,
        ExecutorPreference::Auto,
    );
    assert_eq!(translated.executor, ExecutorKind::AmdSpecialized);
    assert_eq!(translated.interpolation, InterpolationSupport::FramegenOnly);
    assert_eq!(
        translated.hdr,
        HdrSupport {
            passthrough_10_bit: false,
            tonemap_to_hdr10: true,
            inject_hdr10_metadata: true,
        }
    );

    let plan = GraphPlanner::new().plan(&request, &translated);
    let interpolation = plan
        .video_ops
        .iter()
        .find_map(|op| match op {
            VideoOp::Interpolate(plan) => Some(plan),
            _ => None,
        })
        .expect("interpolation op");
    assert_eq!(interpolation.target_fps, 60);
    assert_eq!(
        interpolation.decision.realization,
        InterpolationRealization::NativeBackend(NativeInterpolationEngine::AmdFrc)
    );
    let hdr = plan
        .video_ops
        .iter()
        .find_map(|op| match op {
            VideoOp::Hdr(plan) => Some(plan),
            _ => None,
        })
        .expect("hdr op");
    let transform = hdr.transform.as_ref().expect("hdr transform");
    assert_eq!(transform.kind, HdrTransformKind::AmdTonemapToHdr10);
    assert_eq!(transform.stage, OpExecutionStage::Executor);
    assert_eq!(
        hdr.metadata.as_ref().map(|metadata| metadata.stage),
        Some(OpExecutionStage::Executor)
    );

    // FRC only doubles: fixed targets go to the portable interpolators.
    let target_request = PipelineRequest {
        interpolation: InterpolationRequest::TargetFps(60),
        ..request.clone()
    };
    let target_plan = GraphPlanner::new().plan(&target_request, &translated);
    assert!(target_plan.video_ops.iter().any(|op| matches!(
        op,
        VideoOp::Interpolate(plan) if !matches!(
            plan.decision.realization,
            InterpolationRealization::NativeBackend(_)
        )
    )));
}

/// The Universal preference on an AMD GPU keeps VCEEncC as a separate
/// vendor-native candidate; Auto selects it.
#[test]
fn backend_capability_inventory_selects_vceenc_for_amd_and_keeps_it_as_candidate_under_universal() {
    let caps = EncoderCapabilities {
        has_vpp_resize: true,
        has_fruc: false,
        has_truehdr: false,
        has_rife: false,
        has_native_double: false,
        has_hdr_tonemap: false,
    };
    let request = pipeline_request(
        UpscaleRequest::Quality(3),
        InterpolationRequest::Off,
        HdrRequest::Off,
    );
    let auto = caps.to_backend_capability_inventory_for_request(
        RuntimePlatform::Windows,
        "amd",
        "vceenc",
        &request,
        ExecutorPreference::Auto,
    );
    assert_eq!(auto.selected_executor, ExecutorKind::AmdSpecialized);
    assert_eq!(auto.selected_backend.family, BackendFamily::Amd);
    assert_eq!(auto.vendor_native_backend, None);

    let inventory = caps.to_backend_capability_inventory_for_request(
        RuntimePlatform::Windows,
        "amd",
        "vceenc",
        &request,
        ExecutorPreference::Universal,
    );

    assert_eq!(inventory.selected_executor, ExecutorKind::Universal);
    assert_eq!(inventory.selected_backend.family, BackendFamily::Universal);
    assert_eq!(
        inventory.selected_backend.resize,
        FeatureAvailability::Exact
    );

    let vendor_native = inventory
        .vendor_native_backend
        .expect("amd native backend candidate");
    assert_eq!(vendor_native.family, BackendFamily::Amd);
    assert_eq!(vendor_native.binary_name, "VCEEncC");
    assert_eq!(
        vendor_native.binary_availability,
        EncoderBinaryAvailability::Available
    );
    assert_eq!(vendor_native.resize, FeatureAvailability::Approximate);
    assert_eq!(
        vendor_native.interpolation,
        FeatureAvailability::Unavailable
    );
    assert_eq!(
        vendor_native.hdr_transform,
        FeatureAvailability::Unavailable
    );
}

#[test]
fn runtime_executor_factory_uses_specialized_nvenc_for_specialized_kind() {
    let plan = executor_plan(ExecutorKind::NvidiaSpecialized);
    let intermediate_plan = plan.to_intermediate_plan();
    let profile = encoding_profile();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        intermediate_plan: &intermediate_plan,
        backend: &runtime_backend("nvidia", "nvenc", "NVEncC64.exe"),
        profile: &profile,
        denoise: false,
    });
    let stage_context = StageRuntimeContext::single_process(
        "session-1",
        &plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );

    let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

    assert_eq!(spec.process.program, PathBuf::from("NVEncC64.exe"));
    assert_eq!(spec.process.log_label, "nvenc");
    assert!(spec.process.args.iter().any(|arg| arg == "--multipass"));
}

#[test]
fn runtime_executor_factory_uses_ffmpeg_universal_for_universal_kind() {
    let plan = executor_plan(ExecutorKind::Universal);
    let intermediate_plan = plan.to_intermediate_plan();
    let profile = encoding_profile();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        intermediate_plan: &intermediate_plan,
        backend: &runtime_backend("nvidia", "ffmpeg", "NVEncC64.exe"),
        profile: &profile,
        denoise: false,
    });
    let stage_context = StageRuntimeContext::single_process(
        "session-1",
        &plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );

    let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

    assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
    assert!(spec.process.log_label.starts_with("ffmpeg-universal-"));
    assert!(spec.process.args.iter().any(|arg| arg == "-hide_banner"));
    assert!(!spec.process.args.iter().any(|arg| arg == "--multipass"));
}

#[test]
fn runtime_executor_factory_uses_vceenc_for_amd_specialized_kind() {
    let plan = ExecutionPlan {
        executor: ExecutorKind::AmdSpecialized,
        latency_mode: LatencyMode::Low,
        requires_local_hls_relay: true,
        video_ops: vec![
            VideoOp::NormalizeInput,
            VideoOp::Resize(ResizePlan {
                target_resolution: "3840x2160".to_string(),
                quality: Some(2),
                stage: OpExecutionStage::Executor,
            }),
        ],
    };
    let intermediate_plan = plan.to_intermediate_plan();
    let profile = encoding_profile();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        intermediate_plan: &intermediate_plan,
        backend: &runtime_backend("amd", "vceenc", "VCEEncC64.exe"),
        profile: &profile,
        denoise: false,
    });
    let stage_context = StageRuntimeContext::single_process(
        "session-1",
        &plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );

    let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

    assert_eq!(spec.process.program, PathBuf::from("VCEEncC64.exe"));
    assert_eq!(spec.process.log_label, "vceenc");
    assert!(
        spec.process
            .args
            .windows(2)
            .any(|w| w == ["--output-res", "3840x2160"]),
        "output-res: {:?}",
        spec.process.args
    );
    assert!(
        spec.process
            .args
            .windows(2)
            .any(|w| w == ["--vpp-resize", "amf_fsr"]),
        "vpp-resize: {:?}",
        spec.process.args
    );
    assert!(!spec.process.args.iter().any(|arg| arg == "--vpp-fruc"));
    assert!(!spec
        .process
        .args
        .iter()
        .any(|arg| arg == "--vpp-ngx-truehdr"));
}

#[test]
fn runtime_executor_factory_keeps_amd_on_universal_ffmpeg_path() {
    let plan = executor_plan(ExecutorKind::Universal);
    let intermediate_plan = plan.to_intermediate_plan();
    let profile = encoding_profile();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        intermediate_plan: &intermediate_plan,
        backend: &runtime_backend("amd", "ffmpeg", FFMPEG_PROGRAM),
        profile: &profile,
        denoise: false,
    });
    let stage_context = StageRuntimeContext::single_process(
        "session-1",
        &plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );

    let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

    assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
    assert!(spec.process.log_label.starts_with("ffmpeg-universal-"));
    assert_ne!(spec.process.log_label, "nvenc");
    assert!(!spec.process.args.iter().any(|arg| arg == "--vpp-fruc"));
}

#[test]
fn runtime_executor_factory_renders_cpu_plans_through_universal_ffmpeg() {
    let plan = executor_plan(ExecutorKind::Cpu);
    let intermediate_plan = plan.to_intermediate_plan();
    let profile = encoding_profile();
    let executor = build_runtime_executor(RuntimeExecutorBuildContext {
        intermediate_plan: &intermediate_plan,
        backend: &runtime_backend("unknown", "ffmpeg", FFMPEG_PROGRAM),
        profile: &profile,
        denoise: false,
    });
    let stage_context = StageRuntimeContext::single_process(
        "session-1",
        &plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );

    let spec = executor.build_executor_spec(&stage_context, 1).unwrap();

    assert_eq!(spec.process.program, PathBuf::from(FFMPEG_PROGRAM));
    // No GPU vendor matched, so the Universal executor encodes on the CPU.
    assert_eq!(spec.process.log_label, "ffmpeg-universal-cpu");
    assert!(spec
        .process
        .args
        .windows(2)
        .any(|window| window == ["-c:v", "libx265"]));
    assert!(spec
        .process
        .args
        .windows(2)
        .any(|window| window == ["-f", "hls"]));
}

#[test]
fn staged_contexts_insert_optional_preprocess_between_normalizer_and_executor() {
    let execution_plan = ExecutionPlan {
        executor: ExecutorKind::Universal,
        latency_mode: LatencyMode::Balanced,
        requires_local_hls_relay: true,
        video_ops: vec![
            VideoOp::NormalizeInput,
            VideoOp::Resize(ResizePlan {
                target_resolution: "3840x2160".to_string(),
                quality: Some(2),
                stage: OpExecutionStage::Preprocess,
            }),
        ],
    };
    let base = StageRuntimeContext::single_process(
        "session-1",
        &execution_plan,
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );
    let contexts = StagedPipelineContexts::new(&base, true);
    assert!(StagedPipelineContexts::new(&base, false)
        .preprocess
        .is_none());

    let preprocess = contexts.preprocess.expect("preprocess context");
    assert_eq!(
        contexts.normalizer.transport.input,
        FrameTransport::SourcePull
    );
    assert_eq!(
        contexts.normalizer.transport.output,
        FrameTransport::StdoutPipe
    );
    assert_eq!(preprocess.transport.input, FrameTransport::StdoutPipe);
    assert_eq!(preprocess.transport.output, FrameTransport::StdoutPipe);
    assert_eq!(
        contexts.executor.transport.input,
        FrameTransport::StdoutPipe
    );
    assert_eq!(
        contexts.executor.transport.output,
        FrameTransport::StdoutPipe
    );
    // Only the packager writes the session's HLS output.
    assert_eq!(
        contexts.packager.transport.input,
        FrameTransport::StdoutPipe
    );
    assert_eq!(contexts.packager.transport.output, base.transport.output);
}

#[test]
fn staged_launch_gate_keeps_nvidia_on_legacy_path_and_enables_universal_when_preprocess_is_required(
) {
    let descriptor = relayed_source_descriptor();
    let nvidia_plan = ExecutionPlan {
        executor: ExecutorKind::NvidiaSpecialized,
        latency_mode: LatencyMode::Low,
        requires_local_hls_relay: true,
        video_ops: vec![VideoOp::NormalizeInput],
    };
    let universal_preprocess_plan = ExecutionPlan {
        executor: ExecutorKind::Universal,
        latency_mode: LatencyMode::Balanced,
        requires_local_hls_relay: true,
        video_ops: vec![
            VideoOp::NormalizeInput,
            VideoOp::Resize(ResizePlan {
                target_resolution: "2560x1440".to_string(),
                quality: Some(2),
                stage: OpExecutionStage::Preprocess,
            }),
        ],
    };
    let universal_legacy_plan = ExecutionPlan {
        executor: ExecutorKind::Universal,
        latency_mode: LatencyMode::Balanced,
        requires_local_hls_relay: true,
        video_ops: vec![VideoOp::NormalizeInput],
    };
    let no_relay_descriptor = SourceDescriptor {
        relay: None,
        ..descriptor.clone()
    };

    assert!(!should_attempt_staged_launch(&descriptor, &nvidia_plan));
    assert!(should_attempt_staged_launch(
        &descriptor,
        &universal_preprocess_plan
    ));
    assert!(!should_attempt_staged_launch(
        &descriptor,
        &universal_legacy_plan
    ));
    // HLS source without relay (edge case): must not stage regardless of
    // executor — the relay server is not running to serve segments.
    assert!(!should_attempt_staged_launch(
        &no_relay_descriptor,
        &nvidia_plan
    ));
    assert!(!should_attempt_staged_launch(
        &no_relay_descriptor,
        &universal_preprocess_plan
    ));
}

#[test]
fn staged_launch_gate_allows_universal_non_hls_sources_when_preprocess_is_required() {
    // A bounded/file/non-HLS source has no relay. Before this fix the relay
    // check blocked ALL non-relay sources from staged launch, making
    // external RIFE and minterpolate interpolation structurally unreachable
    // for local or direct-stream inputs.
    let non_hls_descriptor = SourceDescriptor {
        classification: SourceClassification {
            transport: SourceTransport::LocalPath,
            kind: SourceKind::Other,
        },
        original_url: "/media/video.mkv".to_string(),
        runtime_url: "/media/video.mkv".to_string(),
        runtime_headers: HashMap::new(),
        session_headers: HashMap::new(),
        relay: None,
        metadata: None,
    };

    // Non-HLS Universal + preprocess-owned interpolation: staged launch
    // must now be attempted so the preprocess stage can run RIFE/minterpolate.
    let universal_interpolation_plan = ExecutionPlan {
        executor: ExecutorKind::Universal,
        latency_mode: LatencyMode::Balanced,
        requires_local_hls_relay: false,
        video_ops: vec![
            VideoOp::NormalizeInput,
            VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision: InterpolationDecision::portable_fallback(OpExecutionStage::Preprocess),
            }),
        ],
    };
    // Non-HLS Universal + no preprocess: legacy path is fine, no staging needed.
    let universal_no_preprocess_plan = ExecutionPlan {
        executor: ExecutorKind::Universal,
        latency_mode: LatencyMode::Balanced,
        requires_local_hls_relay: false,
        video_ops: vec![VideoOp::NormalizeInput],
    };
    // Non-HLS NVIDIA: preserve existing behavior — staged only for HLS+relay.
    let nvidia_plan = ExecutionPlan {
        executor: ExecutorKind::NvidiaSpecialized,
        latency_mode: LatencyMode::Low,
        requires_local_hls_relay: false,
        video_ops: vec![VideoOp::NormalizeInput],
    };

    assert!(
        should_attempt_staged_launch(&non_hls_descriptor, &universal_interpolation_plan),
        "non-HLS Universal with preprocess-owned interpolation must now stage"
    );
    assert!(
        !should_attempt_staged_launch(&non_hls_descriptor, &universal_no_preprocess_plan),
        "non-HLS Universal without preprocess does not need staged launch"
    );
    assert!(
        !should_attempt_staged_launch(&non_hls_descriptor, &nvidia_plan),
        "non-HLS NVIDIA must keep existing legacy-only behavior (no relay)"
    );
}

#[test]
fn is_retryable_failure_recognizes_known_encoder_startup_errors() {
    assert!(is_retryable_failure(
        "Error: input video info not parsed yet"
    ));
    assert!(is_retryable_failure(
        "ERROR: Failed to initialize file reader"
    ));
    // AMF encoder hard-failure on Linux (RDNA 4 / RADV).
    assert!(is_retryable_failure(
        "\x1b[31mBreak in task AMFENC: unknown error..\x1b[39m"
    ));
    assert!(!is_retryable_failure("ERROR: out of memory"));
    assert!(!is_retryable_failure(""));
}

#[test]
fn staged_launch_gate_stages_nvidia_only_for_rife() {
    let descriptor = relayed_source_descriptor();
    let nvidia_plan = |decision| ExecutionPlan {
        executor: ExecutorKind::NvidiaSpecialized,
        latency_mode: LatencyMode::Low,
        requires_local_hls_relay: true,
        video_ops: vec![
            VideoOp::NormalizeInput,
            VideoOp::Interpolate(InterpolationPlan {
                target_fps: 60,
                decision,
            }),
        ],
    };

    assert!(should_attempt_staged_launch(
        &descriptor,
        &nvidia_plan(InterpolationDecision::rife(OpExecutionStage::Preprocess))
    ));
    assert!(!should_attempt_staged_launch(
        &descriptor,
        &nvidia_plan(InterpolationDecision::native_backend(
            OpExecutionStage::Executor
        ))
    ));
    // NVIDIA without FRUC plans portable minterpolate, which stays on the
    // single-process path.
    assert!(!should_attempt_staged_launch(
        &descriptor,
        &nvidia_plan(InterpolationDecision::portable_fallback(
            OpExecutionStage::Preprocess
        ))
    ));
}

#[test]
fn remuxed_ingest_gate_covers_nvenc_on_relayed_hls_sources_only() {
    let relayed = relayed_source_descriptor();
    let unrelayed = SourceDescriptor {
        relay: None,
        ..relayed.clone()
    };
    let direct_file = SourceDescriptor {
        classification: SourceClassification {
            transport: SourceTransport::RemoteHttp,
            kind: SourceKind::Other,
        },
        original_url: "https://example.com/video.mp4".to_string(),
        runtime_url: "https://example.com/video.mp4".to_string(),
        relay: None,
        ..relayed.clone()
    };

    let nvidia = executor_plan(ExecutorKind::NvidiaSpecialized);
    assert!(should_remux_source_ingest(&relayed, &nvidia));
    assert!(!should_remux_source_ingest(&unrelayed, &nvidia));
    assert!(!should_remux_source_ingest(&direct_file, &nvidia));
    for executor in [
        ExecutorKind::AmdSpecialized,
        ExecutorKind::Universal,
        ExecutorKind::Cpu,
    ] {
        assert!(
            !should_remux_source_ingest(&relayed, &executor_plan(executor)),
            "{executor:?}"
        );
    }
}

#[test]
fn remuxed_ingest_contexts_feed_the_executor_and_keep_its_hls_output() {
    let base = StageRuntimeContext::single_process(
        "session-1",
        &executor_plan(ExecutorKind::NvidiaSpecialized),
        &relayed_source_descriptor(),
        Path::new("session\\index.m3u8"),
    );
    let contexts = RemuxedIngestContexts::new(&base);

    assert_eq!(contexts.remuxer.transport.input, FrameTransport::SourcePull);
    assert_eq!(
        contexts.remuxer.transport.output,
        FrameTransport::RemuxedSourcePipe
    );
    assert_eq!(
        contexts.executor.transport.input,
        FrameTransport::RemuxedSourcePipe
    );
    assert_eq!(contexts.executor.transport.output, base.transport.output);
}

#[test]
fn remuxed_ingest_retries_direct_after_ingest_failures_only() {
    use crate::runtime::PipelineStageId;

    assert!(is_ingest_failure(
        PipelineStageId::Normalizer,
        "Error opening input: Server returned 403 Forbidden"
    ));
    assert!(is_ingest_failure(
        PipelineStageId::Executor,
        "ERROR: Failed to initialize file reader"
    ));
    assert!(!is_ingest_failure(
        PipelineStageId::Executor,
        "ERROR: out of memory"
    ));
    assert!(!is_ingest_failure(PipelineStageId::Packager, ""));
}
