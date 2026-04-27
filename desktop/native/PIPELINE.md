# Streaming Pipeline Architecture

> **Purpose**: Quick-reference for coding agents. Covers the full data path from source URL to HLS output, module responsibilities, key types, and design conventions.

---

## High-Level Data Flow

```
Client Request (POST /stream/start)
        │
        ▼
  source/mod.rs          ← probe URL, classify content (HLS / local file / live)
        │
        ▼ SourceDescriptor
  graph/planner.rs       ← build ExecutionPlan (what ops, what stage owns each op)
        │
        ▼ IntermediateExecutionPlan
  pipeline.rs            ← resolve executor, wire together all stage builders
        │
        ▼ spawned child processes
  ┌─────┴──────────────────────────────────────────────────────┐
  │  Stage 1: normalize/  (FfmpegNormalizer)                   │
  │           any input  →  NUT stream                         │
  ├────────────────────────────────────────────────────────────┤
  │  Stage 2: preprocess/ (optional, one of:)                  │
  │    • StreamingRifePreprocessor  (rife_stream.py, NUT→NUT)  │
  │    • RifePreprocessor           (batch frames, NUT→NUT)    │
  │    • FfmpegPreprocessor         (libplacebo, NUT→NUT)      │
  ├────────────────────────────────────────────────────────────┤
  │  Stage 3: exec/  (one of:)                                 │
  │    • NvidiaSpecializedExecutor  (NVEncC + NGX + TrueHDR + FRUC)             │
  │    • AmdExecutor                (VCEEncC)                  │
  │    • UniversalExecutor          (FFmpeg + libplacebo)      │
  ├────────────────────────────────────────────────────────────┤
  │  Stage 4: exec/ffmpeg_packager.rs  (FfmpegHlsPackager)     │
  │           MPEG-TS  →  HLS segments + playlist              │
  └─────┬──────────────────────────────────────────────────────┘
        │
        ▼
  GET /tmp/{session_id}/index.m3u8   (served by Axum)
```

Stages are connected **stdout → stdin** (byte-streaming NUT / MPEG-TS).  
All stage processes are supervised by `runtime/supervisor.rs`.

---

## Module Map

| Path | Role |
|---|---|
| `server.rs` | Axum HTTP server (port **14002**); session CRUD endpoints |
| `pipeline.rs` | Orchestrator: selects executor, wires stages, manages `Session` map |
| `graph/` | Planning layer — converts `PipelineRequest` into `ExecutionPlan` |
| `runtime/` | Stage lifecycle, process piping, heartbeat supervision |
| `exec/` | Concrete backend implementations (NVEncC, VCEEncC, FFmpeg) |
| `preprocess/` | Portable pre-encode processing (RIFE, libplacebo resize/HDR) |
| `normalize/` | Input normalization (any source → NUT) |
| `source/` | URL probing, content classification, HLS relay |
| `gpu.rs` | GPU info probing (vendor, driver, encoder backend name) |
| `settings.rs` | Persistent user settings (`resolution`, `quality`, `framegen`, `hdr`, …) |
| `deps/` | Dependency installation (streaming RIFE Python env, model download) |

---

## HTTP API (port 14002)

| Method | Path | Auth | Purpose |
|---|---|---|---|
| `GET` | `/v1/status` | None | GPU info, active sessions, encoder capabilities |
| `POST` | `/v1/auth/request` | None | Request an API token for a browser origin (shows consent dialog in desktop mode; returns 403 in headless if origin not pre-approved) |
| `POST` | `/v1/auth/rotate-token` | `X-Referee-Token` | Generate a new API token, invalidating the current one |
| `GET` | `/v1/origins` | `X-Referee-Token` | List all persistently approved origins |
| `POST` | `/v1/origins` | `X-Referee-Token` | Add or update an approved origin |
| `DELETE` | `/v1/origins/:origin` | `X-Referee-Token` | Remove an approved origin (`:origin` is URL-encoded) |
| `POST` | `/v1/stream/start` | `X-Referee-Token` | Start a pipeline session; returns `sessionId` + HLS URL |
| `POST` | `/v1/stream/heartbeat/{session_id}` | `X-Referee-Token` | Keep session alive (must be called every ~10 s) |
| `POST` | `/v1/stream/stop` | `X-Referee-Token` | Tear down a session |
| `GET` | `/v1/input/{session_id}` | None | Input proxy (relays source stream locally) |
| `GET` | `/v1/tmp/{session_id}/{filename}` | None | Serve HLS segments / playlist from tmp dir |

