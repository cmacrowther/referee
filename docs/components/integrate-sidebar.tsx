"use client"

import { useEffect, useState } from "react"
import { ChevronRight } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { cn } from "@/lib/utils"

type HttpMethod = "GET" | "POST" | "DELETE"

type NavItem = {
  id: string
  label: string
  method?: HttpMethod
  children?: NavItem[]
}

const methodColors: Record<HttpMethod, string> = {
  GET: "border-emerald-500/20 bg-emerald-500/10 text-emerald-400",
  POST: "border-blue-500/20 bg-blue-500/10 text-blue-400",
  DELETE: "border-red-500/20 bg-red-500/10 text-red-400",
}

const navItems: NavItem[] = [
  {
    id: "client-package",
    label: "npm Client Package",
  },
  {
    id: "quick-start",
    label: "Quick Start",
    children: [
      { id: "session-lifecycle", label: "Session Lifecycle" },
      { id: "heartbeat-contract", label: "Heartbeat Contract" },
    ],
  },
  { id: "cors-proxy", label: "CORS & Reverse Proxy" },
  {
    id: "api",
    label: "API Reference",
    children: [
      { id: "api-versioning", label: "Versioning & Auth" },
      { id: "endpoint-auth-request", label: "/v1/auth/request", method: "POST" },
      { id: "endpoint-auth-rotate", label: "/v1/auth/rotate-token", method: "POST" },
      { id: "endpoint-origins-list", label: "/v1/origins", method: "GET" },
      { id: "endpoint-origins-add", label: "/v1/origins", method: "POST" },
      { id: "endpoint-origins-delete", label: "/v1/origins/:origin", method: "DELETE" },
      { id: "endpoint-status", label: "/v1/status", method: "GET" },
      { id: "endpoint-ping", label: "/v1/ping", method: "GET" },
      { id: "endpoint-stream-start", label: "/v1/stream/start", method: "POST" },
      { id: "endpoint-stream-heartbeat", label: "/v1/stream/heartbeat", method: "POST" },
      { id: "endpoint-stream-stop", label: "/v1/stream/stop", method: "POST" },
      { id: "endpoint-hls", label: "/v1/tmp/... (HLS)", method: "GET" },
      { id: "endpoint-settings-stream", label: "/v1/settings/stream", method: "POST" },
      { id: "supported-formats", label: "Supported Source Formats" },
      { id: "error-codes", label: "Error Codes" },
    ],
  },
  { id: "faq", label: "FAQ" },
]

function flattenItems(items: NavItem[]): string[] {
  return items.flatMap((item) => [item.id, ...flattenItems(item.children ?? [])])
}

function hasActiveDescendant(item: NavItem, activeId: string): boolean {
  return (item.children ?? []).some(
    (child) => child.id === activeId || hasActiveDescendant(child, activeId)
  )
}

function getAutoExpandedIds(items: NavItem[], activeId: string, ancestors: string[] = []): string[] {
  for (const item of items) {
    if (item.id === activeId) {
      return item.children ? [...ancestors, item.id] : ancestors
    }

    if (!item.children) continue

    const expandedIds = getAutoExpandedIds(item.children, activeId, [...ancestors, item.id])
    if (expandedIds.length > 0) {
      return expandedIds
    }
  }

  return []
}

function renderItemLabel(item: NavItem, depth: number) {
  if (!item.method) {
    return <span className="block truncate">{item.label}</span>
  }

  return (
    <span className={cn("flex min-w-0 items-center gap-2 overflow-hidden", depth > 0 && "w-full")}>
      <Badge
        variant="outline"
        className={cn(
          "shrink-0 px-1.5 py-0 text-[10px] leading-4 font-mono tracking-[0.08em]",
          methodColors[item.method]
        )}
      >
        {item.method}
      </Badge>
      <code className="block min-w-0 truncate whitespace-nowrap font-mono text-[11px] text-inherit sm:text-[12px]">
        {item.label}
      </code>
    </span>
  )
}

export function IntegrateSidebar() {
  const [activeId, setActiveId] = useState<string>("")
  const [expandedIds, setExpandedIds] = useState<string[]>([])

  useEffect(() => {
    const allIds = flattenItems(navItems)
    const offset = 112

    const updateActiveId = () => {
      let nextActiveId = ""

      for (const id of allIds) {
        const element = document.getElementById(id)
        if (!element) continue

        if (element.getBoundingClientRect().top <= offset) {
          nextActiveId = id
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

  const autoExpandedIds = getAutoExpandedIds(navItems, activeId)

  const setGroupOpen = (id: string, open: boolean) => {
    setExpandedIds((currentIds) => {
      if (open) {
        return currentIds.includes(id) ? currentIds : [...currentIds, id]
      }

      return currentIds.filter((currentId) => currentId !== id)
    })
  }

  const renderItems = (items: NavItem[], depth = 0) => (
    <ul
      className={cn(
        "space-y-1",
        depth > 0 && "mt-1 border-l border-border/60 pl-3"
      )}
    >
      {items.map((item) => {
        const isActive = activeId === item.id
        const childActive = hasActiveDescendant(item, activeId)
        const isOpen = autoExpandedIds.includes(item.id) || expandedIds.includes(item.id)
        const linkClassName = cn(
          "block overflow-hidden rounded-md transition-colors",
          depth === 0 ? "px-2 py-1.5 text-sm font-medium" : "px-2 py-1 text-[13px]",
          isActive
            ? "bg-accent/10 font-medium text-accent"
            : childActive && depth === 0
              ? "font-medium text-foreground"
              : "text-muted-foreground hover:bg-secondary/50 hover:text-foreground"
        )

        if (!item.children) {
          return (
            <li key={item.id}>
              <a
                href={`#${item.id}`}
                aria-current={isActive ? "location" : undefined}
                title={item.method ? `${item.method} ${item.label}` : item.label}
                className={linkClassName}
              >
                {renderItemLabel(item, depth)}
              </a>
            </li>
          )
        }

        return (
          <li key={item.id}>
            <Collapsible open={isOpen} onOpenChange={(open) => setGroupOpen(item.id, open)}>
              <div className="flex items-center gap-1">
                <a
                  href={`#${item.id}`}
                  aria-current={isActive ? "location" : undefined}
                  title={item.method ? `${item.method} ${item.label}` : item.label}
                  className={cn(linkClassName, "min-w-0 flex-1")}
                >
                  {renderItemLabel(item, depth)}
                </a>
                <CollapsibleTrigger className="group flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-secondary/50 hover:text-foreground">
                  <ChevronRight className="h-3.5 w-3.5 transition-transform duration-200 group-data-[state=open]:rotate-90" />
                  <span className="sr-only">
                    {isOpen ? "Collapse" : "Expand"} {item.label}
                  </span>
                </CollapsibleTrigger>
              </div>
              <CollapsibleContent className="integrate-sidebar-collapsible-content">
                {renderItems(item.children, depth + 1)}
              </CollapsibleContent>
            </Collapsible>
          </li>
        )
      })}
    </ul>
  )

  return (
    <nav aria-label="Page sections" className="px-4 py-6">
      <p className="mb-6 px-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
        On this page
      </p>
      {renderItems(navItems)}
    </nav>
  )
}
