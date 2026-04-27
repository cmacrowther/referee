// ─── Types ───────────────────────────────────────────────────────────────────

export type RefereeGpuVendor = "nvidia" | "amd" | (string & {})
export type RefereeEncoderBackend = "nvenc" | "vceenc" | (string & {})
export type RefereeExecutorKind = "nvidiaSpecialized" | "universal" | "cpu" | (string & {})
export type RefereeContentKind = "live" | "vod" | (string & {})
export type RefereeStartupStage = "starting" | "ready" | (string & {})

export interface RefereeSettings {
  resolution: string
  quality: number
  framegen: boolean
  hdr: boolean
}

export interface SessionInfo {
  id: string
  sourceUrl: string
  /** Absolute URL to the upscaled HLS playlist for this active status session. */
  outputUrl: string
  appName: string | null
  streamTitle: string | null
  sourceContentKind: RefereeContentKind | null
  upscaler: string | null
  sourceResolution: string | null
  outputResolution: string
  sourceFps: number | null
  targetFps: number | null
  framegenEnabled: boolean
  hdrEnabled: boolean
  qualityLevel: number
  executor: RefereeExecutorKind
  encoderBackend: RefereeEncoderBackend | null
  startupComplete: boolean
  retryingStartup: boolean
  startupStage: RefereeStartupStage
}

export interface RefereeStatus {
  gpuReady: boolean | null
  gpuName: string | null
  gpuVendor: RefereeGpuVendor | null
  gpuUtilization: number | null
  encoderBackend: RefereeEncoderBackend | null
  encoderHasFramegen: boolean | null
  encoderHasTruehdr: boolean | null
  encoderHasRife: boolean | null
  /** Reserved for future servers; current REFEREE status responses do not include this field. */
  encoderHasStreamingRife?: boolean | null
  activeSessions: number
  sessions: SessionInfo[]
  primarySession: SessionInfo | null
  settings: RefereeSettings
  selectedExecutor: RefereeExecutorKind | null
  nvidiaAiAvailable: boolean | null
  amdAiAvailable: boolean | null
}

export interface StartSessionOptions {
  /** The source stream URL (HLS, DASH, or direct). */
  url: string
  /** Your app's name — shown in the REFEREE UI. */
  appName?: string
  /** A label for the stream — shown in the REFEREE UI. */
  streamTitle?: string
  /**
   * Override content kind. Auto-detected from the URL if omitted.
   * Use `"live"` for live streams and `"vod"` for on-demand content.
   */
  contentKind?: "live" | "vod"
  /** Extra HTTP headers forwarded to the encoder when fetching the stream. */
  headers?: Record<string, string>
}

export interface StartSessionResponse {
  sessionId: string
  /** Absolute URL to the upscaled HLS playlist. Load this directly in your player. */
  url: string
  resolution: string
  sourceResolution: string | null
  effectiveQuality: number
  evictedSessions: string[]
  appName: string | null
  streamTitle: string | null
}

export interface RefereeError {
  error: string
  code: string
}

export interface AuthResponse {
  token: string
  /** `true` if the approval was saved (Always Allow or pre-approved). */
  persistent: boolean
}

export interface RefereeClientOptions {
  /**
   * Base URL of the REFEREE server.
   * @default "http://localhost:14002"
   */
  baseUrl?: string
  /**
   * API token for authenticated requests. If omitted, the client will
   * automatically call `POST /v1/auth/request` on the first authenticated
   * call (triggers the REFEREE desktop consent dialog).
   */
  token?: string
  /**
   * Your app's display name — shown in the REFEREE consent dialog and stored
   * with the token approval. Used when auto-requesting a token.
   */
  appName?: string
}

// ─── RefereeClient ──────────────────────────────────────────────────────────

/**
 * Lightweight client for the REFEREE upscaling API.
 *
 * @example
 * ```ts
 * const referee = new RefereeClient({ appName: 'MyApp' });
 *
 * // Check availability
 * const status = await referee.getStatus();
 * if (!status.gpuReady) return; // no compatible GPU
 *
 * // Start an upscaling session
 * const session = await referee.startSession({
 *   url: 'https://example.com/live/stream.m3u8',
 *   streamTitle: 'Friday Night Stream',
 * });
 *
 * // Load session.url in your player, then when done:
 * await referee.stopSession(session.sessionId);
 * ```
 */
export class RefereeClient {
  readonly baseUrl: string
  readonly appName: string | undefined
  private _token: string | undefined
  private _tokenPromise: Promise<string> | null = null