**Authentication — token bootstrap:**

| Scenario | How to get the token |
|---|---|
| **Desktop app** | Read from the REFEREE UI settings panel, or call the `get_api_token` Tauri IPC command |
| **Headless — browser origin** | `POST /v1/auth/request` with the `Origin` header; if the origin is pre-approved the token is returned immediately |
| **Headless — `REFEREE_API_TOKEN` env var** | Set this env var (≥ 32 chars) before server start — the server uses it as the token instead of generating a UUID |
| **Headless — startup logs** | A masked hint (`...last4`) is logged; the full token is in `config.json` in the app data directory |

**Authentication — pre-approving origins (headless):**

In headless mode `POST /v1/auth/request` returns `403 HEADLESS_MODE` for unknown origins. To bootstrap access without a UI:

1. **Env var at startup**: `REFEREE_ALLOWED_ORIGINS=https://myapp.com,https://other.com` — origins are upserted into the approved list before the server accepts connections.
2. **HTTP API at runtime**: `POST /v1/origins` with a valid `X-Referee-Token` header to add origins dynamically.
3. **Manual file edit**: Edit `approved-origins.json` in the app data directory and restart.

**`POST /v1/stream/start` request fields:**

| Field | Type | Required | Description |
|---|---|---|---|
| `url` | string | ✓ | Source stream URL |
| `appName` | string | — | Display name of the integrating app |
| `streamTitle` | string | — | Title of the current stream |
| `headers` | object | — | Extra HTTP headers forwarded to the source |
| `contentKind` | `"animated"` \| `"liveAction"` | — | Override auto-detected content kind; skips probe and forces upscaler selection |

**`POST /v1/stream/start` response fields:**

| Field | Description |
|---|---|
| `sessionId` | UUID for the created session |
| `url` | Full absolute HLS playlist URL (e.g. `http://192.168.1.x:14002/v1/tmp/{id}/index.m3u8`) |
| `resolution` | Output resolution string |
| `sourceResolution` | Detected source resolution (if probed) |
| `effectiveQuality` | Clamped quality level actually used |
| `evictedSessions` | Array of session IDs that were stopped to make room for this session |

**`POST /v1/stream/stop` request fields:**

| Field | Type | Required | Description |
|---|---|---|---|
| `sessionId` | string | — | ID of the session to stop |
| `stopAll` | boolean | — | If `true`, stops all active sessions (takes precedence over `sessionId`) |

At least one of `sessionId` or `stopAll: true` must be provided.

**Error response shape:**

```json
{ "code": "SESSION_NOT_FOUND", "error": "Session not found" }
```

Known error codes: `INVALID_REQUEST`, `NO_ENCODER`, `SESSION_NOT_FOUND`, `PIPELINE_EXITED`, `PIPELINE_TIMEOUT`, `UNAUTHORIZED`, `MISSING_ORIGIN`, `INVALID_ORIGIN`, `HEADLESS_MODE`, `CONSENT_DENIED`, `CONSENT_TIMEOUT`, `ORIGIN_NOT_FOUND`.

**Session timeout constants** (in `pipeline.rs`):
- `HEARTBEAT_TIMEOUT_MS = 15_000` — kill session if heartbeat stops after first check-in
- `ORPHAN_SESSION_TIMEOUT_MS = 300_000` — kill session if first heartbeat never arrives
- `PIPELINE_CLEANUP_TIMEOUT_MS = 10_000` — hard kill timeout on cleanup

