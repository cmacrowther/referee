"use client"

import {
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react"
import { RefereeShellPreview } from "@/components/referee-shell-preview"
import { RefereeWordmark } from "./referee-wordmark"

const DEFAULT_VIEWPORT_WIDTH = 720
const DEFAULT_PANEL_WIDTH = 254
const MIN_FRAME_WIDTH = 180
const STAGE_SCALE_OVERFLOW_ALLOWANCE = 260

type FlowLaneDirection = "forward" | "return"

interface MotionMetrics {
  stageScale: number
  viewportWidth: number
  panelWidth: number
  travelSpan: number
  worldWidth: number
  cameraStart: number
  cameraEnd: number
  cameraForwardLaunch: number
  cameraForwardQuarter: number
  cameraForwardHalf: number
  cameraForwardThreeQuarter: number
  cameraForwardDock: number
  cameraReturnLaunch: number
  cameraReturnQuarter: number
  cameraReturnHalf: number
  cameraReturnThreeQuarter: number
  cameraReturnDock: number
}

function getPacketTrackingOffset(
  viewportWidth: number,
  panelWidth: number,
  travelSpan: number,
  progress: number,
) {
  return viewportWidth / 2 - panelWidth - travelSpan * progress
}

function createMotionMetrics(frameWidth: number): MotionMetrics {
  const normalizedFrameWidth = Math.max(MIN_FRAME_WIDTH, Math.round(frameWidth))
  const normalizedViewportWidth = DEFAULT_VIEWPORT_WIDTH
  const normalizedPanelWidth = DEFAULT_PANEL_WIDTH
  const stageScale = Math.min(
    1,
    (normalizedFrameWidth + STAGE_SCALE_OVERFLOW_ALLOWANCE) / normalizedViewportWidth,
  )
  const travelSpan = normalizedViewportWidth
  const worldWidth = normalizedPanelWidth * 2 + travelSpan
  const cameraStart = normalizedViewportWidth / 2 - normalizedPanelWidth / 2
  const cameraEnd = cameraStart - (normalizedPanelWidth + travelSpan)

  return {
    stageScale,
    viewportWidth: normalizedViewportWidth,
    panelWidth: normalizedPanelWidth,
    travelSpan,
    worldWidth,
    cameraStart,
    cameraEnd,
    cameraForwardLaunch: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.08,
    ),
    cameraForwardQuarter: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.24,
    ),
    cameraForwardHalf: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.5,
    ),
    cameraForwardThreeQuarter: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.76,
    ),
    cameraForwardDock: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.92,
    ),
    cameraReturnLaunch: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.92,
    ),
    cameraReturnQuarter: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.76,
    ),
    cameraReturnHalf: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.5,
    ),
    cameraReturnThreeQuarter: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.24,
    ),
    cameraReturnDock: getPacketTrackingOffset(
      normalizedViewportWidth,
      normalizedPanelWidth,
      travelSpan,
      0.08,
    ),
  }
}

function haveMetricsChanged(current: MotionMetrics, next: MotionMetrics) {
  return (Object.keys(next) as Array<keyof MotionMetrics>).some(
    (key) => current[key] !== next[key],
  )
}

function WindowControls() {
  return (
    <span className="hero-whistle-window-controls" aria-hidden="true">
      <span className="hero-whistle-window-dot" />
      <span className="hero-whistle-window-dot" />
      <span className="hero-whistle-window-dot" />
    </span>
  )
}

function FlowLane({
  direction,
  label,
}: {
  direction: FlowLaneDirection
  label: string
}) {
  return (
    <div className={`hero-whistle-stage-lane hero-whistle-stage-lane-${direction}`}>
      <div className="hero-whistle-stage-lane-core">
        <span className="hero-whistle-stage-lane-packet">
          <span className="hero-whistle-stage-lane-packet-label">{label}</span>
        </span>
      </div>
    </div>
  )
}