  constructor(options: RefereeClientOptions = {}) {
    this.baseUrl = (options.baseUrl ?? "http://localhost:14002").replace(/\/$/, "")
    this._token = options.token
    this.appName = options.appName
  }

  // ─── Auth ─────────────────────────────────────────────────────────────────

  /**
   * Return the current token, or request one automatically.
   *
   * On the first call without a pre-configured token, this sends
   * `POST /v1/auth/request`. In desktop mode REFEREE shows a consent dialog;
   * in headless mode the origin must be pre-approved.
   *
   * The resolved token is cached for all subsequent calls.
   */
  async getToken(): Promise<string> {
    if (this._token) return this._token

    // Deduplicate concurrent calls — only one request in flight at a time.
    if (this._tokenPromise) return this._tokenPromise

    this._tokenPromise = this._requestToken().finally(() => {
      this._tokenPromise = null
    })
    return this._tokenPromise
  }

  private async _requestToken(): Promise<string> {
    const res = await fetch(`${this.baseUrl}/v1/auth/request`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ appName: this.appName }),
    })

    if (!res.ok) {
      const body = await res.json().catch(() => ({})) as Partial<RefereeError>
      throw new RefereeApiError(
        body.error ?? `Auth request failed with status ${res.status}`,
        body.code ?? "AUTH_FAILED",
        res.status
      )
    }

    const data = await res.json() as AuthResponse
    this._token = data.token
    return data.token
  }

  /**
   * Replace the active token. Call this if you already have a token
   * (e.g. retrieved from your backend) and want to skip the auth flow.
   */
  setToken(token: string): void {
    this._token = token
  }

  // ─── Status ───────────────────────────────────────────────────────────────

  /**
   * Check that REFEREE is running and a compatible GPU has been detected.
   * This endpoint is **unauthenticated** — safe to call before requesting a token.
   *
   * @throws {RefereeApiError} If REFEREE is unreachable or returns a non-2xx status.
   */
  async getStatus(): Promise<RefereeStatus> {
    return this._get<RefereeStatus>("/v1/status", { auth: false })
  }

  /**
   * Returns `true` if REFEREE is reachable and reports `gpuReady: true`.
   * Never throws — returns `false` for any error.
   */
  async isAvailable(): Promise<boolean> {
    try {
      const status = await this.getStatus()
      return status.gpuReady === true
    } catch {
      return false
    }
  }

  // ─── Session ──────────────────────────────────────────────────────────────

  /**
   * Start a hardware-accelerated upscaling session.
   *
   * This call **blocks until the upscaled HLS playlist is ready** — up to
   * ~3 minutes on first GPU start. Set your HTTP client timeout accordingly.
   *
   * On success, load `session.url` directly in your HLS player.
   *
   * @throws {RefereeApiError} On 503 (no GPU), 504 (timeout), or other errors.
   */
  async startSession(options: StartSessionOptions): Promise<StartSessionResponse> {
    return this._post<StartSessionResponse>("/v1/stream/start", options)
  }

  /**
   * Send a heartbeat to keep the session alive.
   * Call every **10 seconds** while the user is watching.
   *
   * Returns `false` if the session was cleaned up server-side (404).
   * Returns `true` on success.
   * Never throws for expected conditions — only re-throws unexpected errors.
   */
  async sendHeartbeat(sessionId: string): Promise<boolean> {
    try {
      const res = await this._rawPost(`/v1/stream/heartbeat/${sessionId}`)
      if (res.status === 404) return false
      return res.ok
    } catch {
      // Network blip — don't kill the interval; the 15 s server timeout
      // gives us a small window to recover.
      return true
    }
  }

  /**
   * Explicitly stop a session and release GPU resources immediately.
   *
   * Always call this when playback ends. Do not rely solely on heartbeat timeout.
   *
   * @param sessionId  The session to stop. If omitted and `stopAll` is true, all sessions are stopped.
   * @param stopAll    Stop all active sessions instead of a specific one.
   */
  async stopSession(sessionId?: string, stopAll?: boolean): Promise<void> {
    await this._post("/v1/stream/stop", { sessionId, stopAll })
  }

  /**
   * Best-effort navigation-safe stop for browser unload/pagehide handlers.
   *
   * This uses `fetch(..., { keepalive: true })` instead of `sendBeacon` because
   * REFEREE stop requests require the `X-Referee-Token` header.
   */
  stopSessionOnUnload(sessionId?: string, stopAll?: boolean): void {
    const token = this._token
    if (!token || typeof fetch === "undefined") return

    fetch(`${this.baseUrl}/v1/stream/stop`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Referee-Token": token,
      },
      body: JSON.stringify({ sessionId, stopAll }),
      keepalive: true,
    }).catch(() => {})
  }

  // ─── Managed session (with auto-heartbeat) ────────────────────────────────

  /**
   * Start a session and automatically manage the heartbeat loop.
   *
   * Returns an object with the session info and a `dispose()` function.
   * Call `dispose()` to stop the heartbeat and the session — wire it to
   * your component's cleanup logic (`useEffect` return, `pagehide`, etc.).
   *
   * @example
   * ```ts
   * const { session, dispose } = await referee.startManagedSession({
   *   url: sourceUrl,
   *   appName: 'MyApp',
   * });
   * player.src = session.url;
   *
   * // On unmount / page close:
   * dispose();
   * ```
   */
  async startManagedSession(
    options: StartSessionOptions,
    heartbeatIntervalMs = 10_000
  ): Promise<{
    session: StartSessionResponse
    dispose: () => void
  }> {
    const session = await this.startSession(options)
    let intervalHandle: ReturnType<typeof setInterval> | null = null

    const startHeartbeat = () => {
      intervalHandle = setInterval(async () => {
        const alive = await this.sendHeartbeat(session.sessionId)
        if (!alive) stopHeartbeat()
      }, heartbeatIntervalMs)
    }

    const stopHeartbeat = () => {
      if (intervalHandle !== null) {
        clearInterval(intervalHandle)
        intervalHandle = null
      }
    }

    startHeartbeat()

    // Best-effort stop on page unload.
    const handlePageHide = () => {
      this.stopSessionOnUnload(session.sessionId)
    }
    if (typeof window !== "undefined") {
      window.addEventListener("pagehide", handlePageHide)
    }

    const dispose = () => {
      stopHeartbeat()
      if (typeof window !== "undefined") {
        window.removeEventListener("pagehide", handlePageHide)
      }
      this.stopSession(session.sessionId).catch(() => {})
    }

    return { session, dispose }
  }

  // ─── Origins ──────────────────────────────────────────────────────────────

  /** List all persistently approved origins. */
  async listOrigins(): Promise<Array<{ origin: string; appName: string | null; approvedAt: string }>> {
    return this._get("/v1/origins")
  }

  /** Add or update an approved origin. Useful for headless deployments. */
  async addOrigin(origin: string, appName?: string): Promise<void> {
    await this._post("/v1/origins", { origin, appName })
  }

  /** Remove an approved origin. */
  async deleteOrigin(origin: string): Promise<void> {
    const token = await this.getToken()
    const res = await fetch(
      `${this.baseUrl}/v1/origins/${encodeURIComponent(origin)}`,
      {
        method: "DELETE",
        headers: { "X-Referee-Token": token },
      }
    )
    if (!res.ok) await this._throwApiError(res)
  }

  // ─── Internal helpers ─────────────────────────────────────────────────────

  private async _get<T>(path: string, opts: { auth?: boolean } = {}): Promise<T> {
    const headers: Record<string, string> = {}
    if (opts.auth !== false) {
      headers["X-Referee-Token"] = await this.getToken()
    }
    const res = await fetch(`${this.baseUrl}${path}`, { headers })
    if (!res.ok) await this._throwApiError(res)
    return res.json() as Promise<T>
  }

  private async _post<T = void>(path: string, body?: unknown): Promise<T> {
    const res = await this._rawPost(path, body)
    if (!res.ok) await this._throwApiError(res)
    return res.json() as Promise<T>
  }

  private async _rawPost(path: string, body?: unknown): Promise<Response> {
    const token = await this.getToken()
    return fetch(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Referee-Token": token,
      },
      ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
    })
  }

  private async _throwApiError(res: Response): Promise<never> {
    const body = await res.json().catch(() => ({})) as Partial<RefereeError>
    throw new RefereeApiError(
      body.error ?? `Request failed with status ${res.status}`,
      body.code ?? "UNKNOWN",
      res.status
    )
  }
}

// ─── Error class ─────────────────────────────────────────────────────────────

export class RefereeApiError extends Error {
  readonly code: string
  readonly status: number

  constructor(message: string, code: string, status: number) {
    super(message)
    this.name = "RefereeApiError"
    this.code = code
    this.status = status
  }
}

// ─── Convenience factory ──────────────────────────────────────────────────────

/**
 * Create a `RefereeClient` instance with default options.
 * Equivalent to `new RefereeClient(options)`.
 */
export function createRefereeClient(options?: RefereeClientOptions): RefereeClient {
  return new RefereeClient(options)
}
