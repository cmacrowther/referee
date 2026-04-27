"use client"

import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import { Badge } from "@/components/ui/badge"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { Braces } from "lucide-react"

const endpoints = [
  {
    id: "endpoint-auth-request",
    method: "POST",
    path: "/v1/auth/request",
    description: "Request an API token for a browser origin. If the origin is already approved the token is returned immediately. In desktop mode, an unknown origin shows a consent dialog; in headless mode it returns 403 HEADLESS_MODE.",
    request: `{
  "appName": "MyApp"   // optional display name
}`,
    requestFields: [
      { name: "appName", type: "string", required: false, description: "Display name shown in the consent dialog and stored with the approval." },
    ],
    response: `{
  "token": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
  "persistent": true
}`,
    responseFields: [
      { name: "token", type: "string", description: "The API token to include as X-Referee-Token on mutating requests." },
      { name: "persistent", type: "boolean", description: "true if the approval was saved (Always Allow or pre-approved); false for a one-time Allow." },
    ],
    note: "The Origin header must be present — browsers set it automatically on cross-origin requests. Direct (non-browser) callers must supply it manually or use X-Referee-Token directly.",
  },
  {
    id: "endpoint-auth-rotate",
    method: "POST",
    path: "/v1/auth/rotate-token",
    description: "Generate a new API token. The new token is effective immediately — the old token is invalidated. The new value is also persisted to config.json so it survives a server restart.",
    response: `{
  "token": "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy"
}`,
    responseFields: [
      { name: "token", type: "string", description: "The newly generated token. Update all clients to use this value." },
    ],
    note: "Protected by X-Referee-Token middleware — you must authenticate with the current token to rotate it.",
  },
  {
    id: "endpoint-origins-list",
    method: "GET",
    path: "/v1/origins",
    description: "Returns all persistently approved origins (those that were granted \"Always Allow\" or added via env var / API).",
    response: `[
  {
    "origin": "https://myapp.com",
    "appName": "MyApp",
    "approvedAt": "1744000000"
  }
]`,
    responseFields: [
      { name: "origin", type: "string", description: "The approved origin URL." },
      { name: "appName", type: "string", description: "Display name stored at approval time, or null." },
      { name: "approvedAt", type: "string", description: "Unix timestamp (seconds) when the origin was approved." },
    ],
  },
  {
    id: "endpoint-origins-add",
    method: "POST",
    path: "/v1/origins",
    description: "Add or update an approved origin. Useful for headless deployments where no consent dialog is available.",
    request: `{
  "origin": "https://myapp.com",
  "appName": "MyApp"   // optional
}`,
    requestFields: [
      { name: "origin", type: "string", required: true, description: "The origin to approve. Must start with http:// or https://." },
      { name: "appName", type: "string", required: false, description: "Optional display name stored with the approval." },
    ],
    response: `{
  "status": "created"
}`,
    note: "Returns HTTP 201 Created on success.",
  },
  {
    id: "endpoint-origins-delete",
    method: "DELETE",
    path: "/v1/origins/:origin",
    description: "Remove an approved origin. Future auth requests from this origin will no longer receive an immediate token.",
    response: `{
  "status": "deleted"
}`,
    note: ":origin must be URL-encoded (e.g. https%3A%2F%2Fmyapp.com). Returns 404 ORIGIN_NOT_FOUND if the origin was not in the approved list.",
  },
  {
    id: "endpoint-status",
    method: "GET",
    path: "/v1/status",
    description: (
      <>
        Check that <RefereeWordmark variant="inline" /> is running and a compatible GPU is
        detected.
      </>
    ),
    response: `{
  "gpuReady": true,
  "gpuName": "RTX 4090",
  "gpuVendor": "nvidia",
  "gpuUtilization": 45,
  "encoderBackend": "nvenc",
  "encoderHasFramegen": true,
  "encoderHasTruehdr": true,
  "activeSessions": 0,
  "sessions": [],
  "primarySession": null,
  "settings": { "resolution": "1920x1080", "quality": 3, "framegen": false, "hdr": false },
  "selectedExecutor": "nvenc",
  "nvidiaAiAvailable": true,
  "amdAiAvailable": false,
  "encoderHasRife": true
}`,

    responseFields: [
      { name: "gpuReady", type: "boolean", description: "true if a compatible GPU and encoder were detected" },
      { name: "gpuName", type: "string", description: "Normalized GPU model name, or null" },
      { name: "gpuVendor", type: "string", description: "GPU vendor — \"nvidia\" or \"amd\"" },
      { name: "gpuUtilization", type: "number", description: "GPU utilization percentage, or null if unavailable" },
      { name: "encoderBackend", type: "string", description: "Active encoder backend (\"nvenc\" or \"vceenc\"), or null" },
      { name: "activeSessions", type: "number", description: "Number of currently active upscaling sessions" },
      { name: "sessions", type: "array", description: "Array of active session info objects" },
      { name: "primarySession", type: "object", description: "The first active session object, or null" },
      { name: "settings", type: "object", description: "Current settings snapshot — resolution, quality, framegen, hdr" },
      { name: "encoderHasFramegen", type: "boolean", description: "True if the detected encoder supports frame generation (NVIDIA only)" },
      { name: "encoderHasTruehdr", type: "boolean", description: "True if the detected encoder supports TrueHDR tone-mapping (NVIDIA only)" },
      { name: "selectedExecutor", type: "string | null", description: "The executor kind selected by the current settings" },
      { name: "nvidiaAiAvailable", type: "boolean | null", description: "Whether the NVIDIA AI executor is supported on this system" },
      { name: "amdAiAvailable", type: "boolean | null", description: "Whether the AMD AI executor is supported on this system" },
      { name: "encoderHasRife", type: "boolean | null", description: "Whether the encoder supports RIFE upscaling" },
    ],
  },
  {
    id: "endpoint-ping",
    method: "GET",
    path: "/v1/ping",
    description: "Lightweight liveness and discovery probe. Returns identity and GPU readiness without requiring authentication. Used by REFEREE peers to discover each other on the local network.",
    response: `{
  "instanceId": "peer-a1b2c3d4-...",
  "hostname": "my-pc",
  "version": "0.6.0",
  "platform": "windows",
  "gpuReady": true,
  "gpuVendor": "nvidia",
  "gpuName": "RTX 4090"
}`,
    responseFields: [
      { name: "instanceId", type: "string", description: "Unique instance identifier for this REFEREE installation" },
      { name: "hostname", type: "string", description: "System hostname" },
      { name: "version", type: "string", description: "REFEREE server version string" },
      { name: "platform", type: "string", description: "Host OS — e.g. \"windows\", \"linux\", \"macos\"" },
      { name: "gpuReady", type: "boolean", description: "true if a compatible GPU and encoder are available" },
      { name: "gpuVendor", type: "string", description: "GPU vendor — \"nvidia\" or \"amd\", or \"unknown\" if not detected" },
      { name: "gpuName", type: "string", description: "GPU model name, or null if not detected" },
    ],
    note: "Unauthenticated. Useful for liveness checks and LAN peer discovery before calling /v1/status.",
  },
  {
    id: "endpoint-stream-start",
    method: "POST",
    path: "/v1/stream/start",
    description: "Send a source stream URL to begin hardware-accelerated upscaling.",
    request: `{
  "url": "https://example.com/live/stream.m3u8",
  "appName": "MyApp",
  "streamTitle": "Friday Night Stream",
  "contentKind": "live",
  "headers": {
    "Authorization": "Bearer <token>"
  }
}`,
    requestFields: [
      { name: "url", type: "string", required: true, description: "The source stream URL (HLS, DASH, or direct)" },
      {
        name: "appName",
        type: "string",
        required: false,
        description: (
          <>
            Your app&apos;s name (shown in <RefereeWordmark variant="inline" />&apos;s UI)
          </>
        ),
      },
      {
        name: "streamTitle",
        type: "string",
        required: false,
        description: (
          <>
            A label for the stream (shown in <RefereeWordmark variant="inline" />&apos;s UI)
          </>
        ),
      },
      { name: "headers", type: "object", required: false, description: "Extra HTTP headers forwarded to the encoder when fetching the stream. Hop-by-hop and forwarding headers are blocked (e.g. Host, X-Forwarded-For, Transfer-Encoding, Connection, Upgrade) — including them returns 400 INVALID_HEADERS." },
      { name: "contentKind", type: "string", required: false, description: "Override source content kind: \"live\" or \"vod\". Auto-detected if omitted." },
    ],
    response: `{
  "sessionId": "a1b2c3d4-...",
  "url": "http://localhost:14002/v1/tmp/a1b2c3d4-.../index.m3u8",
  "resolution": "1920x1080",
  "sourceResolution": "1280x720",
  "effectiveQuality": 3,
  "evictedSessions": [],
  "appName": "MyApp",
  "streamTitle": "Friday Night Stream"
}`,
    responseFields: [
      { name: "sessionId", type: "string", description: "Unique session ID — use for heartbeats and stop calls" },
      { name: "url", type: "string", description: "Absolute URL to the upscaled HLS playlist" },
      {
        name: "resolution",
        type: "string",
        description: (
          <>
            Output resolution (configured in <RefereeWordmark variant="inline" /> settings)
          </>
        ),
      },
      { name: "sourceResolution", type: "string", description: "Detected source resolution, or null if probe timed out" },
      { name: "effectiveQuality", type: "number", description: "The quality level (1–5) actually applied by the encoder" },
      { name: "evictedSessions", type: "array", description: "IDs of sessions that were stopped to make room for this one" },
      { name: "appName", type: "string", description: "Echoed back from the request" },
      { name: "streamTitle", type: "string", description: "Echoed back from the request" },
    ],
    note: "This request blocks until the upscaled HLS playlist is ready — up to ~3 minutes on first GPU start. Set your HTTP client timeout accordingly. Only one session runs at a time; starting a new session automatically stops any existing one. The returned url is already absolute — load it directly in your player.",
  },
  {
    id: "endpoint-stream-heartbeat",
    method: "POST",
    path: "/v1/stream/heartbeat/{session_id}",
    description: (
      <>
        Send this periodically (every 10s) to prevent <RefereeWordmark variant="inline" /> from
        cleaning up your session. The session ID is a path parameter — no request body is required.
      </>
    ),
    response: `{
  "status": "ok"
}`,
    errorResponse: `// 404 + SESSION_NOT_FOUND if session was already cleaned up
{
  "error": "Session not found",
  "code": "SESSION_NOT_FOUND"
}`,
  },
  {
    id: "endpoint-stream-stop",
    method: "POST",
    path: "/v1/stream/stop",
    description: "Explicitly stop an upscaling session and release GPU resources.",
    request: `{
  "sessionId": "a1b2c3d4-..."
}

// — or — stop all active sessions at once:
{
  "stopAll": true
}`,
    requestFields: [
      { name: "sessionId", type: "string", required: false, description: "ID of the session to stop. Required unless stopAll is true." },
      { name: "stopAll", type: "boolean", required: false, description: "If true, stops all active sessions. Overrides sessionId." },
    ],
    response: `{
  "status": "stopped"
}`,
    note: "Always call /v1/stream/stop when playback ends. Do not rely solely on heartbeat timeout — explicit stops free GPU resources immediately.",
  },
  {
    id: "endpoint-hls",
    method: "GET",
    path: "/v1/tmp/{session_id}/{filename}",
    description: "Serves the HLS output files for an active session. The url returned by /v1/stream/start is already an absolute URL — load it directly in your player.",
    response: `// index.m3u8 — HLS playlist (application/vnd.apple.mpegurl)
#EXTM3U
#EXT-X-VERSION:3
...

// seg000.ts, seg001.ts, … — MPEG-TS segments (video/mp2t)`,
    responseFields: [
      { name: "index.m3u8", type: "m3u8", description: "Rolling 3-segment HLS playlist. Your player refreshes this at its standard interval." },
      { name: "seg{N}.ts", type: "ts", description: "1-second MPEG-TS video segments. Older segments are deleted automatically as the window advances." },
    ],
    note: "All responses include Access-Control-Allow-Origin: * so cross-origin media players can fetch segments directly.",
  },
  {
    id: "endpoint-settings-stream",
    method: "POST",
    path: "/v1/settings/stream",
    description: "Apply live settings changes to the encoder without restarting any active session. Fields are all optional — omit any you do not want to change.",
    request: `{
  "resolution": "1920x1080",
  "quality": 4,
  "framegen": false,
  "hdr": false,
  "executorPreference": null
}`,
    requestFields: [
      { name: "resolution", type: "string", required: false, description: "Output resolution, e.g. \"1920x1080\" or \"3840x2160\"." },
      { name: "quality", type: "number", required: false, description: "Encoder quality level 1–5." },
      { name: "framegen", type: "boolean", required: false, description: "Enable frame generation (NVIDIA only)." },
      { name: "hdr", type: "boolean", required: false, description: "Enable TrueHDR tone-mapping (NVIDIA only)." },
      { name: "executorPreference", type: "string | null", required: false, description: "Override executor selection. Pass null to use automatic selection." },
    ],
    response: `{
  "status": "ok"
}`,
    note: "Protected by X-Referee-Token. Returns 400 INVALID_SETTINGS if a field value is out of range.",
  },
]

