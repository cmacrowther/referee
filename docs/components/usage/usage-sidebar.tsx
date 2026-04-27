"use client"

import { useEffect, useState } from "react"
import { cn } from "@/lib/utils"

type NavItem = {
  id: string
  label: string
}

const navItems: NavItem[] = [
  { id: "quick-start", label: "Quick Start" },
  { id: "connect-player", label: "Connecting a Player" },
  { id: "quality-settings", label: "Stream Quality" },
  { id: "gpu-features", label: "Frame Gen & TrueHDR" },
  { id: "tray-mode", label: "Background & Tray Mode" },
  { id: "relay", label: "REFEREE Relay" },
  { id: "performance", label: "Performance Reference" },
  { id: "troubleshooting", label: "Troubleshooting" },
]

export function UsageSidebar() {
  const [activeId, setActiveId] = useState<string>("")

  useEffect(() => {
    const offset = 112

    const updateActiveId = () => {
      let nextActiveId = ""

      for (const item of navItems) {
        const element = document.getElementById(item.id)
        if (!element) continue

        if (element.getBoundingClientRect().top <= offset) {
          nextActiveId = item.id
          continue
        }

        break
      }

      setActiveId((currentId) => (currentId === nextActiveId ? currentId : nextActiveId))
    }

    updateActiveId()
    window.addEventListener("scroll", updateActiveId, { passive: true })
    window.addEventListener("resize", updateActiveId)

    return () => {
      window.removeEventListener("scroll", updateActiveId)
      window.removeEventListener("resize", updateActiveId)
    }
  }, [])

  return (
    <nav aria-label="Page sections" className="px-4 py-6">
      <p className="mb-6 px-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
        On this page
      </p>
      <ul className="space-y-1">
        {navItems.map((item) => {
          const isActive = activeId === item.id
          return (
            <li key={item.id}>
              <a
                href={`#${item.id}`}
                aria-current={isActive ? "location" : undefined}
                className={cn(
                  "block overflow-hidden rounded-md px-2 py-1.5 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-accent/10 font-medium text-accent"
                    : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
                )}
              >
                {item.label}
              </a>
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
