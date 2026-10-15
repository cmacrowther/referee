"use client"

import { useEffect, useRef, useState } from "react"
import { ArrowUpRight, Cpu, Globe2, Pause, Play, Sparkles } from "lucide-react"
import type { ShowcaseScene } from "./hero-whistle-scene"
import styles from "./hero-whistle.module.css"

const steps = [
  { title: "Stream small", detail: "Send a lower-bitrate source.", icon: Globe2 },
  { title: "Enhance locally", detail: "Let the viewer’s GPU do the work.", icon: Cpu },
  { title: "Watch in detail", detail: "Play the enhanced stream in the browser.", icon: Sparkles },
] as const

// Remains visible while the renderer loads, and when WebGL is unavailable.
function ScenePoster() {
  return (
    <svg className={styles.poster} viewBox="0 0 1000 430" fill="none" aria-hidden="true">
      <defs>
        <linearGradient id="referee-sky" x2="0" y2="1">
          <stop stopColor="#242134" />
          <stop offset="1" stopColor="#cf7148" />
        </linearGradient>
        <linearGradient id="referee-stream">
          <stop stopColor="#ff823b" stopOpacity=".15" />
          <stop offset=".5" stopColor="#ff823b" />
          <stop offset="1" stopColor="#ffc997" stopOpacity=".4" />
        </linearGradient>
        <radialGradient id="referee-glow">
          <stop stopColor="#fd690b" stopOpacity=".2" />
          <stop offset="1" stopColor="#fd690b" stopOpacity="0" />
        </radialGradient>
        <pattern id="referee-pixels" width="12" height="12" patternUnits="userSpaceOnUse">
          <path d="M12 0H0V12" stroke="#17111a" strokeOpacity=".3" />
        </pattern>
      </defs>
      <ellipse cx="500" cy="237" rx="240" ry="170" fill="url(#referee-glow)" />
      {[-24, -12, 0, 12, 24].map((offset) => (
        <path key={offset} d={`M300 ${217 + offset}C390 ${217 + offset} 399 215 449 215S594 ${215 + offset} 672 ${204 + offset}`} stroke="url(#referee-stream)" strokeWidth="1.5" />
      ))}
      <g transform="translate(62 144) rotate(-5 120 80)">
        <rect width="250" height="162" rx="10" fill="#15151a" stroke="#645048" />
        <rect x="8" y="24" width="234" height="130" rx="3" fill="url(#referee-sky)" />
        <circle cx="180" cy="65" r="22" fill="#ffbe85" />
        <path d="M8 122L66 67 127 121 166 90 242 143V154H8Z" fill="#5d4055" />
        <path d="M8 145L100 99 153 146 200 122 242 145V154H8Z" fill="#252331" />
        <rect x="8" y="24" width="234" height="130" fill="url(#referee-pixels)" />
        <circle cx="14" cy="12" r="2" fill="#ff8a4b" />
        <path d="M24 12H62" stroke="#777078" strokeWidth="2" />
      </g>
      <g transform="translate(500 215)">
        <ellipse rx="99" ry="76" transform="rotate(-32)" stroke="#fd690b" strokeOpacity=".6" />
        <ellipse rx="94" ry="79" transform="rotate(30)" stroke="#ffc69d" strokeOpacity=".25" />
        <rect x="-64" y="-55" width="128" height="110" rx="12" fill="#191616" stroke="#fd690b" />
        <rect x="-51" y="-43" width="102" height="86" rx="6" fill="#271a15" stroke="#fd690b" strokeOpacity=".5" />
        <text textAnchor="middle" y="5" fill="#fff0e5" fontSize="17" fontWeight="800" fontFamily="sans-serif">REFEREE</text>
        <text textAnchor="middle" y="25" fill="#e89763" fontSize="8" letterSpacing="3" fontFamily="monospace">LOCAL GPU</text>
      </g>
      <g transform="translate(684 113) rotate(4 135 100)">
        <rect width="276" height="187" rx="10" fill="#171719" stroke="#a4775e" />
        <rect x="8" y="25" width="260" height="154" rx="3" fill="url(#referee-sky)" />
        <circle cx="204" cy="69" r="26" fill="#ffd9a8" />
        <path d="M8 149L79 68 140 132 183 92 268 164V179H8Z" fill="#694458" />
        <path d="M57 93L79 68 101 93 88 87 78 92 73 83Z" fill="#e6bbb1" />
        <path d="M8 169L112 109 171 156 221 130 268 167V179H8Z" fill="#292535" />
        <path d="M80 127L112 109 135 128 114 119 106 125Z" fill="#bb9299" />
        <circle cx="15" cy="12" r="2" fill="#ff8a4b" />
        <path d="M26 12H70" stroke="#94807a" strokeWidth="2" />
      </g>
    </svg>
  )
}

