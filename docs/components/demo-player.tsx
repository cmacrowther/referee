"use client"

import { useState, useRef } from "react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { RefereeWordmark } from "@/components/referee-wordmark"
import { useRefereeStatus } from "@/hooks/use-referee-status"
import { 
  Play, 
  Pause, 
  Volume2, 
  VolumeX, 
  Maximize, 
  Settings,
  Zap,
  WifiOff,
  CheckCircle2,
  AlertCircle
} from "lucide-react"

export function DemoPlayer() {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [isPlaying, setIsPlaying] = useState(false)
  const [isMuted, setIsMuted] = useState(false)
  const { status: refereeStatus } = useRefereeStatus()
  const [upscalingEnabled, setUpscalingEnabled] = useState(true)

  return (
    <div className="space-y-4">
      {/* Status Bar */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Badge 
            variant="outline" 
            className={`px-3 py-1 ${
              refereeStatus === "connected" 
                ? "border-accent/50 text-accent" 
                : refereeStatus === "checking"
                ? "border-muted-foreground/50 text-muted-foreground"
                : "border-destructive/50 text-destructive"
            }`}
          >
            {refereeStatus === "connected" && (
              <>
                <CheckCircle2 className="h-3 w-3 mr-1.5" />
                <RefereeWordmark variant="inline" />{" "}
                <span>Connected</span>
              </>
            )}
            {refereeStatus === "checking" && (
              <>
                <Settings className="h-3 w-3 mr-1.5 animate-spin" />
                Checking...
              </>
            )}
            {refereeStatus === "disconnected" && (
              <>
                <WifiOff className="h-3 w-3 mr-1.5" />
                <RefereeWordmark variant="inline" />{" "}
                <span>Not Detected</span>
              </>
            )}
          </Badge>
          {refereeStatus === "connected" && (
            <Badge variant="outline" className="px-3 py-1 border-muted-foreground/30 text-muted-foreground">
              <Zap className="h-3 w-3 mr-1.5" />
              Upscaling Active
            </Badge>
          )}
        </div>
        {refereeStatus === "disconnected" && (
          <a 
            href="https://github.com/cmacrowther/referee/releases"
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-accent hover:underline"
          >
            Download <RefereeWordmark variant="inline" /> to enable upscaling
          </a>
        )}
      </div>

      {/* Video Player */}
      <div className="relative aspect-video bg-card border border-border rounded-lg overflow-hidden group">
        {/* Video Element */}
        <video
          ref={videoRef}
          className="w-full h-full object-cover"
          poster="https://upload.wikimedia.org/wikipedia/commons/thumb/c/c5/Big_buck_bunny_poster_big.jpg/1280px-Big_buck_bunny_poster_big.jpg"
          muted={isMuted}
        >
          <source 
            src="https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4" 
            type="video/mp4" 
          />
        </video>

        {/* Play Overlay */}
        {!isPlaying && (
          <div className="absolute inset-0 flex items-center justify-center bg-background/50">
            <button 
              onClick={() => {
                const video = videoRef.current
                if (video) {
                  video.play()
                  setIsPlaying(true)
                }
              }}
              className="w-20 h-20 rounded-full bg-accent text-accent-foreground flex items-center justify-center hover:scale-105 transition-transform"
            >
              <Play className="h-8 w-8 ml-1" />
            </button>
          </div>
        )}

        {/* Controls Bar */}
        <div className="absolute bottom-0 left-0 right-0 p-4 bg-gradient-to-t from-background/90 to-transparent opacity-0 group-hover:opacity-100 transition-opacity">
          <div className="flex items-center gap-4">
            <button 
              onClick={() => {
                const video = videoRef.current
                if (video) {
                  if (isPlaying) {
                    video.pause()
                  } else {
                    video.play()
                  }
                  setIsPlaying(!isPlaying)
                }
              }}
              className="text-foreground hover:text-accent transition-colors"
            >
              {isPlaying ? <Pause className="h-5 w-5" /> : <Play className="h-5 w-5" />}
            </button>
            
            <button 
              onClick={() => {
                const video = videoRef.current
                if (video) {
                  video.muted = !video.muted
                  setIsMuted(!isMuted)
                }
              }}
              className="text-foreground hover:text-accent transition-colors"
            >
              {isMuted ? <VolumeX className="h-5 w-5" /> : <Volume2 className="h-5 w-5" />}
            </button>

            <div className="flex-1">
              <div className="h-1 bg-muted rounded-full overflow-hidden">
                <div className="h-full w-1/3 bg-accent rounded-full" />
              </div>
            </div>

            <div className="flex items-center gap-3">
              {refereeStatus === "connected" && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setUpscalingEnabled(!upscalingEnabled)}
                  className={upscalingEnabled ? "text-accent" : "text-muted-foreground"}
                >
                  <Zap className="h-4 w-4 mr-1" />
                  {upscalingEnabled ? "4K" : "OFF"}
                </Button>
              )}
              <button 
                onClick={() => {
                  const video = videoRef.current
                  if (video) {
                    video.requestFullscreen()
                  }
                }}
                className="text-foreground hover:text-accent transition-colors"
              >
                <Maximize className="h-5 w-5" />
              </button>
            </div>
          </div>
        </div>

        {/* Upscaling Indicator */}
        {refereeStatus === "connected" && upscalingEnabled && (
          <div className="absolute top-4 right-4">
            <Badge className="bg-accent/90 text-accent-foreground border-0">
              <Zap className="h-3 w-3 mr-1" />
              AI Upscaling
            </Badge>
          </div>
        )}
      </div>

      {/* Video Info */}
      <div className="flex items-center justify-between text-sm text-muted-foreground">
        <div>
          <span className="font-medium text-foreground">Big Buck Bunny</span>
          <span className="mx-2">·</span>
          <span>Blender Foundation</span>
        </div>
        <div className="flex items-center gap-4">
          <span>1080p source</span>
          {refereeStatus === "connected" && upscalingEnabled && (
            <>
              <span className="text-accent">→</span>
              <span className="text-accent font-medium">4K upscaled</span>
            </>
          )}
        </div>
      </div>

      {/* Connection Help */}
      {refereeStatus === "disconnected" && (
        <div className="flex items-start gap-3 p-4 bg-secondary/50 border border-border rounded-lg">
          <AlertCircle className="h-5 w-5 text-muted-foreground shrink-0 mt-0.5" />
          <div className="text-sm">
            <p className="font-medium text-foreground mb-1">
              <RefereeWordmark variant="inline" /> not detected
            </p>
            <p className="text-muted-foreground">
              To experience AI upscaling, download and run <RefereeWordmark variant="inline" /> on
              your Windows PC with a compatible GPU. The video will play normally without
              upscaling until <RefereeWordmark variant="inline" /> is connected.
            </p>
          </div>
        </div>
      )}
    </div>
  )
}
