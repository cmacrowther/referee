"use client"

import { useState } from "react"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Button } from "@/components/ui/button"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { BookOpen, Check, Copy, Package, Rocket } from "lucide-react"

const installExamples = {
  npm: "npm install @cmacrowther/referee-client",
  pnpm: "pnpm add @cmacrowther/referee-client",
  yarn: "yarn add @cmacrowther/referee-client",
}

const packageUsage = `import { RefereeClient } from '@cmacrowther/referee-client';
import { useReferee } from '@cmacrowther/referee-client/react';

// ─── Vanilla JS / TypeScript ─────────────────────────────────────────────────
const referee = new RefereeClient({ appName: 'MyApp' });

// Token is requested automatically on first authenticated call.
// In desktop mode this triggers a consent dialog; pre-approve
// origins on headless via REFEREE_ALLOWED_ORIGINS env var.
const status = await referee.getStatus();
if (!status.gpuReady) return; // no compatible GPU

// Start session (blocking until HLS output is ready)
const { session, dispose } = await referee.startManagedSession({
  url: 'https://example.com/live/stream.m3u8',
  streamTitle: 'Friday Night Stream',
});

// session.url is the absolute HLS URL — load it directly in your player
player.src = session.url;

// dispose() stops heartbeat + session and cleans up event listeners
window.addEventListener('pagehide', dispose);

// ─── React ───────────────────────────────────────────────────────────────────
function VideoPlayer({ sourceUrl }) {
  const { start, stop, status, playbackUrl } = useReferee({ appName: 'MyApp' });

  useEffect(() => {
    start({ url: sourceUrl, streamTitle: 'Live Stream' });
    return () => stop();
  }, [sourceUrl]);

  if (status === 'unavailable') return <NativePlayer src={sourceUrl} />;
  if (playbackUrl)              return <HlsPlayer src={playbackUrl} />;
  return <LoadingSpinner />;
}`