---

## Planning Layer (`graph/`)

### `PipelineRequest`
The top-level user intent. Built in `server.rs` from the POST body + current `ServerSettings`.

```rust
PipelineRequest {
    source_transport: SourceTransport,   // RemoteHttp | LocalFile
    source_kind: SourceKind,             // Hls | Other
    source_content_kind: SourceContentKind, // Animated | LiveAction | Unknown
    source_resolution: Option<String>,
    output_resolution: String,           // e.g. "1920x1080"
    source_fps: Option<f64>,
    latency_mode: LatencyMode,           // Low | Normal
    upscale: UpscaleRequest,
    interpolation: InterpolationRequest,
    hdr: HdrRequest,
}
```

### `UpscaleRequest / InterpolationRequest / HdrRequest`

```
UpscaleRequest:    Off | Quality(1..4) | Anime4k2x | Artcnn2x
InterpolationRequest: Off | To60
HdrRequest:        Off | Passthrough10Bit | TonemapToHdr10 | InjectHdr10Metadata
```

> `Anime4k2x` forces the Anime4K shader; `Artcnn2x` forces the ArtCNN shader. When `Quality(n)`
> is used, the planner selects between these automatically based on detected source content kind.

### `ExecutionPlan` → `IntermediateExecutionPlan`

`GraphPlanner::plan()` produces an `ExecutionPlan` containing ordered `VideoOp`s:

```
NormalizeInput → Resize(ResizePlan) → Interpolate(InterpolationPlan)
              → Hdr(HdrPlan) → Anime4k2xUpscale | Artcnn2xUpscale
```

Each op is assigned an **`OpExecutionStage`**:

| Stage | Who runs it |
|---|---|
| `Preprocess` | Preprocessor process (libplacebo / RIFE) |
| `Executor` | Backend encoder (NVEncC / VCEEncC / FFmpeg) |
| `Packager` | HLS muxer metadata |
| `Deferred` | Not yet implemented |

`IntermediateExecutionPlan` further resolves op **ownership**:

| Owner | Description |
|---|---|
| `Normalizer` | Always handles `NormalizeInput` |
| `SharedPreprocess` | Portable ops (RIFE, libplacebo resize/HDR, shaders) |
| `Executor` | Native ops (NGX-VSR, FRUC, TrueHDR, AMD VPP resize) |
| `Packager` | HLS metadata |
| `Deferred` | Future work |

Native accelerator plans carry a **fallback owner** so if the accelerator is unavailable at runtime the op automatically promotes to `SharedPreprocess`.

### `BackendCapabilities`

Holds three tiers of capability inventories probed at startup:
1. `selected_executor` — the planner's chosen executor kind
2. `selected_backend` — capability facts for that backend
3. `vendor_native_backend` — optional vendor path (NVIDIA NGX, AMD VPP)
4. `universal_fallback` — always-available FFmpeg libplacebo path

---

## Executor Selection

### Graph level (`ExecutorKind`)

```
NvidiaSpecialized  — Windows + NVIDIA + (NGX-VSR OR FRUC OR TrueHDR)
Universal          — everything else (remapped at runtime)
Cpu                — legacy CPU-only fallback
```

### Runtime level (`ExecutorFamily`)

`exec/family.rs` re-evaluates `ExecutorKind::Universal` based on probed GPU vendor:

```
Universal + AMD GPU   → AmdExecutor    (VCEEncC)
Universal + NVIDIA    → UniversalExecutor (FFmpeg + NVENC)
Universal + other     → UniversalExecutor (FFmpeg + VAAPI or CPU)
```

`ExecutorPreference` (user setting) can override auto-selection:
`Auto | NvidiaAi | AmdAi | Universal`

---

## Backend Implementations (`exec/`)

