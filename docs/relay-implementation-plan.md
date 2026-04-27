# REFEREE Relay Implementation Plan

## Goal

Enable a REFEREE instance running on one machine in a user's LAN to delegate stream execution to another linked REFEREE instance on the same LAN.

If a relay peer is linked and selected:

- the browser or app should still talk only to the local REFEREE instance
- the local REFEREE instance should forward stream start, heartbeat, and stop control to the selected peer
- the selected peer should be the machine that runs the upscaling pipeline
- playback should still come back through localhost on the original machine via a local proxy

## Confirmed Decisions

The following design choices were explicitly confirmed:

1. `localhost:14002` remains the only client-facing entrypoint.
2. Peer linking should reuse the existing consent and token flow instead of introducing a new pairing system.
3. The client-visible HLS contract should remain `http://localhost:14002/v1/tmp/{session_id}/index.m3u8` even for relay-backed sessions.

## Recommended Architecture

Use a `local control plane / remote execution plane / local playback proxy` model.

### Flow

1. The browser or app starts a session against the local REFEREE instance.
2. If no relay is linked, REFEREE uses the current local pipeline path.
3. If a relay is linked, the local REFEREE forwards the stream-start request to the selected peer.
4. The remote peer owns the actual GPU session and runs the full upscaling pipeline.
5. The local peer stores a remote-backed session record.
6. The local peer returns a localhost HLS URL to the client.
7. When the client requests the playlist and segments, the local peer proxies the remote peer's HLS output and rewrites playlist URLs back through localhost.
8. Heartbeat and stop requests sent to the local peer are forwarded to the remote session.

## Why This Shape Fits The Current Codebase

This design fits the current seams better than exposing the remote peer directly:

- The existing client contract already assumes the browser talks to a single local server.
- The current session lifecycle is centralized in the local server handlers.
- The codebase already has manifest rewriting and HLS relay logic that can be adapted for remote HLS proxying.
- Reusing the existing token and consent flow avoids a second trust model.
- The current approval model already persists trust by `Origin`, so a synthetic peer origin derived from `instanceId` can plug into the existing consent UI without a second approval surface.
- Keeping playback on localhost avoids extra browser CORS and origin-approval complexity.

## Existing Code Areas To Build On

### Discovery

- `desktop/ui/src/components/settings/RelayCard.tsx`
- `desktop/native/src/commands.rs`

Today this only scans the LAN via `GET /v1/ping`.

### Stream lifecycle

- `desktop/native/src/server.rs`
- `desktop/native/src/pipeline.rs`

Today sessions are assumed to be fully local and pipeline-backed.

### Source and playlist proxying

- `desktop/native/src/source/mod.rs`
- `desktop/native/src/source/hls_relay.rs`

This is the strongest existing seam for the localhost playback proxy portion of relay support.

### Settings and renderer state

- `desktop/native/src/settings.rs`
- `desktop/ui/src/lib/types.ts`
- `desktop/ui/src/lib/renderer-api.ts`

These will need new relay-specific persisted fields and UI types.

## Phased Implementation Plan

## Phase 1: Peer Identity And Relay Settings

### Objectives

- Give every REFEREE instance a stable identity.
- Persist selected relay peer configuration locally.

### Changes

- Add a stable `instanceId` to local persisted settings or instance state.
- Add a `relay` settings block to store:
  - whether relay is enabled
  - linked peer ID
  - linked peer hostname
  - linked peer IP
  - remote token or stored link credential
  - last-known peer metadata
- Generate `instanceId` once and persist it across restarts in the same way `apiToken` is already persisted today.
- Treat `instanceId` as canonical peer identity and use IP or hostname only as mutable reachability metadata.
- Extend desktop renderer types to expose this configuration.

### Files

- `desktop/native/src/settings.rs`
- `desktop/ui/src/lib/types.ts`

## Phase 2: Discovery Upgrade

### Objectives

- Make LAN discovery return enough data to select peers reliably.

### Changes