const examples = {
  auth: {
    title: "Auth Token",
    code: `const REFEREE_BASE = 'http://localhost:14002';

// ─── Option A: Request a token automatically (recommended) ───────────────────
//
// POST /v1/auth/request triggers the REFEREE desktop consent dialog.
// If the origin is already approved (or pre-set via REFEREE_ALLOWED_ORIGINS),
// the token is returned immediately without user interaction.
// The returned token is persistent — cache it in sessionStorage or memory.

async function requestToken(appName) {
  const res = await fetch(\`\${REFEREE_BASE}/v1/auth/request\`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ appName }),
  });

  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    // 403 HEADLESS_MODE — origin not pre-approved on a headless server.
    // Add it via REFEREE_ALLOWED_ORIGINS env var or POST /v1/origins.
    throw new Error(err.error ?? \`Auth failed: \${res.status}\`);
  }

  const { token, persistent } = await res.json();
  console.log(\`Token \${persistent ? '(saved)' : '(one-time)'}: \${token}\`);
  return token;
}

// ─── Option B: Use a known token directly ────────────────────────────────────
//
// If you control the server, set REFEREE_API_TOKEN on startup and
// hardcode or inject the token — no auth request needed.
// Retrieve it from:
//   • Desktop app: Settings → API Token
//   • Headless:    REFEREE_API_TOKEN env var → token is printed on startup
//   • Headless:    POST /v1/auth/rotate-token (generates a new one)

const REFEREE_TOKEN = process.env.REFEREE_API_TOKEN ?? '<your-referee-token>';

// ─── Usage ───────────────────────────────────────────────────────────────────
let token = null;

async function getOrRequestToken() {
  if (token) return token;
  token = await requestToken('MyApp');
  return token;
}

// All authenticated calls pass the token as X-Referee-Token
async function startSession(sourceUrl) {
  const t = await getOrRequestToken();
  const res = await fetch(\`\${REFEREE_BASE}/v1/stream/start\`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Referee-Token': t,
    },
    body: JSON.stringify({ url: sourceUrl, appName: 'MyApp' }),
  });
  return res.json();
}`,
  },
  basic: {
    title: "Basic Integration",
    code: `const REFEREE_BASE = 'http://localhost:14002';
// Obtain the token from the REFEREE desktop UI or headless server startup log.
const REFEREE_TOKEN = '<your-referee-token>';
let heartbeatInterval = null;
let activeSessionId = null;

// 1. Check that REFEREE is running
async function isRefereeReady() {
  try {
    const res = await fetch(\`\${REFEREE_BASE}/v1/status\`);
    const data = await res.json();
    return data.gpuReady === true;
  } catch {
    return false;
  }
}

// 2. Start an upscaling session
async function startSession(sourceUrl, appName, streamTitle) {
  const res = await fetch(\`\${REFEREE_BASE}/v1/stream/start\`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Referee-Token': REFEREE_TOKEN,
    },
    body: JSON.stringify({ url: sourceUrl, appName, streamTitle })
  });

  if (!res.ok) {
    const err = await res.json();
    throw new Error(err.error || \`Start failed with status \${res.status}\`);
  }

  const session = await res.json();
  activeSessionId = session.sessionId;

  // 3. Begin heartbeat
  startHeartbeat(session.sessionId);

  // 4. session.url is already absolute — load it directly in your player
  return session;
}`,
  },
  heartbeat: {
    title: "Heartbeat Loop",
    code: `// Heartbeat loop - send every 10 seconds
// The session ID is a path parameter — no request body needed.
function startHeartbeat(sessionId) {
  stopHeartbeat(); // Clear any existing interval

  heartbeatInterval = setInterval(async () => {
    try {
      const res = await fetch(
        \`\${REFEREE_BASE}/v1/stream/heartbeat/\${sessionId}\`,
        { method: 'POST', headers: { 'X-Referee-Token': REFEREE_TOKEN } }
      );

      if (res.status === 404) {
        // Session was cleaned up server-side — stop beating
        console.warn('REFEREE session lost. Stopping heartbeat.');
        stopHeartbeat();
        onSessionLost();
      }
    } catch (err) {
      // Network error — don't stop the interval.
      // The 15s timeout gives us a small window to recover from transient failures.
      console.warn('Heartbeat failed (will retry):', err.message);
    }
  }, 10000);
}

function stopHeartbeat() {
  if (heartbeatInterval) {
    clearInterval(heartbeatInterval);
    heartbeatInterval = null;
  }
}

// Handle unexpected session loss
function onSessionLost() {
  activeSessionId = null;
  // Update your UI — show "stream ended" message,
  // fall back to original source, or attempt restart.
}`,
  },
  authenticated: {
    title: "Authenticated Streams",
    code: `// For streams behind auth/reverse proxy, pass headers
async function startSession(sourceUrl, appName, streamTitle, headers = {}) {
  const res = await fetch(\`\${REFEREE_BASE}/v1/stream/start\`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Referee-Token': REFEREE_TOKEN,
    },
    body: JSON.stringify({ url: sourceUrl, appName, streamTitle, headers })
  });
  // ...rest of function unchanged
}

// Common auth header examples:

// Bearer token
await startSession(url, appName, streamTitle, {
  'Authorization': \`Bearer \${userToken}\`
});

// Session cookie
await startSession(url, appName, streamTitle, {
  'Cookie': \`session=\${sessionCookie}\`
});

// Cloudflare Access service token
await startSession(url, appName, streamTitle, {
  'CF-Access-Client-Id': '<id>',
  'CF-Access-Client-Secret': '<secret>'
});

// Pangolin resource API key
await startSession(url, appName, streamTitle, {
  'X-Pangolin-Token': '<resource-api-key>'
});`,
  },
  stopSession: {
    title: "Stop Session",
    code: `// Stop session and clean up
async function stopSession() {
  stopHeartbeat();

  if (!activeSessionId) return;

  try {
    await fetch(\`\${REFEREE_BASE}/v1/stream/stop\`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Referee-Token': REFEREE_TOKEN,
      },
      body: JSON.stringify({ sessionId: activeSessionId })
    });
  } catch {
    // Best-effort — server will clean up via timeout if this fails
  }

  activeSessionId = null;
}

// Usage example
async function main() {
  // Start upscaling
  const session = await startSession(
    'https://example.com/live/stream.m3u8',
    'MyApp',
    'Game Day'
  );

  // Load into your player (e.g., hls.js)
  player.src = session.playbackUrl;

  // When the user stops watching
  await stopSession();
}`,
  },
  typescript: {
    title: "TypeScript Types",
    code: `// Copy these interfaces into your project for full type safety.

interface RefereeSettings {
  resolution: string;   // e.g. "1920x1080"
  quality: number;      // 1–5
  framegen: boolean;
  hdr: boolean;
}

interface SessionInfo {
  id: string;
  sourceUrl: string;
  outputUrl: string;    // absolute HLS URL
  outputResolution: string;
  sourceResolution: string | null;
  qualityLevel: number;
  startupComplete: boolean;
  retryingStartup: boolean;
  startupStage: 'starting' | 'ready'; // present on /v1/status session objects
  appName: string | null;
  streamTitle: string | null;
}

interface RefereeStatus {
  gpuReady: boolean;
  gpuName: string | null;
  gpuVendor: 'nvidia' | 'amd' | null;
  gpuUtilization: number | null;
  encoderBackend: 'nvenc' | 'vceenc' | null;
  encoderHasFramegen: boolean | null;
  encoderHasTruehdr: boolean | null;
  encoderHasRife: boolean | null;
  activeSessions: number;
  sessions: SessionInfo[];
  primarySession: SessionInfo | null;
  settings: RefereeSettings;
  selectedExecutor: 'nvidiaSpecialized' | 'universal' | 'cpu' | string | null;
  nvidiaAiAvailable: boolean | null;
  amdAiAvailable: boolean | null;
}

interface StartSessionRequest {
  url: string;                          // required
  appName?: string;
  streamTitle?: string;
  contentKind?: 'live' | 'vod';
  headers?: Record<string, string>;
}

interface StartSessionResponse {
  sessionId: string;
  url: string;          // absolute HLS URL — load directly in your player
  resolution: string;
  sourceResolution: string | null;
  effectiveQuality: number;
  evictedSessions: string[];
  appName: string | null;
  streamTitle: string | null;
}

interface StopSessionRequest {
  sessionId?: string;    // required unless stopAll is true
  stopAll?: boolean;
}

interface RefereeError {
  error: string;
  code: string;  // e.g. 'UNAUTHORIZED', 'SESSION_NOT_FOUND', 'NO_ENCODER'
}`,
  },
  react: {
    title: "React Hook",
    code: `import { useState, useEffect, useRef, useCallback } from 'react';

const REFEREE_BASE = 'http://localhost:14002';
// Obtain the token from the REFEREE desktop UI or headless server startup log.
const REFEREE_TOKEN = '<your-referee-token>';

type RefereeStatus = 'idle' | 'checking' | 'starting' | 'playing' | 'error' | 'unavailable';

export function useReferee() {
  const [status, setStatus] = useState<RefereeStatus>('idle');
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const heartbeatRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const stopHeartbeat = useCallback(() => {
    if (heartbeatRef.current) {
      clearInterval(heartbeatRef.current);
      heartbeatRef.current = null;
    }
  }, []);

  const stop = useCallback(async () => {
    stopHeartbeat();
    const id = sessionIdRef.current;
    sessionIdRef.current = null;
    setPlaybackUrl(null);
    setStatus('idle');
    if (!id) return;
    try {
      await fetch(\`\${REFEREE_BASE}/v1/stream/stop\`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Referee-Token': REFEREE_TOKEN,
        },
        body: JSON.stringify({ sessionId: id }),
      });
    } catch { /* best-effort */ }
  }, [stopHeartbeat]);

  const start = useCallback(async (options: {
    url: string;
    appName?: string;
    streamTitle?: string;
    headers?: Record<string, string>;
  }) => {
    const { url: sourceUrl, appName, streamTitle, headers } = options;
    setError(null);
    setStatus('checking');

    try {
      const statusRes = await fetch(\`\${REFEREE_BASE}/v1/status\`).then(r => r.json());
      if (!statusRes.gpuReady) {
        setStatus('unavailable');
        setError('No compatible GPU detected.');
        return null;
      }
    } catch {
      setStatus('unavailable');
      setError('REFEREE is not running.');
      return null;
    }

    setStatus('starting');
    try {
      const res = await fetch(\`\${REFEREE_BASE}/v1/stream/start\`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'X-Referee-Token': REFEREE_TOKEN,
        },
        body: JSON.stringify({ url: sourceUrl, appName, streamTitle, headers }),
      });
      if (!res.ok) {
        const err = await res.json();
        throw new Error(err.error ?? \`HTTP \${res.status}\`);
      }
      const session = await res.json();
      sessionIdRef.current = session.sessionId;
      const url = session.url; // already absolute
      setPlaybackUrl(url);
      setStatus('playing');

      // Begin heartbeat
      heartbeatRef.current = setInterval(async () => {
        const hb = await fetch(
          \`\${REFEREE_BASE}/v1/stream/heartbeat/\${session.sessionId}\`,
          { method: 'POST', headers: { 'X-Referee-Token': REFEREE_TOKEN } }
        ).catch(() => null);
        if (hb?.status === 404) {
          stopHeartbeat();
          setStatus('idle');
          setPlaybackUrl(null);
          sessionIdRef.current = null;
        }
      }, 10000);

      return url;
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : 'Unknown error';
      setError(msg);
      setStatus('error');
      return null;
    }
  }, [stopHeartbeat]);

  // Pause heartbeat when tab is hidden; resume on focus
  useEffect(() => {
    const handleVisibility = () => {
      if (document.hidden) {
        stopHeartbeat();
      } else if (sessionIdRef.current) {
        // Re-establish heartbeat on return
        heartbeatRef.current = setInterval(async () => {
          await fetch(
            \`\${REFEREE_BASE}/v1/stream/heartbeat/\${sessionIdRef.current}\`,
            { method: 'POST', headers: { 'X-Referee-Token': REFEREE_TOKEN } }
          ).catch(() => null);
        }, 10000);
      }
    };
    document.addEventListener('visibilitychange', handleVisibility);
    return () => document.removeEventListener('visibilitychange', handleVisibility);
  }, [stopHeartbeat]);

  // Cleanup on unmount
  useEffect(() => () => { stop(); }, [stop]);

  return { start, stop, status, playbackUrl, error };
}`,
  },
  hlsPlayer: {
    title: "HLS Player",
    code: `// npm install hls.js
import Hls from 'hls.js';

const REFEREE_BASE = 'http://localhost:14002';
let hls: Hls | null = null;

/**
 * Attach a REFEREE HLS session to a <video> element.
 * Call this after startSession() returns the playback URL.
 */
function attachPlayer(video: HTMLVideoElement, playbackUrl: string) {
  destroyPlayer(); // tear down any previous instance

  if (Hls.isSupported()) {
    hls = new Hls({
      liveSyncDurationCount: 2,   // chase live edge aggressively
      liveMaxLatencyDurationCount: 4,
    });

    hls.loadSource(playbackUrl);
    hls.attachMedia(video);

    hls.on(Hls.Events.MANIFEST_PARSED, () => {
      video.play().catch(() => {
        // Autoplay may be blocked — show a play button instead
      });
    });

    // Recovery: stall or fatal network error → restart session or fall back
    hls.on(Hls.Events.ERROR, async (_event, data) => {
      if (!data.fatal) return;

      if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
        // Check if REFEREE is still alive
        try {
          const status = await fetch(\`\${REFEREE_BASE}/v1/status\`).then(r => r.json());
          if (status.gpuReady && status.activeSessions > 0) {
            hls?.startLoad(); // pipeline still running — retry segments
          } else {
            onRefereeUnavailable(video); // fall back to native source
          }
        } catch {
          onRefereeUnavailable(video);
        }
      } else {
        hls?.destroy();
      }
    });

  } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
    // Safari / iOS — native HLS support
    video.src = playbackUrl;
    video.play().catch(() => {});
  } else {
    console.error('HLS is not supported in this browser.');
  }
}

function destroyPlayer() {
  hls?.destroy();
  hls = null;
}

// Called when REFEREE becomes unavailable mid-stream
function onRefereeUnavailable(video: HTMLVideoElement) {
  destroyPlayer();
  // Fall back to your original, non-upscaled source URL
  // video.src = originalSourceUrl;
  console.warn('REFEREE pipeline lost — falling back to native playback.');
}`,
  },
  degradation: {
    title: "Graceful Degradation",
    code: `const REFEREE_BASE = 'http://localhost:14002';

/**
 * Try to start a REFEREE upscaling session.
 * If REFEREE is unavailable or incompatible, fall back to the
 * original source URL so playback always starts.
 *
 * Returns the URL to load in your player and whether REFEREE is active.
 */
async function withReferee(
  sourceUrl: string,
  nativeUrl: string,
  options?: { appName?: string; streamTitle?: string }
): Promise<{ url: string; upscaled: boolean; reason?: string }> {

  // 1. Is REFEREE running at all?
  let gpuReady = false;
  try {
    const status = await fetch(\`\${REFEREE_BASE}/v1/status\`, { signal: AbortSignal.timeout(2000) });
    const data = await status.json();
    gpuReady = data.gpuReady === true;
  } catch {
    return { url: nativeUrl, upscaled: false, reason: 'REFEREE is not running' };
  }

  if (!gpuReady) {
    return { url: nativeUrl, upscaled: false, reason: 'No compatible GPU detected' };
  }

  // 2. Attempt to start the session
  try {
    const res = await fetch(\`\${REFEREE_BASE}/v1/stream/start\`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Referee-Token': REFEREE_TOKEN,
      },
      body: JSON.stringify({ url: sourceUrl, ...options }),
    });

    if (res.status === 503) {
      return { url: nativeUrl, upscaled: false, reason: 'GPU not ready (503)' };
    }
    if (res.status === 504) {
      return { url: nativeUrl, upscaled: false, reason: 'Stream unreachable or GPU busy (504)' };
    }
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      return { url: nativeUrl, upscaled: false, reason: err.error ?? \`HTTP \${res.status}\` };
    }

    const session = await res.json();
    return {
      url: session.url, // already absolute
      upscaled: true,
    };
  } catch {
    return { url: nativeUrl, upscaled: false, reason: 'REFEREE unreachable during startup' };
  }
}

// ─── Example usage ───────────────────────────────────────────────────────────

async function play(sourceUrl: string, nativeUrl: string) {
  const { url, upscaled, reason } = await withReferee(sourceUrl, nativeUrl, {
    appName: 'MyApp',
    streamTitle: 'Live Stream',
  });

  if (!upscaled) {
    console.info(\`Playing native source. Reason: \${reason}\`);
    // Optionally show a small badge: "Upscaling unavailable"
  }

  // Load \`url\` into your player regardless — it's always valid
  loadInPlayer(url);
}`,
  },
  lifecycle: {
    title: "Lifecycle & Cleanup",
    code: `const REFEREE_BASE = 'http://localhost:14002';
const REFEREE_TOKEN = '<your-referee-token>';
let heartbeatInterval: ReturnType<typeof setInterval> | null = null;
let activeSessionId: string | null = null;

// ─── 1. Page Visibility — pause/resume heartbeat ─────────────────────────────
//
// Browsers throttle or suspend timers in background tabs.
// Stop the heartbeat when hidden; restart it when the user returns.

document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    if (heartbeatInterval) {
      clearInterval(heartbeatInterval);
      heartbeatInterval = null;
    }
  } else if (activeSessionId) {
    // Tab became visible again — re-establish heartbeat
    heartbeatInterval = setInterval(
      () => sendHeartbeat(activeSessionId!),
      10000
    );
  }
});

// ─── 2. SPA navigation — React useEffect cleanup ─────────────────────────────
//
// In React Router / Next.js, return a cleanup fn from useEffect.
// This fires when the component unmounts (route changes away).

// Example:
// useEffect(() => {
//   start(sourceUrl);
//   return () => { stopSession(); };
// }, [sourceUrl]);

// ─── 3. Page unload — sendBeacon is navigation-safe ──────────────────────────
//
// fetch() is cancelled on navigation. sendBeacon() is not.
// Use it to stop the session when the user closes the tab or navigates away.

window.addEventListener('pagehide', () => {
  if (!activeSessionId) return;
  const body = JSON.stringify({ sessionId: activeSessionId });
  navigator.sendBeacon(
    \`\${REFEREE_BASE}/v1/stream/stop\`,
    new Blob([body], { type: 'application/json' })
  );
});

// ─── 4. Multi-tab detection ───────────────────────────────────────────────────
//
// REFEREE only supports one session at a time.
// Check /status before starting to warn the user if another tab is active.
// Note: /v1/stream/start returns an \`evictedSessions\` array containing the IDs
// of any sessions that were automatically stopped to make room for the new one.

async function checkForConflict(): Promise<boolean> {
  try {
    const res = await fetch(\`\${REFEREE_BASE}/v1/status\`);
    const data = await res.json();
    return data.activeSessions > 0;
  } catch {
    return false;
  }
}

async function startWithConflictCheck(sourceUrl: string) {
  const conflict = await checkForConflict();
  if (conflict) {
    const proceed = confirm(
      'REFEREE is active in another tab. Starting here will stop that session. Continue?'
    );
    if (!proceed) return;
  }
  await startSession(sourceUrl);
}`,
  },
}

