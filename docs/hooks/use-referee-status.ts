"use client"

import { useState, useEffect } from "react"

type ConnectionStatus = "checking" | "connected" | "disconnected"

interface RefereeStatusResult {
  status: ConnectionStatus
  isConnected: boolean
  hdrEnabled: boolean
  framegenEnabled: boolean
}

const STATUS_POLL_MS = 5_000
const DEFAULT_BASE = "http://localhost:14002"

export function useRefereeStatus(baseUrl: string = DEFAULT_BASE): RefereeStatusResult {
  const [status, setStatus] = useState<ConnectionStatus>("checking")
  const [hdrEnabled, setHdrEnabled] = useState(false)
  const [framegenEnabled, setFramegenEnabled] = useState(false)

  useEffect(() => {
    const check = async () => {
      try {
        const res = await fetch(`${baseUrl}/v1/status`, {
          signal: AbortSignal.timeout(2000),
        })
        if (res.ok) {
          const data = await res.json()
          setStatus(data.gpuReady === true ? "connected" : "disconnected")
          setHdrEnabled(data.settings?.hdr === true)
          setFramegenEnabled(data.settings?.framegen === true)
          return
        }
        setStatus("disconnected")
      } catch {
        setStatus("disconnected")
      }
    }
    // Reset to "checking" so callers react immediately when baseUrl changes
    setStatus("checking")
    check()
    const interval = setInterval(check, STATUS_POLL_MS)
    return () => clearInterval(interval)
  }, [baseUrl])

  return {
    status,
    isConnected: status === "connected",
    hdrEnabled,
    framegenEnabled,
  }
}