export function HeroWhistle() {
  const showcaseRef = useRef<HTMLDivElement | null>(null)
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const hasStartedMotionRef = useRef(false)
  const [hasMeasuredScale, setHasMeasuredScale] = useState(false)
  const [metrics, setMetrics] = useState<MotionMetrics>(() =>
    createMotionMetrics(DEFAULT_VIEWPORT_WIDTH),
  )
  const [motionReady, setMotionReady] = useState(false)

  useLayoutEffect(() => {
    let animationFrameId = 0
    let followupAnimationFrameId = 0

    const readMetrics = () => {
      const viewportWidth =
        viewportRef.current?.clientWidth ??
        showcaseRef.current?.clientWidth ??
        DEFAULT_VIEWPORT_WIDTH
      const nextMetrics = createMotionMetrics(viewportWidth)
      setHasMeasuredScale(true)

      setMetrics((currentMetrics) =>
        haveMetricsChanged(currentMetrics, nextMetrics) ? nextMetrics : currentMetrics,
      )
    }

    const scheduleMotionStart = () => {
      if (hasStartedMotionRef.current) {
        return
      }

      window.cancelAnimationFrame(animationFrameId)
      window.cancelAnimationFrame(followupAnimationFrameId)
      setMotionReady(false)

      animationFrameId = window.requestAnimationFrame(() => {
        followupAnimationFrameId = window.requestAnimationFrame(() => {
          hasStartedMotionRef.current = true
          setMotionReady(true)
        })
      })
    }

    readMetrics()
    scheduleMotionStart()

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", readMetrics)
      return () => {
        window.cancelAnimationFrame(animationFrameId)
        window.cancelAnimationFrame(followupAnimationFrameId)
        window.removeEventListener("resize", readMetrics)
      }
    }

    const observer = new ResizeObserver(() => {
      readMetrics()

      if (!hasStartedMotionRef.current) {
        scheduleMotionStart()
      }
    })

    if (showcaseRef.current) {
      observer.observe(showcaseRef.current)
    }

    if (viewportRef.current) {
      observer.observe(viewportRef.current)
    }

    return () => {
      window.cancelAnimationFrame(animationFrameId)
      window.cancelAnimationFrame(followupAnimationFrameId)
      observer.disconnect()
    }
  }, [])

  const style = {
    ...(hasMeasuredScale ? { "--hero-whistle-stage-scale": metrics.stageScale } : {}),
    "--hero-whistle-viewport-width": `${metrics.viewportWidth}px`,
    "--hero-whistle-panel-width": `${metrics.panelWidth}px`,
    "--hero-whistle-travel-span": `${metrics.travelSpan}px`,
    "--hero-whistle-world-width": `${metrics.worldWidth}px`,
    "--hero-whistle-camera-start": `${metrics.cameraStart}px`,
    "--hero-whistle-camera-end": `${metrics.cameraEnd}px`,
    "--hero-whistle-camera-forward-launch": `${metrics.cameraForwardLaunch}px`,
    "--hero-whistle-camera-forward-quarter": `${metrics.cameraForwardQuarter}px`,
    "--hero-whistle-camera-forward-half": `${metrics.cameraForwardHalf}px`,
    "--hero-whistle-camera-forward-three-quarter": `${metrics.cameraForwardThreeQuarter}px`,
    "--hero-whistle-camera-forward-dock": `${metrics.cameraForwardDock}px`,
    "--hero-whistle-camera-return-launch": `${metrics.cameraReturnLaunch}px`,
    "--hero-whistle-camera-return-quarter": `${metrics.cameraReturnQuarter}px`,
    "--hero-whistle-camera-return-half": `${metrics.cameraReturnHalf}px`,
    "--hero-whistle-camera-return-three-quarter": `${metrics.cameraReturnThreeQuarter}px`,
    "--hero-whistle-camera-return-dock": `${metrics.cameraReturnDock}px`,
  } as CSSProperties

  return (
    <div className="hero-whistle-showcase" ref={showcaseRef}>
      <div className="hero-whistle-backdrop" aria-hidden="true" />
      <div className="hero-whistle-grid" aria-hidden="true" />
      <div className="hero-whistle-halo hero-whistle-halo-left" aria-hidden="true" />
      <div className="hero-whistle-halo hero-whistle-halo-right" aria-hidden="true" />

      <div
        className="hero-whistle-stage-viewport"
        ref={viewportRef}
        style={style}
        data-motion-ready={motionReady ? "true" : "false"}
      >
        <div className="hero-whistle-stage-scale">
          <div className="hero-whistle-stage-camera">
            <div className="hero-whistle-stage-world">
              <div className="hero-whistle-stage-flow" aria-hidden="true">
                <FlowLane direction="forward" label="RAW 480P" />
                <FlowLane direction="return" label="4K HDR" />
              </div>

              <div className="hero-whistle-stage-panel-anchor hero-whistle-stage-panel-anchor-source">
                <div className="hero-whistle-stage-panel hero-whistle-stage-panel-source">
                  <div className="hero-whistle-screen hero-whistle-screen-panel">
                    <div className="hero-whistle-screen-head">
                      <span className="hero-whistle-chip">WEB PLAYER</span>
                      <WindowControls />
                    </div>

                    <div className="hero-whistle-screen-body hero-whistle-screen-body-source">
                      <div className="hero-whistle-screen-frame hero-whistle-screen-frame-wide">
                        <video
                          src="/bbb.webm"
                          className="hero-whistle-video hero-whistle-video-wide"
                          muted
                          loop
                          playsInline
                          autoPlay
                          aria-hidden="true"
                        />
                        <div className="hero-whistle-screen-overlay" aria-hidden="true" />
                      </div>

                      <div className="hero-whistle-player-controls" aria-hidden="true">
                        <div className="hero-whistle-player-progress">
                          <div className="hero-whistle-player-progress-fill" />
                          <div className="hero-whistle-player-progress-thumb" />
                        </div>
                        <div className="hero-whistle-player-bar">
                          <button className="hero-whistle-player-btn" tabIndex={-1}>
                            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                              <rect x="1.5" y="1" width="2.5" height="8" rx="1" fill="currentColor" />
                              <rect x="6" y="1" width="2.5" height="8" rx="1" fill="currentColor" />
                            </svg>
                          </button>
                          <span className="hero-whistle-player-time">0:24 / 3:47</span>
                          <div className="hero-whistle-player-spacer" />
                          <svg
                            className="hero-whistle-player-icon"
                            width="12"
                            height="12"
                            viewBox="0 0 12 12"
                            fill="none"
                          >
                            <path
                              d="M1 4.5C1 3.12 2.12 2 3.5 2h5C9.88 2 11 3.12 11 4.5v3C11 8.88 9.88 10 8.5 10h-5C2.12 10 1 8.88 1 7.5v-3z"
                              stroke="currentColor"
                              strokeWidth="1"
                            />
                            <path d="M4 5.1l2.5 1-2.5 1V5.1z" fill="currentColor" />
                          </svg>
                          <svg
                            className="hero-whistle-player-icon"
                            width="12"
                            height="12"
                            viewBox="0 0 12 12"
                            fill="none"
                          >
                            <path
                              d="M2 2h8v8H2z"
                              stroke="currentColor"
                              strokeWidth="1"
                              strokeLinejoin="round"
                            />
                            <path
                              d="M4.5 4.5h3v3h-3z"
                              stroke="currentColor"
                              strokeWidth="1"
                              strokeLinejoin="round"
                            />
                          </svg>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <div className="hero-whistle-stage-panel-anchor hero-whistle-stage-panel-anchor-referee">
                <div className="hero-whistle-stage-panel hero-whistle-stage-panel-referee">
                  <div className="hero-whistle-screen hero-whistle-screen-panel hero-whistle-screen-referee">
                    <div className="hero-whistle-screen-head">
                      <span className="hero-whistle-chip">
                        <RefereeWordmark className="hero-whistle-chip-wordmark" variant="inline" />
                      </span>
                      <WindowControls />
                    </div>

                    <div className="hero-whistle-screen-body hero-whistle-referee-shell-panel">
                      <RefereeShellPreview />
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
