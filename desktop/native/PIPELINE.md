# Streaming Pipeline Architecture

> **Purpose:** Working reference for coding agents and contributors changing the Rust backend in `desktop/native/`. It covers the HTTP surface, how a stream request becomes child processes, which module owns each decision, and the invariants and known gaps to respect when changing things.
>
> **Last verified against source:** v0.6.0 (commit `8fd2b5e`), 2026-10-14.
> **Conventions:** Paths are relative to `desktop/native/` unless they start with `/` (repo root). Symbols are named so you can grep for them. Line numbers are left out on purpose because they go stale. If this document and the code disagree, the code wins. Fix the document in the same change (see [§21](#21-keeping-this-document-current)).

---

## Contents

1. [Orientation](#1-orientation)
2. [Build targets, features, and commands](#2-build-targets-features-and-commands)
3. [Process startup, data layout, and environment](#3-process-startup-data-layout-and-environment)
4. [HTTP API](#4-http-api)
5. [End-to-end: from `POST /v1/stream/start` to HLS](#5-end-to-end-from-post-v1streamstart-to-hls)
6. [Planning layer (`graph/`)](#6-planning-layer-graph)
7. [Runtime execution (`pipeline/`, `exec/`, `normalize/`, `preprocess/`)](#7-runtime-execution)
8. [Supervision, timeouts, and retries (`runtime/`)](#8-supervision-timeouts-and-retries)
9. [Sessions](#9-sessions)
10. [Source handling (`source/`)](#10-source-handling-source)
11. [LAN relay (REFEREE Relay)](#11-lan-relay-referee-relay)
12. [Dependencies and GPU detection (`deps/`, `gpu.rs`)](#12-dependencies-and-gpu-detection)
13. [Settings and persistence (`settings.rs`)](#13-settings-and-persistence-settingsrs)
14. [Desktop shell integration (Tauri)](#14-desktop-shell-integration-tauri)
15. [Logging and debugging](#15-logging-and-debugging)
16. [Testing](#16-testing)
17. [Invariants and conventions](#17-invariants-and-conventions)
18. [Known gaps and sharp edges](#18-known-gaps-and-sharp-edges)
19. [Change recipes](#19-change-recipes)
20. [Type quick reference](#20-type-quick-reference)
21. [Keeping this document current](#21-keeping-this-document-current)

---

## 1. Orientation

REFEREE is a local HTTP service on port **14002**. A web page sends it a source stream URL. REFEREE plans a GPU processing pipeline (upscale, frame interpolation, HDR), launches encoder child processes that pull the source, and serves the result back as a local HLS playlist. The same backend runs inside the Tauri desktop app or as a headless `referee-server` binary.

### Five facts that most often trip people up

1. **Most sessions are a single child process.** The 4-stage normalizer → preprocess → executor → packager topology exists, but `should_attempt_staged_launch` uses it only when the plan has preprocessor-owned work: on `Universal` and `AmdSpecialized` for any such work (in practice RIFE, `minterpolate`, or libplacebo HDR that VCEEncC lacks), and on `NvidiaSpecialized` only for **RIFE**. Everything else runs one encoder process that ingests, filters, encodes, and muxes HLS inline. See [§7.1](#71-launch-mode-single-process-vs-staged).
2. **There are two crate roots.** `src/lib.rs` (desktop) and `src/server_main.rs` (headless) each declare the module tree with `mod`. A new module must be declared in both, and Tauri-only code must be `#[cfg(not(feature = "headless"))]`. See [§2.2](#22-module-gating-between-desktop-and-headless).
3. **An AMD session can change executor mid-startup.** `ExecutorKind` (`NvidiaSpecialized | AmdSpecialized | Universal | Cpu`) maps one-to-one onto a runtime executor in `exec/family.rs` (`Cpu` renders through Universal), but when VCEEncC fails before startup completes, the session is replanned onto `Universal` FFmpeg for its retry and loses AMF FRC ([§8.3](#83-retries)).
4. **User settings drive the pipeline, not the request body.** Output resolution, quality, framegen, HDR, and executor preference come from persisted `ServerSettings`. The `/v1/stream/start` body supplies only the source URL, headers, display labels, an optional `contentKind` hint, and an optional `displayRefreshRate` for the Match Display frame generation target.
5. **Only remote HLS sources are probed.** Non-HLS URLs (for example a direct `.mp4`) get no metadata. With no source resolution, `build_upscale_request` returns `UpscaleRequest::Off`, so **no upscaling happens** for those sources. See [§18](#18-known-gaps-and-sharp-edges).

### Repository map

| Path (repo root) | Contents |
|---|---|
| `/desktop/native/` | Rust backend + Tauri shell (this document) |
| `/desktop/native/shaders/` | GLSL embedded at compile time: Anime4K, ArtCNN, `cas/Referee_CAS.glsl` (template) |
| `/desktop/native/rife-worker/` | Standalone `rife-worker` crate (own `Cargo.toml`, `build.rs`, `Cargo.lock`): `src/` (worker), `cpp/rife_c.{h,cpp}` (C shim over VS-RIFE), `vendor/` (fetched, gitignored) |
| `/scripts/fetch-rife-vendor.js`, `/scripts/build-rife-worker.js` | Fetch the pinned ncnn + VS-RIFE sources; build the worker and stage it as the Tauri sidecar (`/desktop/native/binaries/`, gitignored) |
| `/desktop/ui/` | Tauri renderer (React 19 + Vite + Tailwind). Vite root is set in `/vite.config.ts`. Two entries: `index.html` (main) and `player.html` (stream player window) |
| `/packages/client/` | `@cmacrowther/referee-client`: TypeScript SDK and `useReferee()` React hook. Heartbeats every 10 s by default |
| `/docs/` | Next.js docs site (referee.craw.ca). `/docs/relay-implementation-plan.md` holds the relay design notes |
| `/docker/`, `/docker-compose.yml` | Headless images (`Dockerfile.nvidia`, `Dockerfile.amd`) |
| `/scripts/sync-tauri-version.js` | Copies `/package.json` `version` into `tauri.conf.json` and `Cargo.toml`. Runs before `npm run dev` and `npm run build` |
| `/.github/workflows/release.yml` | Tests + build + GitHub release, triggered by a version bump on `main` |
| `/desktop/binary-platform.js` | Legacy Node helper for picking Rigaya assets. **Not referenced** by any build script or Rust code |

### Module map (`src/`)

| Path | Role |
|---|---|
| `main.rs` | Desktop binary entry → `referee_lib::run()` |
| `lib.rs` | Desktop app: Tauri builder, state wiring, background loops, command registration |
| `server_main.rs` | Headless binary entry: same server without Tauri; env-var bootstrap |
| `server.rs` + `server/` | Axum HTTP server. `routes.rs` (router, CORS, security headers), `stream.rs` (start/heartbeat/stop/tmp/input handlers + `PipelineRequest` construction), `auth.rs` (token middleware, consent, origins, settings patch), `status.rs` (status/ping), `relay.rs` (relay peer control), `state.rs` (`ServerState`, `ServerSettings`, `RateLimiter`), `responses.rs` (error envelope), `clients.rs` (shared reqwest clients), `tests.rs` |
| `pipeline/` | Orchestrator. `capabilities.rs` (encoder probing, executor-kind selection), `framegen.rs` (frame generation target/engine settings → `InterpolationRequest`, Match Display rate), `launch.rs` (`PipelineLaunch`, `RuntimeBackend`, executor construction, single-process and staged launches), `run.rs` (`start_pipeline`, the attempt/retry loop), `session.rs` (`Session`, `SessionMap`, readiness wait, cleanup), `profile.rs` (encoding profiles), `tests.rs` |
| `graph/` | Pure planning: `request.rs` (`PipelineRequest`), `capabilities.rs`, `planner.rs` (`GraphPlanner`), `plan.rs` (`ExecutionPlan`, `VideoOp`), `intermediate.rs` (ownership/bindings) |
| `runtime/` | `stages.rs` (`StageRuntimeContext`, `ProcessSpec`, `FrameTransport`, `StartupMode`, HLS constants, stage traits), `supervisor.rs` (`PipelineSupervisor`, stderr tailing, stage bridges) |
| `exec/` | Executors: `nvidia.rs`, `amd.rs`, `universal.rs`. Plus `family.rs` (runtime executor selection), `ffmpeg_packager.rs`, `libplacebo_filters.rs` (`LibplaceboParams`, shared by the Universal executor and the FFmpeg preprocessor), and `mod.rs` (shared Rigaya input/output args, FFmpeg HLS muxer args, GOP alignment, `EncoderInput`) |
| `normalize/` | `FfmpegNormalizer`: any source → NUT (staged path only) |
| `preprocess/` | `StreamingRifePreprocessor` (drives `rife-worker`), `FfmpegPreprocessor` (libplacebo + `minterpolate`) |
| `source/` | `mod.rs` (classification, manifest/ffprobe probing, content-kind detection, SSRF guard), `hls_relay.rs` (input proxy + manifest rewriting), `easy_proxy.rs` (optional EasyProxy extraction and routing, §10.6) |
| `easy_proxy_service.rs` | Managed EasyProxy: install state, on-demand start, idle stop (§10.7). Its installer is `deps/easy_proxy/` |
| `gpu.rs` | GPU vendor/name detection, encoder/ffmpeg/rife path resolution, utilization polling |
| `deps/` | First-run downloads (`download.rs`), archive extraction (`install.rs`), binary resolution and validation (`resolution.rs`), shader install (`shaders.rs`) |
| `settings.rs` | `Settings` (`config.json`), `SettingsManager`, relay/player sub-settings |
| `commands.rs` | Tauri `#[tauri::command]` handlers (setup flow, settings, relay linking, updates, player launch) |
| `tray.rs` | System tray and window management (`main`, `stream-player` labels) |
| `updater.rs` | REFEREE's own updates from GitHub releases: check, verified download, elevated install, the automatic-update loop (§14) |
| `paths.rs`, `process.rs` | App-data paths; `CREATE_NO_WINDOW` helpers |

---

## 2. Build targets, features, and commands

### 2.1 Binaries and features (`Cargo.toml`)

| Binary | Source | Required feature | Notes |
|---|---|---|---|
| `referee` (default-run) | `src/main.rs` → `lib.rs::run()` | none | Desktop app. With `headless` enabled, `run()` panics. |
| `referee-server` | `src/server_main.rs` | `headless` | Headless HTTP server. No Tauri runtime, but the build still needs the webkit/gtk headers on Linux because `tauri-build` runs in `build.rs`. |
| `rife-worker` | separate crate `rife-worker/` | `engine` (default) | NUT-in/NUT-out RIFE interpolator ([§7.6](#76-preprocessors-preprocess)). Its `build.rs` compiles ncnn `20250503` (CMake, Vulkan with ncnn's built-in loader, so **no Vulkan SDK**) and VS-RIFE `r9_mod_v33` from `rife-worker/vendor/`, which `npm run fetch:rife` fills. Needs CMake and a C++17 toolchain (`scripts/build-rife-worker.js` finds Visual Studio's bundled CMake on Windows). Windows links the C runtime statically (`rife-worker/.cargo/config.toml`). `npm run build` builds it and bundles it as a Tauri sidecar next to the app (`tauri.rife.conf.json`, merged only for release builds); the Docker images install it at `/usr/local/bin`. Without `engine`, only the pure logic compiles (`cargo test --no-default-features`, run by CI). |

The lib crate is `referee_lib` (`staticlib`, `cdylib`, `rlib`).

### 2.2 Module gating between desktop and headless

- `lib.rs` always compiles `graph`, `source`, and `process`. Every other module is behind `#[cfg(not(feature = "headless"))]`.
- `server_main.rs` re-declares the modules it needs (`deps, exec, gpu, graph, normalize, pipeline, preprocess, process, runtime, server, settings, source`). It does **not** include `commands` or `tray`.
- Shared modules that touch Tauri (`server/*`) gate those parts per item: `#[cfg(not(feature = "headless"))]` with a headless twin that is a no-op or returns `HEADLESS_MODE` (for example `request_user_consent`, `emit_status_update`, `maybe_show_stream_window`). In headless builds, `DesktopAppHandle` is aliased to `()`.

### 2.3 Commands

| Task | Command (repo root unless noted) |
|---|---|
| Desktop dev (Vite + Tauri) | `npm run dev` |
| Headless server dev | `npm run dev:server` (= `cargo run --features headless --bin referee-server`) |
| Desktop release build | `npm run build` (NSIS on Windows; deb/rpm on Linux). Builds `rife-worker` first, so it needs CMake and a C++ toolchain |
| `rife-worker` | `npm run fetch:rife` (pinned sources), `npm run build:rife` (build + stage sidecar), `cargo test --no-default-features` in `desktop/native/rife-worker` (pure logic). Debug builds of the app find `rife-worker/target/release/rife-worker[.exe]` without staging |
| Headless release build | `npm run build:server` |
| UI tests | `npm test` (Vitest, watch) / `npm test -- --run` |
| Rust tests as CI runs them | `cd desktop/native && cargo test --features headless --no-default-features` |
| Rust tests incl. desktop-only modules | `cargo test --manifest-path desktop/native/Cargo.toml` (needs the Tauri system deps; this is how `commands.rs` tests run) |
| Everything | `npm run test:all` |
| Docker | `docker compose --profile nvidia up` (host port **14003** → 14002) or `--profile amd up` (14002 → 14002) |

---

## 3. Process startup, data layout, and environment

### 3.1 Desktop startup (`lib.rs::run`)

1. On Linux, set `GDK_BACKEND=x11` (WebKitGTK blank-window workaround). Register the shell, notification, single-instance, and autostart (`--hidden`) plugins.
2. Resolve paths (`paths.rs`). **Delete and recreate** `{temp}/RefereeProxy`. Create `lib/` and `logs/`. Run `deps::ensure_universal_shaders(lib_dir)`.
3. Set up tracing to stdout plus a daily rolling file `logs/referee.log` (default filter `referee=info`; `RUST_LOG` overrides).
4. `server::validate_http_clients()` fails fast if the shared reqwest clients cannot be built.
5. Load `SettingsManager` (`config.json`). Generate and persist `apiToken` and `instanceId` (UUIDv4) if they are missing.
6. `gpu::detect_compatible_gpu(lib_dir)`. If an encoder is already present, probe `detect_encoder_capabilities`; `has_rife` comes from `GpuInfo::has_rife` (worker and model both resolved).
7. Build `ServerState` (rate limiters: auth 5/min/IP, stream 3/min/IP; `setup_complete = false`) and `AppState`. Set up the tray and show the main window.
8. Spawn background tasks: the HTTP server, the GPU refresh loop (starts after setup), and the renderer status push loop ([§3.3](#33-background-loops)).
9. Setup (binary download) is **driven by the renderer**: `renderer_ready` checks `deps::binaries_ready`. It either spawns `run_setup_download` or marks setup complete. See [§14](#14-desktop-shell-integration-tauri).
10. `RunEvent::ExitRequested` with no exit code is prevented, so the app lives in the tray.

### 3.2 Headless startup (`server_main.rs::main`)

1. Data dir: `%APPDATA%\com.referee.proxy` (Windows; falls back to temp if `APPDATA` is unset) or `$HOME/.local/share/com.referee.proxy` (falls back to `/tmp`). Wipe `{temp}/RefereeProxy`, install shaders, log to `logs/referee-server.log`.
2. `init_auth_token`: if `REFEREE_API_TOKEN` is set and is ≥ 32 chars after trimming, use it **and persist it** to `config.json`. Otherwise use the persisted value or generate a UUID. The full token is never logged, only `...last4`.
3. Detect the GPU. Build `ServerState` with `app_handle: None`.
4. `REFEREE_ALLOWED_ORIGINS` (comma-separated, each must start with `http://` or `https://`) is inserted into the approved origins **if not already present**, then persisted.
5. `bootstrap_headless_setup`: if the vendor is known and `binaries_ready` is false, download binaries (progress goes to the log), re-detect the GPU, and probe capabilities. Then set `setup_complete = true` **whether or not the download succeeded**.
6. Spawn the 60 s GPU refresh loop. Run the server; on Ctrl-C, `server::shutdown` stops all sessions.

### 3.3 Background loops

| Loop | Where | Cadence | Notes |
|---|---|---|---|
| GPU re-detection | `lib.rs`, `server_main.rs` | 60 s | Desktop waits for `setup_done` first. Overwrites `gpu_info` (paths and utilization). Does **not** re-probe `encoder_capabilities`. |
| Renderer status push | `lib.rs` | 1 s with sessions, 5 s idle | Only while the `main` or `stream-player` window is visible. Emits `status-update` only when the serialized JSON changed. Reads GPU utilization only while a session is active, through the shared non-blocking `gpu::latest_utilization` sampler (at most one `nvidia-smi` every 2 s across all callers). |
| Per-session supervisor | `pipeline/run.rs::start_pipeline` → `PipelineSupervisor::run` | 250 ms child poll | One tokio task per local session. |
| Update check | `updater.rs::spawn_auto_updates` (desktop only) | 2 min after launch, then every 6 h | Always checks. With `autoUpdate` on it also downloads, then installs once no session has run and setup has not been in progress for 5 min (polled every 30 s). |

### 3.4 On-disk layout

| Location | Contents |
|---|---|
| App data (`com.referee.proxy`) | `config.json` (settings, `apiToken`, `instanceId`, relay link), `approved-origins.json` (`{origin: {appName, approvedAt}}`), `logs/`, `lib/`, `easyproxy-data/` (managed EasyProxy's `CONFIG_DIR`: its admin settings and recordings; kept when EasyProxy is updated or removed) |
| `lib/` | `ffmpeg[.exe]`, `ffprobe[.exe]` (+ DLLs on Windows), `NVEncC64.exe` (Windows+NVIDIA), `VCEEncC64.exe` / `vceencc` (AMD), `shaders/anime4k/Anime4K_Upscale_CNN_x2_M.glsl`, `shaders/artcnn/ArtCNN_C4F16.glsl`, `vs-rife/models/<model-dir>/`, optionally `rife-worker[.exe]`, and when managed EasyProxy is installed `easyproxy/` (`python/`, `app/`, `referee_launcher.py`, `install.json`; §10.7) |
| App cache (`com.referee.proxy`) `updates/` | The downloaded REFEREE installer (`REFEREE-<version>-update.exe`/`.deb`/`.rpm`). Per-user, because the installer runs elevated. Removed at startup and before each download. |
| `{std::env::temp_dir()}/RefereeProxy/{session_id}/` | `index.m3u8`, `segment_%06d.ts`, and (Universal with shaders/CAS) `referee-universal-runtime.glsl`. Wiped on every startup and removed when the session ends. |

Docker volume mount: `/home/referee/.local/share/com.referee.proxy`. In the NVIDIA image `LD_LIBRARY_PATH` includes its `lib/`.

### 3.5 Environment variables

| Variable | Read by | Effect |
|---|---|---|
| `REFEREE_API_TOKEN` | headless only | Fixed API token (≥ 32 chars, else ignored with an error). Persisted to `config.json`. |
| `REFEREE_ALLOWED_ORIGINS` | headless only | Comma-separated origins to pre-approve. |
| `REFEREE_FFMPEG_PATH` | `deps/resolution.rs` | Use this FFmpeg instead of the bundled one. Must pass validation ([§12.3](#123-ffmpeg-resolution-and-validation)) or resolution **errors**. |
| `REFEREE_RIFE_WORKER_PATH` | `deps/resolution.rs` | Explicit `rife-worker` binary. |
| `REFEREE_RIFE_MODEL_PATH` | `deps/resolution.rs` | Explicit RIFE model directory. |
| `RUST_LOG` | both | `tracing_subscriber::EnvFilter`. Default `referee=info`. |
| `REFEREE_TARGET_PLATFORM` | `/desktop/binary-platform.js` (legacy) | Unused by the Rust backend. |

---

## 4. HTTP API

Bound to `0.0.0.0:14002` (`server.rs::PORT`). If the port is in use, the process logs an error and calls `exit(1)`.

### 4.1 Routes (`server/routes.rs::start_server`)

| Method | Path | Auth | Handler | Purpose |
|---|---|---|---|---|
| `GET` | `/v1/status` | none | `status::handle_status` | GPU, executor prediction, sessions, settings snapshot, capability flags |
| `GET` | `/v1/ping` | none | `status::handle_ping` | Instance identity for relay discovery and linking |
| `POST` | `/v1/auth/request` | none (rate-limited) | `auth::handle_auth_request` | Exchange a browser `Origin` for the API token (consent flow) |
| `POST` | `/v1/auth/rotate-token` | token | `auth::handle_rotate_token` | New UUID token; persisted; emits `token-rotated` |
| `GET` | `/v1/origins` | token | `auth::handle_list_origins` | `[{origin, appName, approvedAt}]`, sorted |
| `POST` | `/v1/origins` | token | `auth::handle_add_origin` | Body `{origin, appName?}` → `201 {"status":"created"}` |
| `DELETE` | `/v1/origins/{origin}` | token | `auth::handle_delete_origin` | `{origin}` is URL-encoded |
| `POST` | `/v1/settings/stream` | token | `auth::handle_stream_settings_update` | Partial settings patch ([§4.6](#46-post-v1settingsstream)) |
| `POST` | `/v1/stream/start` | token (rate-limited) | `stream::handle_stream_start` | Start a session; blocks until the playlist exists |
| `POST` | `/v1/stream/heartbeat/{session_id}` | token | `stream::handle_heartbeat` | Keep the session alive |
| `POST` | `/v1/stream/stop` | token | `stream::handle_stream_stop` | Stop one or all sessions |
| `GET` | `/v1/input/{session_id}?url=` | none | `stream::handle_input_proxy` | Input proxy that encoders pull the source through ([§10.4](#104-input-proxy-hls_relayrs)) |
| `GET` | `/v1/tmp/{session_id}/{filename}[?url=]` | none | `stream::serve_tmp_file` | Serves HLS output, or proxies it for relay sessions |

**Middleware and cross-cutting behavior**

- **Token check** (`auth::require_api_token`): the `X-Referee-Token` header must equal the current token exactly. Applied via `route_layer` to the protected router only.
- **CORS** (`AllowOrigin::predicate`): `/v1/auth/request` and `/v1/status` allow any origin. All other routes allow only origins in `approved_origins` (live `DashMap`, so changes apply immediately). Methods: GET/POST/DELETE/OPTIONS. Headers: `Content-Type`, `X-Referee-Token`. `/v1/tmp` and `/v1/input` responses also set `Access-Control-Allow-Origin: *` by hand.
- **`TCP_NODELAY`** is set on every accepted connection (`ListenerExt::tap_io` in `start_server`), so playlists, segments, and streamed proxy bodies are not held back by Nagle.
- **Security headers** (outer layer): `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`.
- **Rate limits** (`state::RateLimiter`, fixed 60 s window keyed by `"auth:{ip}"` / `"stream:{ip}"`): `/v1/auth/request` allows 5/min (429 + `Retry-After: 60`); `/v1/stream/start` allows 3/min (429).
- **Body errors:** a malformed JSON body is rejected by Axum's `Json` extractor with its own plain-text 400/415/422, *not* the JSON error envelope.

### 4.2 Authentication: getting a token

| Scenario | How |
|---|---|
| Browser page (desktop) | `POST /v1/auth/request` (browser supplies `Origin`; the body's app name, `appName` or `app_name`, is display-only). Already-approved origin → `{token, persistent: true}` immediately. Otherwise the desktop raises the window, emits `consent-request`, and waits up to **180 s**. Allow once → `{token, persistent: false}`. Always allow → persisted origin + `persistent: true`. Deny → 403 `CONSENT_DENIED`. Timeout → 408 `CONSENT_TIMEOUT`. |
| Browser page (headless) | Same endpoint. Pre-approved origin → token. Otherwise **403 `HEADLESS_MODE`** with remediation text. |
| Pre-approving origins (headless) | `REFEREE_ALLOWED_ORIGINS` at startup, `POST /v1/origins` at runtime, or edit `approved-origins.json` and restart. |
| Operator / server-side | `REFEREE_API_TOKEN` (headless), or read `apiToken` from `config.json`. The Tauri command `get_api_token` exists but the renderer does not currently call it. |
| Relay peers | `link_relay_peer` calls the peer's `/v1/auth/request` with a synthetic origin `https://peer-<instance-id>.referee.invalid` and requires a persistent approval ([§11](#11-lan-relay-referee-relay)). |

Consent internals: `pending_consents` maps a nonce to a oneshot sender, resolved by the `respond_to_consent` command. `pending_consent_ui` stores the payload so a renderer that missed the event can call `get_pending_consent`. The event is re-emitted at 250/750/1500 ms. The window is forced always-on-top while the prompt is shown, then restored.

### 4.3 `POST /v1/stream/start`

**Request** (`stream::StreamStartRequest`):

| Field | Type | Required | Notes |
|---|---|---|---|
| `url` | string | ✓ | Must parse as `http` or `https` (`INVALID_URL` otherwise). `file://` and paths are rejected. |
| `appName` | string | — | Trimmed; empty → null. Display and notifications only. |
| `streamTitle` | string | — | Trimmed; empty → null. |
| `headers` | `{string: string}` | — | Forwarded to the source. Rejects `host`, `x-forwarded-*`, `x-real-ip`, `via`, `forwarded`, `transfer-encoding`, `connection`, `upgrade`, `keep-alive`, `proxy-connection`, `te`, `trailer`, and any value containing CR/LF (`INVALID_HEADERS`). |
| `contentKind` | `"animated"` \| `"liveAction"` \| `"unknown"` | — | Overrides detected content kind (confidence 1.0). Only affects shader choice on the Universal path. Detection still runs. |
| `displayRefreshRate` | number (Hz) | — | The viewer's display refresh rate, used only by the `matchDisplay` frame generation target ([§6.1](#61-building-pipelinerequest-serverstreamrsbuild_pipeline_request_from_parts)). When omitted, `handle_stream_start` fills it from this machine's primary display (`gpu::detect_display_refresh_hz`, Windows only) **before** the relay branch, so a relay peer matches the viewer's display rather than its own. |

**Handler order:** rate limit → `validate_stream_start_request` → `is_ssrf_target` (400 `SSRF_BLOCKED`) → header validation → if relay is enabled, forward to the peer ([§11](#11-lan-relay-referee-relay)), else `handle_local_stream_start` ([§5](#5-end-to-end-from-post-v1streamstart-to-hls)).

**Response** (`StreamStartResponse`):

| Field | Notes |
|---|---|
| `sessionId` | UUIDv4 |
| `url` | Always `http://localhost:14002/v1/tmp/{sessionId}/index.m3u8` (`relay::local_output_url`). It is **localhost**, not the LAN IP. |
| `resolution` | Output resolution from settings |
| `sourceResolution` | Detected `WxH` or null |
| `appName`, `streamTitle` | Echoed (normalized) |
| `effectiveQuality` | `clamp_quality` → 1..=4 |
| `evictedSessions` | IDs stopped to make room (only one session runs at a time) |

The call **blocks** until `index.m3u8` exists and is non-empty (up to 180 s locally, 195 s through a relay).

### 4.4 Heartbeat and stop

- `POST /v1/stream/heartbeat/{id}` → `{"status":"ok"}`. Uses `try_send` on a capacity-1 channel, so a full channel counts as alive. It never touches the network, relay sessions included: their channel feeds the relay keepalive ([§11](#11-lan-relay-referee-relay)), which removes the session once the remote half is gone, so the next heartbeat gets 404 `SESSION_NOT_FOUND`.
- `POST /v1/stream/stop` body `{sessionId?: string, stopAll?: bool}` → `{"status":"stopped"}`. `stopAll: true` wins. Neither field → 400 `INVALID_REQUEST`. Unknown ID → 404. For relay sessions the forwarded stop is best-effort, so an unreachable peer never fails the stop.

### 4.5 `GET /v1/status` and `GET /v1/ping`

`StatusResponse` (camelCase): `gpuReady`, `gpuName`, `gpuVendor`, `gpuUtilization`, `encoderBackend`, `selectedExecutor` (`"nvidiaSpecialized" | "amdSpecialized" | "universal" | "cpu"`), `nvidiaAiAvailable`, `amdAiAvailable`, `activeSessions`, `sessions: SessionInfo[]`, `primarySession`, `settings: {resolution, quality, framegen, framegenTarget, framegenEngine, hdr}`, `encoderHasFramegen` (FRUC), `encoderHasNativeDouble` (NVIDIA Framegen or AMD FRC), `encoderHasTruehdr`, `encoderHasHdrTonemap` (VCEEncC libplacebo SDR→HDR10), `encoderHasRife`.

- `gpuReady` = supported local GPU (vendor `nvidia|amd` and `backend` set) **and** `setup_complete`. When relay is enabled, it reflects the linked peer's `/v1/ping` instead, read through the cached `RelayLinkMonitor` (`relay::gpu_ready_for_status`, [§11](#11-lan-relay-referee-relay)).
- `selectedExecutor`: the primary session's executor, or a prediction from current settings (`status::selected_executor_for_status`).
- Capability flags are `null` until encoder capabilities have been probed.
- With a relay-backed primary session, GPU, executor, capability, and `gpuReady` fields are replaced by the remote peer's `/v1/status` values (2 s timeout). The link probe is skipped when that succeeds.
- Every status producer (`/v1/status`, `status-update`, the desktop status loop, `stop_stream`) runs this final pass through `status::finalize_status`.
- Sessions appear in `sessions` **while still starting** (`startupStage: "starting"`), before `/v1/stream/start` returns.

`PingResponse`: `instanceId`, `hostname`, `version` (crate version), `platform` (`std::env::consts::OS`), `gpuReady`, `gpuVendor`, `gpuName`.

### 4.6 `POST /v1/settings/stream`

Takes a JSON object patch and applies it through `SettingsManager::merge_and_update`. It accepts every key that method handles, not only stream keys: `resolution`, `quality`, `framegen`, `framegenTarget`, `framegenEngine`, `hdr`, `executorPreference`, `alwaysOnTop`, `showOnProxyStart`, `minimizeToTray`, `closeToTray`, `notifications`, `autoUpdate`, `encodingProfiles`, `player`, `instanceId`, `relay`. The one exception is `easyProxy`, which is dropped from the patch: the input proxy trusts that server's origin past the SSRF guard (§10.6), so only the desktop app or `config.json` may set it. It refreshes `ServerSettings` and emits `settings-sync` on desktop. Used by relay peers to push stream settings to the processing machine. Non-object → 400 `INVALID_SETTINGS`.

### 4.7 Error envelope and codes

```json
{ "code": "SESSION_NOT_FOUND", "error": "Session not found" }
```

`/v1/tmp` and `/v1/input` return bare status codes (no JSON).

| Code | HTTP | Raised by |
|---|---|---|
| `INVALID_REQUEST` | 400 | Missing `url`; stop with neither field |
| `INVALID_URL` | 400 | Non-http(s) or unparsable `url` |
| `INVALID_HEADERS` | 400 | Blocked header name or CR/LF in value |
| `SSRF_BLOCKED` | 400 | URL resolves to loopback/link-local, or DNS fails |
| `MISSING_ORIGIN` / `INVALID_ORIGIN` | 400 | `/v1/auth/request`, `POST /v1/origins` |
| `INVALID_SETTINGS` | 400 | Settings patch is not an object |
| `UNAUTHORIZED` | 401 | Token middleware |
| `HEADLESS_MODE` | 403 | Unapproved origin on headless `/v1/auth/request` |
| `NO_APP_HANDLE` | 403 | Desktop consent with no app handle |
| `CONSENT_DENIED` | 403 | User denied |
| `SESSION_NOT_FOUND` / `ORIGIN_NOT_FOUND` | 404 | Heartbeat/stop/delete-origin |
| `CONSENT_TIMEOUT` | 408 | No decision in 180 s |
| `RATE_LIMITED` | 429 | Auth or stream-start limiter |
| `PIPELINE_EXITED` | 502 | Session vanished before the playlist appeared |
| `EASYPROXY_UNAVAILABLE` | 502 | EasyProxy routing is on, but the managed server is still installing, failed to install, or failed to start, or the external server's URL is invalid, it is unreachable, or it refuses the API password (§10.6–10.7) |
| `EASYPROXY_EXTRACTION_FAILED` | 502 | EasyProxy's extractor found no video at the source URL, did not answer in time, or returned a video on a loopback or link-local address (§10.6) |
| `RELAY_UNAVAILABLE`, `RELAY_START_FAILED`, `RELAY_STATUS_SYNC_FAILED` | 502 | Relay stream start ([§11](#11-lan-relay-referee-relay)) |
| `NO_ENCODER` | 503 | Supported vendor but no encoder/backend resolved |
| `RELAY_REQUIRED` | 503 | No supported local GPU vendor and relay not enabled |
| `PIPELINE_TIMEOUT` | 504 | Playlist not ready within 180 s (session is cleaned up) |

To add a code, put the constant in `server.rs::error_codes` and build responses with `responses::error_response` / `json_error_response`.

---

## 5. End-to-end: from `POST /v1/stream/start` to HLS

```
POST /v1/stream/start                                   server/stream.rs::handle_stream_start
  │ rate limit → validate URL → SSRF check → validate headers
  ├─ relay enabled? ── yes ──► handle_relay_stream_start ──► peer /v1/stream/start   (§11)
  ▼ no
handle_local_stream_start
  ├─ resolve_runtime_backend(GpuInfo)        → RuntimeBackend {vendor, backend, encoder, ffmpeg, rife paths}
  ├─ resolve_easy_proxy_route()              → Option<EasyProxyRoute>; starts or checks the server (502 EASYPROXY_UNAVAILABLE)   (§10.6–10.7)
  ├─ extract_easy_proxy_source()             → EasyProxyRoute::extract: EasyProxy URL + password header, when routing is on (502 EASYPROXY_EXTRACTION_FAILED)
  ├─ stop_existing_sessions()                → evictedSessions   (one session at a time)
  ├─ clamp_quality(); create {tmp}/RefereeProxy/{id}/
  ├─ build_startup_context()
  │    ├─ describe_source()                  source/      → SourceDescriptor
  │    ├─ (apply contentKind override)
  │    ├─ build_pipeline_request_from_parts() → PipelineRequest   (§6.1)
  │    ├─ build_planner_capabilities()        → BackendCapabilities (executor kind chosen here, §6.2–6.3)
  │    ├─ GraphPlanner::plan()                graph/       → ExecutionPlan (§6.4)
  │    └─ disable_universal_portable_interpolation()       (§6.5)
  ├─ encoding profile = settings.encoding_profiles[res] or get_encoding_profile(res)
  ├─ Session::new_local(SessionInfo{startup_stage:"starting"}) → SessionMap
  ├─ start_pipeline(PipelineLaunch)           pipeline/run.rs → spawns a tokio task and returns
  │    ├─ SessionPipeline::prepare()          pipeline/launch.rs
  │    │    ├─ detect_encoder_capabilities(), detect_ffmpeg_filter("hqdn3d")   (cached per binary)
  │    │    └─ to_intermediate_plan() → build_runtime_executor()   (§7.2)
  │    ├─ should_attempt_staged_launch()?
  │    │    yes → launch_staged()          normalizer → [preprocess] → executor → packager
  │    │    no  → launch_single_process()  one executor process, inline HLS mux
  │    └─ loop: PipelineSupervisor::run(hb_rx) → resolve_attempt() → retry? → finalize_pipeline_session()   (§8)
  ├─ wait_for_packager_playlist(index.m3u8, 180 000 ms, poll 100 ms)
  ├─ session.mark_startup_ready() → startup_stage "ready"; emit status-update; optional notification
  └─ 200 { sessionId, url, resolution, sourceResolution, effectiveQuality, evictedSessions }

Playback:  player ──GET /v1/tmp/{id}/index.m3u8, segment_NNNNNN.ts──► serve_tmp_file (path-traversal guarded)
Keepalive: client ──POST /v1/stream/heartbeat/{id} every ~10 s──► mpsc → supervisor resets 15 s deadline
Teardown:  POST /v1/stream/stop | heartbeat timeout | stage exit → cleanup_session / finalize_pipeline_session
```

---

## 6. Planning layer (`graph/`)

Everything in `graph/` is pure (no I/O, no process spawning). Side effects such as probing happen before the planner (`source/`, `pipeline/`) or after it (`exec/`).

### 6.1 Building `PipelineRequest` (`server/stream.rs::build_pipeline_request_from_parts`)

| Field | Source |
|---|---|
| `source_transport`, `source_kind` | `SourceDescriptor.classification` |
| `source_content_kind` | Probe result, or the `contentKind` override; default `Unknown` |
| `source_resolution`, `source_fps` | Probe metadata (remote HLS only) |
| `output_resolution` | `settings.resolution` (`1920x1080` \| `2560x1440` \| `3840x2160`) |
| `latency_mode` | Always `LatencyMode::Low` |
| `upscale` | `Quality(q)` only if source dimensions are known **and** output width or height is greater; otherwise `Off` (`build_upscale_request`) |
| `interpolation` | `FramegenSettings::interpolation_request` (`pipeline/framegen.rs`): `framegen` off → `Off`. Engine `nativeDouble` (alias `nvidiaFramegen`, the pre-AMD name) → `NativeDouble`. Otherwise `TargetFps(n)` with n = 30, 60, or, for `matchDisplay`, the display refresh rate from `displayRefreshRate` (60 when unknown). Truncated Windows rates are restored (59 → 60), and the rate is divided by the smallest whole number that brings it within 144 fps (120 fps at 4K, the HEVC level 5.2 limit), so 144 Hz → 144 at 1080p but 72 at 4K, and 240 Hz → 120. Engine `rife` maps to `RifeTargetFps(n)` when RIFE can run for the stream (`RuntimeBackend::has_rife` and a known source fps), else `TargetFps(n)`, logged. When the request is `RifeTargetFps`, `build_startup_context` also plans the same request as `TargetFps(n)` into `rife_fallback_plan` ([§8.3](#83-retries)). |
| `hdr` | `settings.hdr` → `TonemapToHdr10` / `Off`. `Passthrough10Bit` and `InjectHdr10Metadata` are never produced by the server today. |

`UpscaleRequest::Anime4k2x` / `Artcnn2x` exist for forcing a shader but are not produced by the server either.

### 6.2 Executor kind selection (`pipeline/capabilities.rs::select_executor_kind_for_request`)

| `ExecutorPreference` | Result |
|---|---|
| `Universal` | `Universal` |
| `NvidiaAi` | `NvidiaSpecialized` if `nvidia_ai_available` (Windows + vendor `nvidia` + any of `has_vpp_resize`/`has_fruc`/`has_truehdr`), else `Universal` |
| `AmdAi` | `AmdSpecialized` if `amd_ai_available` (vendor `amd` + VCEEncC, detected by `has_vpp_resize`; Windows or Linux), else `Universal` |
| `Auto` (default) | `NvidiaSpecialized` if Windows + `nvidia` + the request benefits: `Quality(1..=4)` with `has_vpp_resize`, **or** interpolation that raises the frame rate (`InterpolationRequest::raises_frame_rate`) with `has_fruc`, **or** `TonemapToHdr10` with `has_truehdr`. Otherwise `AmdSpecialized` if `amd_ai_available`, whatever the request. Otherwise `Universal`. |

`ExecutorKind::Cpu` is never returned. At runtime it renders through the Universal executor ([§7.2](#72-runtime-executor-resolution)).

### 6.3 Capabilities: probe → inventory → planner view

```
<encoder> --help ──parse_encoder_backend_capabilities(help, backend)──► BackendFamilyCapabilities
      └─ EncoderCapabilities::from_backend_capability_profile ──► EncoderCapabilities {has_vpp_resize, has_fruc, has_truehdr, has_rife, has_native_double, has_hdr_tonemap}
            └─ to_backend_capability_inventory_for_request(platform, vendor, backend, request, preference)
                  ──► BackendCapabilityInventory {selected_executor, selected_backend, vendor_native_backend, universal_fallback}
                        └─ to_backend_capabilities() ──► BackendCapabilities {executor, resize, interpolation, hdr, upscale}
```

- **NVEncC parsing** (`exec/nvidia.rs::parse_nvenc_capabilities`): an `ngx: yes|no` banner line overrides everything. Without it, `has_vpp_resize` needs `--vpp-resize` plus an NGX marker (`ngx-vsr`, `vsr-quality`, or `--vpp-ngx-truehdr`). `has_fruc` comes from an `nvof fruc: yes` banner or the presence of `--vpp-fruc`. `has_truehdr` comes from `--vpp-ngx-truehdr`. `has_native_double` needs `--vpp-nvvfx-framegen` and no `nvvfx: no` banner, and `detect_encoder_capabilities` also requires an Ada or newer GPU (`gpu::nvidia_gpu_supports_frame_generation`: `nvidia-smi --query-gpu=compute_cap` ≥ 8.9, probed once per process). NVEncC 9.38+ bundles the Maxine runtime (`NVVideoEffects.dll`, `nvVFXVideoFrameGeneration.dll`).
- **VCEEncC parsing** (`parse_encoder_backend_capabilities`): `--vpp-resize` → `resize: Approximate` → `has_vpp_resize`. `--vpp-frc` → `frame_generation: Exact` → `has_native_double`, which `detect_encoder_capabilities` also limits to Windows (AMF FRC needs DX11). `--vpp-libplacebo-tonemapping` → `hdr_transform` and `metadata_injection: Exact` → `has_hdr_tonemap`. `--help` cannot show whether the GPU and driver support AMF FRC or FSR; a VCEEncC startup failure falls back to Universal ([§8.3](#83-retries)). `detect_vceenc_amf_fsr11` checks the same help text for `amf_fsr_11`.
- **Backend string `"universal"`** (FFmpeg as encoder; see [§12.1](#121-gpu-detection-gpurs)) falls into the `other` arm → `LegacyCompatibility`, all flags `false`.
- `has_rife` = `rife-worker` and a RIFE model are both resolved (`GpuInfo::has_rife` at startup, `StreamingRifeParams` in `SessionPipeline::prepare`). The UI offers RIFE only when it is true.
- **Inventory:** `selected_backend` is the vendor-native profile for `NvidiaSpecialized` and `AmdSpecialized`, the universal FFmpeg profile (`resize/hdr_transform/metadata_injection: Exact`, `interpolation: Unavailable`) for `Universal`, and legacy for `Cpu`. `vendor_native_backend` is dropped if it has the same family as the selected one.
- **`to_backend_capabilities`:**

| Output field | Rule |
|---|---|
| `resize` | `QualityRange{1,4}` if the selected backend can resize, else `Unsupported` |
| `interpolation` | `NativeWithFramegen` if the selected backend has FRUC and frame generation, `Native` with FRUC only, `FramegenOnly` with frame generation only (AMD FRC), else `Unsupported` |
| `hdr` | NVIDIA / AMD: `tonemap` = TrueHDR / VCEEncC libplacebo tonemapping, `inject` = false on NVIDIA and `has_hdr_tonemap` on AMD, `passthrough = false`. Universal: all `true` (FFmpeg profile). Cpu: all `false`. |
| `upscale.selected_path` | `NativeBackend(NvidiaNgxVsr \| AmdVceEncResize)` if the selected backend is NVIDIA / AMD with resize. **Else** `TemporaryUniversalCompatibility` (Universal; an unused AMD resize is kept as `preferred_native_backend` for diagnostics only) or `SharedPreprocess` (NVIDIA/AMD/Cpu). |

### 6.4 `GraphPlanner::plan` rules (`graph/planner.rs`)

Op order is fixed: `NormalizeInput` → *(one upscale op)* → `Interpolate` → `Hdr`. The upscale op is either `Resize` **or** a shader op (`Anime4k2xUpscale` / `Artcnn2xUpscale`), never both.

**Upscale.** It is planned when `quality.is_some()` or the known source resolution differs from the output. `Anime4k2x`/`Artcnn2x` requests count as quality 2.

| `upscale.selected_path` | Condition | Op emitted |
|---|---|---|
| `NativeBackend` | `native_resize_supports_request` (quality within 1..4) | `Resize{stage: Executor}` |
| `NativeBackend` | quality out of range | `Resize{stage: Preprocess}` |
| `SharedPreprocess` | — | `Resize{stage: Preprocess}` |
| `TemporaryUniversalCompatibility` | executor `Universal` | `Anime4k2xUpscale{Executor}` if content `Animated` or request `Anime4k2x`; `Artcnn2xUpscale{Executor}` for `LiveAction`/`Unknown` or `Artcnn2x`; falls back to `Resize{Executor}` if no shader op applies |

**Interpolation.** The target is `InterpolationRequest::target_fps`: `TargetFps(n)` → n; `NativeDouble` → `round(2 × source_fps)` (60 when fps is unknown). A source counts as already at the target when it reaches 99% of it (`TARGET_FPS_TOLERANCE`), so 29.97 and 59.94 sources are not interpolated to 30 or 60. Doubling is skipped for sources at 60 fps or above.

| Condition | Plan |
|---|---|
| `Off` | none |
| source already at the target | `Disabled(SourceAlreadyAtOrAboveTargetFps)` |
| `RifeTargetFps` (any executor) | `Rife @ Preprocess` |
| `NativeDouble` and `NativeWithFramegen` or `FramegenOnly` | `NativeBackend(NvidiaFramegen \| AmdFrc) @ Executor` (`native_double_engine`) |
| `Native` or `NativeWithFramegen` | `NativeBackend(NvidiaFruc) @ Executor` at the exact target (24 → 30, 24 → 60). Doubling without frame generation falls back here. |
| `Unsupported` or `FramegenOnly` | `PortableFallback @ Preprocess` at the target. On AMD this is every fixed target (FRC only doubles) and doubling without FRC. |

**HDR** (`TonemapToHdr10`, the only mode the server sends): 10-bit, BT.2020 / SMPTE 2084 / BT.2020nc. Metadata `Hdr10Static` goes to `Executor` if `inject_hdr10_metadata`, else `Packager`. Transform kind is `NvidiaTrueHdrTonemapToHdr10` / `AmdTonemapToHdr10` when the executor is `NvidiaSpecialized` / `AmdSpecialized` with tonemap support, else `TonemapToHdr10`. Transform stage is `Executor` if `tonemap_to_hdr10`, else `Preprocess`.

### 6.5 Post-plan adjustment

`server/stream.rs::disable_universal_portable_interpolation`: if the executor is `Universal` **and the vendor is not AMD**, any `PortableFallback` interpolation is replaced with `Disabled(PortableFallbackNotImplemented)`. As a result, without RIFE, **frame generation works only on NVIDIA via NVEncC and on AMD via AMF FRC or the staged preprocess path.** `framegen_enabled` / `target_fps` in `SessionInfo` are computed after this step.

### 6.6 `IntermediateExecutionPlan` (`graph/intermediate.rs`)

`ExecutionPlan::to_intermediate_plan()` converts each `VideoOp` into an `IntermediateOperation` with an `IntermediateOpBinding {owner, conceptual_owner, accelerator, unresolved_reason}`. `Hdr` splits into `HdrMetadata` + `HdrTransform`.

| Op | `owner` | `conceptual_owner` | `accelerator` |
|---|---|---|---|
| `NormalizeInput` | `Normalizer` | `Normalizer` | — |
| `Resize @ Executor`, NVIDIA, quality set | `Executor` | `Executor` | `NvidiaNgxVsr` |
| `Resize @ Executor`, AMD | `Executor` | `Executor` | `AmdVppResize` |
| `Resize @ Executor`, other | `Executor` | `SharedPreprocess` | — |
| `Resize @ Preprocess` | `SharedPreprocess` | `SharedPreprocess` | — |
| `Interpolate` native @ Executor, NVIDIA or AMD | `Executor` | `Executor` | `NvidiaFruc`, `NvidiaFramegen`, or `AmdFrc` (from the realization's engine) |
| `Interpolate` portable or RIFE @ Preprocess | `SharedPreprocess` | `SharedPreprocess` | — |
| `Interpolate` disabled | `Deferred` | `SharedPreprocess` if `PortableFallbackNotImplemented`, else `Deferred` | — (`unresolved_reason` set) |
| `HdrMetadata @ stage` | stage owner | stage owner | — |
| `HdrTransform` TrueHDR @ Executor, NVIDIA | `Executor` | `Executor` | `NvidiaTrueHdr` |
| `HdrTransform` AMD tonemap @ Executor, AMD | `Executor` | `Executor` | `AmdHdrTonemap` |
| `HdrTransform @ Executor`, other | `Executor` | `SharedPreprocess` | — |
| Shader upscale @ Executor | `Executor` | `SharedPreprocess` | — |

- `ExecutionPlan::for_universal_fallback` (graph level) relabels an `AmdSpecialized` plan `Universal` and disables native-only interpolation (`Disabled(NativeEngineUnavailable)`), so its intermediate plan has no AMD bindings. Used for the FFmpeg retry ([§8.3](#83-retries)).
- `shared_preprocess_plan()` collects ops whose `conceptual_owner == SharedPreprocess` and tags each with a `SharedPreprocessExecutionMode` derived from the concrete `owner`: `SharedPreprocess → PreprocessorOwned`, `Executor → TemporaryExecutorFallback`, `Deferred → DeferredUntilPortableRenderer`.
- `SharedPreprocessPlan::requires_stage()` is true only when at least one op is `PreprocessorOwned`. **This flag decides whether a staged launch is attempted.**
- `NativeAcceleratorPlan.fallback_owner` is always `SharedPreprocess`. **No runtime code reads it.** If a native accelerator is unavailable at runtime, nothing promotes the op automatically.

### 6.7 Worked examples

**A. Windows + NVIDIA, NVEncC with NGX/FRUC/TrueHDR, 1280×720@30 HLS → 1920×1080, framegen + HDR on**

- Executor: `NvidiaSpecialized`. Capabilities: resize 1..4, interpolation `Native`, HDR tonemap only, upscale `NativeBackend(NvidiaNgxVsr)`.
- Plan: `Resize{1080p, q, Executor}`, `Interpolate{60, Native@Executor}`, `Hdr{metadata@Packager, transform NvidiaTrueHdr@Executor}`.
- Runtime: single NVEncC process (NVIDIA is never staged). Notable args: `--vpp-resize algo=ngx-vsr,vsr-quality=q --vpp-fruc fps=60/1 --avsync forcecfr --colorprim bt2020 --transfer smpte2084 --colormatrix bt2020nc --colorrange limited --vpp-ngx-truehdr --vpp-unsharp … --profile main10 … --format hls`.

**B. Linux + NVIDIA (or Windows + NVIDIA without NVEncC), same request, content detected `Animated`**

- Backend `"universal"` → all encoder flags false → executor `Universal`, upscale `TemporaryUniversalCompatibility`.
- Plan: `Anime4k2xUpscale{Executor}`, `Interpolate{PortableFallback@Preprocess}` → **disabled** by §6.5, HDR all `@Executor`.
- Runtime: `UniversalExecutor` (`hevc_nvenc`), single FFmpeg process. libplacebo runs the Anime4K shader + CAS, tonemaps to HDR10, and writes HLS directly. No frame generation.

**C. Windows + AMD, VCEEncC with `--vpp-resize`, `--vpp-frc`, and `--vpp-libplacebo-tonemapping`, same request with Frame Rate "Double via AMD Fluid Motion"**

- Executor `AmdSpecialized`. Capabilities: resize 1..4, interpolation `FramegenOnly`, HDR tonemap + inject, upscale `NativeBackend(AmdVceEncResize)`.
- Plan: `Resize{1080p, q, Executor}`, `Interpolate{60, NativeBackend(AmdFrc)@Executor}`, `Hdr{metadata@Executor, transform AmdTonemapToHdr10@Executor}`.
- Runtime: single VCEEncC process (nothing is preprocess-owned). Notable args: `--vpp-libplacebo-tonemapping src_csp=sdr,dst_csp=hdr10,…,inverse_tone_mapping=true,dst_max=1000 --output-res 1920x1080 --vpp-resize amf_fsr_11 --vpp-scaler-sharpness 0.75 --vpp-frc profile=high,search=native --avsync forcecfr --vpp-unsharp … --colorprim bt2020 … --max-cll 1000,400 --master-display … --output-depth 10 --profile main10 -c hevc … --gop-len 60 --format hls`.
- With Frame Rate "60 FPS" instead, FRC does not apply (it only doubles), so `Interpolate{PortableFallback@Preprocess}` stages the launch: FFmpeg normalizer → `FfmpegPreprocessor` (`minterpolate=fps=60`) → VCEEncC (`--avsw -i -`, MPEG-TS to stdout) → FFmpeg packager. RIFE is the better-looking choice for fixed targets.

---

## 7. Runtime execution

### 7.1 Launch mode: single-process vs staged

`pipeline/launch.rs::should_attempt_staged_launch(source, plan)`:

| Condition | Mode |
|---|---|
| Source is HLS-like and has no input relay | single-process |
| `ExecutorKind::NvidiaSpecialized` with RIFE | **staged** (NVEncC reads the worker's NUT on stdin with `--avsw -i -`) |
| `ExecutorKind::NvidiaSpecialized` otherwise | single-process (on purpose: the staged FFmpeg→NVEncC handoff caused timeline jumps and A/V drift; the RIFE path avoids it, [§7.5](#75-normalizer-normalizeffmpeg_normalizerrs-staged-only)) |
| `ExecutorKind::Universal` or `AmdSpecialized`, and the shared preprocess plan `requires_stage()` | **staged** |
| `ExecutorKind::Universal` or `AmdSpecialized` otherwise | single-process |
| `ExecutorKind::Cpu` | single-process |

If a staged launch fails to build or spawn, the session falls back to single-process **for that attempt and all later ones** (`SessionPipeline::launch_attempt` clears `allow_staged`). When the plan needs preprocessing but runs single-process, a warning is logged that preprocess-owned work (interpolation, resize, HDR) will be skipped.

**Single-process topology** (`launch_single_process`). The stage context comes from `StageRuntimeContext::single_process`: input `SourcePull`, output `HlsOutput`.

```
source ─► /v1/input/{id}?url=… (remote HLS) ─► [Executor: NVEncC | VCEEncC | ffmpeg]
                                                ingest + filters + HEVC/AAC encode + inline HLS mux
                                                         │
                                                         ▼
                                      {tmp}/RefereeProxy/{id}/index.m3u8 + segment_%06d.ts
```

**Staged topology** (`launch_staged`; contexts from `StagedPipelineContexts::new`):

```
/v1/input/{id}?url=…
   │
   ▼
Normalizer (ffmpeg) ─NUT─► [Preprocess: rife-worker | ffmpeg] ─NUT─► Executor ─MPEG-TS─► Packager (ffmpeg) ─► index.m3u8 + segments
```

Every arrow is a `tokio::io::copy_buf` bridge task (`PipelineStageConnection`, 1 MiB `BRIDGE_BUFFER_BYTES` read buffer) that the supervisor spawns between `ChildStdout` and `ChildStdin`. On Windows, tokio child pipes run every read and write on the blocking pool, so the buffer size sets the number of thread hops per second on rawvideo hops. Processes are not joined with raw OS pipes. All stages are spawned up front, in order (`spawn_connected_stages`). If any spawn or connect fails, already-spawned stages are killed in LIFO order.

### 7.2 Runtime executor resolution

`exec/family.rs::select_runtime_executor_target` (returns `RuntimeExecutorTarget::{Nvidia, Amd, Universal}` from the executor kind alone) + `pipeline/launch.rs::build_runtime_executor`:

| `ExecutorKind` | Runtime executor |
|---|---|
| `NvidiaSpecialized` | `NvidiaSpecializedExecutor` (NVEncC) |
| `AmdSpecialized` | `AmdExecutor` (VCEEncC). A startup failure replans the session onto `Universal` ([§8.3](#83-retries)). |
| `Universal` | `UniversalExecutor` (FFmpeg) on any vendor, including AMD without VCEEncC or with the Universal preference |
| `Cpu` | `UniversalExecutor`. Unreachable in practice: selection never returns `Cpu`. |

`UniversalExecutor` encode backend (`select_universal_encode_backend`):

| Platform + vendor | FFmpeg encoder | HW decode on source pull | Pixel format (8/10-bit) |
|---|---|---|---|
| Linux + NVIDIA | `hevc_nvenc` | `-hwaccel cuda` | nv12 / p010le |
| Windows + NVIDIA | `hevc_nvenc` | `-hwaccel d3d11va` | nv12 / p010le |
| Linux + AMD | `hevc_vaapi` (`-vaapi_device /dev/dri/renderD128`, `format=…,hwupload`) | — | nv12 / p010le |
| Windows + AMD | `hevc_amf` (`-quality balanced -rc vbr_latency`) | `-hwaccel d3d11va` | nv12 / p010le |
| other | `libx265` (`-preset medium`) | — | yuv420p / yuv420p10le |

### 7.3 Wire formats

| Hop | Container | Video | Audio |
|---|---|---|---|
| Source → normalizer / single-process executor | whatever the source is (via input proxy) | — | — |
| Normalizer → preprocess / executor | NUT | `rawvideo` yuv420p (nv12 if backend is `nvenc`, which never stages) | `pcm_s16le` (`aresample async=0`, PTS rebased to 0) |
| `FfmpegPreprocessor` → executor | NUT | `rawvideo` yuv420p / yuv420p10le, or `-c copy` passthrough | `copy` |
| `rife-worker` → executor | NUT | `rawvideo` nv12 (NVEncC) or yuv420p, CFR at the target fps, starting at 0 | `pcm_s16le`, starting at 0 (passed through) |
| Executor → packager (staged) | MPEG-TS (`-flush_packets 1` for FFmpeg) | HEVC | AAC |
| Packager / single-process output | HLS: `index.m3u8` + `segment_%06d.ts` (MPEG-TS segments) | HEVC (`hvc1` tag on Universal inline HLS) | AAC (Universal: 160 kb/s) |

### 7.4 Executors (`exec/`)

Shared behavior:
- **Input args.** Rigaya CLIs (NVEncC/VCEEncC) use `exec::build_rigaya_input_args`. Source pull adds `-i <url>`, `--input-option headers:` (`source::ffmpeg_header_block` with `BROWSER_USER_AGENT` + forwarded headers), and reconnect options. Low-latency mode adds `fflags:nobuffer`. Buffered mode adds `--input-analyze 4 --input-probesize 1000000`. Stdin uses `--avsw -i -`. FFmpeg builds its own input args: `-re`, reconnect, `+genpts[+nobuffer]`; stdin uses `-f nut -i pipe:0`.
- **Denoise.** If the resolved FFmpeg has `hqdn3d` (`detect_ffmpeg_filter`), source-pull paths denoise. Rigaya uses `--vpp-convolution3d ythresh=4,cthresh=3,t_ythresh=6,t_cthresh=4.5`; Universal uses `hqdn3d`; the staged normalizer appends `hqdn3d`.
- **Sharpening.** `CAS_DEFAULT_STRENGTH = 0.5` is always on. Rigaya uses `--vpp-unsharp radius=3,weight=0.50,threshold=10.0`. Universal renders `shaders/cas/Referee_CAS.glsl` with `{{CAS_STRENGTH}}` substituted, concatenated after any upscale shader into `{session_dir}/referee-universal-runtime.glsl`, and passed to libplacebo as `custom_shader_path` (process `current_dir` = session dir).
- **Output.** Rigaya CLIs share `exec::build_rigaya_output_args` (AAC plus inline HLS or MPEG-TS on stdout; NVEncC adds `--strict-gop` and `hls_delete_threshold` through `RigayaInlineHls`). FFmpeg HLS muxing (Universal inline HLS and the packager) shares `exec::build_ffmpeg_hls_output_args`. The playlist window (`HLS_PLAYLIST_SIZE = 8`) and segment pattern live in `runtime/stages.rs`.
- **GOP.** `exec::segment_aligned_gop` = `round(output_fps × hls_time)`, so keyframes align to segment boundaries. `output_fps` is `StageRuntimeContext::planned_output_fps` (`ExecutionPlan::output_frame_rate`: the interpolation target when interpolation runs, even with an unknown source rate, else the source rate), with a fallback of 60, or 30 on NVIDIA.
- **Rate control** comes from `EncodingProfile` ([§7.8](#78-encoding-profiles)). All executors use readiness `ReadyOnHeartbeat` (the `PipelineStageId::Executor` default).

**`NvidiaSpecializedExecutor`** (`exec/nvidia.rs`, log label `nvenc`, stderr `Raw`)
- Renders from intermediate bindings. `Resize` owned by the executor sets `--output-res` and, with `NvidiaNgxVsr`, adds `--vpp-resize algo=ngx-vsr,vsr-quality=N`. A preprocess-owned `Resize` still sets `--output-res` ("CompatibilityFallback"). `NvidiaFruc` adds `--vpp-fruc fps=<round(output_fps)>/1`. `NvidiaFramegen` adds `--vpp-nvvfx-framegen multiplier=2,mode=high` below 4K output and `mode=medium` at 4K (`framegen_mode`; `high` costs about 1.5× `medium`). NVEncC runs both **after** VSR and TrueHDR, at output resolution, and converts to 8-bit RGB for them, so frame generation with HDR processes the HDR-mapped frames in 8 bits. Any HDR op adds `--colorprim/--transfer/--colormatrix/--colorrange` and sets `--profile main10` for 10-bit. `NvidiaTrueHdr` adds `--vpp-ngx-truehdr`.
- **Rejects** shader upscale ops with `InvalidInput`.
- Source pull from HLS **always uses buffered ingest**, even on attempt 1 (startup mode `Buffered`). Source pull adds `--crop` to match the target aspect ratio. `--avsync forcecfr` is always set.
- `--multipass` is `1pass` or `none` (probed via `--help`). `--cu-max 64` at ≥ 1440p. `--aq --aq-temporal`. Inline HLS adds `--strict-gop`, `hls_list_size:8`, `hls_delete_threshold:8`, and, **only when the source fps is known**, `--max-procfps ceil(source_fps × 1.05)`. The cap counts input frames, before FRUC or frame generation adds any. The 5% `PROCESSING_FPS_CAP_HEADROOM` exists because NVEncC's limiter (`CProcSpeedControl`) resets its reference clock when it falls behind, so at exactly 1.0× any stall becomes permanent lag behind the live edge.

**`AmdExecutor`** (`exec/amd.rs`, log label `vceenc`, stderr `Raw`), the AMD counterpart of the NVIDIA executor
- Renders from intermediate bindings. `Resize` sets `--output-res`; with `AmdVppResize` it adds `--vpp-resize` and `--vpp-scaler-sharpness` by quality (`AmfResize::for_quality`): q1 `amf_fsr` 0.5, q2 FSR 1.1 (`amf_fsr_11` when `detect_vceenc_amf_fsr11`, else `amf_fsr`) 0.5, q3 0.75, q4 1.0. `AmdFrc` adds `--vpp-frc profile=high,search=native` (2×). `AmdHdrTonemap` adds `--vpp-libplacebo-tonemapping src_csp=sdr,dst_csp=hdr10,tonemapping_function=spline,inverse_tone_mapping=true,dst_max=1000` (`spline` has an inverse; libplacebo's `bt2390` does not). Executor-owned HDR metadata adds `--max-cll 1000,400` and a BT.2020 1000-nit `--master-display`. Any HDR op adds the shared `--colorprim/--transfer/--colormatrix/--colorrange` (`push_rigaya_hdr_color_args`) and `--output-depth 10 --profile main10` for 10-bit, else `--profile main`.
- VCEEncC applies filters in a fixed order whatever the argument order: tone mapping, denoise, resize, sharpening, then FRC at output resolution, as FRUC runs after VSR on NVIDIA.
- **Rejects** shader upscale ops with `InvalidInput` (the planner only produces them for Universal).
- Shares the NVIDIA executor's source handling through `exec/mod.rs`: buffered ingest for HLS pulls (`rigaya_startup_mode`), `--vpp-convolution3d` denoise and the aspect `--crop` on source pulls, `--avsync forcecfr` always, `--vpp-unsharp` CAS, and `--max-procfps ceil(source_fps × 1.05)` on inline HLS when the source fps is known (`rigaya_processing_fps_cap`; it counts input frames, before FRC). Inline HLS keeps evicted segments like NVIDIA but has no `--strict-gop` (VCEEncC lacks it).
- Encode: `-c hevc --preset <fast|balanced|slow>` (`p1/p2 → fast`, `p6/p7 → slow`, else `balanced`), `--vbr`, `--max-bitrate`, `--bframes`, `--pa lookahead=N`, `--gop-len` from the segment-aligned GOP (doubled with FRC).

**`UniversalExecutor`** (`exec/universal.rs`, log label `ffmpeg-universal-{nvenc|vaapi|amf|cpu}`, stderr `FfmpegProgress`)
- Builds a `UniversalFilterPlan` from executor-owned ops only. Filter chain: `[hqdn3d,] libplacebo=… [, format=…[,hwupload]]`. libplacebo args: aspect-preserving resize (`force_original_aspect_ratio=decrease`, `normalize_sar`, `pad_crop_ratio=0`). Scaler is `bilinear` (q1), `spline36` (q2), or `ewa_lanczos` (q3–4), and `none` when a shader does the scaling. Then `custom_shader_path`, `tonemapping=bt.2390`, color tags, and `format=yuv420p[10le]`.
- Because CAS is always on, libplacebo always runs, so a Vulkan device is initialized (`-init_hw_device vulkan=vk:0 -filter_hw_device vk`) for any non-CPU backend.
- `-progress pipe:2 -nostats`. The supervisor turns progress blocks into `N frames: X fps, Y kbps, GPU g%, CPU c%` log lines. GPU % is the shared non-blocking sample from `gpu::latest_utilization`, never an inline `nvidia-smi` call, so the stderr reader keeps draining the pipe.
- A `deferred_interpolation` flag is recorded when interpolation was planned but is not preprocess-owned. FFmpeg never interpolates in the executor.

**`FfmpegHlsPackager`** (`exec/ffmpeg_packager.rs`, staged only, log label `ffmpeg-hls-packager`, readiness `ReadyOnSpawn`)
- `-f mpegts -i pipe:0 -map 0:v:0 -map 0:a:0? -c copy -f hls -hls_time <profile.hls_time> -hls_list_size 8 -hls_segment_type mpegts -hls_flags delete_segments -hls_segment_filename segment_%06d.ts index.m3u8`. CWD is the session dir, and the playlist and segments must share a directory.
- Does not touch HDR metadata. `HdrMetadata @ Packager` ops are realized only when an executor applies the HDR intent.

### 7.5 Normalizer (`normalize/ffmpeg_normalizer.rs`, staged only)

- Input must be `SourcePull` and output `StdoutPipe`. With an input relay it reads `relay.relay_url` using a `LiveInputProfile` per latency mode (`Low`: thread queue 256, analyze 2 s, probesize 3 MB, `+genpts+nobuffer`), plus `-rw_timeout 15 s` and reconnect (max delay 2 s). Otherwise it reads `runtime_url` with forwarded headers.
- `-vf`: optional crop to the target aspect ratio (`with_output_resolution`), `setpts=PTS-STARTPTS`, optional `hqdn3d`. `-fps_mode passthrough`. Audio: `asetpts=PTS-STARTPTS,aresample=async=…:first_pts=0`.
- `with_stream_offsets_preserved(true)` (set when the plan uses RIFE) drops both `setpts`/`asetpts`, so FFmpeg shifts the streams by the input's shared start and the source's A/V offset survives; audio is still padded to start at 0. `rife-worker` then starts both of its output streams at exactly 0, so the executor cannot skew them. Verified with a flash/beep clip through normalizer → worker → NVEncC: sync matched the source.
- Readiness `ReadyOnSpawn`. Stderr is `Filtered`, suppressing `"Invalid timestamps stream="`.

### 7.6 Preprocessors (`preprocess/`)

Selection (`pipeline/launch.rs::build_shared_preprocess_spec`):
1. If the plan uses RIFE (`ExecutionPlan::uses_rife`) and `StreamingRifeParams` is present (worker **and** model resolved): `StreamingRifePreprocessor`. If building its spec fails (**no source fps**), fall through with a warning.
2. `FfmpegPreprocessor`. Portable (non-RIFE) interpolation always uses it, so the AMD "optical flow" engine is `minterpolate` and RIFE is an explicit choice.

| | `StreamingRifePreprocessor` | `FfmpegPreprocessor` |
|---|---|---|
| Program | `rife-worker` | `ffmpeg` |
| Args | `--ffmpeg-path --model-path --source-fps --target-fps --pix-fmt <nv12\|yuv420p>` (Windows `\\?\` prefix stripped; GPU picked by ncnn, discrete first) | `-f nut -i pipe:0 … -vf <filters> -c:v rawvideo -pix_fmt … -c:a copy -f nut pipe:1` |
| Renders | Interpolation only, any source-to-target ratio | `PreprocessorOwned` ops: Resize/HDR via libplacebo, then `minterpolate=fps=N:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1`. Shader ops are skipped. With no filters it falls back to `-c copy`. |
| Readiness | `ReadyOnStderrSentinel { "[rife_worker] ready" }` | `ReadyOnSpawn` |
| Failure handling | Exit before startup → retry on `rife_fallback_plan` ([§8.3](#83-retries)) | — |

**`rife-worker` internals** (`rife-worker/src/`): init Vulkan/ncnn and load the model → print `[rife_worker] ready` → read stdin until the NUT stream headers are complete (`nut.rs`: video size, audio present) → spawn an FFmpeg decoder (NUT → rgb24 on an exact source-rate grid from 0 via `fps=…:start_time=0`; audio padded to 0 and sent to a loopback TCP relay) and an FFmpeg encoder (rgb24 at the target rate from 0 + relayed audio with `-copyts` → NUT on the worker's stdout) → `timeline.rs` places each output frame on the target grid as a copy or a blend at an exact timestep (NTSC rates are exact fractions) → a pool of RIFE threads (default 2; concurrent `process_v4` calls on one instance) blends, and an in-order writer feeds the encoder. A lag monitor repeats the nearest frame instead of blending once output is 1 s behind real time, until it is within 0.25 s, so a live stream keeps its place. Stats every 30 s and at exit. `--probe` loads the model and runs one inference. Exit codes: 2 for bad args; fatal errors print `[rife_worker] fatal: …`. Measured on an RTX 4070 SUPER (rife-v4.25-lite): about 50 blends/s at 720p end to end (24→60 in real time) and 24/s at 1080p, where the GPU is the limit and the lag monitor repeats about half of the in-between frames.

### 7.7 `FrameTransport` and `ProcessSpec` (`runtime/stages.rs`)

| `FrameTransport` | Status |
|---|---|
| `SourcePull` | Stage reads the source URL itself (normalizer, single-process executors) |
| `StdoutPipe` | Live. Bridged by the supervisor. |
| `HlsOutput {playlist_path, segment_dir}` | Terminal output of the packager or a single-process executor |

A transport a builder cannot handle returns `FrameTransport::unsupported(stage, direction)` (`InvalidInput`). FFmpeg stages map pipes with `ffmpeg_input_url` / `ffmpeg_output_url` (`pipe:0` / `pipe:1`).

`ProcessSpec` carries `stage`, `program`, `args`, `transport`, `current_dir`, `readiness_policy` (defaults to `PipelineStageId::default_readiness_policy`), `log_label`, and `stderr_mode` (`Raw | Filtered{suppress} | FfmpegProgress{gpu_vendor}`). `spawn_stage` always pipes stderr, sets `kill_on_drop`, and hides the console window; stdin and stdout are piped exactly when the matching transport is `StdoutPipe`. Every stage builder's `build_command` returns a `ProcessSpec` directly, and the stage traits (`BackendExecutor::build_executor_spec(ctx, attempt)` → `ExecutorSpec { process, startup_mode }`, `Normalizer`, `Preprocessor`, `Packager`) wrap it.

### 7.8 Encoding profiles

`pipeline/profile.rs::get_encoding_profile`. Bitrates are kb/s; `max_bitrate` is 1.5 × `bitrate`. A user override in `settings.encoding_profiles[resolution]` replaces the default wholesale.

| Resolution | `bitrate` | `max_bitrate` | `preset` | `lookahead` | `bframes` | `hls_time` |
|---|---|---|---|---|---|---|
| `2560x1440` | 35 000 | 52 500 | `p4` | 12 | 3 | 1 s |
| `3840x2160` | 50 000 | 75 000 | `p4` | 8 | 3 | 1 s |
| other (`1920x1080`) | 25 000 | 37 500 | `p4` | 8 | 3 | 1 s |

`preset` uses NVENC naming (`p1`–`p7`). VCEEncC translates it. Universal NVENC passes it through. AMF, VAAPI, and CPU ignore it.

---

## 8. Supervision, timeouts, and retries

### 8.1 `PipelineSupervisor::run` (`runtime/supervisor.rs`)

Each loop iteration:
1. **Poll children** (`try_wait`). If **any** stage exited (any exit code, including 0): mark it, kill the other stages, abort bridges, flush its stderr (≤ 5 s), and return `StageExited{stage, exit_code}`.
2. `select!` on:
   - **heartbeat message** → `first_heartbeat_seen = true`, deadline = now + **15 s**, mark heartbeat-policy stages ready.
   - **heartbeat channel closed** (the session's sender was dropped by `cleanup_session`) → stop everything → `HeartbeatChannelClosed`.
   - **deadline** (initially now + **300 s**) → kill everything → `OrphanTimeout` (no heartbeat ever) or `InactivityTimeout` (heartbeats stopped). `waiting_on` names the first stage with a heartbeat or sentinel policy. It is diagnostic only.
   - **250 ms tick** → advance stderr-sentinel readiness.

Readiness is **informational**. It never gates spawning or data flow. Session readiness is gated only on the playlist file. Stderr tails keep the last 64 000 bytes per stage for post-mortems.

| Stage | Readiness policy |
|---|---|
| Normalizer, `FfmpegPreprocessor`, Packager | `ReadyOnSpawn` |
| `rife-worker` | `ReadyOnStderrSentinel("[rife_worker] ready")` |
| Every executor | `ReadyOnHeartbeat` (client heartbeat) |

### 8.2 What the session task does with each outcome (`pipeline/run.rs::resolve_attempt`)

| Stop reason | Action |
|---|---|
| `HeartbeatChannelClosed` | Explicit stop: break → `finalize_pipeline_session` |
| `OrphanTimeout` / `InactivityTimeout` | Warn, remove session, finalize |
| `StageExited` | Log the stderr tail (non-zero exit). Evaluate retry ([§8.3](#83-retries)). If no retry: log "Stream ended…", remove session, finalize |

`finalize_pipeline_session` removes the session from the map, deletes the session dir, and marks `PipelineCompletionSignal` complete. `cleanup_session` (stop path) removes the session, drops `heartbeat_tx`, and waits ≤ **10 s** for completion before deleting the dir. Relay-backed sessions skip local teardown; dropping the record closes their keepalive channel.

### 8.3 Retries

`MAX_PIPELINE_START_ATTEMPTS = 2`. A retry happens only while `startup_complete == false` and `attempt < 2`, in one of these two cases:

1. **Executor failure** (`AttemptResolution::Retry`): the failed stage is `Executor` with a non-zero exit, and either
   - the executor is `AmdSpecialized` on attempt 1, whatever stderr says (an AMF filter the GPU or driver lacks fails with varied messages). `SessionPipeline::switch_to_universal_fallback` re-prepares the session on `ExecutionPlan::for_universal_fallback` (FFmpeg with AMF on Windows or VA-API on Linux; FSR becomes libplacebo resize, VCEEncC tonemapping becomes libplacebo HDR, AMF FRC is dropped so the output runs at the source rate), recomputes the launch mode, and updates `SessionInfo`; or
   - the executor is `NvidiaSpecialized` **or** the backend is `vceenc`, and stderr contains `input video info not parsed yet`, `failed to initialize file reader`, or `break in task amfenc`.
2. **RIFE failure:** the plan uses RIFE and the `Preprocess` stage exited non-zero → `SessionPipeline::switch_to_rife_fallback` re-prepares the session on `rife_fallback_plan` (FRUC on NVIDIA, `minterpolate` on AMD, none on Universal-NVIDIA), recomputes the launch mode, and updates `SessionInfo`.

Both replans update `SessionInfo` through `run.rs::sync_session_plan` (`framegenEnabled`, `framegenEngine`, `executor`, `targetFps`). Before a retry, `clear_session_output` deletes every file in the session dir, `retrying_startup` is set, and the attempt number increments. Attempt 2 uses `StartupMode::Buffered` (larger analyze/probe).

### 8.4 Timeout and limit reference

| Name | Value | Location |
|---|---|---|
| `HEARTBEAT_TIMEOUT_MS` | 15 000 | `pipeline/launch.rs` |
| `ORPHAN_SESSION_TIMEOUT_MS` | 300 000 | `pipeline/launch.rs` |
| `PIPELINE_CLEANUP_TIMEOUT_MS` | 10 000 | `pipeline/session.rs` |
| `MAX_PIPELINE_START_ATTEMPTS` | 2 | `pipeline/launch.rs` |
| Playlist wait / poll | 180 000 ms / 100 ms | literal in `server/stream.rs::handle_local_stream_start`; `pipeline/session.rs::wait_for_packager_playlist` |
| `CHILD_POLL_INTERVAL` | 250 ms | `runtime/supervisor.rs` |
| `BRIDGE_BUFFER_BYTES` | 1 MiB | `runtime/supervisor.rs` |
| `INPUT_PROXY_CONNECT_TIMEOUT` / `INPUT_PROXY_READ_TIMEOUT` | 10 s / 15 s | `source/hls_relay.rs` |
| `UTILIZATION_SAMPLE_INTERVAL` | 2 s | `gpu.rs` |
| `PROCESSING_FPS_CAP_HEADROOM` | 1.05 | `exec/nvidia.rs` |
| `STDERR_TAIL_LIMIT` / `STDERR_FLUSH_TIMEOUT` | 64 000 B / 5 s | `runtime/supervisor.rs` |
| `CONSENT_REQUEST_TIMEOUT_SECS` | 180 | `server.rs` |
| `RELAY_STATUS_TIMEOUT_MS` (ping probe) | 1 500 | `server.rs` |
| `RELAY_CONNECT_TIMEOUT_MS` (relay control + playback proxy clients) | 1 500 | `server.rs` |
| `RELAY_TMP_PROXY_READ_TIMEOUT_SECS` | 10 | `server.rs` |
| `RELAY_STREAM_START_TIMEOUT_SECS` | 195 | `server.rs` |
| `RELAY_SESSION_STATUS_TIMEOUT_SECS` / `…_CONTROL_…` | 5 / 5 | `server.rs` |
| `RELAY_LIVE_STATS_TIMEOUT_MS` | 2 000 | `server.rs` |
| `RELAY_STATUS_SYNC_ATTEMPTS` / `…_RETRY_DELAY_MS` | 3 / 250 | `server.rs` |
| `RELAY_KEEPALIVE_INTERVAL_MS` / `RELAY_PEER_LOSS_TIMEOUT_MS` | 3 000 / 20 000 | `server.rs` |
| `RELAY_LINK_PROBE_TTL_MS` / `RELAY_REBIND_SCAN_INTERVAL_SECS` | 3 000 / 30 | `server.rs` |
| `RELAY_DISCOVERY_TIMEOUT_MS` | 1 000 | `commands.rs`, `server.rs` |
| `STREAM_PROBE_TIMEOUT_SECS` (manifest GET) | 12 | `source/mod.rs` |
| ffprobe fallback | 10 s | `source/mod.rs::probe_with_ffprobe` |
| `CONTENT_PROBE_TIMEOUT_SECS` | 8 | `source/mod.rs` |
| Normalizer `-rw_timeout` / reconnect max | 15 s / 2 s | `normalize/ffmpeg_normalizer.rs` |
| `rife-worker` NUT header limit / lag monitor | 16 MiB / enter 1 s, exit 0.25 s | `rife-worker/src/worker.rs` |
| Rate limits | auth 5/60 s, stream-start 3/60 s per IP | `lib.rs`, `server_main.rs` |

---

## 9. Sessions

- `SessionMap = Arc<DashMap<String, Session>>`. `Session { info: SessionInfo, backing: SessionBacking }`.
- `SessionBacking::Local(LocalSessionBacking { source_headers, heartbeat_tx, session_dir, completion, startup_attempt, startup_mode })` or `SessionBacking::Remote(RemoteSessionBacking { remote_base_url, remote_session_id, remote_token, selected_peer })`.
- **One session at a time:** both local and relay starts call `stop_existing_sessions` first.

`SessionInfo` (serialized camelCase; returned in `/v1/status` and relayed between peers):

| Field | Meaning |
|---|---|
| `id`, `sourceUrl`, `outputUrl` | IDs and URLs (`outputUrl` is the localhost playlist) |
| `appName`, `streamTitle` | Display labels |
| `sourceContentKind` | Detected/overridden kind; `null` when `Unknown` |
| `upscaler` | `"Anime4K2x"`, `"ArtCNN"`, `"Libplacebo"` (any `Resize` on Universal, including AMD native), `"NVIDIA AI"`, or `null` |
| `sourceResolution`, `outputResolution`, `sourceFps`, `targetFps` | `targetFps` = interpolation target if executable, else source fps |
| `framegenEnabled` | Interpolation is planned and executable (after §6.5) |
| `framegenEngine` | `"NVIDIA FRUC"`, `"NVIDIA Framegen"`, `"Portable"`, or `null` (`server/stream.rs::active_framegen_label`). Shows the engine that actually runs after fallbacks. |
| `hdrEnabled` | The **setting** value, not whether HDR was realized |
| `qualityLevel`, `executor`, `encoderBackend` | Plan info |
| `startupComplete`, `retryingStartup`, `startupStage` | `"starting"` → `"ready"` once the playlist exists |

---

## 10. Source handling (`source/`)

### 10.1 Classification (`classify_source_url`)

- Transport: `http(s)` → `RemoteHttp`, `file:` → `LocalFile`, absolute path → `LocalPath`, else `Other`. Only `RemoteHttp` passes HTTP validation.
- Kind: `Hls` if the URL (lowercased, anywhere, including the query string) contains `.m3u8` or `.m3u`. Otherwise `Other`.
- `requires_local_hls_relay()` = `RemoteHttp && Hls`. This drives both the input proxy and metadata probing.

### 10.2 Metadata probing (`probe_source_metadata_with_classification`)

Only for remote HLS. Everything else gets `metadata: None`.
1. GET the manifest (12 s, Chrome UA + forwarded headers). Parse `#EXT-X-STREAM-INF` and pick the highest-`BANDWIDTH` variant with `RESOLUTION` (plus `FRAME-RATE`).
2. If there are no STREAM-INF entries (media playlist) **or the selected variant has no `FRAME-RATE`**, run `ffprobe` (`ffmpeg_path`'s sibling, else `ffprobe` on PATH; 1 MB / 1 s probe budget, 10 s timeout) over all video streams as JSON. `select_ffprobe_video_stream` prefers streams matching the manifest's resolution, then the largest frame, then the highest fps, because lower variants often run at a lower rate. `merge_probed_metadata` keeps manifest values and fills only the gaps.
3. Content probe (always attempted): FFmpeg decodes 6 frames scaled to 64×36 rgb24 (8 s). It runs **concurrently** with step 2 (`tokio::join!`), so stream start waits for the slower probe, not the sum. `classify_rgb24_frames` scores flat gradients, low-variance 4×4 tiles, and palette size. `|animated − live| ≤ 0.12` → `Unknown`. Confidence = `clamp((|Δ| − 0.12) / 0.38)`.

### 10.3 `SourceDescriptor`

`classification`, `original_url`, `runtime_url` (the relay URL when relayed), `runtime_headers` (empty when relayed, because the proxy adds them), `session_headers` (always the caller's headers; stored on the session for the proxy), `relay: Option<RelayedSource>`, `metadata`.

### 10.4 Input proxy (`hls_relay.rs`)

- `runtime_url` = `http://127.0.0.1:14002/v1/input/{session_id}?url=<encoded original>`. Encoders and the normalizer pull through this.
- `relay_request` checks http(s), runs an SSRF check on **every** proxied URL, forwards the session's headers (UA `REFEREE-InputProxy`), and streams non-manifest bodies. Sessions routed through EasyProxy pick their URL and headers differently (`resolve_upstream_request`, §10.6). The upstream client has a 10 s connect timeout and a 15 s read timeout. Encoders pulling through the proxy set no read timeout of their own, so an upstream that stalls mid-segment fails the body, and the encoder's reconnect logic takes over instead of the pipeline freezing.
- Manifests (by `.m3u8`/`.m3u` path or mpegurl content-type) are rewritten so every URI line and `URI="…"` attribute points back through the proxy. If there are two or more video variants, **only the highest-bandwidth variant is kept**, which pins the top rendition.
- The same rewriter (`rewrite_hls_manifest_with_target_resolver`) is reused for relay `/v1/tmp` proxying.

### 10.5 SSRF guard (`is_ssrf_target`)

Blocks loopback (127/8, ::1) and link-local (169.254/16, fe80::/10). Also blocks when there is no host, when DNS fails, or when DNS returns nothing. RFC-1918 ranges are **allowed** on purpose, for LAN sources. Applied at `/v1/stream/start` and in the input proxy.

`is_literal_ssrf_target` is the same check without DNS (IP literals and `localhost` names only). It is used for targets EasyProxy fetches on REFEREE's behalf: REFEREE never connects to those, and a lookup would send the hostname outside EasyProxy's network path.

### 10.6 EasyProxy routing (`easy_proxy.rs`)

Optional, off by default (`easyProxy.enabled`). [EasyProxy](https://github.com/realbestia1/EasyProxy) is a Python HLS/DASH proxy. `easyProxy.mode` picks who runs it: `managed` (default), where REFEREE installs and starts it (§10.7), or `external`, a server the user runs at `easyProxy.url` (default `http://127.0.0.1:7860`). An external URL must be a bare origin: EasyProxy writes rewritten URLs from scheme and host alone, so a path prefix cannot work, and credentials are rejected so they never reach logged URLs.

- **Start (`server/easy_proxy.rs::resolve_easy_proxy_route`):** runs before existing sessions are evicted. Managed mode calls `EasyProxyService::ensure_running`. External mode builds an `EasyProxyRoute` and runs `check()`: `GET /health` reads the version, and `GET /proxy/manifest.m3u8` with no `url` must answer 400 (password accepted); 401 means a missing or wrong password. Any failure is 502 `EASYPROXY_UNAVAILABLE`, and the source is never fetched directly instead.
- **Extract (`extract()`, via `server/easy_proxy.rs::extract_easy_proxy_source`):** also runs before eviction, so a page without a video leaves the current stream running. It sends `GET /extractor/video?url=<source>&h_<Name>=…` with the password header and reads EasyProxy's MediaFlow-style JSON (`destination_url`, `request_headers`, `mediaflow_proxy_url`, `query_params`). The timeout is 190 s, because EasyProxy's slowest extractors allow themselves 180 s.
  - **Site extractor matched:** the source becomes `<mediaflow_proxy_url path>?d=<destination_url>&h_<Name>=…&<query_params>&orig_url=<source>`, on REFEREE's address for EasyProxy. Only the path is taken from `mediaflow_proxy_url`, because EasyProxy writes its own public host into it. The `h_` params come from the extractor's headers. `orig_url` lets EasyProxy extract again when the video's link expires (403). The URL must pass `allows_relay_target`.
  - **`extractor_key` is `generic`** (no site extractor, e.g. a direct stream): the source goes through `route()` unchanged. EasyProxy's generic extractor would only percent-decode the URL a second time and swap in its own headers.
  - **Failure:** an error answer (EasyProxy's `{"error": …}` text, or the HTTP status), a timeout, or a loopback video is 502 `EASYPROXY_EXTRACTION_FAILED`. The source is not fetched any other way.
- **Rewrite:** `route()` maps the source onto `/proxy/manifest.m3u8?url=…` when it classifies as HLS (so REFEREE's input proxy still sits in front), else `/proxy/stream?url=…`. Client headers become `h_<Name>=<value>` params, which EasyProxy sends upstream and copies into the URLs it rewrites. The API password goes in the `x-api-password` header, never in a URL. A URL already on the EasyProxy origin is left alone and not sent to the extractor. Probes and direct-fetch encoders use the rewritten URL and password header.
- **Session:** `Session::with_easy_proxy` stores the route, so a settings change does not reroute a running stream. `source_headers` are the extractor's headers for a video it found, else the client's.
- **Input proxy (`resolve_upstream_request`):** a target on the EasyProxy origin must be one of its media paths (`/proxy/*`, `/segment/*`, `/decrypt/*`, `/key`), and every http(s) URL in its query (`url`, `d`, `key_url`, `orig_url`, …) must pass `is_literal_ssrf_target`. `proxy` is exempt because it names EasyProxy's own outbound proxy, such as WARP on `socks5://127.0.0.1:1080`. Only these requests get the password header, which keeps it away from other hosts and keeps EasyProxy's admin API out of reach of anyone holding a session ID. Any other target is first checked with `is_literal_ssrf_target`, then sent through EasyProxy with `route()`.
- **Relay:** forwarded starts carry the original URL. The processing peer applies its own `easyProxy` setting.
- **Logs:** `redact_url_for_log` masks `h_*` and `api_password` values (nested `url=` included) in the `[Input Proxy]` and `[Content Probe]` lines. Pipeline command lines at debug level are not redacted.

### 10.7 Managed EasyProxy (`easy_proxy_service.rs`, `deps/easy_proxy/`)

- **Pins** (`deps/easy_proxy/mod.rs`): EasyProxy has no releases, so one commit is pinned (`EASY_PROXY_COMMIT`, `EASY_PROXY_VERSION`) and its `codeload` tarball is checked against `EASY_PROXY_ARCHIVE_DIGEST`. CPython 3.12 comes from a pinned python-build-standalone `install_only` build (`PYTHON_RELEASE`, `PYTHON_BUILD`: Windows x86_64 and Linux x86_64 only; elsewhere the install state is `unsupported`). `constraints.txt` pins every Python package to the version `pip freeze` produced with that Python.
- **Install** (`install`): takes `lib/.easyproxy.lock`, recovers an interrupted swap, then builds `lib/.easyproxy-stage-<uuid>/`. It downloads both archives through `download_verified` (size and SHA-256 checked), drops `easyproxy.apk`, runs `python -I -m pip install --only-binary :all: --no-binary construct --no-cache-dir -r app/requirements.txt -c constraints.txt` with `PIP_CONFIG_FILE` pointed at the null device, imports one module per requirement, writes `referee_launcher.py` and `install.json`, and swaps the directory in (the old install moves to `.easyproxy-old-<uuid>` and is then deleted). Progress phase `easyproxy`: Python 0–35%, EasyProxy 40–45%, pip 45–90% (counted from pip's `Collecting` lines), validation 95%. About 230 MB installed.
- **Install state** compares `install.json` (commit, Python build, constraints hash) with the compiled pins: `missing`, `outdated`, or `current`. The launcher is rewritten on each start when it differs, so changing it needs no reinstall.
- **When it installs:** at server start when managed mode is on and the install is missing or outdated (`spawn_easy_proxy_supervisor`); when the setting changes to managed and on (`apply_easy_proxy_settings`, called from `save_settings` and `save_stream_settings` only when `easyProxy` changed); from the UI (`install_easy_proxy`); and on a stream start that finds no install. A stream start never waits for an install: it fails with a message giving the progress. After a failed install, stream starts report the error instead of reinstalling; the UI's Retry or the next launch tries again.
- **Process** (`ensure_running`): `python -I -u -X utf8 referee_launcher.py` in the install directory, with `PORT`, `API_PASSWORD`, `CONFIG_DIR=easyproxy-data`, and `REFEREE_PARENT_PID`. The launcher imports EasyProxy's `app` and serves it on **127.0.0.1** (EasyProxy's own `main()` binds 0.0.0.0, which would be an open proxy on the LAN). It exits within about 2 s of REFEREE's process disappearing (psutil watchdog), since nothing else would stop an orphan. The server is ready when `check()` passes, within 60 s; an early exit reports the last stderr lines. The API password (32 hex characters) and the port are chosen once per REFEREE run and reused across restarts while the port is free, so routes held by sessions stay valid. Development builds (`debug_assertions`, `ServerAccess::for_build`) use `ServerAccess::Open` instead: port 7860 while it is free (else a random port, with a warning) and no API password, with `API_PASSWORD` removed from the child's environment, so `http://127.0.0.1:7860/extractor` and the admin pages can be opened by hand. It still binds loopback only. Tests construct the service with `ServerAccess::Private`. A running server that fails `check()` is restarted. stderr goes to the log at debug level only, because EasyProxy logs request URLs.
- **Idle stop:** every 30 s the supervisor stops the server when no session holds a route to it and it was last used more than `IDLE_SHUTDOWN` (5 min) ago. Turning managed mode off stops it at once unless a session uses it. Installs stop it first, because Windows cannot move files that a running Python holds open.
- **Remove** (`remove_easy_proxy`): refused while streams are active. Stops the server and deletes `lib/easyproxy/`, moving it aside first so a half-deleted tree never looks installed. `easyproxy-data/` is kept.
- **Bumping a pin:** update the constant and digest, run `pip freeze` with the pinned Python to refresh `constraints.txt`, and confirm Linux wheels exist (`pip download --only-binary :all: --platform manylinux…`). Then run the ignored live test `live_managed_easy_proxy_installs_starts_and_routes_a_stream`, which installs, starts, and pulls a segment of a public test stream through the input proxy.

---

## 11. LAN relay (REFEREE Relay)

Lets a machine without a supported GPU hand processing to a linked peer while clients keep talking to `localhost:14002`. Design notes are in `/docs/relay-implementation-plan.md`.

**Linking (desktop only, `commands.rs`).**
- `discover_lan_peers` probes `/v1/ping` on every host in the local /24 (1 s timeout). `local_ip()` uses a UDP connect to 8.8.8.8. If the scan finds the linked instance at a new address, the link is repointed (`adopt_moved_relay_peer`).
- `link_relay_peer` requests a token from the peer via `/v1/auth/request` with origin `https://peer-<sanitized-instance-id>.referee.invalid`. The peer's user must choose **Always allow** (a non-persistent grant is rejected).
- The result is saved to `settings.relay` (`enabled`, `linkedPeerId/Hostname/Ip`, `remoteToken`, `lastKnownPeer`). `SettingsManager::update` redacts `remoteToken` and `apiToken` when logging changes.
- `get_linked_relay_status` reads through the server's `RelayLinkMonitor` (below), so the UI's 10 s poll shares probes with status.
- `save_stream_settings` forwards stream settings to the peer's `/v1/settings/stream` when linked.

**Link monitor (`RelayLinkMonitor`, `ServerState::relay_link`).** It caches one `/v1/ping` probe of the linked peer, verified by instance ID (or by IP for links saved without one).
- Status reads are stale-while-revalidate. The first read for a link probes inline. After that, reads answer from the snapshot, and a snapshot older than 3 s triggers one background refresh. Concurrent probes are single-flight.
- Stream start probes fresh before the token is sent.
- **Rebind:** when the saved address fails or answers with a different instance, the /24 is scanned for the linked instance ID. The scan runs at most once per 30 s from status, or immediately at stream start. The new address is persisted (`linkedPeerIp`, `lastKnownPeer`) and emits `settings-sync`. Rebinding requires the responders to agree on the hostname. A multi-homed peer resolves to its lowest address, while conflicting claims are ignored. IP-only links are never rebound.

**Runtime (`server/relay.rs`).**

| Operation | Behavior |
|---|---|
| Start | `resolve_active_relay_target` checks the link fresh (with rebind). `gpuReady` must not be false. Otherwise 502 `RELAY_UNAVAILABLE`. Then evict local sessions, forward the original request body with `X-Referee-Token: remoteToken`, and fetch the remote `/v1/status` for the session info (3 attempts, 250 ms apart; on final failure, roll back by stopping it remotely). Store a `Session::new_remote` under a **new local ID** and spawn its keepalive. |
| Playback | `/v1/tmp/{localId}/{file}` → `serve_remote_tmp_file`. Flat files in the remote session dir are rewritten to `http://localhost:14002/v1/tmp/{localId}/{name}`. Anything else keeps `…/{name}?url=<remote>`, which must match the peer's scheme/host/port and the `/v1/tmp/{remoteId}/` prefix. The proxy client has a 1.5 s connect timeout and a 10 s read timeout. |
| Heartbeat | Local only: it feeds the session's keepalive channel. The **keepalive** task heartbeats the peer every 3 s, with several attempts inside the peer's 15 s window. It applies the local heartbeat contract on the client side (300 s for the first heartbeat, 15 s after). It ends the session when the client goes quiet (and stops the peer), when the peer returns 404, or when there has been no peer acknowledgement for 20 s. |
| Stop | Local cleanup first, then a best-effort forwarded stop. If the peer is unreachable, the closed keepalive stops heartbeating it, and the peer reaps the session within 15 s. |
| Status | See [§4.5](#45-get-v1status-and-get-v1ping). Control calls use a 1.5 s connect timeout, so an offline peer fails fast. |

---

## 12. Dependencies and GPU detection

### 12.1 GPU detection (`gpu.rs`)

- **Windows:** PowerShell `Get-CimInstance Win32_VideoController`. The first adapter whose compatibility or name matches NVIDIA (`nvidia|geforce|rtx`) or AMD (`amd|advanced micro|radeon`) wins.
- **Linux:** `nvidia-smi --query-gpu=name,utilization.gpu`, then `lspci` VGA/3D/display lines.
- Utilization comes only from `nvidia-smi`.
- `detect_display_refresh_hz` reads the primary display's current refresh rate (`EnumDisplaySettingsW`, Windows only; `None` elsewhere) for Match Display. `nvidia_gpu_supports_frame_generation` reads `nvidia-smi --query-gpu=compute_cap` once per process.

Backend resolution:

| Platform / vendor | Binary present in `lib/` | `backend` | `encoder_path` |
|---|---|---|---|
| Windows + nvidia | `NVEncC64.exe` | `"nvenc"` | NVEncC |
| Windows + nvidia | missing | `"universal"` | ffmpeg |
| any + amd | `VCEEncC64.exe` / `vceencc` | `"vceenc"` | VCEEncC |
| any + amd | missing | `"universal"` | ffmpeg |
| Linux + nvidia, or unknown vendor | — | `"universal"` | ffmpeg |
| no valid FFmpeg | — | `None` | `None` |

`GpuInfo` also carries `ffmpeg_path`, `rife_worker_path`, `rife_model_path`, and `utilization`. A vendor other than `nvidia|amd` gets `RELAY_REQUIRED` at stream start.

### 12.2 Launch dependency updates (`deps::download_binaries`)

`binaries_ready` = a valid FFmpeg resolves (setup scope), plus the vendor encoder exists (AMD → VCEEncC; Windows+NVIDIA → NVEncC).

Desktop and headless startup check for updates on every launch, including existing installations. Desktop checks run once per process; repeated renderer-ready calls preserve the current lifecycle. Version manifests (`lib/.dependency-*.json`) record the release tag, asset ID, update timestamp, digest, and installed file sizes. Changed rolling assets, missing files, and failed validation trigger reinstallation. Explicit FFmpeg/model overrides remain user managed.

| Asset | Source (GitHub `releases/latest` unless noted) | Asset match | When |
|---|---|---|---|
| FFmpeg + ffprobe | `BtbN/FFmpeg-Builds` | name contains `win64-gpl-` / `linux64-gpl-`, not `shared`; prefer the newest numbered release line | Check every launch unless an explicit FFmpeg override is configured |
| NVEncC | `rigaya/NVEnc` | `_x64.7z` | Windows + NVIDIA |
| VCEEncC | `rigaya/VCEEnc` | `_x64.7z` (Windows) / `_amd64.deb` (Linux) | AMD |
| RIFE models | `styler00dollar/VapourSynth-RIFE-ncnn-Vulkan`, tag `r9_mod_v33` source tarball | known model dirs → `lib/vs-rife/models/` | Optional (failure only logs a warning) |

Downloads stream to staging files with HTTP status checks, bounded retries, a 15-second metadata timeout, a 30-second download idle timeout, and a 10-minute download deadline per attempt. Asset sizes and GitHub SHA-256 digests (when supplied) are checked before extraction. FFmpeg must expose libplacebo/Vulkan/shaderc, ffprobe must run, and vendor encoders must return valid help output; probes have a 15-second deadline. RIFE models require nonempty `flownet.param` and `flownet.bin` files and stay on the app-supported tag.

Archives (`.zip`, `.7z`, `.deb`, `.tar.xz`, `.tar.gz`) are extracted in unique staging directories. Binary installation retains `.exe/.dll/.json` on Windows, and extensionless files, shared libraries, and JSON on Linux (chmod 755). Conflicting duplicate basenames fail before installation. An OS file lock serializes updates across instances; a durable replacement journal backs up installed files and rolls back failed or interrupted replacements. The installed package is validated again before committing its version record. Failed checks retain usable installed packages and retry next launch; missing required packages produce a retryable setup error. Startup removes abandoned staging downloads. Setup completion is published after GPU capabilities refresh.

### 12.3 FFmpeg resolution and validation

Probe output (`-filters`, `-buildconf`, encoder `--help`) goes through `process::cached_probe_output`, keyed by path, args, size, and mtime, so each binary is spawned once per process rather than on every stream start.

`resolve_ffmpeg_binary` order: `REFEREE_FFMPEG_PATH` (an invalid binary is an **error**) → bundled `lib/ffmpeg[.exe]` → `PATH` candidates (runtime scope only). Bundled is preferred. A candidate is valid only if `-filters` lists `libplacebo` **and** `-buildconf` contains `--enable-vulkan`, `--enable-libshaderc`, and `--enable-libplacebo`.

`rife-worker`: `REFEREE_RIFE_WORKER_PATH` → next to the running executable (the Tauri sidecar) → debug builds only: `rife-worker/target/release/` → `lib/` → `PATH` (Docker). Model: `REFEREE_RIFE_MODEL_PATH` → first existing of `RIFE_DEFAULT_MODEL_DIRS` (starting `rife-v4.25-lite_ensembleFalse`) under `lib/vs-rife/models/`.

### 12.4 Shaders

`ensure_universal_shaders` compares the compile-time-embedded Anime4K (`Anime4K_Upscale_CNN_x2_M.glsl`) and ArtCNN (`ArtCNN_C4F16.glsl`) shaders with `lib/shaders/{anime4k,artcnn}/` at launch. Missing or changed assets are refreshed through the same recoverable installation mechanism; identical files are left untouched. At runtime the executor looks them up at `ffmpeg_path.parent()/shaders/…`.

---

## 13. Settings and persistence (`settings.rs`)

`config.json` (`Settings`). Unknown or invalid values are normalized on load and update: resolution must be one of the three presets (else `1920x1080`), quality is clamped to 1..=4, and optional strings are trimmed.

| JSON key | Default | Used by |
|---|---|---|
| `resolution` | `"1920x1080"` | Pipeline output |
| `quality` | `2` | VSR quality / libplacebo scaler |
| `framegen` | `true` | Interpolation request |
| `framegenTarget` | `"60fps"` (`"30fps"`, `"matchDisplay"`) | §6.1. Unknown values load as the default instead of failing the file. |
| `framegenEngine` | `"opticalFlow"` (`"rife"`, `"nativeDouble"`; `"nvidiaFramegen"` loads as `"nativeDouble"`) | §6.1. Unknown values load as the default. |
| `hdr` | `true` | HDR request |
| `executorPreference` | `"auto"` (`"nvidiaAi"`, `"amdAi"`, `"universal"`; aliases `"nvidia"`, `"nvidiaSpecialized"`, `"amd"`) | §6.2 |
| `encodingProfiles` | `{}` | §7.8 |
| `alwaysOnTop`, `showOnProxyStart`, `minimizeToTray`, `closeToTray` (`true`), `notifications` | mostly `false` | Desktop UI |
| `autoUpdate` | `true` (also when missing from an older `config.json`) | §14 self-update: install new releases while idle |
| `player` `{enabled, selectedPlayer, customPath}` | disabled | External player launch |
| `relay` | disabled | §11 |
| `easyProxy` `{enabled, mode, url, apiPassword}` | disabled; `mode` `"managed"` (`"external"`; unknown values load as managed); `url` `null` = `http://127.0.0.1:7860` | §10.6–10.7. `url` and `apiPassword` are for external mode. Not settable through `/v1/settings/stream`. `apiPassword` is redacted in the change log. |
| `instanceId`, `apiToken` | generated at startup | Identity / auth |

- `ServerSettings` (`server/state.rs`) is the in-memory subset the server reads (`resolution, quality, framegen, framegen_target, framegen_engine, hdr, executor_preference, show_on_proxy_start, notifications, encoding_profiles, relay, easy_proxy`). After any persisted change, rebuild it with `ServerSettings::from(&settings)`. Both the Tauri commands and `/v1/settings/stream` do this.
- `SettingsManager::update` normalizes, logs changed top-level keys, and writes pretty JSON. `merge_and_update` applies a partial JSON object.
- Settings are read **once per session start**. Changes don't affect a running session.

---

## 14. Desktop shell integration (Tauri)

**Commands** (`commands.rs`, registered in `lib.rs`):
- Settings: `get_initial_settings`, `save_settings`, `save_stream_settings`.
- Boot and window: `get_boot_setting`, `set_boot_setting`, `minimize_window`, `close_window`, `toggle_pin`, `open_external`, `open_github`.
- Setup: `renderer_ready`, `get_setup_state`, `retry_setup`.
- Auth: `get_api_token`, `get_pending_consent`, `respond_to_consent`, `get_approved_origins`, `revoke_approved_origin`.
- Streams: `stop_stream`, `start_debug_stream` (debug builds only; see §15).
- Relay: `discover_lan_peers`, `get_linked_relay_status`, `link_relay_peer`, `unlink_relay_peer`.
- EasyProxy: `check_easy_proxy` (managed: starts the server; external: checks the values being edited), `get_easy_proxy_status`, `install_easy_proxy`, `remove_easy_proxy` (§10.6–10.7).
- Updates: `get_update_status`, `check_for_update` (returns the status; a failed check is in its `error`), `install_update` (downloads if needed, installs, exits; rejects while sessions run).
- Players: `detect_players`, `launch_player`.
- Misc: `get_app_version`.

The renderer calls them through `/desktop/ui/src/lib/renderer-api.ts`.

**Events** (backend → renderer):

| Event | Emitted from | Payload |
|---|---|---|
| `status-update` | `lib.rs` push loop, `server/status.rs::emit_status_update` | `StatusResponse` |
| `consent-request` | `server/auth.rs::request_user_consent` | `{nonce, origin, appName}` |
| `settings-sync` | `commands.rs` settings and relay commands, `/v1/settings/stream` | `Settings` |
| `setup-gpu-detected` / `setup-progress` / `setup-complete` / `setup-error` | `commands.rs::run_setup_download` | `{vendor,name}` / `SetupProgress{phase,percent,detail}` / `()` / `{message}` |
| `update-status` | `updater.rs`, on every change | `UpdateStatus{currentVersion, phase, latestVersion, releaseUrl, progress, error, lastChecked, unsupportedReason}` |
| `navigate-view` | `tray.rs` | `"settings"` |
| `token-rotated` | `server/auth.rs::handle_rotate_token` | new token (the renderer does not subscribe) |

The renderer also subscribes to `boot-setting-sync`, but nothing in the backend emits it.

**Windows:** `main` (`index.html`) and `stream-player` (`player.html`), both in `tray.rs`. `showOnProxyStart` raises `main` on stream start. `notifications` shows "… stream is live" / "… stream stopped".

**Self-update** (`updater.rs`): a check lists `cmacrowther/referee` releases and takes the highest semver above the running version, skipping drafts, GitHub prereleases and semver prerelease tags. The release must carry this installation's asset (decided by `tauri::utils::platform::bundle_type()`, which the bundler stamps into the binary; NSIS → `*_x64-setup.exe`, deb → `*_amd64.deb`, rpm → `*.x86_64.rpm`, arm64 equivalents) **with** a GitHub `sha256:` digest. The download goes through `deps::download_verified` and is re-hashed before reuse. Install: Windows starts the NSIS installer with `ShellExecuteExW` (the per-machine installer requires the UAC prompt, which `CreateProcess` cannot raise) using `/P /UPDATE /R` (passive, update mode, relaunch), then exits. Linux runs `pkexec dpkg -i` / `pkexec rpm -U`, then `request_restart()`. Builds without a bundle type (dev, `cargo run`) report `unsupportedReason` and only link the release page. A failed or declined install is not retried automatically until the next launch. The renderer never supplies the URL that is downloaded.

**CSP** (`tauri.conf.json`): `connect-src` and `media-src` allow `http://127.0.0.1:14002`.

---

## 15. Logging and debugging

- **Files:** `logs/referee.log.<date>` (desktop) or `logs/referee-server.log.<date>` (headless) under app data. Stdout too.
- **Levels:**
  - `RUST_LOG=referee=debug` logs **full command lines for every stage** (`[Pipeline:<backend>]: Command:` / `[Pipeline:<backend>:<stage>]: Command:`), retry decisions, and cleanup.
  - `trace` adds the full `SourceDescriptor` / `PipelineRequest` / `BackendCapabilities` / `ExecutionPlan` dump per session (`[Pipeline]: Prepared session …`), per-heartbeat lines, bridge byte counts, and `frame=`/`size=` progress lines.
- **Log prefixes:** `[Main]`, `[Debug]`, `[Setup]`, `[GPU]`, `[Deps/…]`, `[Settings]`, `[Auth]`, `[Planner]` (content classification + chosen upscaler), `[Metadata Probe]`, `[Content Probe]`, `[Input Proxy]`, `[EasyProxy]`, `[EasyProxy:server]` (managed server stderr, debug), `[Deps:pip]` (debug), `[Relay]`, `[Relay Proxy]`, `[Pipeline]`, `[Pipeline:<backend>]`, `[Pipeline:<stage>:<label>]` (child stderr), `[PipelineSupervisor]`.
- **On failure:** the failed stage's last stderr is logged at `error` (`Last stderr for session …`). Capability probe results are logged at session start (`Capabilities — vpp-resize=…, fruc=…, native-double=…, truehdr=…, rife=…, hqdn3d=…`).
- **Test streams without a web client:** in a dev build (`npm run dev`) the standby screen shows a **Test Stream** field (`desktop/ui/src/dev/DebugStreamPanel.tsx`, delivered through `useRendererDebugPanel`'s `statusPanel`, so production bundles leave it out). Submitting calls `start_debug_stream`, which runs the same validation, SSRF check, relay routing, and local start as `/v1/stream/start` (`server::stream::start_stream`, without the per-IP rate limit). It names the app `REFEREE Debug` and heartbeats the session every 5 s until it is stopped or ends, because no client is there to do it. It then opens the built-in player unless player auto-open is configured, in which case the renderer's auto-launch opens the chosen player. Release builds reject the command.
- **Reproducing a stage by hand:** copy the logged command line. Inside the pipeline, `/v1/input/{id}` URLs only work while that session exists, so replace them with the original source URL.
- **Quick API checks:**

  ```sh
  curl -s localhost:14002/v1/status | jq
  curl -s -X POST localhost:14002/v1/stream/start \
       -H "X-Referee-Token: $TOKEN" -H "Content-Type: application/json" \
       -d '{"url":"https://example.com/master.m3u8"}'
  ```

---

## 16. Testing

- **Rust:** unit tests sit inline as `#[cfg(test)] mod tests` at the bottom of each file. The largest suites are `pipeline/tests.rs`, `exec/universal.rs`, `server/tests.rs`, `exec/nvidia.rs`, `graph/planner.rs`, and `exec/amd.rs`. Tests assert on **generated argument vectors and plan structures**, not on running encoders. No GPU is needed.
- **`server/tests.rs` helpers:** `settings()`, `runtime_backend()`, `amd_runtime_backend()`, `source_descriptor(...)`, `planner_capabilities()`, `encoder_capabilities()`, `gpu_info()`, `insert_session_with_executor()`, and `spawn_relay_mock_server()` (a real Axum server on an ephemeral port for relay round-trips).
- **CI** (`release.yml`, only when the `package.json` version changes on `main`): `cargo test --features headless --no-default-features` on Windows and Ubuntu, plus `npm test -- --run`. Desktop-only tests (`commands.rs`, `updater.rs`, `deps/`, `settings.rs`) do **not** run in CI. Run plain `cargo test` locally to cover them.
- **Desktop tests on Windows:** `tauri-build` embeds the Common Controls v6 manifest only into `[[bin]]` targets, and the lib test binary imports `TaskDialogIndirect`, so plain `cargo test` exits with `STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) before any test runs. Link the manifest dependency into the test build, then run the binary it prints: `cargo rustc --manifest-path desktop/native/Cargo.toml --lib --profile test -- -C link-arg=/MANIFEST:EMBED -C "link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"`, then `desktop/native/target/debug/deps/referee_lib-<hash>.exe`.
- **UI:** Vitest + Testing Library, co-located `*.test.tsx`. `@tauri-apps/api` is mocked in `/desktop/ui/src/__mocks__`.
- **When you change an executor or planner rule,** update or add an argument-level assertion next to the existing ones in that module. `pipeline/tests.rs` covers executor selection, staged-launch decisions, and context wiring across modules.

---

## 17. Invariants and conventions

1. **Two-tier executor model.** `ExecutorKind` (graph, stable, serialized) is distinct from `RuntimeExecutorTarget` (runtime, vendor-probed). Don't add vendor branching to the planner beyond what `BackendCapabilities` expresses. Resolve vendors in `exec/family.rs`.
2. **The plan decides and executors render.** `ExecutionPlan` is the source of truth. Stage builders get `IntermediateExecutionPlan` (`to_intermediate_plan()`) and act only on ops whose `owner` is theirs. Normalizer and preprocessors re-derive the intermediate plan from `context.execution_plan`, so any replanning must change the session's `ExecutionPlan` (as `switch_to_universal_fallback` and `switch_to_rife_fallback` do), not one executor's copy.
3. **Planner purity.** `graph/` does no I/O. Probing belongs in `source/` or `pipeline/`. Command construction belongs in `exec/`, `normalize/`, and `preprocess/`.
4. **Session readiness = a non-empty `index.m3u8`.** This holds for every launch mode. Stage readiness policies are diagnostic.
5. **Wire formats:** NUT (rawvideo + PCM) between normalizer, preprocess, and executor. MPEG-TS from executor to packager. HLS out.
6. **Heartbeat contract:** the first heartbeat must arrive within 300 s of launch, and every later one within 15 s. Clients should send every ~10 s and always call stop. Relay sessions enforce the same contract locally (`relay::relay_client_expiry`, reusing `HEARTBEAT_TIMEOUT_MS` / `ORPHAN_SESSION_TIMEOUT_MS`).
7. **One active session.** A new start evicts everything, and the response reports the evicted IDs.
8. **Never hold a `DashMap` guard across `.await`.** Existing code clones what it needs and `drop`s the guard first (see `handle_heartbeat`, `stop_tracked_session`, `serve_tmp_file`).
9. **Child processes:** spawn through `ProcessSpec` so that piped stderr, `kill_on_drop`, and `CREATE_NO_WINDOW` always apply. One-off capability probes go through `process::cached_probe_output`.
10. **Headless parity:** every Tauri reference is `cfg`-gated with a headless twin, and new modules are declared in both crate roots.
11. **Security posture:** only http(s) sources; SSRF check on start and on every proxied URL (EasyProxy sessions: media paths only, password sent only to EasyProxy, §10.6); forwarded-header blocklist; path-traversal check on `/v1/tmp`; token on all mutating routes; CORS limited to approved origins.

---

## 18. Known gaps and sharp edges

Verified against the code at the version above. Fix or re-verify these before relying on the affected behavior.

| Area | Behavior | Where |
|---|---|---|
| Non-HLS sources | No metadata → `UpscaleRequest::Off` and no source fps. That means no upscaling, RIFE can't build (needs fps), and NVEncC assumes 30 fps for GOP (it skips `--max-procfps` pacing when fps is unknown) | `source/mod.rs`, `server/stream.rs::build_upscale_request` |
| RIFE throughput | ncnn RIFE is GPU-bound at 1080p (about 24 blends/s on an RTX 4070 SUPER) and shares the GPU with VSR/NVENC; the lag monitor keeps live streams in real time by repeating frames. NVEncC's own `--vpp-rife-ov` (TensorRT, faster) is not usable: it runs after VSR, rejects VSR's yuv444 output, needs 32-aligned frames, and needs a 1.8 GB CUDA ONNX runtime | `rife-worker/src/worker.rs` |
| Packager start | The packager reads MPEG-TS with `-fflags +nobuffer`, which discards the packets it probes: the first ~1–2 s of every staged stream (audio and video shifted equally, so sync holds) | `exec/ffmpeg_packager.rs` |
| Frame generation + HDR | FRUC and NVIDIA Framegen run after TrueHDR in 8-bit RGB, which can band smooth HDR gradients. Whether AMF FRC accepts the 10-bit frames after VCEEncC tonemapping is unverified | `exec/nvidia.rs`, `exec/amd.rs` |
| Match Display source | Without a client `displayRefreshRate`, the primary display of the REFEREE machine is used, which may not be the monitor showing the player. Headless and non-Windows hosts fall back to 60 Hz | `server/stream.rs::handle_stream_start`, `gpu.rs` |
| Unknown source fps | Interpolation still runs to the target, so a 30 fps target on an unknown 60 fps source would decimate | `graph/request.rs` |
| Frame generation coverage | NVEncC FRUC / NVIDIA Framegen (Windows+NVIDIA), AMF FRC doubling (Windows+AMD), RIFE on any path with the worker and a model, and the AMD staged `minterpolate` path. Universal on NVIDIA disables it (§6.5). `NvidiaSpecialized` without FRUC plans portable interpolation but never stages, so it's skipped with a warning | `server/stream.rs`, `pipeline/launch.rs::should_attempt_staged_launch` |
| AMD hardware validation | The AMD executor's arguments are unit-tested only; AMF FSR 1.1, FRC, and libplacebo tonemapping have not been run on an AMD GPU from REFEREE. `--help` cannot reveal per-GPU AMF support, so an unsupported feature surfaces as a VCEEncC startup failure and an FFmpeg retry without FRC | `exec/amd.rs`, `pipeline/run.rs` |
| AMD fixed frame rates | AMF FRC only doubles, so "30 FPS", "60 FPS", and Match Display on AMD use `minterpolate` (CPU, visibly softer) unless RIFE is chosen | `graph/planner.rs` |
| Universal HDR curve | The Universal executor and `FfmpegPreprocessor` tone-map with libplacebo `bt.2390`, which has no inverse, so SDR→HDR10 there keeps SDR levels inside PQ instead of expanding highlights as the AMD executor's `spline` does | `exec/libplacebo_filters.rs` |
| `fallback_owner` | Declared on every native accelerator but never read. There is no automatic promotion to shared preprocess | `graph/intermediate.rs` |
| Shader location | Executor looks for shaders under `ffmpeg_path.parent()`, but they're installed under `lib/`. These match only for the bundled FFmpeg. With `REFEREE_FFMPEG_PATH` or a `PATH` FFmpeg, shader materialization fails and the Universal shader path can't start | `pipeline/launch.rs::universal_executor_context`, `deps/shaders.rs` |
| HDR packager metadata | `HdrMetadata @ Packager` has no renderer. The packager is `-c copy` | `exec/ffmpeg_packager.rs` |
| `hdrEnabled` | Reports the setting, not whether HDR was realized | `server/stream.rs` |
| Staged A/V alignment | The normalizer rebases video (`setpts=PTS-STARTPTS`) and audio (`asetpts=PTS-STARTPTS`) each to its **own** first timestamp. That discards the source's start offset between the streams (often 20–150 ms in HLS), leaving a constant A/V offset on the staged AMD `minterpolate` path. The RIFE path already keeps the offset (`with_stream_offsets_preserved`); extend it to the other staged paths once VCEEncC's `--avsync` handling of NUT timestamps is confirmed | `normalize/ffmpeg_normalizer.rs` |
| Startup readiness depth | Ready = playlist non-empty = one 1 s segment. The player (`liveSyncDurationCount: 3`) starts with at most 1 s of buffer while output is paced near 1.0×, so the first seconds can rebuffer | `pipeline/session.rs::wait_for_packager_playlist`, `/desktop/ui/src/components/player/StreamPlayerWindow.tsx` |
| Capabilities refresh | The 60 s GPU loop updates `gpu_info` but not `encoder_capabilities`. `SessionPipeline::prepare` re-reads capabilities per session through `process::cached_probe_output`, which re-spawns only when the binary's size or mtime changes | `lib.rs`, `server_main.rs` |
| `ExecutorKind::Cpu` | Never produced by selection; renders through the Universal executor if it ever is | `pipeline/capabilities.rs`, `exec/family.rs` |
| Relay peer versions | A peer that does not know `"amdSpecialized"` cannot parse a newer AMD peer's `selectedExecutor` | `server/relay.rs` |
| Headless setup | `setup_complete` becomes true even if the binary download failed | `server_main.rs` |
| Rate-limiter buckets | Never evicted (grow by distinct IP) | `server/state.rs` |
| Docs drift | The `require_api_token` doc comment lists only stream routes | `server/auth.rs` |
| Test-only planner queries | `normalization_plan`, `has_temporary_executor_fallbacks`, `has_unimplemented_portable_operations`, `requires_shared_preprocess`, and similar intermediate-plan queries are only called from tests and show as dead-code warnings | `graph/intermediate.rs`, `graph/plan.rs` |

---

## 19. Change recipes

**Add an HTTP endpoint**
1. Write the handler in the right `server/*.rs` file. Return `HandlerResult<Json<…>>` or `axum::response::Response`, and build errors with `responses::error_response`.
2. Register it in `server/routes.rs::start_server`: `protected_routes` if it mutates state, otherwise the public router. If browsers on unapproved origins must reach it, extend the CORS predicate.
3. Add any new error code to `server.rs::error_codes`.
4. Gate Tauri-specific behavior with `#[cfg(not(feature = "headless"))]` plus a headless twin.
5. Add a test in `server/tests.rs`. Update §4 here, the client SDK (`/packages/client/src/index.ts`) if it is client-facing, and the docs site API reference (`/docs/components/api-reference.tsx`).

**Add a user setting that affects the pipeline**
1. Add the field to `settings.rs::Settings` (serde rename, `Default`, `normalize` if it needs validation) and to `merge_and_update`.
2. Add it to `ServerSettings` + `From<&Settings>`, and to `LocalStreamSettings` if stream start reads it.
3. Thread it into `build_pipeline_request_from_parts` or `PipelineLaunch`, and through `RuntimeExecutorBuildContext` if executors need it.
4. Expose it in the UI (`/desktop/ui/src/components/settings/`) and in `StatusResponse.settings` if clients need it. Update §13.

**Add a processing op (a new `VideoOp`)**
1. `graph/plan.rs`: add the plan struct and `VideoOp` variant, and handle it in `VideoOp::requires_stage`.
2. `graph/planner.rs`: decide when it's planned and at which `OpExecutionStage`, driven by a new field on `BackendCapabilities` (`graph/capabilities.rs`) if capability-dependent.
3. `graph/intermediate.rs`: add an `IntermediateOperation` variant, a binding rule in `from_execution_plan`, and (if portable) a `SharedPreprocessOperation` variant + `shared_preprocess_operation_plan` arm.
4. Render it in each executor's `map_intermediate_plan` / `apply_intermediate_operation`, and in `FfmpegPreprocessor` if it can be preprocess-owned. Every `match` must stay exhaustive, so the compiler will point at each site.
5. If it should trigger a staged launch, make sure it ends up `PreprocessorOwned` and check `should_attempt_staged_launch`.
6. Add planner, intermediate, and executor-arg tests. Update §6 and §7.

**Add or change an encoder flag**
- NVEncC: `exec/nvidia.rs::build_command` / `PlannedNvencOptions`.
- VCEEncC: `exec/amd.rs::build_command` / `PlannedVceEncOptions`.
- FFmpeg: `exec/universal.rs::build_video_encode_args` / `build_filter_graph`.
- Gate new Rigaya flags on a probed capability (`parse_nvenc_capabilities` / `parse_encoder_backend_capabilities`) and add it to `EncoderCapabilities` if the planner needs to know.
- Assert the new argument in that module's tests.

**Add a new runtime executor**
1. Implement `runtime::BackendExecutor`: build a `ProcessSpec::new(PipelineStageId::Executor, …)` and return `ExecutorSpec { process, startup_mode }`. Support `SourcePull` + `HlsOutput` for single-process and `StdoutPipe` in/out for staged; reuse `exec::resolve_encoder_input`, the shared output-arg builders, and `segment_aligned_gop`.
2. Add a `RuntimeExecutorTarget` variant in `exec/family.rs` and construct it in `pipeline/launch.rs::build_runtime_executor`.
3. Decide retry signatures in `pipeline/run.rs::is_retryable_failure` / `resolve_attempt`.
4. Add downloads and detection in `deps/` and `gpu.rs` if it needs a new binary.

---

## 20. Type quick reference

```rust
// graph/request.rs
enum LatencyMode          { UltraLow, Low /*default; always used*/, Balanced }
enum UpscaleRequest       { Off, Quality(u8 /*1..=4*/), Anime4k2x, Artcnn2x }
enum InterpolationRequest { Off, TargetFps(u32), RifeTargetFps(u32), NativeDouble }
enum HdrRequest           { Off, Passthrough10Bit, TonemapToHdr10, InjectHdr10Metadata }

// graph/capabilities.rs
enum BackendFamily              { Nvidia, Amd, Universal, LegacyCompatibility }
enum EncoderBinaryAvailability  { Available, Missing, Unknown }
enum FeatureAvailability        { Unavailable, Approximate, Exact }
enum NativeUpscaleBackend       { NvidiaNgxVsr, AmdVceEncResize }
enum SelectedUpscalePath        { NativeBackend, SharedPreprocess, TemporaryUniversalCompatibility }
enum ResizeSupport              { Unsupported, Basic, QualityRange { min_quality, max_quality } }
enum InterpolationSupport       { Unsupported, Native, NativeWithFramegen, FramegenOnly }
struct HdrSupport               { passthrough_10_bit, tonemap_to_hdr10, inject_hdr10_metadata }
struct BackendCapabilities      { executor, resize, interpolation, hdr, upscale }

// graph/plan.rs
enum ExecutorKind     { NvidiaSpecialized /*alias "nvidia"*/, Universal /*legacy alias "amd"*/, Cpu, AmdSpecialized }
enum OpExecutionStage { Preprocess, Executor, Packager, Deferred }
enum VideoOp          { NormalizeInput, Resize(ResizePlan), Interpolate(InterpolationPlan), Hdr(HdrPlan),
                        Anime4k2xUpscale(Anime4k2xPlan), Artcnn2xUpscale(Artcnn2xPlan) }
enum InterpolationRealization       { NativeBackend(NativeInterpolationEngine), PortableFallback, Rife, Disabled }
enum NativeInterpolationEngine      { NvidiaFruc /*default*/, NvidiaFramegen, AmdFrc }
enum InterpolationUnsupportedReason { SourceAlreadyAtOrAboveTargetFps, PortableFallbackNotImplemented, NativeEngineUnavailable }
enum HdrTransformKind { Passthrough10Bit, TonemapToHdr10, NvidiaTrueHdrTonemapToHdr10, AmdTonemapToHdr10 }
enum OutputBitDepth   { Bit8, Bit10 }

// graph/intermediate.rs
enum IntermediateOpOwner           { Normalizer, SharedPreprocess, Executor, Packager, Deferred }
enum NativeAcceleratorKind         { NvidiaNgxVsr, NvidiaFruc, NvidiaFramegen, NvidiaTrueHdr, AmdVppResize, AmdFrc, AmdHdrTonemap }
enum SharedPreprocessExecutionMode { PreprocessorOwned, TemporaryExecutorFallback, DeferredUntilPortableRenderer }

// runtime/stages.rs + supervisor.rs
enum FrameTransport  { SourcePull, StdoutPipe, HlsOutput { playlist_path, segment_dir } }
enum PipelineStageId { Normalizer, Preprocess, Executor, Packager }
enum PipelineStageReadinessPolicy { ReadyOnSpawn, ReadyOnHeartbeat, ReadyOnStderrSentinel { sentinel } }
enum PipelineSupervisorStopReason { HeartbeatChannelClosed, OrphanTimeout{..}, InactivityTimeout{..}, StageExited{..} }
enum StderrMode      { Raw, Filtered { suppress }, FfmpegProgress { gpu_vendor } }
enum StartupMode     { LowLatency /*attempt 1*/, Buffered /*attempt 2+*/ }
struct ExecutorSpec   { process: ProcessSpec, startup_mode: StartupMode }

// exec/
enum RuntimeExecutorTarget  { Nvidia, Amd, Universal }
enum UniversalEncodeBackend { Nvenc, Vaapi, Amf, CpuFallback }
enum EncoderInput           { SourceUrl { url, extra_headers }, StdinPipe }

// pipeline/
struct PipelineLaunch   { session_id, source, execution_plan, rife_fallback_plan, output_resolution, profile, session_dir, backend: RuntimeBackend }
struct RuntimeBackend   { gpu_vendor, encoder_backend, encoder_path, ffmpeg_path, rife_worker_path, rife_model_path }
enum ExecutorPreference { Auto, NvidiaAi /*aliases "nvidia","nvidiaSpecialized"*/, AmdAi /*alias "amd"*/, Universal }
struct EncoderCapabilities { has_vpp_resize, has_fruc, has_truehdr, has_rife, has_native_double, has_hdr_tonemap }
enum FramegenTarget     { Fps30 /*"30fps"*/, Fps60 /*"60fps", default*/, MatchDisplay /*"matchDisplay"*/ }
enum FramegenEngine     { OpticalFlow /*default*/, Rife, NativeDouble /*"nativeDouble", alias "nvidiaFramegen"*/ }
struct FramegenSettings { enabled, target, engine }
enum SessionBacking     { Local(LocalSessionBacking), Remote(RemoteSessionBacking) }
enum PlaylistWaitOutcome { Ready, SessionEnded, TimedOut }

// source/mod.rs
enum SourceTransport   { RemoteHttp, LocalFile, LocalPath, Other }
enum SourceKind        { Hls, Other }
enum SourceContentKind { Animated, LiveAction, Unknown }  // serde: "animated" | "liveAction" | "unknown"
```

All graph and source enums serialize as camelCase.

---

## 21. Keeping this document current

- Update this file **in the same change** as any modification to routes, error codes, planner rules, binding rules, launch-mode logic, executor arguments, timeouts, settings, or downloads.
- When fixing an item from [§18](#18-known-gaps-and-sharp-edges), delete its row.
- After substantive edits, bump the "Last verified" line at the top with the version, commit, and date.
- Quick drift checks:

  ```sh
  rg -n '\.route\(' desktop/native/src/server/routes.rs        # §4.1
  rg -n 'pub const [A-Z_]+: &str' desktop/native/src/server.rs # §4.7
  rg -n 'const .*_(MS|SECS|LIMIT|ATTEMPTS|INTERVAL)' desktop/native/src  # §8.4
  rg -n 'fn should_attempt_staged_launch' -A 30 desktop/native/src/pipeline/launch.rs  # §7.1
  ```
