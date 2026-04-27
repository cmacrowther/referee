"use client"

import { useState, useEffect, useRef, useCallback } from "react"
import type Hls from "hls.js"
import { useRefereeStatus } from "@/hooks/use-referee-status"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Input } from "@/components/ui/input"
import { PlatformDownloadButton } from "@/components/platform-download-button"
import { RefereeWordmark } from "@/components/referee-wordmark"
import {
  Zap,
  Sparkles,
  WifiOff,
  CheckCircle2,
  Maximize,
  Minimize,
} from "lucide-react"

interface TestStream {
  id: string
  label: string
  tag: string
  sourceUrl: string
  posterUrl: string
  sourceResolution: string
  streamTitle: string
  attributionYear: number
  attributionAuthor: string
  attributionLicense: string
  attributionLicenseUrl: string
  attributionCdn: string
}

// © Blender Foundation | creativecommons.org/licenses/by/3.0/
const STREAMS: TestStream[] = [
  {
    id: "bbb",
    label: "Big Buck Bunny",
    tag: "Animation",
    sourceUrl: "https://test-streams.mux.dev/x36xhzz/url_6/193039199_mp4_h264_aac_hq_7.m3u8",
    posterUrl: "https://upload.wikimedia.org/wikipedia/commons/thumb/c/c5/Big_buck_bunny_poster_big.jpg/1280px-Big_buck_bunny_poster_big.jpg",
    sourceResolution: "480p",
    streamTitle: "Big Buck Bunny",
    attributionYear: 2008,
    attributionAuthor: "Blender Foundation",
    attributionLicense: "CC BY 3.0",
    attributionLicenseUrl: "https://creativecommons.org/licenses/by/3.0/",
    attributionCdn: "Mux test streams CDN",
  },
  {
    id: "tos",
    label: "Tears of Steel",
    tag: "Live Action",
    sourceUrl: "https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/.m3u8",
    posterUrl: "https://upload.wikimedia.org/wikipedia/commons/5/55/Tears_of_Steel_poster.jpg",
    sourceResolution: "1080p",
    streamTitle: "Tears of Steel",
    attributionYear: 2012,
    attributionAuthor: "Blender Foundation",
    attributionLicense: "CC BY 3.0",
    attributionLicenseUrl: "https://creativecommons.org/licenses/by/3.0/",
    attributionCdn: "Unified Streaming demo CDN",
  },
]
const RESOLUTION_LABELS: Record<string, string> = {
  "3840x2160": "4K",
  "2560x1440": "1440p",
  "1920x1080": "1080p",
  "1280x720": "720p",
  "854x480": "480p",
  "640x360": "360p",
}
function formatResolution(res: string): string {
  return RESOLUTION_LABELS[res] ?? res
}
const REFEREE_BASE = "http://localhost:14002"
const REMOTE_REFEREE_BASE_STORAGE_KEY = "referee-demo.remote-referee-base"
const REFEREE_TOKEN_STORAGE_KEY = "referee-demo.referee-token"
const HEARTBEAT_MS = 10_000


function normalizeRefereeBase(value: string) {
  const trimmed = value.trim()
  if (!trimmed) return null

  const withProtocol = /^https?:\/\//i.test(trimmed) ? trimmed : `http://${trimmed}`

  try {
    const url = new URL(withProtocol)
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      return null
    }

    const normalizedPath = url.pathname.replace(/\/+$/, "")
    return `${url.origin}${normalizedPath === "/" ? "" : normalizedPath}`
  } catch {
    return null
  }
}

/**
 * Returns true when the page is served over HTTPS but the given Referee base
 * URL is plain HTTP on a non-loopback host. Browsers block this as mixed
 * active content, so we surface a warning before the user tries to connect.
 */
function isMixedContentRisk(refereeBase: string): boolean {
  if (typeof window === "undefined") return false
  if (window.location.protocol !== "https:") return false
  try {
    const url = new URL(refereeBase)
    if (url.protocol !== "http:") return false
    const h = url.hostname
    return h !== "localhost" && h !== "127.0.0.1" && h !== "[::1]" && h !== "::1"
  } catch {
    return false
  }
}

function resolveRefereeUrl(base: string, value: string) {
  try {
    const baseUrl = new URL(base)
    const valueUrl = new URL(value)
    valueUrl.protocol = baseUrl.protocol
    valueUrl.hostname = baseUrl.hostname
    valueUrl.port = baseUrl.port
    return valueUrl.toString()
  } catch {
    return value
  }
}