export function HeroWhistle() {
  const hostRef = useRef<HTMLDivElement>(null)
  const sceneRef = useRef<ShowcaseScene | null>(null)
  const playbackRef = useRef({ paused: false, step: 0 })
  const [ready, setReady] = useState(false)
  const [activeStep, setActiveStep] = useState(0)
  const [paused, setPaused] = useState(false)
  const [reducedMotion, setReducedMotion] = useState(false)

  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)")
    const syncMotion = () => {
      setReducedMotion(preference.matches)
      setPaused(preference.matches)
      playbackRef.current.paused = preference.matches
      sceneRef.current?.setPaused(preference.matches)
    }
    syncMotion()
    preference.addEventListener("change", syncMotion)
    return () => preference.removeEventListener("change", syncMotion)
  }, [])

  useEffect(() => {
    const host = hostRef.current
    if (!host) return
    let cancelled = false
    let started = false
    const loadScene = async () => {
      if (started) return
      started = true
      try {
        const { createShowcaseScene } = await import("./hero-whistle-scene")
        if (cancelled) return
        sceneRef.current = createShowcaseScene(host, {
          onStep: setActiveStep,
          onUnavailable: () => setReady(false),
          paused: playbackRef.current.paused,
        })
        sceneRef.current.selectStep(playbackRef.current.step)
        setReady(true)
      } catch {
        // The illustrated explanation is also a complete WebGL fallback.
        if (!cancelled) setReady(false)
      }
    }
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) {
        observer.disconnect()
        void loadScene()
      }
    }, { rootMargin: "200px" })
    observer.observe(host)
    return () => {
      cancelled = true
      observer.disconnect()
      sceneRef.current?.dispose()
      sceneRef.current = null
    }
  }, [])

  const selectStep = (index: number) => {
    playbackRef.current.step = index
    setActiveStep(index)
    sceneRef.current?.selectStep(index)
  }

  const togglePaused = () => {
    playbackRef.current.paused = !paused
    sceneRef.current?.setPaused(!paused)
    setPaused(!paused)
  }

  return (
    <div className={`hero-whistle-showcase ${styles.showcase}`} data-step={activeStep}>
      <div className={styles.header}>
        <span className={styles.eyebrow}><span className={styles.statusDot} /> THE BIG PICTURE</span>
        <span className={styles.localBadge}><Cpu size={12} /> Powered by your GPU</span>
      </div>
      <div className={styles.intro}>
        <h2>Small stream. <span>Big picture.</span></h2>
        <p>Less across the network. More on your screen.</p>
      </div>
      <div className={styles.visual} role="img" aria-label="A small source stream flows into REFEREE on the viewer’s local GPU, then becomes a detailed enhanced stream for playback in the web browser.">
        <div className={styles.stageLabels} aria-hidden="true">
          <span>SOURCE STREAM <b>480p</b></span>
          <span className={styles.coreLabel}>ON YOUR MACHINE</span>
          <span>ENHANCED PLAYBACK <b>Up to 4K</b></span>
        </div>
        <ScenePoster />
        <div className={styles.canvas} ref={hostRef} data-ready={ready} aria-hidden="true" />
        <div className={styles.visualFooter} aria-hidden="true">
          <span>LOWER BITRATE</span>
          <span className={styles.engineName}>REFEREE <span> / </span> LOCAL GPU</span>
          <span>GREATER DETAIL <ArrowUpRight size={11} /></span>
        </div>
      </div>
      <div className={styles.story} aria-label="Explore the enhancement pipeline">
        {steps.map(({ title, detail, icon: Icon }, index) => (
          <button key={title} className={styles.step} aria-pressed={activeStep === index} onClick={() => selectStep(index)}>
            <span className={styles.stepTop}><span className={styles.stepNumber}>0{index + 1}</span><Icon size={15} /></span>
            <span className={styles.stepTitle}>{title}</span>
            <span className={styles.stepDetail}>{detail}</span>
            <span className={styles.stepProgress} aria-hidden="true" />
          </button>
        ))}
      </div>
      <div className={styles.footer}>
        <span>Illustrative pipeline <span className={styles.footerDivider}>/</span> Quality depends on hardware and settings.</span>
        <button className={styles.motionToggle} onClick={togglePaused} aria-pressed={paused} aria-label={paused ? "Play showcase animation" : "Pause showcase animation"} disabled={!ready}>
          {paused ? <Play size={12} /> : <Pause size={12} />}
          <span>{paused ? (reducedMotion ? "Motion off" : "Paused") : "Pause"}</span>
        </button>
      </div>
    </div>
  )
}