- Extend `GET /v1/ping` to return:
  - `instanceId`
  - hostname
  - version
  - GPU readiness
  - GPU vendor
  - optional GPU name
- De-duplicate discovered peers by `instanceId` and suppress the local machine when the returned `instanceId` matches the local one.
- Keep the first implementation scoped to the current IPv4 `/24` scan behavior unless discovery itself is being broadened as a separate task.
- Update peer discovery in the desktop layer to surface that information in the relay UI.

### Files

- `desktop/native/src/server.rs`
- `desktop/native/src/commands.rs`
- `desktop/ui/src/components/settings/RelayCard.tsx`
- `desktop/ui/src/lib/types.ts`

## Phase 3: Peer Linking Via Existing Consent Flow

### Objectives

- Reuse the current trust model for machine-to-machine linking.

### Changes

- On local "Link" action, call the remote peer's `POST /v1/auth/request`.
- Identify the requesting machine using a stable synthetic origin derived from the local `instanceId`, for example `https://peer-{instanceId}.referee.invalid`.
- Let the remote REFEREE instance show the existing consent UI.
- When approved, persist the returned remote token locally for future authenticated calls.
- Support unlinking by clearing local relay credentials and selection state.

### Notes

- This keeps the user-visible approval experience consistent.
- It avoids inventing a separate pairing handshake unless a later security pass requires one.
- Using a deterministic synthetic origin avoids tying durable approval to mutable IP addresses or hostnames while still fitting the current `http` or `https` origin validation.

### Files

- `desktop/native/src/server.rs`
- `desktop/native/src/commands.rs`
- `desktop/native/src/settings.rs`
- `desktop/ui/src/components/settings/RelayCard.tsx`
- `desktop/ui/src/lib/renderer-api.ts`
- `desktop/ui/src/lib/types.ts`

## Phase 4: Remote-Backed Session Model

### Objectives

- Separate session ownership from execution location.

### Changes

- Refactor `Session` so it can represent both:
  - a local pipeline-owned session
  - a remote-backed relay session
- Add remote session fields such as:
  - remote base URL
  - remote session ID
  - remote token
  - selected peer identity
- Update cleanup logic so it no longer assumes every session owns local child processes or local packager output.
- Prefer an explicit backing enum such as `SessionBacking::Local` and `SessionBacking::Remote` instead of spreading execution-location state across many unrelated optional fields.

### Files

- `desktop/native/src/pipeline.rs`
- `desktop/native/src/server.rs`

### Risk

This is the most important refactor because the current session model is strongly tied to local pipeline ownership.

## Phase 5: Forwarded Stream Start

### Objectives

- Route startup through the linked peer while preserving the current client contract.

### Changes

- In `POST /v1/stream/start`:
  - keep current local behavior when no relay is linked
  - forward the request to the linked peer when relay is active
  - create a local remote-backed session record
  - return a localhost playback URL instead of the remote peer's direct `/v1/tmp/...` URL
- Preserve existing request fields such as source URL, headers, app name, and stream title.

### Files

- `desktop/native/src/server.rs`

## Phase 6: Local Proxy For Remote HLS Output

### Objectives

- Keep browser playback on localhost even when the pipeline runs remotely.

### Changes

- Add a local endpoint that proxies the remote peer's playlist and segment requests.
- Rewrite remote playlist entries so all follow-up requests come back through localhost.
- Reuse the HLS manifest rewriting approach already present in `source/hls_relay.rs`.
- Ensure cache headers remain appropriate for live playlists and segments.
- Prefer extending the existing `/v1/tmp/{session_id}/{filename}` path to branch between local file serving and remote relay proxying so `StreamStartResponse.url` does not need a new client-visible route.

### Files

- `desktop/native/src/server.rs`
- `desktop/native/src/source/hls_relay.rs`
- optionally `desktop/native/src/source/mod.rs` if shared relay helpers should be generalized

### Why This Matters

Returning the remote peer's direct playlist URL would create unnecessary browser exposure, cross-origin complexity, and mismatch with the current localhost-first model.

