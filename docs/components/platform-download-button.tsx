"use client"

import { useEffect, useMemo, useState } from "react"
import { Button } from "@/components/ui/button"

type Platform = "windows" | "linux" | "unknown"

type PlatformDownloadButtonProps = {
  className?: string
  openInNewTab?: boolean
}

function WindowsIcon() {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      className="h-4 w-4 mr-2 fill-current"
    >
      <path d="M3 4.64 10.5 3.5v8.06H3V4.64ZM12 3.29 21 2v9.56h-9V3.29ZM3 12.94h7.5V21.06L3 20v-7.06ZM12 12.94h9V22l-9-1.29v-7.77Z" />
    </svg>
  )
}

function LinuxIcon() {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      className="h-4 w-4 mr-2 fill-current"
    >
      <path d="M12.06 2.5c-2.26 0-3.78 1.92-3.78 4.44 0 1.23.34 2.34.89 3.08-.67.5-1.05 1.2-1.05 2.03 0 .87.43 1.65 1.17 2.15-.36.3-.58.7-.58 1.15 0 .93.95 1.57 1.89 1.57h.13c.09.58.51 1.59 1.33 1.59.83 0 1.25-1.01 1.34-1.59h.13c.94 0 1.89-.64 1.89-1.57 0-.45-.22-.85-.58-1.15.74-.5 1.16-1.28 1.16-2.15 0-.83-.37-1.53-1.04-2.03.55-.74.88-1.85.88-3.08 0-2.52-1.51-4.44-3.78-4.44Zm-1.24 4.12c.38 0 .68.31.68.69 0 .38-.3.69-.68.69a.69.69 0 0 1-.69-.69c0-.38.31-.69.69-.69Zm2.48 0c.38 0 .69.31.69.69 0 .38-.31.69-.69.69a.69.69 0 0 1 0-1.38Z" />
      <path d="M6.37 16.72c-.56.14-1 .6-1 1.18 0 .68.6 1.24 1.35 1.24.08 0 .15-.01.22-.02.2 1.35 1.55 2.38 3.15 2.38.96 0 1.83-.37 2.42-.97.6.6 1.47.97 2.43.97 1.6 0 2.95-1.03 3.15-2.38.07.01.14.02.22.02.75 0 1.35-.56 1.35-1.24 0-.58-.44-1.04-1-1.18-.38.92-1.37 1.54-2.57 1.54-.71 0-1.35-.22-1.83-.58-.33.37-.79.58-1.3.58s-.97-.21-1.3-.58c-.48.36-1.12.58-1.83.58-1.2 0-2.19-.62-2.56-1.54Z" />
    </svg>
  )
}

function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "unknown"

  const ua = navigator.userAgent.toLowerCase()
  const platform = navigator.platform.toLowerCase()

  if (platform.includes("win") || ua.includes("windows")) {
    return "windows"
  }
  if (platform.includes("linux") || ua.includes("linux") || ua.includes("x11")) {
    return "linux"
  }

  return "unknown"
}

export function PlatformDownloadButton({
  className,
  openInNewTab = false,
}: PlatformDownloadButtonProps) {
  const [platform, setPlatform] = useState<Platform>("unknown")

  useEffect(() => {
    setPlatform(detectPlatform())
  }, [])

  const { href, label, icon } = useMemo(() => {
    if (platform === "linux") {
      return {
        href: "/api/download?platform=linux",
        label: "Download for Linux",
        icon: <LinuxIcon />,
      }
    }

    if (platform === "windows") {
      return {
        href: "/api/download",
        label: "Download for Windows",
        icon: <WindowsIcon />,
      }
    }

    return {
      href: "/api/download",
      label: "Download for your OS",
      icon: <WindowsIcon />,
    }
  }, [platform])

  return (
    <Button size="lg" variant="outline" className={className} asChild>
      <a
        href={href}
        target={openInNewTab ? "_blank" : undefined}
        rel={openInNewTab ? "noopener noreferrer" : undefined}
      >
        {icon}
        {label}
      </a>
    </Button>
  )
}