export function ComparisonPlayer() {
  const containerRef = useRef<HTMLDivElement>(null)
  const sourceVideoRef = useRef<HTMLVideoElement>(null)
  const upscaledVideoRef = useRef<HTMLVideoElement>(null)
  const hlsRef = useRef<Hls | null>(null)
  const sourceHlsRef = useRef<Hls | null>(null)
  const sessionIdRef = useRef<string | null>(null)
  const sessionBaseRef = useRef(REFEREE_BASE)
  const heartbeatIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const tokenRef = useRef("")
  const tokenPersistentRef = useRef(true) // tokens loaded from localStorage are persistent by definition
  const upscaledNetworkErrorCountRef = useRef(0)
  const isDraggingRef = useRef(false)
  const sliderPosRef = useRef(50)
  const pendingSliderPosRef = useRef<number | null>(null)
  const sliderAnimationFrameRef = useRef<number | null>(null)

  const [selectedStreamId, setSelectedStreamId] = useState("bbb")
  const activeStream = STREAMS.find(s => s.id === selectedStreamId) ?? STREAMS[0]

  const [refereeBase, setRefereeBase] = useState(REFEREE_BASE)
  const { status: refereeStatus, hdrEnabled, framegenEnabled } = useRefereeStatus(refereeBase)
  const [demoStarted, setDemoStarted] = useState(false)
  const [startError, setStartError] = useState<string | null>(null)
  const [tokenInput, setTokenInput] = useState("")
  const [refereeToken, setRefereeToken] = useState("")
  const [autoTokenAcquired, setAutoTokenAcquired] = useState(false)
  const [autoAuthPending, setAutoAuthPending] = useState(false)
  const autoAuthAttemptedRef = useRef(false)
  const [sliderPos, setSliderPos] = useState(50)
  const [isDragging, setIsDragging] = useState(false)
  const [hasPlaybackStarted, setHasPlaybackStarted] = useState(false)
  const [upscaledHlsUrl, setUpscaledHlsUrl] = useState<string | null>(null)
  const [upscaledResolution, setUpscaledResolution] = useState<string | null>(null)
  const [remoteBaseInput, setRemoteBaseInput] = useState("")
  const [remoteBaseError, setRemoteBaseError] = useState<string | null>(null)
  const [showRemoteConnectForm, setShowRemoteConnectForm] = useState(false)
  const [isFullscreen, setIsFullscreen] = useState(false)

  const isUsingCustomRefereeBase = refereeBase !== REFEREE_BASE

  useEffect(() => {
    const savedToken = window.localStorage.getItem(REFEREE_TOKEN_STORAGE_KEY)
    if (savedToken) {
      setTokenInput(savedToken)
      setRefereeToken(savedToken)
      tokenRef.current = savedToken
    }

    const savedBase = window.localStorage.getItem(REMOTE_REFEREE_BASE_STORAGE_KEY)
    if (!savedBase) return

    const normalizedBase = normalizeRefereeBase(savedBase)
    if (!normalizedBase || normalizedBase === REFEREE_BASE) {
      window.localStorage.removeItem(REMOTE_REFEREE_BASE_STORAGE_KEY)
      return
    }

    setRefereeBase(normalizedBase)
    setRemoteBaseInput(normalizedBase)
    setShowRemoteConnectForm(true)
  }, [])

  useEffect(() => {
    tokenRef.current = refereeToken
  }, [refereeToken])

  // Auto-request an API token from the REFEREE desktop app via the consent endpoint.
  // The browser sends the Origin header automatically; REFEREE shows a native dialog
  // so the user approves (or denies) without ever entering a token manually.
  useEffect(() => {
    if (refereeStatus !== "connected") {
      // Reset so we retry when REFEREE reconnects or the base URL changes.
      setAutoTokenAcquired(false)
      setAutoAuthPending(false)
      autoAuthAttemptedRef.current = false
      return
    }
    if (autoAuthAttemptedRef.current) return
    autoAuthAttemptedRef.current = true

    const tryAutoAuth = async () => {
      setAutoAuthPending(true)
      try {
        const res = await fetch(`${refereeBase}/v1/auth/request`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ appName: "REFEREE Docs Demo" }),
          signal: AbortSignal.timeout(65_000), // slightly longer than server's 60 s timeout
        })
        if (res.ok) {
          const data = await res.json()
          const token: string | undefined = data?.token
          if (token) {
            setTokenInput(token)
            setRefereeToken(token)
            tokenRef.current = token
            // Only persist to localStorage when the approval is permanent ("Always Allow"
            // or a pre-approved origin). "Allow Once" approvals are intentionally
            // session-only: the absence of a saved token ensures REFEREE re-prompts on
            // the next page load. Clear any previously-saved Allow-Once token so it
            // cannot be used to bypass the consent flow on reload.
            const persistent = data.persistent !== false
            tokenPersistentRef.current = persistent
            if (persistent) {
              window.localStorage.setItem(REFEREE_TOKEN_STORAGE_KEY, token)
            } else {
              window.localStorage.removeItem(REFEREE_TOKEN_STORAGE_KEY)
            }
            setAutoTokenAcquired(true)
          }
        }
        // Non-OK (e.g. 403 headless / denied) — silently fall back to token input.
      } catch {
        // Network error or timeout — silently fall back to token input.
      } finally {
        setAutoAuthPending(false)
      }
    }

    tryAutoAuth()
  }, [refereeStatus, refereeBase])

  // --- Stream lifecycle ---
  const stopUpscaledStream = useCallback(async () => {
    if (heartbeatIntervalRef.current) {
      clearInterval(heartbeatIntervalRef.current)
      heartbeatIntervalRef.current = null
    }
    if (hlsRef.current) {
      hlsRef.current.destroy()
      hlsRef.current = null
    }
    const id = sessionIdRef.current
    if (id) {
      const activeSessionBase = sessionBaseRef.current
      sessionIdRef.current = null
      setUpscaledHlsUrl(null)
      fetch(`${activeSessionBase}/v1/stream/stop`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Referee-Token": tokenRef.current,
        },
        body: JSON.stringify({ sessionId: id }),
      }).catch(() => {})
    }
  }, [])

  const startUpscaledStream = useCallback(async (stream: TestStream) => {
    const activeRefereeBase = refereeBase
    const activeToken = tokenRef.current

    try {
      const res = await fetch(`${activeRefereeBase}/v1/stream/start`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Referee-Token": activeToken,
        },
        body: JSON.stringify({ url: stream.sourceUrl, appName: `REFEREE Demo`, streamTitle: stream.streamTitle }),
      })
      if (!res.ok) {
        if (res.status === 401) {
          setStartError("Invalid API token. Copy your token from the REFEREE desktop app or server startup log.")
        } else {
          setStartError(`Failed to start pipeline (HTTP ${res.status}). Check that REFEREE is running.`)
        }
        setDemoStarted(false)
        return
      }
      setStartError(null)
      const { sessionId, url, resolution } = await res.json()
      sessionIdRef.current = sessionId
      sessionBaseRef.current = activeRefereeBase
      setUpscaledHlsUrl(resolveRefereeUrl(activeRefereeBase, url))
      if (resolution) setUpscaledResolution(resolution)
      heartbeatIntervalRef.current = setInterval(() => {
        fetch(
          `${sessionBaseRef.current}/v1/stream/heartbeat/${sessionId}`,
          { method: "POST", headers: { "X-Referee-Token": activeToken } }
        ).catch(() => {})
      }, HEARTBEAT_MS)
    } catch {
      setStartError("Could not reach REFEREE. Check that it is running and accessible.")
      setDemoStarted(false)
    }
  }, [refereeBase])

  // When REFEREE disconnects, tear down any active demo
  useEffect(() => {
    if (refereeStatus !== "connected") {
      setDemoStarted(false)
      setHasPlaybackStarted(false)
      stopUpscaledStream()
    }
  }, [refereeStatus, stopUpscaledStream])

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      stopUpscaledStream()
    }
  }, [stopUpscaledStream])

  const handleStartDemo = useCallback(async () => {
    const token = tokenInput.trim()
    setRefereeToken(token)
    tokenRef.current = token
    if (token && tokenPersistentRef.current) {
      window.localStorage.setItem(REFEREE_TOKEN_STORAGE_KEY, token)
    }
    setStartError(null)
    setDemoStarted(true)
    setHasPlaybackStarted(false)
    await startUpscaledStream(activeStream)
  }, [tokenInput, activeStream, startUpscaledStream])

  const handleSelectStream = useCallback((streamId: string) => {
    if (streamId === selectedStreamId) return
    if (demoStarted) {
      stopUpscaledStream()
      setDemoStarted(false)
      setHasPlaybackStarted(false)
      setUpscaledResolution(null)
    }
    setSelectedStreamId(streamId)
  }, [selectedStreamId, demoStarted, stopUpscaledStream])

  const handleRemoteConnect = useCallback(
    (event: React.FormEvent<HTMLFormElement>) => {
      event.preventDefault()

      const normalizedBase = normalizeRefereeBase(remoteBaseInput)
      if (!normalizedBase) {
        setRemoteBaseError("Enter a valid http:// or https:// REFEREE URL.")
        return
      }

      setRemoteBaseError(null)
      setRemoteBaseInput(normalizedBase)
      setShowRemoteConnectForm(true)
      setRefereeBase(normalizedBase)

      const token = tokenInput.trim()
      setRefereeToken(token)
      tokenRef.current = token
      if (token) {
        window.localStorage.setItem(REFEREE_TOKEN_STORAGE_KEY, token)
      }

      if (normalizedBase === REFEREE_BASE) {
        window.localStorage.removeItem(REMOTE_REFEREE_BASE_STORAGE_KEY)
        return
      }

      window.localStorage.setItem(REMOTE_REFEREE_BASE_STORAGE_KEY, normalizedBase)
    },
    [remoteBaseInput, tokenInput]
  )

  const handleUseLocalReferee = useCallback(() => {
    setRemoteBaseError(null)
    setRemoteBaseInput("")
    setShowRemoteConnectForm(false)
    setRefereeBase(REFEREE_BASE)
    window.localStorage.removeItem(REMOTE_REFEREE_BASE_STORAGE_KEY)
  }, [])

  // --- Attach hls.js to source video when the active player is visible ---
  useEffect(() => {
    const video = sourceVideoRef.current
    if (!video || !demoStarted || !upscaledHlsUrl) return

    let active = true

    const attach = async () => {
      const HlsModule = await import("hls.js")
      const Hls = HlsModule.default
      if (!active) return

      if (Hls.isSupported()) {
        const hls = new Hls({
          enableWorker: true,
          maxLiveSyncPlaybackRate: 1.0, // Disable HLS live-edge rate correction; our sync logic controls rate
        })
        sourceHlsRef.current = hls
        hls.loadSource(activeStream.sourceUrl)
        hls.attachMedia(video)
      } else if (video.canPlayType("application/vnd.apple.mpegurl")) {
        video.src = activeStream.sourceUrl
      }
    }

    attach()

    return () => {
      active = false
      if (sourceHlsRef.current) {
        sourceHlsRef.current.destroy()
        sourceHlsRef.current = null
      }
      video.pause()
      video.removeAttribute("src")
      video.load()
    }
  }, [demoStarted, upscaledHlsUrl, activeStream.sourceUrl])

  // --- Attach hls.js when upscaledHlsUrl changes ---
  useEffect(() => {
    const video = upscaledVideoRef.current
    if (!video || !upscaledHlsUrl) return

    let active = true
    const resumeUpscaled = () => {
      if (!active) return
      video.play().catch(() => {})
    }

    const attach = async () => {
      const HlsModule = await import("hls.js")
      const Hls = HlsModule.default
      if (!active) return

      if (hlsRef.current) {
        hlsRef.current.destroy()
        hlsRef.current = null
      }

      if (Hls.isSupported()) {
        const hls = new Hls({
          enableWorker: true,
          lowLatencyMode: false,
          backBufferLength: 0,
          liveSyncDurationCount: 3,
          liveMaxLatencyDurationCount: 10,
          maxLiveSyncPlaybackRate: 1.05,
        })
        hlsRef.current = hls
        hls.loadSource(upscaledHlsUrl)
        hls.attachMedia(video)
        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          upscaledNetworkErrorCountRef.current = 0
          resumeUpscaled()
        })
        hls.on(Hls.Events.LEVEL_LOADED, () => {
          upscaledNetworkErrorCountRef.current = 0
          resumeUpscaled()
        })
        hls.on(Hls.Events.ERROR, (_event, data) => {
          if (!data.fatal) return

          if (data.type === Hls.ErrorTypes.NETWORK_ERROR) {
            upscaledNetworkErrorCountRef.current += 1
            if (upscaledNetworkErrorCountRef.current < 3) {
              hls.startLoad()
              return
            }
            // Stream was stopped on the REFEREE side — reset so the user can start a new session.
            upscaledNetworkErrorCountRef.current = 0
            stopUpscaledStream()
            setDemoStarted(false)
            setHasPlaybackStarted(false)
            return
          }

          if (data.type === Hls.ErrorTypes.MEDIA_ERROR) {
            hls.recoverMediaError()
            return
          }

          hls.destroy()
          if (hlsRef.current === hls) {
            hlsRef.current = null
          }
        })
      } else if (video.canPlayType("application/vnd.apple.mpegurl")) {
        video.src = upscaledHlsUrl
      }
    }

    video.addEventListener("loadedmetadata", resumeUpscaled)
    video.addEventListener("canplay", resumeUpscaled)
    attach()

    return () => {
      active = false
      video.removeEventListener("loadedmetadata", resumeUpscaled)
      video.removeEventListener("canplay", resumeUpscaled)
      if (hlsRef.current) {
        hlsRef.current.destroy()
        hlsRef.current = null
      }
      video.pause()
      video.removeAttribute("src")
      video.load()
    }
  }, [stopUpscaledStream, upscaledHlsUrl])

  useEffect(() => {
    const src = sourceVideoRef.current
    if (!src || !demoStarted || !upscaledHlsUrl) return

    const handlePlaying = () => setHasPlaybackStarted(true)

    src.addEventListener("playing", handlePlaying)
    return () => src.removeEventListener("playing", handlePlaying)
  }, [demoStarted, upscaledHlsUrl])

  // --- Sync source to upscaled (upscaled is master) ---
  useEffect(() => {
    if (!hasPlaybackStarted) return

    const upscaled = upscaledVideoRef.current
    const source = sourceVideoRef.current
    if (!upscaled || !source) return

    const SYNC_INTERVAL_MS = 1_000
    // Drift beyond this triggers a hard seek on source instead of rate adjustment
    const HARD_SEEK_THRESHOLD_S = 3.0
    // Drift beyond this nudges source playback rate to converge smoothly
    const NUDGE_THRESHOLD_S = 0.2
    const RATE_FAST = 1.06   // source is behind upscaled → speed up source
    const RATE_SLOW = 0.94   // source is ahead of upscaled → slow down source

    const syncTick = () => {
      // Don't chase while upscaled is stalled/buffering
      if (upscaled.paused || upscaled.readyState < 2) return
      if (source.readyState < 2) return

      const drift = upscaled.currentTime - source.currentTime
      const absDrift = Math.abs(drift)

      if (absDrift > HARD_SEEK_THRESHOLD_S) {
        source.currentTime = upscaled.currentTime
        source.playbackRate = 1.0
      } else if (drift > NUDGE_THRESHOLD_S) {
        source.playbackRate = RATE_FAST
      } else if (drift < -NUDGE_THRESHOLD_S) {
        source.playbackRate = RATE_SLOW
      } else {
        source.playbackRate = 1.0
      }
    }

    // Mirror upscaled pause/play → source so the source never runs ahead
    const onUpscaledPause = () => { if (!source.paused) source.pause() }
    const onUpscaledPlay = () => { if (source.paused) source.play().catch(() => {}) }

    upscaled.addEventListener("pause", onUpscaledPause)
    upscaled.addEventListener("play", onUpscaledPlay)

    const interval = setInterval(syncTick, SYNC_INTERVAL_MS)

    return () => {
      clearInterval(interval)
      upscaled.removeEventListener("pause", onUpscaledPause)
      upscaled.removeEventListener("play", onUpscaledPlay)
      source.playbackRate = 1.0
    }
  }, [hasPlaybackStarted])

  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsFullscreen(document.fullscreenElement === containerRef.current)
    }

    handleFullscreenChange()
    document.addEventListener("fullscreenchange", handleFullscreenChange)

    return () => {
      document.removeEventListener("fullscreenchange", handleFullscreenChange)
    }
  }, [])

  useEffect(() => {
    sliderPosRef.current = sliderPos
    containerRef.current?.style.setProperty(
      "--comparison-slider-position",
      `${sliderPos}%`
    )
  }, [sliderPos])

  useEffect(() => {
    return () => {
      if (sliderAnimationFrameRef.current !== null) {
        window.cancelAnimationFrame(sliderAnimationFrameRef.current)
      }
    }
  }, [])

  const handleToggleFullscreen = useCallback(async () => {
    const container = containerRef.current
    if (!container) return

    try {
      if (document.fullscreenElement === container) {
        await document.exitFullscreen()
        return
      }

      await container.requestFullscreen()
    } catch {
      // Ignore rejected fullscreen requests from browsers or user settings.
    }
  }, [])

  // --- Drag slider ---
  const getSliderPositionFromClientX = useCallback((clientX: number) => {
    const container = containerRef.current
    if (!container) return sliderPosRef.current

    const rect = container.getBoundingClientRect()
    if (rect.width <= 0) return sliderPosRef.current

    return Math.min(100, Math.max(0, ((clientX - rect.left) / rect.width) * 100))
  }, [])

  const applySliderPosition = useCallback((nextPos: number) => {
    sliderPosRef.current = nextPos
    containerRef.current?.style.setProperty(
      "--comparison-slider-position",
      `${nextPos}%`
    )
  }, [])

  const queueSliderPosition = useCallback(
    (clientX: number) => {
      pendingSliderPosRef.current = getSliderPositionFromClientX(clientX)

      if (sliderAnimationFrameRef.current !== null) {
        return
      }

      sliderAnimationFrameRef.current = window.requestAnimationFrame(() => {
        sliderAnimationFrameRef.current = null

        if (pendingSliderPosRef.current === null) return

        applySliderPosition(pendingSliderPosRef.current)
        pendingSliderPosRef.current = null
      })
    },
    [applySliderPosition, getSliderPositionFromClientX]
  )

  const commitSliderPosition = useCallback(() => {
    if (sliderAnimationFrameRef.current !== null) {
      window.cancelAnimationFrame(sliderAnimationFrameRef.current)
      sliderAnimationFrameRef.current = null
    }

    if (pendingSliderPosRef.current !== null) {
      applySliderPosition(pendingSliderPosRef.current)
      pendingSliderPosRef.current = null
    }

    setSliderPos(sliderPosRef.current)
  }, [applySliderPosition])

  const handleSliderPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!e.isPrimary) return
      if (e.pointerType === "mouse" && e.button !== 0) return

      e.preventDefault()
      isDraggingRef.current = true
      setIsDragging(true)
      queueSliderPosition(e.clientX)
      e.currentTarget.setPointerCapture(e.pointerId)
    },
    [queueSliderPosition]
  )

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!isDraggingRef.current) return

      queueSliderPosition(e.clientX)
    },
    [queueSliderPosition]
  )

  const handlePointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!isDraggingRef.current) return

      isDraggingRef.current = false
      setIsDragging(false)

      if (e.currentTarget.hasPointerCapture(e.pointerId)) {
        e.currentTarget.releasePointerCapture(e.pointerId)
      }

      commitSliderPosition()
    },
    [commitSliderPosition]
  )
  const gatePanelClass =
    "relative z-10 flex w-[calc(100%-1rem)] max-w-md flex-col items-center gap-4 rounded-2xl border border-white/10 bg-black/45 px-4 py-5 text-center shadow-[0_24px_80px_rgba(0,0,0,0.45)] backdrop-blur-sm sm:w-auto sm:gap-6 sm:px-8 sm:py-8"
  const gateHeadingClass =
    "text-base font-medium text-white drop-shadow-[0_1px_2px_rgba(0,0,0,0.45)]"
  const gateBodyClass = "text-sm leading-relaxed text-white/78"
  const sideLabelClass =
    "absolute top-3 z-10 rounded bg-black/50 px-2 py-0.5 text-xs font-semibold text-white/90 pointer-events-none"

  // --- Pre-connection gate UI ---
  if (!demoStarted || !upscaledHlsUrl) {
    return (
      <div className="space-y-4">
        {/* Placeholder uses a taller, content-driven layout on mobile */}
        <div className="relative flex min-h-[34rem] items-center justify-center overflow-hidden rounded-lg border border-border bg-card py-4 sm:min-h-0 sm:py-0 sm:aspect-video">
          {/* Subtle background pattern */}
          <div className="absolute inset-0 bg-gradient-to-br from-accent/5 via-transparent to-accent/5" />
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img
            src={activeStream.posterUrl}
            alt=""
            aria-hidden="true"
            className="absolute inset-0 w-full h-full object-cover blur-lg scale-110 brightness-[0.4] saturate-[0.8]"
          />
          <div className="absolute inset-0 bg-black/35" />
          <div className="absolute inset-0 bg-gradient-to-br from-black/75 via-black/40 to-black/80" />

          <div className={gatePanelClass}>
            {refereeStatus === "checking" && (
              <>
                <div className="h-14 w-14 rounded-full border-2 border-white/20 border-t-accent animate-spin" />
                <div>
                  <p className={`${gateHeadingClass} mb-1`}>
                    Looking for <RefereeWordmark variant="inline" />
                  </p>
                  <p className={gateBodyClass}>
                    {isUsingCustomRefereeBase
                      ? `Checking ${refereeBase} for a reachable REFEREE instance...`
                      : "Checking for a local REFEREE instance on your system&hellip;"}
                  </p>
                </div>
              </>
            )}

            {refereeStatus === "disconnected" && (
              <>
                <div className="flex h-14 w-14 items-center justify-center rounded-full border border-white/10 bg-white/10">
                  <WifiOff className="h-6 w-6 text-white/80" />
                </div>
                <div>
                  <p className={`${gateHeadingClass} mb-2`}>
                    <RefereeWordmark variant="inline" /> not detected
                  </p>
                  <p className={`${gateBodyClass} mb-4`}>
                    To run this demo you need REFEREE running locally on Windows or Linux with a
                    supported NVIDIA RTX or AMD Radeon GPU, or a remote REFEREE instance this
                    browser can reach. The demo player will appear once a connection is
                    established.
                  </p>
                  <div className="flex w-full flex-col items-center gap-4">
                    <PlatformDownloadButton
                      className="w-full border-white/15 bg-white/10 text-white shadow-[0_16px_40px_rgba(0,0,0,0.35)] hover:bg-white/15 hover:text-white sm:w-auto"
                      openInNewTab
                    />

                    {!showRemoteConnectForm && !isUsingCustomRefereeBase && (
                      <button
                        type="button"
                        className="text-sm font-medium text-white/75 underline decoration-white/30 underline-offset-4 transition-colors hover:text-white"
                        onClick={() => {
                          setShowRemoteConnectForm(true)
                          setRemoteBaseInput("")
                          setRemoteBaseError(null)
                        }}
                      >
                        Running REFEREE on a different machine?
                      </button>
                    )}

                    {(showRemoteConnectForm || isUsingCustomRefereeBase) && (
                      <form
                        className="flex w-full flex-col gap-3 text-left"
                        onSubmit={handleRemoteConnect}
                      >
                        <div className="space-y-2">
                          <label
                            htmlFor="remote-referee-url"
                            className="text-xs font-medium uppercase tracking-[0.14em] text-white/60"
                          >
                            Remote REFEREE URL
                          </label>
                          <Input
                            id="remote-referee-url"
                            type="text"
                            inputMode="url"
                            autoComplete="url"
                            value={remoteBaseInput}
                            onChange={(event) => {
                              setRemoteBaseInput(event.target.value)
                              if (remoteBaseError) {
                                setRemoteBaseError(null)
                              }
                            }}
                            placeholder="https://referee.example.com"
                            className="h-10 border-white/12 bg-white/8 px-3 text-sm text-white placeholder:text-white/35"
                          />
                        </div>
                        {/* Token section — hidden while consent is pending, and hidden once authorized */}
                        {!autoAuthPending && !autoTokenAcquired && (
                          <div className="space-y-2">
                            <label
                              htmlFor="remote-referee-token"
                              className="text-xs font-medium uppercase tracking-[0.14em] text-white/60"
                            >
                              API Token
                            </label>
                            <Input
                              id="remote-referee-token"
                              type="password"
                              value={tokenInput}
                              onChange={(event) => setTokenInput(event.target.value)}
                              placeholder="Paste your REFEREE token…"
                              className="h-10 border-white/12 bg-white/8 px-3 text-sm text-white placeholder:text-white/35"
                            />
                          </div>
                        )}

                        <div className="flex flex-col gap-2 sm:flex-row">
                          <Button
                            type="submit"
                            className="flex-1 shadow-[0_16px_40px_rgba(253,105,11,0.28)]"
                          >
                            Connect to Remote REFEREE
                          </Button>
                          {isUsingCustomRefereeBase && (
                            <Button
                              type="button"
                              variant="ghost"
                              className="text-white hover:bg-white/10 hover:text-white"
                              onClick={handleUseLocalReferee}
                            >
                              Use Local REFEREE
                            </Button>
                          )}
                        </div>

                        {remoteBaseError && (
                          <p className="text-xs text-[#ffb4b4]">{remoteBaseError}</p>
                        )}

                        {!remoteBaseError && isMixedContentRisk(normalizeRefereeBase(remoteBaseInput) ?? remoteBaseInput) && (
                          <p className="text-xs leading-relaxed text-[#ffb4b4]">
                            This page is served over HTTPS, so browsers will block plain HTTP connections to remote hosts.
                            Expose REFEREE over HTTPS first — for example via an{" "}
                            <a
                              href="https://ngrok.com"
                              target="_blank"
                              rel="noopener noreferrer"
                              className="underline decoration-[#ffb4b4]/50 underline-offset-2 hover:text-white"
                            >
                              ngrok
                            </a>{" "}
                            tunnel — then enter the <span className="font-mono">https://</span> URL here.
                          </p>
                        )}

                        <p className="text-xs leading-relaxed text-white/55 break-words">
                          Enter the base URL for REFEREE, for example{" "}
                          <span className="break-all font-mono text-white/70">https://referee.example.com</span>{" "}
                          or a plain HTTP address if your browser and REFEREE are on the same machine.
                        </p>
                      </form>
                    )}
                  </div>
                </div>
              </>
            )}

            {refereeStatus === "connected" && !demoStarted && (
              <>
                <div className="flex h-14 w-14 items-center justify-center rounded-full border border-accent/25 bg-accent/20">
                  <CheckCircle2 className="h-6 w-6 text-accent" />
                </div>
                <div>
                  <p className={`${gateHeadingClass} mb-1`}>
                    <RefereeWordmark variant="inline" /> Connected
                  </p>
                  <p className={`${gateBodyClass} mb-4`}>
                    Start the demo to compare the original source stream
                    side-by-side with <strong className="text-white">REFEREE</strong>'s AI-upscaled output.
                  </p>
                  {/* Stream selector */}
                  <div className="w-full mb-5 flex gap-2">
                    {STREAMS.map(stream => (
                      <button
                        key={stream.id}
                        type="button"
                        onClick={() => handleSelectStream(stream.id)}
                        className={`flex-1 rounded-lg border px-3 py-2 text-left text-sm transition-colors ${
                          selectedStreamId === stream.id
                            ? "border-accent/50 bg-accent/15 text-white"
                            : "border-white/10 bg-white/5 text-white/70 hover:bg-white/8 hover:text-white"
                        }`}
                      >
                        <div className="font-medium">{stream.label}</div>
                        <div className="text-xs opacity-70">{stream.tag} &middot; {stream.sourceResolution}</div>
                      </button>
                    ))}
                  </div>
                  {isUsingCustomRefereeBase && (
                    <p className="mb-5 text-xs text-white/60">
                      Connected via{" "}
                      <span className="font-mono text-white/75">{refereeBase}</span>
                    </p>
                  )}
                  {/* Token section — hidden while consent is pending, and hidden once authorized */}
                  {!autoAuthPending && !autoTokenAcquired && (
                    <div className="w-full mb-5 space-y-2 text-left">
                      <label
                        htmlFor="local-referee-token"
                        className="text-xs font-medium uppercase tracking-[0.14em] text-white/60"
                      >
                        API Token
                      </label>
                      <Input
                        id="local-referee-token"
                        type="password"
                        value={tokenInput}
                        onChange={(e) => setTokenInput(e.target.value)}
                        placeholder="Paste your REFEREE token…"
                        className="h-10 border-white/12 bg-white/8 px-3 text-sm text-white placeholder:text-white/35"
                      />
                      <p className="text-[11px] leading-relaxed text-white/50">
                        Find your token in the REFEREE desktop app settings or server startup log.
                      </p>
                    </div>
                  )}
                  {startError && (
                    <p className="mb-4 text-sm text-[#ffb4b4]">{startError}</p>
                  )}
                  <div className="relative w-full">
                    <div
                      aria-hidden="true"
                      className="absolute inset-[-0.45rem] rounded-[1.1rem] bg-[radial-gradient(circle,rgba(253,105,11,0.34)_0%,rgba(253,105,11,0.12)_42%,rgba(253,105,11,0)_78%)] blur-md"
                    />
                    <Button
                      size="lg"
                      onClick={handleStartDemo}
                      disabled={autoAuthPending}
                      className="group relative h-auto w-full overflow-hidden rounded-xl border border-[#ffb07a]/25 bg-[linear-gradient(135deg,#b84508_0%,#fd690b_55%,#ff9d55_100%)] px-5 py-3.5 text-white shadow-[0_18px_48px_rgba(253,105,11,0.34)] transition duration-300 hover:scale-[1.03] hover:shadow-[0_24px_60px_rgba(253,105,11,0.45)] disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100"
                    >
                        <span
                          aria-hidden="true"
                          className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.22),transparent_55%)] opacity-80"
                        />
                        <span
                          aria-hidden="true"
                          className="pointer-events-none absolute inset-0 translate-x-[-140%] bg-[linear-gradient(120deg,transparent_0%,rgba(255,255,255,0.18)_42%,transparent_68%)] transition-transform duration-700 group-hover:translate-x-[140%]"
                        />
                        <span className="relative flex items-center justify-center gap-3">
                          <span className="flex flex-col items-center">
                            <span className="text-sm font-semibold sm:text-base">
                              {autoAuthPending ? "Waiting for authorization…" : "Start Demo"}
                            </span>
                          </span>
                        </span>
                    </Button>
                  </div>
                </div>
              </>
            )}

            {refereeStatus === "connected" && demoStarted && !upscaledHlsUrl && (
              <>
                {startError ? (
                  <>
                    <div className="flex h-14 w-14 items-center justify-center rounded-full border border-red-500/25 bg-red-500/15">
                      <WifiOff className="h-6 w-6 text-red-400" />
                    </div>
                    <div>
                      <p className={`${gateHeadingClass} mb-3`}>Failed to start session</p>
                      <p className={`${gateBodyClass} mb-5`}>{startError}</p>
                      <Button
                        variant="ghost"
                        className="text-white hover:bg-white/10 hover:text-white"
                        onClick={() => { setDemoStarted(false); setStartError(null) }}
                      >
                        Try again
                      </Button>
                    </div>
                  </>
                ) : (
                  <>
                    <div className="h-14 w-14 rounded-full border-2 border-white/20 border-t-accent animate-spin" />
                    <div>
                      <p className={`${gateHeadingClass} mb-1`}>Starting upscaling pipeline</p>
                      <p className={gateBodyClass}>
                        <RefereeWordmark variant="inline" /> is initializing the AI upscaling pipeline. This usually takes 15-30 seconds.
                      </p>
                    </div>
                  </>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    )
  }

  // --- Active comparison player ---
  return (
    <div className="space-y-4">
      {/* Status Bar */}
      <div className="flex items-center justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <Badge
            variant="outline"
            className="px-3 py-1 border-accent/50 text-accent"
          >
            <CheckCircle2 className="h-3 w-3 mr-1.5" />
            <RefereeWordmark variant="inline" /> Connected
          </Badge>
          <Badge variant="outline" className="px-3 py-1 border-muted-foreground/30 text-muted-foreground">
            <Sparkles className="h-3 w-3 mr-1.5" />
            GPU Upscaling
          </Badge>
          {hdrEnabled && (
            <Badge variant="outline" className="px-3 py-1 border-muted-foreground/30 text-muted-foreground">
              <Sparkles className="h-3 w-3 mr-1.5" />
              Injecting TrueHDR
            </Badge>
          )}
          {framegenEnabled && (
            <Badge variant="outline" className="px-3 py-1 border-muted-foreground/30 text-muted-foreground">
              <Sparkles className="h-3 w-3 mr-1.5" />
              2X Frame-Gen
            </Badge>
          )}
          {isUsingCustomRefereeBase && (
            <Badge variant="outline" className="px-3 py-1 border-muted-foreground/30 text-muted-foreground">
              Remote Endpoint
            </Badge>
          )}
        </div>
      </div>

      {/* Video Comparison Container */}
      <div
        ref={containerRef}
        className="relative overflow-hidden aspect-video bg-card border border-border rounded-lg select-none"
        style={{ cursor: isDragging ? "col-resize" : "default" }}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
      >
        {!hasPlaybackStarted && (
          <>
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={activeStream.posterUrl}
              alt=""
              aria-hidden="true"
              className="absolute inset-0 w-full h-full object-cover blur-md scale-110 brightness-[0.45] saturate-[0.85]"
            />
            <div className="absolute inset-0 bg-black/30" />
          </>
        )}
        {/* Left: Source video — full size, underneath */}
        <Button
          type="button"
          variant="secondary"
          size="icon-sm"
          onClick={handleToggleFullscreen}
          className="absolute bottom-3 right-3 z-30 border border-white/15 bg-black/55 text-white shadow-[0_12px_32px_rgba(0,0,0,0.3)] backdrop-blur-sm hover:bg-black/70 hover:text-white"
          aria-label={isFullscreen ? "Exit fullscreen" : "Enter fullscreen"}
          title={isFullscreen ? "Exit fullscreen" : "Enter fullscreen"}
        >
          {isFullscreen ? <Minimize className="h-4 w-4" /> : <Maximize className="h-4 w-4" />}
        </Button>
        <video
          ref={sourceVideoRef}
          className="pointer-events-none absolute inset-0 w-full h-full object-cover bg-transparent"
          autoPlay
          muted
          loop
          playsInline
          controlsList="nodownload nofullscreen noremoteplayback"
          disablePictureInPicture
          disableRemotePlayback
          tabIndex={-1}
        />

        {/* Right: Upscaled video — clipped to reveal from sliderPos rightward */}
        <div
          className="absolute inset-0"
          style={{
            clipPath: "inset(0 0 0 var(--comparison-slider-position, 50%))",
            willChange: isDragging ? "clip-path" : undefined,
          }}
        >
          <video
            ref={upscaledVideoRef}
            className="pointer-events-none absolute inset-0 w-full h-full object-cover bg-transparent"
            autoPlay
            loop
            playsInline
            controlsList="nodownload nofullscreen noremoteplayback"
            disablePictureInPicture
            disableRemotePlayback
            tabIndex={-1}
          />
          <span className={`${sideLabelClass} right-3 flex items-center`}>
            <Zap className="mr-1 h-3 w-3 fill-orange-500 text-orange-500" />
            REFEREE Upscaled
          </span>
        </div>

        <div
          className="pointer-events-none absolute inset-0"
          style={{
            clipPath: "inset(0 calc(100% - var(--comparison-slider-position, 50%)) 0 0)",
            willChange: isDragging ? "clip-path" : undefined,
          }}
        >
          <span className={`${sideLabelClass} left-3`}>
            Source
          </span>
        </div>

        {/* Slider divider line */}
        <div
          className="absolute top-0 bottom-0 z-20 w-px bg-white/80 shadow-[0_0_6px_rgba(0,0,0,0.6)] pointer-events-none"
          style={{
            left: "var(--comparison-slider-position, 50%)",
            transform: "translateX(-50%)",
          }}
        />
        {/* Drag hit area */}
        <div
          className="absolute inset-y-0 z-20 w-14 -translate-x-1/2 cursor-col-resize"
          style={{
            left: "var(--comparison-slider-position, 50%)",
            touchAction: "none",
          }}
          onPointerDown={handleSliderPointerDown}
        >
          <div
            className="absolute top-1/2 left-1/2 flex h-9 w-9 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-white shadow-lg"
            aria-hidden="true"
          >
            <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
              <path d="M7 5L3 9L7 13" stroke="#444" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" />
              <path d="M11 5L15 9L11 13" stroke="#444" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        </div>
      </div>

      {/* Video info + attribution */}
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between sm:gap-4 text-sm text-muted-foreground">
        <div className="flex flex-col gap-1.5">
          <p className="text-xs text-left">
            <span className="font-medium text-foreground">{activeStream.label}</span>
            {" — "}
            &copy; {activeStream.attributionYear} {activeStream.attributionAuthor},{" "}
            <a
              href={activeStream.attributionLicenseUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="hover:underline"
            >
              {activeStream.attributionLicense}
            </a>
            . Hosted via {activeStream.attributionCdn}.
          </p>
          <div className="flex items-center gap-1.5">
            {STREAMS.map(stream => (
              <button
                key={stream.id}
                type="button"
                onClick={() => handleSelectStream(stream.id)}
                className={`text-xs px-2 py-0.5 rounded transition-colors ${
                  selectedStreamId === stream.id
                    ? "bg-accent/15 text-accent font-medium"
                    : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {stream.label}
              </button>
            ))}
          </div>
        </div>
        <div className="flex items-center gap-3 shrink-0">
          <span className="text-xs">{activeStream.sourceResolution} source</span>
          <span className="text-accent text-xs">→</span>
          <span className="text-accent text-xs font-medium">{formatResolution(upscaledResolution ?? "3840x2160")} upscaled</span>
        </div>
      </div>
    </div>
  )
}