function CopyButton({ content }: { content: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <Button
      variant="ghost"
      size="sm"
      className="absolute right-3 top-3 z-10 h-8 w-8 p-0"
      onClick={async () => {
        await navigator.clipboard.writeText(content)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      }}
    >
      {copied ? <Check className="h-4 w-4 text-accent" /> : <Copy className="h-4 w-4" />}
    </Button>
  )
}

function PackageInstall() {
  const [pkg, setPkg] = useState<keyof typeof installExamples>("npm")
  return (
    <div>
      <div className="mb-2 flex gap-2">
        {(Object.keys(installExamples) as Array<keyof typeof installExamples>).map((key) => (
          <button
            key={key}
            onClick={() => setPkg(key)}
            className={`rounded-md px-3 py-1 text-xs font-mono transition-colors ${
              pkg === key
                ? "bg-accent/15 text-accent border border-accent/30"
                : "text-muted-foreground hover:text-foreground"
            }`}
          >
            {key}
          </button>
        ))}
      </div>
      <div className="relative overflow-hidden rounded-xl border border-border/70 bg-secondary/40">
        <CopyButton content={installExamples[pkg]} />
        <pre className="overflow-x-auto px-4 py-3">
          <code className="text-[13px] font-mono text-foreground/90">{installExamples[pkg]}</code>
        </pre>
      </div>
    </div>
  )
}