### `NvidiaSpecializedExecutor` (`exec/nvidia.rs`)
- **Binary**: NVEncC (Rigaya)
- **Windows-only**
- Native ops: `--vpp-resize algo=ngx-vsr,vsr-quality=N`, `--vpp-fruc fps=60000/1001`, `--vpp-ngx-truehdr`
- Probes `--check-features` to determine single-pass availability

### `AmdExecutor` (`exec/amd.rs`)
- **Binary**: VCEEncC
- Native resize: `--vpp-resize amf_fsr`
- Falls back to `UniversalExecutor` for ops VCEEncC can't handle natively (interpolation, HDR tonemapping)

### `UniversalExecutor` (`exec/universal.rs`)
- **Binary**: FFmpeg
- Encode backends selected per platform: NVENC (Windows/NVIDIA), VAAPI (Linux), CPU fallback
- Builds `libplacebo` filter graph for resizing, HDR transforms, custom GLSL shaders
- Emits `-progress pipe:2` for structured progress parsing

### `FfmpegHlsPackager` (`exec/ffmpeg_packager.rs`)
- Standalone HLS muxer; receives MPEG-TS or NUT over pipe
- Writes `segment_XXXXXX.ts` + `index.m3u8` into the session tmp dir
- `-hls_time 1`, `-hls_list_size 8`, optional `-hls_flags delete_segments`

---

## Preprocessors (`preprocess/`)

Only used when the executor cannot handle an op natively.

| Type | File | Description |
|---|---|---|
| `StreamingRifePreprocessor` | `streaming_rife_preprocessor.rs` | Long-lived Python subprocess (`rife_stream.py`). NUT → NUT. Emits `[rife_stream] ready` sentinel when warm. Preferred path. |
| `RifePreprocessor` | `rife_preprocessor.rs` | Batch path: extract frames → RIFE binary → re-encode. NUT → NUT. |
| `FfmpegPreprocessor` | `ffmpeg_preprocessor.rs` | FFmpeg + libplacebo for resize / HDR / shaders when no GPU-native path. NUT → NUT. |

RIFE paths require:
- `EncoderCapabilities.has_streaming_rife` / `has_rife` set to `true`
- Python path, script path, and model directory (`StreamingRifeParams`)

---

## Normalizer (`normalize/`)

`FfmpegNormalizer` is always stage 1.  
Accepts any FFmpeg-compatible source (HLS URL, RTMP, local file) and outputs a **NUT** stream for frame-accurate downstream processing. Preserves audio passthrough.

---

## Source Handling (`source/`)

### `describe_source(url, headers)`
1. Classifies transport: `RemoteHttp | LocalFile | LocalPath | Other`
2. Classifies kind: `Hls | Other`
3. Probes HLS manifest (parses `#EXT-X-STREAM-INF`) or falls back to `ffprobe`
4. Optionally probes content kind: analyses first 6 frames (scaled to 64×36) → `Animated | LiveAction | Unknown`
5. Returns `SourceDescriptor` (relay flag, resolved URL, headers, resolution, FPS)

### `hls_relay`
Relays remote HLS manifests locally. Enables consistent per-session URL rewriting and seek compatibility when the source playlist rolls segments.

---

## Runtime Supervision (`runtime/`)

### `PipelineSupervisor` (`runtime/supervisor.rs`)

Owns all stage processes for a session. Responsibilities:
- **Spawn** stages in order: Normalizer → Preprocess → Executor → Packager
- **Pipe** stdout of stage N to stdin of stage N+1 (`FrameTransport::StdoutPipe`)
- **Track readiness** per stage:

  | Policy | Used by |
  |---|---|
  | `ReadyOnSpawn` | Normalizer, legacy monolithic executor |
  | `ReadyOnHeartbeat` | Executor, packager |
  | `ReadyOnStderrSentinel` | Streaming RIFE (`[rife_stream] ready`) |

- **Poll** child exit status every `CHILD_POLL_INTERVAL = 250 ms`
- **Enforce timeouts**: orphan timeout + inactivity timeout (both configurable)
- **Collect** per-stage stderr tails (last 64 KB) and exit codes on shutdown