## Phase 7: Forwarded Heartbeat And Stop

### Objectives

- Preserve the current client lifecycle while delegating control to the remote session.

### Changes

- Update `POST /v1/stream/heartbeat/{session_id}` to:
  - heartbeat the local session record
  - forward the heartbeat to the remote session if it is relay-backed
- Update `POST /v1/stream/stop` to:
  - forward stop to the remote peer for relay-backed sessions
  - run local cleanup for the proxy-side session record

### Files

- `desktop/native/src/server.rs`
- `desktop/native/src/pipeline.rs`

## Phase 8: Relay UI And Status Presentation

### Objectives

- Make the feature understandable and debuggable from the desktop app.

### Changes

- Expand the relay settings card to support:
  - scan
  - link
  - unlink
  - select active relay
  - show peer capability summary
  - show reachability and link status
- Extend status/session payloads with relay-specific metadata such as:
  - peer name
  - peer IP
  - execution location
  - whether the active stream is remote-backed
- Surface this in the status view so users can tell where processing is happening.

### Files

- `desktop/ui/src/components/settings/RelayCard.tsx`
- `desktop/ui/src/components/views/StatusView.tsx`
- `desktop/ui/src/components/status/*`
- `desktop/ui/src/lib/types.ts`
- `desktop/native/src/server.rs`
- `desktop/native/src/pipeline.rs`

## Phase 9: Tests

### Priority Test Areas

- Relay settings serialization and persistence
- Discovery parsing with stable peer identity
- Link and unlink state transitions
- Remote-backed session creation
- Forwarded start behavior
- Forwarded heartbeat behavior
- Forwarded stop behavior
- Local proxy rewriting for remote HLS manifests
- Cleanup behavior for remote-backed sessions

### Likely Files

- Rust tests in `desktop/native/src/server.rs`
- Rust tests in `desktop/native/src/source/hls_relay.rs`
- Rust tests in `desktop/native/src/pipeline.rs`
- UI tests around `RelayCard.tsx`

## Execution Order Recommendation

The safest delivery order is:

1. peer identity and relay settings
2. richer discovery payloads
3. link and unlink flow on top of existing auth
4. remote-backed session type split
5. remote HLS proxy endpoint
6. forwarded start logic
7. forwarded heartbeat and stop
8. UI polish and status visibility
9. tests and failure-path hardening

## Main Risks

## 1. Session Model Assumptions

The current session lifecycle assumes that session cleanup means dropping local pipeline resources and removing local session directories. That assumption needs to be loosened carefully.

## 2. Peer Identity Based Only On IP

Relying only on IP addresses will be brittle under DHCP changes. A stable `instanceId` should be the canonical identity.

## 3. Direct Remote Playback

Returning a remote peer playlist URL directly would create unnecessary browser and origin complexity. The local playback proxy is the safer design.

## 4. Link Robustness

If a linked peer is offline, renamed, or changes IP, local REFEREE needs clear degraded behavior:

- fail closed for new remote sessions
- preserve a usable UI state
- allow re-scan and rebind to the same `instanceId`

## Suggested Status Labels For UI

Useful relay states to expose in the desktop app:

- `Not linked`
- `Scanning`
- `Link pending approval`
- `Linked`
- `Peer unavailable`
- `Relay active`
- `Relay fallback to local`

## Future Enhancements After Initial Delivery

- automatic failover back to local execution when the selected peer is unreachable
- capability-based peer ranking
- multiple saved peers with explicit priority order
- stronger cryptographic peer pairing if the current consent-token model proves insufficient
- relay-aware status telemetry for bitrate, latency, and remote GPU identity

## Summary

The recommended implementation keeps the browser contract stable, uses the current approval system for trust, and layers relay support into the existing stream lifecycle by introducing remote-backed sessions plus a localhost HLS proxy for remote output.

This approach minimizes disruption to the web client model while fitting the current server, settings, and HLS relay seams already present in the codebase.