export function CodeExamples() {
  const [copiedTab, setCopiedTab] = useState<string | null>(null)

  const copyCode = async (key: string, code: string) => {
    await navigator.clipboard.writeText(code)
    setCopiedTab(key)
    setTimeout(() => setCopiedTab(null), 2000)
  }

  return (
    <section id="integration" className="scroll-mt-28 pt-10 pb-0">
      <div>
        <div className="pb-6">
          <div className="mb-4 flex items-center gap-3">
            <BookOpen className="h-5 w-5 flex-shrink-0 text-accent" />
            <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">
              Integration Guide
            </h1>
          </div>
          <p className="text-base leading-7 text-muted-foreground text-pretty">
            This page provides examples and implementation notes for integrating{" "}
            <RefereeWordmark variant="inline" /> into your web app or streaming workflow.
          </p>
        </div>

        {/* ── npm package ─────────────────────────────────────────────────── */}
        <section id="client-package" className="scroll-mt-28 mt-2 rounded-xl border border-border/60 bg-card/40 px-5 py-6 sm:px-6">
          <div className="mb-4 flex items-center gap-3">
            <Package className="h-5 w-5 flex-shrink-0 text-accent" />
            <h2 className="text-xl font-semibold tracking-tight sm:text-2xl">npm Client Package</h2>
          </div>
          <p className="mb-5 text-sm leading-7 text-muted-foreground">
            The official <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">@cmacrowther/referee-client</code> package
            bundles TypeScript types, a <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono">RefereeClient</code> class
            with auto-auth and managed heartbeating, and a plug-and-play React hook — so you don&apos;t have to wire
            up the lifecycle yourself.
          </p>

          {/* install tabs */}
          <PackageInstall />

          {/* usage */}
          <div className="mt-5">
            <p className="mb-2 text-sm font-medium">Usage</p>
            <div className="relative overflow-hidden rounded-xl border border-border/70 bg-secondary/40">
              <CopyButton content={packageUsage} />
              <pre className="overflow-x-auto p-4 sm:p-6">
                <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">{packageUsage}</code>
              </pre>
            </div>
          </div>
        </section>
        <section id="quick-start" className="scroll-mt-28 pt-6">
          <Tabs defaultValue="auth" className="w-full">
          <div className="-mx-1 overflow-x-auto px-1 pb-2 sm:mx-0 sm:px-0 sm:pb-0">
            <div className="mb-4 flex items-center gap-3">
              <Rocket className="h-5 w-5 flex-shrink-0 text-accent" />
              <h2 className="text-2xl font-semibold tracking-tight sm:text-3xl">Quick Start</h2>
            </div>
            <p className="text-sm leading-7 text-muted-foreground sm:text-base">
              These examples show how to integrate REFEREE into your web app or streaming workflow. The API is designed to be simple and flexible, so you can implement it in any language or framework that can make HTTP requests
              . Each example is standalone — start with the one that best matches your stack, or mix and match patterns as needed. For a quick start, see the{" "}<a href="#degradation" className="underline hover:text-foreground">graceful degradation</a>{" "}example, which shows how to add REFEREE support without disrupting existing playback for users without compatible GPUs.
            </p>
            <TabsList className="mt-8 h-auto w-max min-w-full justify-start gap-2 rounded-none border-b border-border/70 bg-transparent p-0 sm:w-full sm:min-w-0 sm:flex-wrap sm:gap-3">
              {Object.entries(examples).map(([key, { title }]) => (
                <TabsTrigger
                  key={key}
                  value={key}
                  className="flex-none rounded-md border border-transparent px-3 py-2 text-sm font-medium leading-5 data-[state=active]:border-accent/40 data-[state=active]:bg-accent/10 sm:rounded-none sm:border-x-0 sm:border-t-0 sm:border-b-2 sm:px-1 sm:py-0 sm:pb-3 sm:pt-2 sm:data-[state=active]:border-accent sm:data-[state=active]:bg-transparent"
                >
                  {title}
                </TabsTrigger>
              ))}
            </TabsList>
          </div>
          
          {Object.entries(examples).map(([key, { code }]) => (
            <TabsContent key={key} value={key} className="mt-6">
              <div className="relative overflow-hidden rounded-xl border border-border/70 bg-secondary/40">
                <Button
                  variant="ghost"
                  size="sm"
                  className="absolute right-3 top-3 z-10 h-8 w-8 p-0"
                  onClick={() => copyCode(key, code)}
                >
                  {copiedTab === key ? (
                    <Check className="h-4 w-4 text-accent" />
                  ) : (
                    <Copy className="h-4 w-4" />
                  )}
                </Button>
                <pre className="overflow-x-auto p-4 sm:p-6">
                  <code className="text-[13px] font-mono leading-6 text-foreground/90 sm:text-sm">
                    {code}
                  </code>
                </pre>
              </div>
            </TabsContent>
          ))}
          </Tabs>
        </section>

        <div className="mt-12 grid gap-6 lg:grid-cols-[minmax(0,1.45fr)_minmax(280px,0.95fr)]">
          <section
            id="session-lifecycle"
            className="rounded-xl border border-border/60 bg-background/70 px-5 py-6 sm:px-6"
          >
            <h3 className="mb-4 text-xl font-semibold tracking-tight sm:text-2xl">Session Lifecycle</h3>
            <div className="space-y-3 text-sm leading-7 text-muted-foreground">
              <p>
                <span className="text-accent">1.</span> GET /v1/status — Verify{" "}
                REFEREE is healthy and GPU is ready
              </p>
              <p><span className="text-accent">2.</span> POST /v1/stream/start — Blocking call (up to ~3 min) — waits until upscaled HLS output is ready</p>
              <p><span className="text-accent">3.</span> Load HLS URL in player — Begin playback (e.g., hls.js, Video.js)</p>
              <p><span className="text-accent">4.</span> POST /v1/stream/heartbeat/{'{'}sessionId{'}'} — Send every 10 seconds while playing</p>
              <p><span className="text-accent">5.</span> POST /v1/stream/stop — Stop session when playback ends</p>
            </div>
          </section>

          <section
            id="heartbeat-contract"
            className="rounded-xl border border-border/60 bg-card/40 px-5 py-6 sm:px-6"
          >
            <h3 className="mb-4 text-xl font-semibold tracking-tight sm:text-2xl">Heartbeat Contract</h3>
            <div className="grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-1 2xl:grid-cols-2">
              <div className="rounded-lg border border-border/60 bg-background/70 px-4 py-4">
                <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">Recommended interval</p>
                <p className="mt-2 font-mono text-base text-accent">10 seconds</p>
              </div>
              <div className="rounded-lg border border-border/60 bg-background/70 px-4 py-4">
                <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">Server timeout</p>
                <p className="mt-2 font-mono text-base text-accent">15 seconds</p>
              </div>
              <div className="rounded-lg border border-border/60 bg-background/70 px-4 py-4">
                <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">Missed heartbeats tolerated</p>
                <p className="font-mono text-accent">~0–1 consecutive</p>
              </div>
              <div className="rounded-lg border border-border/60 bg-background/70 px-4 py-4">
                <p className="text-xs font-medium uppercase tracking-[0.14em] text-muted-foreground">Orphan grace period</p>
                <p className="mt-2 font-mono text-base text-accent">5 minutes</p>
                <p className="mt-1 text-xs text-muted-foreground">Before first heartbeat only. After heartbeats start, server timeout (15 s) applies directly.</p>
              </div>
            </div>
          </section>
        </div>
      </div>
    </section>
  )
}