### `FrameTransport`

```
SourcePull    — executor reads source URL directly (no pipe)
StdoutPipe    — stdout of previous stage → stdin of this stage
NamedPipe     — OS named pipe (Windows: \\.\pipe\*, Unix: /tmp/referee-*)
HlsOutput     — terminal stage; writes to disk (HLS packager)
LocalSocket   — future (not yet wired)
```

---

## Session Lifecycle

```
POST /stream/start
  │
  ├─ classify + probe source         (source/)
  ├─ plan execution                  (graph/)
  ├─ select executor                 (pipeline.rs + exec/family.rs)
  ├─ build stage specs               (exec/, preprocess/, normalize/)
  ├─ spawn PipelineSupervisor        (runtime/supervisor.rs)
  ├─ wait for packager playlist      (wait_for_packager_playlist)
  └─ return { sessionId, hlsUrl }
         │
         ▼  client polls / plays HLS
  POST /stream/heartbeat  (every ~10 s)
         │
         ▼  heartbeat stops or explicit stop
  POST /stream/stop  OR  timeout
  └─ cleanup_session → kill all stages → collect reports
```

---

## Encoding Profiles (`pipeline.rs`)

Target bitrates per output resolution (used by all executors):

| Resolution | Bitrate | Max Bitrate | Preset | HLS Segment |
|---|---|---|---|---|
| 2560×1440 | 35 Mbps | 52.5 Mbps | p4 | 1 s |
| 3840×2160 | 50 Mbps | 75 Mbps | p4 | 1 s |
| other | 25 Mbps | 37.5 Mbps | p4 | 1 s |

Custom profiles can be stored in `ServerSettings.encoding_profiles` (keyed by resolution string).

---

## Key Enums Quick Reference

```rust
// graph/request.rs
enum UpscaleRequest       { Off, Quality(u8), Anime4k2x, Artcnn2x }
enum InterpolationRequest { Off, To60 }
enum HdrRequest           { Off, Passthrough10Bit, TonemapToHdr10, InjectHdr10Metadata }

// graph/capabilities.rs
enum FeatureAvailability  { Unavailable, Approximate, Exact }
enum ResizeSupport        { Unsupported, Basic, QualityRange { min, max } }
enum InterpolationSupport { Unsupported, To60 }

// graph/plan.rs
enum OpExecutionStage     { Preprocess, Executor, Packager, Deferred }
enum ExecutorKind         { NvidiaSpecialized, Universal, Cpu }

// graph/intermediate.rs
enum NativeAcceleratorKind { NvidiaNgxVsr, NvidiaFruc, NvidiaTrueHdr, AmdVppResize }
enum IntermediateOpOwner  { Normalizer, SharedPreprocess, Executor, Packager, Deferred }

// runtime/stages.rs
enum FrameTransport       { SourcePull, StdoutPipe, NamedPipe, HlsOutput, LocalSocket }

// pipeline.rs
enum ExecutorPreference   { Auto, NvidiaAi, AmdAi, Universal }
```

---

## Design Conventions

1. **Two-tier executor model**: `ExecutorKind` (graph, stable) vs. `ExecutorFamily` (runtime, probed). Never conflate them.
2. **Intermediate plan as contract**: Executors consume `IntermediateExecutionPlan` and only act on ops assigned to them. The planner, not the executor, decides ownership.
3. **Native accelerator + fallback pair**: Every native op carries a `fallback_owner`. Executors *must* honour the fallback if the native path is unavailable.
4. **NUT as inter-stage wire format**: All stage-to-stage pipes carry NUT. Only the packager emits MPEG-TS.
5. **Sentinel-gated readiness for Python stages**: RIFE subprocess prints `[rife_stream] ready` before accepting frames; the supervisor gates downstream stage startup on this sentinel.
6. **No heartbeat = no keep-alive**: The client is responsible for polling. A session with no heartbeat is orphaned and killed after `ORPHAN_SESSION_TIMEOUT_MS`.