const methodColors: Record<string, string> = {
  GET: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
  POST: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  DELETE: "bg-red-500/10 text-red-400 border-red-500/20",
}

export function ApiReference() {
  return (
    <section id="api" className="scroll-mt-28 mt-16 border-t border-border/60 pt-10 pb-0">
      <div>
        <div className="mb-10">
          <div className="mb-4 flex items-center gap-3">
            <Braces className="h-5 w-5 flex-shrink-0 text-accent" />
            <h2 className="text-2xl font-bold tracking-tight sm:text-3xl">
              API Reference
            </h2>
          </div>
          <p className="text-sm leading-7 text-muted-foreground sm:text-base">
            All endpoints accept and return JSON. Base URL:{" "}
            <code className="text-accent">http://localhost:14002</code>
          </p>
          <section id="api-versioning" className="scroll-mt-28 mt-6 space-y-4">
            <div>
              <h3 className="mb-2 text-lg font-semibold tracking-tight sm:text-xl">Versioning</h3>
              <p className="rounded-r-lg border-l-2 border-accent pl-4 text-sm leading-7 text-muted-foreground">
                All routes are prefixed with <code>/v1/</code>. Future breaking changes will increment the version
                number, so existing integrations continue to work.
              </p>
            </div>
            <div>
              <h3 className="mb-2 text-lg font-semibold tracking-tight sm:text-xl">Authentication</h3>
              <div className="rounded-r-lg border-l-2 border-accent pl-4 space-y-3 text-sm leading-7 text-muted-foreground">
                <p>
                  Mutating endpoints (<code>POST /v1/stream/start</code>, <code>/heartbeat</code>, <code>/stop</code>,
                  and the <code>/v1/auth/rotate-token</code> and <code>/v1/origins</code> management endpoints)
                  require an <code>X-Referee-Token</code> header.
                </p>
                <p>
                  <strong className="text-foreground">Desktop app:</strong> retrieve the token from the REFEREE
                  settings panel or call <code>POST /v1/auth/request</code> from a browser page — REFEREE shows a
                  consent dialog and returns the token on approval.
                </p>
                <p>
                  <strong className="text-foreground">Headless server:</strong> set the{" "}
                  <code>REFEREE_API_TOKEN</code> environment variable (≥ 32 chars) before starting the server to
                  use a known token. Pre-approve origins with <code>REFEREE_ALLOWED_ORIGINS</code> (comma-separated)
                  so browser clients can obtain the token via <code>POST /v1/auth/request</code> without a UI.
                  Unknown origins receive <code>403 HEADLESS_MODE</code>.
                </p>
                <p>
                  Unauthenticated endpoints: <code>GET /v1/status</code>, <code>GET /v1/ping</code>,{" "}
                  <code>GET /v1/tmp/*</code> (HLS segments), and <code>POST /v1/auth/request</code>.{" "}
                  Note: <code>GET /v1/origins</code> <strong className="text-foreground">is</strong> token-protected.
                </p>
              </div>
            </div>
          </section>
        </div>

        <Accordion type="single" collapsible className="space-y-4">
          {endpoints.map((endpoint, index) => (
            <AccordionItem
              key={index}
              id={endpoint.id}
              value={`item-${index}`}
              className="scroll-mt-28 overflow-hidden rounded-xl border border-border/60 bg-background/80 px-5 sm:px-6"
            >
              <AccordionTrigger className="py-5 text-left hover:no-underline">
                <div className="flex flex-wrap items-center gap-3 sm:gap-4">
                  <Badge
                    variant="outline"
                    className={`font-mono text-xs ${methodColors[endpoint.method]}`}
                  >
                    {endpoint.method}
                  </Badge>
                  <code className="break-all font-mono text-xs text-foreground sm:text-sm">
                    {endpoint.path}
                  </code>
                </div>
              </AccordionTrigger>
              <AccordionContent className="space-y-6 pb-6">
                <p className="text-sm leading-7 text-muted-foreground sm:text-base">
                  {endpoint.description}
                </p>
                
                {endpoint.request && (
                  <div>
                    <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
                      Request Body
                    </p>
                    <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 p-4">
                      <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">
                        {endpoint.request}
                      </code>
                    </pre>
                    {endpoint.requestFields && (
                      <div className="mt-4 space-y-2">
                        {endpoint.requestFields.map((field) => (
                          <div key={field.name} className="flex flex-col gap-1 text-sm leading-6 sm:flex-row sm:gap-2">
                            <code className="text-accent">{field.name}</code>
                            <span className="text-muted-foreground">({field.type}{field.required ? ", required" : ""})</span>
                            <span className="text-muted-foreground">— {field.description}</span>
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                )}
                
                <div>
                  <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
                    Response
                  </p>
                  <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 p-4">
                    <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">
                      {endpoint.response}
                    </code>
                  </pre>
                  {endpoint.responseFields && (
                    <div className="mt-4 space-y-2">
                      {endpoint.responseFields.map((field) => (
                        <div key={field.name} className="flex flex-col gap-1 text-sm leading-6 sm:flex-row sm:gap-2">
                          <code className="text-accent">{field.name}</code>
                          <span className="text-muted-foreground">({field.type})</span>
                          <span className="text-muted-foreground">— {field.description}</span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {endpoint.errorResponse && (
                  <div>
                    <p className="mb-2 text-[11px] font-semibold uppercase tracking-[0.18em] text-muted-foreground">
                      Error Response
                    </p>
                    <pre className="overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 p-4">
                      <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">
                        {endpoint.errorResponse}
                      </code>
                    </pre>
                  </div>
                )}

                {endpoint.note && (
                  <p className="rounded-r-lg border-l-2 border-accent pl-4 text-sm leading-7 text-muted-foreground">
                    {endpoint.note}
                  </p>
                )}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>

        {/* Supported Source Formats */}
        <div id="supported-formats" className="scroll-mt-28 mt-12">
          <h3 className="mb-4 text-xl font-semibold tracking-tight sm:text-2xl">Supported Source Formats</h3>
          <p className="mb-4 text-sm leading-7 text-muted-foreground sm:text-base">
            Pass any of the following as the <code className="text-accent">url</code> field in{" "}
            <code className="text-accent">POST /v1/stream/start</code>.
            </p>
          <div className="overflow-hidden rounded-xl border border-border/60">
            <div className="overflow-x-auto">
              <table className="w-full min-w-[40rem] text-sm">
              <thead className="bg-secondary/50">
                <tr>
                  <th className="text-left px-4 py-3 font-medium">Format</th>
                  <th className="text-left px-4 py-3 font-medium">Example</th>
                  <th className="text-left px-4 py-3 font-medium">Notes</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                <tr>
                  <td className="px-4 py-3 align-top font-mono text-accent">HLS (.m3u8)</td>
                  <td className="px-4 py-3 align-top font-mono text-xs leading-6 text-muted-foreground">https://…/stream.m3u8</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Recommended. Live and VOD HLS streams. Supports auth headers.</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top font-mono text-accent">DASH (.mpd)</td>
                  <td className="px-4 py-3 align-top font-mono text-xs leading-6 text-muted-foreground">https://…/manifest.mpd</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Best-effort via FFmpeg demuxer — no first-class manifest handling. Behaviour may vary.</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top font-mono text-accent">Direct video URL</td>
                  <td className="px-4 py-3 align-top font-mono text-xs leading-6 text-muted-foreground">https://…/video.mp4</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">MP4, MKV, and other FFmpeg-compatible container formats.</td>
                </tr>

              </tbody>
              </table>
            </div>
          </div>

          {/* HLS proxy / segment extension gotcha */}
          <div className="mt-6 rounded-xl border border-amber-500/25 bg-amber-500/5 p-4">
            <p className="mb-2 text-sm font-medium text-foreground">
              Heads up: HLS segments must end in <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">.ts</code> / <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">.m4s</code>
            </p>
            <p className="mb-3 text-sm leading-7 text-muted-foreground">
              FFmpeg 7.0+ enforces an{" "}
              <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">allowed_segment_extensions</code>{" "}
              allowlist on HLS playlists and rejects segments whose <em>URL path</em> does not end in
              one of <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">ts</code>,{" "}
              <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">m4s</code>, etc. Query
              parameters are not inspected, so a stream-proxy URL like:
            </p>
            <pre className="mb-3 overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 p-3 text-xs font-mono text-foreground/90">
              <code>https://proxy.example.com/api/stream-proxy?url=…/segment.ts&sig=…</code>
            </pre>
            <p className="mb-3 text-sm leading-7 text-muted-foreground">
              will fail metadata probing with{" "}
              <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">URL … is not in allowed_segment_extensions</code>{" "}
              even though the underlying segment is a valid MPEG-TS file.
            </p>
            <p className="mb-2 text-sm font-medium text-foreground">Fixes (pick one):</p>
            <ul className="space-y-2 text-sm leading-6 text-muted-foreground">
              <li>
                <strong className="text-foreground">Reshape the proxy URL</strong> so the path ends
                in <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">.ts</code> /{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">.m4s</code>, e.g.{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">/api/stream-proxy/segment.ts?url=…</code>.
                This is the most portable fix and also keeps third-party HLS players happy.
              </li>
              <li>
                <strong className="text-foreground">Rewrite the playlist server-side</strong> before
                returning it, so segment URIs already include the expected extension on the path.
              </li>
              <li>
                <strong className="text-foreground">Pass the original</strong>{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">.m3u8</code> URL to{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">POST /v1/stream/start</code>
                {" "}with any required headers (<code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">referer</code>,{" "}
                <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">origin</code>, cookies)
                and let REFEREE fetch segments directly instead of routing through an extension-stripping proxy.
              </li>
            </ul>
          </div>
        </div>

        {/* Error Codes */}
        <div id="error-codes" className="scroll-mt-28 mt-12">
          <h3 className="mb-4 text-xl font-semibold tracking-tight sm:text-2xl">Error Codes</h3>
          <p className="mb-4 text-sm leading-7 text-muted-foreground">
            All error responses include a machine-readable <code className="text-accent">code</code> field alongside the human-readable <code className="text-accent">error</code> message:
          </p>
          <pre className="mb-6 overflow-x-auto rounded-xl border border-border/60 bg-secondary/40 p-4">
            <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">{`{
  "error": "Session not found",
  "code": "SESSION_NOT_FOUND"
}`}</code>
          </pre>
          <div className="overflow-hidden rounded-xl border border-border/60">
            <div className="overflow-x-auto">
              <table className="w-full min-w-[40rem] text-sm">
              <thead className="bg-secondary/50">
                <tr>
                  <th className="text-left px-4 py-3 font-medium">Status</th>
                  <th className="text-left px-4 py-3 font-medium">Meaning</th>
                  <th className="text-left px-4 py-3 font-medium">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">INVALID_REQUEST</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Missing or invalid request field</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Check your request body</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">MISSING_ORIGIN</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">No Origin header on auth request</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Browsers set this automatically; non-browser callers must add it manually</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">INVALID_ORIGIN</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Origin is not a valid http/https URL</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Ensure the Origin header starts with http:// or https://</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">401</code> <code className="text-muted-foreground text-xs">UNAUTHORIZED</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Missing or invalid API token</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Include a valid <code>X-Referee-Token</code> header</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">403</code> <code className="text-muted-foreground text-xs">NO_APP_HANDLE</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Desktop app handle is unavailable</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Rare internal state; retry or restart the desktop app</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">403</code> <code className="text-muted-foreground text-xs">CONSENT_DENIED</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">User clicked Deny on the consent dialog</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Prompt the user to allow access in REFEREE and retry</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">403</code> <code className="text-muted-foreground text-xs">HEADLESS_MODE</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Origin not pre-approved on a headless server</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Add origin via <code>POST /v1/origins</code> or <code>REFEREE_ALLOWED_ORIGINS</code> env var</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">404</code> <code className="text-muted-foreground text-xs">ORIGIN_NOT_FOUND</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Origin not in approved list (DELETE)</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Check the origin spelling and encoding</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">404</code> <code className="text-muted-foreground text-xs">SESSION_NOT_FOUND</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Session not found</td>
                  <td className="px-4 py-3 text-muted-foreground">Session was already cleaned up — stop heartbeat</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">502</code> <code className="text-muted-foreground text-xs">PIPELINE_EXITED</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Pipeline exited early</td>
                  <td className="px-4 py-3 text-muted-foreground">Encoder crashed before producing output — check source URL is reachable</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">503</code> <code className="text-muted-foreground text-xs">NO_ENCODER</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">No compatible GPU</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">
                    REFEREE cannot upscale without a compatible GPU
                  </td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">504</code> <code className="text-muted-foreground text-xs">PIPELINE_TIMEOUT</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Pipeline timed out</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">GPU may be busy or source stream unreachable</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">408</code> <code className="text-muted-foreground text-xs">CONSENT_TIMEOUT</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">User did not respond to the consent dialog within 180 s</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Retry the auth request — REFEREE will show the dialog again</td>
                </tr>                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">429</code> <code className="text-muted-foreground text-xs">RATE_LIMITED</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Too many requests (5/min for auth, 3/min for stream-start)</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Wait for the duration in the <code>Retry-After: 60</code> response header before retrying</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">INVALID_URL</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Stream URL is not a valid http/https URL</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Only http:// and https:// URLs are accepted as the <code>url</code> field</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">SSRF_BLOCKED</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Stream URL resolves to a loopback, private, or link-local address</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Use a publicly reachable URL; internal addresses are blocked for security</td>
                </tr>
                <tr>
                  <td className="px-4 py-3 align-top"><code className="text-accent">400</code> <code className="text-muted-foreground text-xs">INVALID_HEADERS</code></td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">The <code>headers</code> object contains a forbidden header name or a CRLF character in a value</td>
                  <td className="px-4 py-3 align-top leading-6 text-muted-foreground">Remove hop-by-hop and forwarding headers (e.g. <code>Host</code>, <code>X-Forwarded-For</code>, <code>Transfer-Encoding</code>)</td>
                </tr>              </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}
