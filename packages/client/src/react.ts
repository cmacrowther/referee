"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import { RefereeClient, RefereeApiError } from "./index.js"
import type { RefereeClientOptions, StartSessionOptions, StartSessionResponse } from "./index.js"

export type RefereeHookStatus =
  | "idle"
  | "checking"
  | "starting"
  | "playing"
  | "error"
  | "unavailable"

export interface UseRefereeOptions extends RefereeClientOptions {
  /** Heartbeat interval in ms. @default 10000 */
  heartbeatIntervalMs?: number
}

export interface UseRefereeResult {
  /** Current status of the REFEREE session. */
  status: RefereeHookStatus
  /** The absolute HLS URL to load in your player. `null` when no session is active. */
  playbackUrl: string | null
  /** Error message if `status === "error"`. */
  error: string | null
  /** Full session info from the last successful `start()` call. */
  session: StartSessionResponse | null
  /**
   * Start an upscaling session.
   * Returns the playback URL on success, or `null` if REFEREE is unavailable.
   */
  start: (options: StartSessionOptions) => Promise<string | null>
  /** Stop the active session and release GPU resources. */
  stop: () => Promise<void>
}

/**
 * React hook for managing a REFEREE upscaling session.
 *
 * Handles the full lifecycle: availability check, session start, heartbeat,
 * and cleanup on unmount.
 *
 * @example
 * ```tsx
 * function VideoPlayer({ sourceUrl }: { sourceUrl: string }) {
 *   const referee = useReferee({ appName: 'MyApp' });
 *
 *   useEffect(() => {
 *     referee.start({ url: sourceUrl, streamTitle: 'Live Stream' });
 *     return () => { referee.stop(); };
 *   }, [sourceUrl]);
 *
 *   if (referee.status === 'unavailable') {
 *     return <NativePlayer src={sourceUrl} />;
 *   }
 *   if (referee.playbackUrl) {
 *     return <HlsPlayer src={referee.playbackUrl} />;
 *   }
 *   return <Spinner />;
 * }
 * ```
 */
export function useReferee(options: UseRefereeOptions = {}): UseRefereeResult {
  const { heartbeatIntervalMs = 10_000, ...clientOptions } = options

  // Stable client instance across renders.
  const clientRef = useRef<RefereeClient | null>(null)
  if (clientRef.current === null) {
    clientRef.current = new RefereeClient(clientOptions)
  }
  const client = clientRef.current

  const [status, setStatus] = useState<RefereeHookStatus>("idle")
  const [playbackUrl, setPlaybackUrl] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [session, setSession] = useState<StartSessionResponse | null>(null)

  const sessionIdRef = useRef<string | null>(null)
  const heartbeatRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const stopHeartbeat = useCallback(() => {
    if (heartbeatRef.current !== null) {
      clearInterval(heartbeatRef.current)
      heartbeatRef.current = null
    }
  }, [])

  const startHeartbeat = useCallback(
    (sessionId: string) => {
      stopHeartbeat()
      heartbeatRef.current = setInterval(async () => {
        const alive = await client.sendHeartbeat(sessionId)
        if (!alive) {
          stopHeartbeat()
          sessionIdRef.current = null
          setStatus("idle")
          setPlaybackUrl(null)
          setSession(null)
        }
      }, heartbeatIntervalMs)
    },
    [client, heartbeatIntervalMs, stopHeartbeat]
  )

  const stop = useCallback(async () => {
    stopHeartbeat()
    const id = sessionIdRef.current
    sessionIdRef.current = null
    setPlaybackUrl(null)
    setSession(null)
    setStatus("idle")
    if (id) {
      await client.stopSession(id).catch(() => {})
    }
  }, [client, stopHeartbeat])

  const start = useCallback(
    async (startOptions: StartSessionOptions): Promise<string | null> => {
      setError(null)
      setStatus("checking")

      const available = await client.isAvailable()
      if (!available) {
        setStatus("unavailable")
        setError("REFEREE is not running or no compatible GPU detected.")
        return null
      }

      setStatus("starting")
      try {
        const s = await client.startSession(startOptions)
        sessionIdRef.current = s.sessionId
        setSession(s)
        setPlaybackUrl(s.url)
        setStatus("playing")
        startHeartbeat(s.sessionId)
        return s.url
      } catch (e) {
        const msg =
          e instanceof RefereeApiError
            ? `${e.message} (${e.code})`
            : e instanceof Error
              ? e.message
              : "Unknown error"
        setError(msg)
        setStatus("error")
        return null
      }
    },
    [client, startHeartbeat]
  )

  // Best-effort stop on page unload.
  useEffect(() => {
    const handlePageHide = () => {
      const id = sessionIdRef.current
      if (!id) return
      client.stopSessionOnUnload(id)
    }
    window.addEventListener("pagehide", handlePageHide)
    return () => window.removeEventListener("pagehide", handlePageHide)
  }, [client])

  // Cleanup on unmount.
  useEffect(() => () => { stop() }, [stop])

  return { status, playbackUrl, error, session, start, stop }
}
