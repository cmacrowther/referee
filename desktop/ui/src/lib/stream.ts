import {
    DEFAULT_ENCODING_PROFILES,
    DEFAULT_PLAYER_SETTINGS,
    DEFAULT_RELAY_SETTINGS,
    DEFAULT_SETTINGS,
    DEFAULT_STATUS,
    type ExecutorPreference,
    type RelayPeerMetadata,
    type Session,
    type Settings,
    type SettingsUiCapabilities,
    type SettingsUiFeatureSupport,
    type SettingsUiLockState,
    type SettingsUiResolvedPath,
    type SettingsUiSupportKind,
    type Status,
} from '@/lib/types'

export const MAX_VSR_QUALITY = 4
export const MAX_4K_VSR_QUALITY = 4

export function getMaxQualityForResolution(resolution: string) {
    return resolution === '3840x2160' ? MAX_4K_VSR_QUALITY : MAX_VSR_QUALITY
}

/**
 * Normalize and clamp an upscale quality value to the valid integer range for a given resolution.
 *
 * @param resolution - The output resolution (e.g., "1920x1080" or "3840x2160") used to determine the maximum allowed quality
 * @param quality - The desired quality value; may be a number, numeric string, or null/undefined. Non-finite values default to `1`
 * @returns An integer quality between `1` and the resolution-specific maximum (inclusive)
 */
export function clampQualityForResolution(resolution: string, quality: number | string | null | undefined) {
    const maxQuality = getMaxQualityForResolution(resolution)
    const parsedQuality = Number(quality)
    const normalizedQuality = Number.isFinite(parsedQuality) ? Math.round(parsedQuality) : 1
    return Math.max(1, Math.min(maxQuality, normalizedQuality))
}

export type ActiveProcessingEngine = 'nvidiaAi' | 'amdAi' | 'universal' | 'cpu' | null

function normalizeNullableString(value: unknown): string | null {
    if (typeof value !== 'string') {
        return null
    }

    const trimmed = value.trim()
    return trimmed ? trimmed : null
}

function normalizeRelayPeerMetadata(peer?: Partial<RelayPeerMetadata> | null): RelayPeerMetadata | null {
    if (!peer || typeof peer !== 'object') {
        return null
    }

    const normalized: RelayPeerMetadata = {
        instanceId: normalizeNullableString(peer.instanceId),
        hostname: normalizeNullableString(peer.hostname),
        ip: normalizeNullableString(peer.ip),
        version: normalizeNullableString(peer.version),
        platform: normalizeNullableString(peer.platform),
        gpuReady: typeof peer.gpuReady === 'boolean' ? peer.gpuReady : null,
        gpuVendor: normalizeNullableString(peer.gpuVendor),
        gpuName: normalizeNullableString(peer.gpuName),
    }

    const hasData = Object.values(normalized).some((value) => value !== null)
    return hasData ? normalized : null
}

/**
 * Normalize legacy or alias executor preference strings into a canonical ExecutorPreference.
 *
 * @param value - Candidate value (legacy alias or canonical identifier) to normalize
 * @returns `'nvidiaAi'`, `'amdAi'`, or `'universal'` for recognized inputs; otherwise the default executor preference
 */
export function normalizeExecutorPreference(value: unknown): ExecutorPreference {
    if (!import.meta.env.DEV) {
        return DEFAULT_SETTINGS.executorPreference
    }

    if (value === 'nvidiaAi' || value === 'nvidia' || value === 'nvidiaSpecialized') {
        return 'nvidiaAi'
    }

    if (value === 'amdAi' || value === 'amd') {
        return 'amdAi'
    }

    if (value === 'universal') {
        return 'universal'
    }

    return DEFAULT_SETTINGS.executorPreference
}

/**
 * Convert a user-provided executor/engine string into a canonical ActiveProcessingEngine identifier.
 *
 * @param value - Optional executor or engine name (may include whitespace, delimiters, or an `executor` suffix)
 * @returns `'nvidiaAi'`, `'amdAi'`, `'universal'`, or `'cpu'` when `value` maps to that engine; `null` otherwise.
 */
export function normalizeActiveProcessingEngine(value?: string | null): ActiveProcessingEngine {
    if (!value) {
        return null
    }

    const normalized = value
        .trim()
        .replace(/[\s_-]/g, '')
        .replace(/executor$/i, '')
        .toLowerCase()
    if (normalized === 'nvidiaspecialized' || normalized === 'nvidia') return 'nvidiaAi'
    if (normalized === 'amd' || normalized === 'amdai') return 'amdAi'
    if (normalized === 'universal') return 'universal'
    if (normalized === 'cpu') return 'cpu'
    return null
}

/**
 * Produce a complete Settings object by merging provided partial overrides with defaults and normalizing specific fields.
 *
 * Normalizations:
 * - `resolution` defaults to the default resolution when missing.
 * - `quality` is converted, rounded, and clamped to the allowed range for the resolved resolution.
 * - Boolean flags (`framegen`, `hdr`, `alwaysOnTop`, `showOnProxyStart`, `minimizeToTray`, `closeToTray`, `notifications`, and `player.enabled`) are coerced to `true`/`false` with sensible defaults.
 * - `executorPreference` is normalized via `normalizeExecutorPreference`.
 * - `encodingProfiles` are merged with `DEFAULT_ENCODING_PROFILES`.
 * - `player` fields are populated from defaults when absent.
 * - `instanceId` is trimmed and normalized to `string | null`.
 * - `relay` fields are populated from defaults when absent, and nested peer metadata is normalized.
 *
 * @param settings - Partial Settings to merge and normalize; missing or undefined values are filled from defaults.
 * @returns A fully populated `Settings` object with normalized and validated fields.
 */
export function normalizeSettings(settings?: Partial<Settings>): Settings {
    const resolution = settings?.resolution || DEFAULT_SETTINGS.resolution
    const executorPreference = normalizeExecutorPreference(settings?.executorPreference)

    return {
        ...DEFAULT_SETTINGS,
        ...settings,
        resolution,
        quality: clampQualityForResolution(resolution, settings?.quality ?? DEFAULT_SETTINGS.quality),
        framegen: Boolean(settings?.framegen ?? DEFAULT_SETTINGS.framegen),
        hdr: Boolean(settings?.hdr ?? DEFAULT_SETTINGS.hdr),
        executorPreference,
        alwaysOnTop: Boolean(settings?.alwaysOnTop ?? DEFAULT_SETTINGS.alwaysOnTop),
        showOnProxyStart: Boolean(settings?.showOnProxyStart ?? DEFAULT_SETTINGS.showOnProxyStart),
        minimizeToTray: Boolean(settings?.minimizeToTray ?? DEFAULT_SETTINGS.minimizeToTray),
        closeToTray: Boolean(settings?.closeToTray ?? DEFAULT_SETTINGS.closeToTray),
        notifications: Boolean(settings?.notifications ?? DEFAULT_SETTINGS.notifications),
        encodingProfiles: {
            ...DEFAULT_ENCODING_PROFILES,
            ...(settings?.encodingProfiles ?? {}),
        },
        player: {
            enabled: Boolean(settings?.player?.enabled ?? DEFAULT_PLAYER_SETTINGS.enabled),
            selectedPlayer: settings?.player?.selectedPlayer ?? DEFAULT_PLAYER_SETTINGS.selectedPlayer,
            customPath: settings?.player?.customPath ?? DEFAULT_PLAYER_SETTINGS.customPath,
        },
        instanceId: normalizeNullableString(settings?.instanceId),
        relay: {
            enabled: Boolean(settings?.relay?.enabled ?? DEFAULT_RELAY_SETTINGS.enabled),
            linkedPeerId: normalizeNullableString(settings?.relay?.linkedPeerId),
            linkedPeerHostname: normalizeNullableString(settings?.relay?.linkedPeerHostname),
            linkedPeerIp: normalizeNullableString(settings?.relay?.linkedPeerIp),
            remoteToken: normalizeNullableString(settings?.relay?.remoteToken),
            lastKnownPeer: normalizeRelayPeerMetadata(settings?.relay?.lastKnownPeer),
        },
    }
}

type StatusLike = Partial<Status> & Record<string, unknown>

/**
 * Return the input when it is a boolean, otherwise return `null`.
 *
 * @param value - The value to test for boolean type
 * @returns The boolean value when `value` is a boolean, `null` otherwise
 */
function readBoolean(value: unknown) {
    return typeof value === 'boolean' ? value : null
}

/**
 * Normalize a possibly partial session object into a fully-typed Session or `null`.
 *
 * Accepts camelCase or legacy snake_case input keys and coerces fields to the expected types:
 * string fields become `string` or `null`, numeric fields become `number` or `null`, boolean flags
 * default to `false` when not a boolean, and `previewFrames` defaults to an empty array when not provided.
 *
 * @param session - A partial session object (may use camelCase or snake_case keys); if missing or not an object, `null` is returned.
 * @returns A normalized `Session` with canonical keys and sane defaults, or `null` when the input is not an object.
 */
function normalizeSession(session?: (Partial<Session> & Record<string, unknown>) | null): Session | null {
    if (!session || typeof session !== 'object') {
        return null
    }

    return {
        id: typeof session.id === 'string' ? session.id : null,
        sourceUrl: typeof session.sourceUrl === 'string' ? session.sourceUrl : typeof session.source_url === 'string' ? session.source_url : null,
        outputUrl: typeof session.outputUrl === 'string' ? session.outputUrl : typeof session.output_url === 'string' ? session.output_url : null,
        appName: typeof session.appName === 'string' ? session.appName : typeof session.app_name === 'string' ? session.app_name : null,
        streamTitle: typeof session.streamTitle === 'string' ? session.streamTitle : typeof session.stream_title === 'string' ? session.stream_title : null,
        sourceContentKind:
            typeof session.sourceContentKind === 'string'
                ? session.sourceContentKind
                : typeof session.source_content_kind === 'string'
                  ? session.source_content_kind
                  : null,
        upscaler: typeof session.upscaler === 'string' ? session.upscaler : null,
        sourceResolution:
            typeof session.sourceResolution === 'string'
                ? session.sourceResolution
                : typeof session.source_resolution === 'string'
                  ? session.source_resolution
                  : null,
        outputResolution:
            typeof session.outputResolution === 'string'
                ? session.outputResolution
                : typeof session.output_resolution === 'string'
                  ? session.output_resolution
                  : null,
        sourceFps: typeof session.sourceFps === 'number' ? session.sourceFps : typeof session.source_fps === 'number' ? session.source_fps : null,
        targetFps: typeof session.targetFps === 'number' ? session.targetFps : typeof session.target_fps === 'number' ? session.target_fps : null,
        framegenEnabled:
            typeof session.framegenEnabled === 'boolean'
                ? session.framegenEnabled
                : typeof session.framegen_enabled === 'boolean'
                  ? session.framegen_enabled
                  : false,
        hdrEnabled: typeof session.hdrEnabled === 'boolean' ? session.hdrEnabled : typeof session.hdr_enabled === 'boolean' ? session.hdr_enabled : false,
        qualityLevel:
            typeof session.qualityLevel === 'number' ? session.qualityLevel : typeof session.quality_level === 'number' ? session.quality_level : null,
        executor: typeof session.executor === 'string' ? session.executor : typeof session.executor_kind === 'string' ? session.executor_kind : null,
        encoderBackend:
            typeof session.encoderBackend === 'string' ? session.encoderBackend : typeof session.encoder_backend === 'string' ? session.encoder_backend : null,
        startupComplete:
            typeof session.startupComplete === 'boolean'
                ? session.startupComplete
                : typeof session.startup_complete === 'boolean'
                  ? session.startup_complete
                  : session.startup_stage === 'ready',
        retryingStartup:
            typeof session.retryingStartup === 'boolean'
                ? session.retryingStartup
                : typeof session.retrying_startup === 'boolean'
                  ? session.retrying_startup
                  : false,
    }
}

/**
 * Normalize a possibly partial or legacy status object into a fully populated `Status`.
 *
 * Accepts a `StatusLike` input (including objects using camelCase or snake_case and legacy keys) and returns a `Status` with normalized and validated fields.
 *
 * @param status - Partial or legacy status object to normalize; may contain sessions, primary session, GPU/encoder fields, feature availability flags, and settings in camelCase or snake_case.
 * @returns A `Status` whose missing fields are filled from `DEFAULT_STATUS`, where:
 * - sessions are normalized and filtered into a `sessions` array and `primarySession` is resolved
 * - `activeSessions` is derived from available keys
 * - GPS and encoder fields (`gpuReady`, `gpuName`, `gpuVendor`, `gpuUtilization`, `encoderBackend`) are validated and normalized
 * - executor/availability flags (`selectedExecutor`, `nvidiaAiAvailable`, `amdAiAvailable`, `encoderHasFramegen`, `encoderHasTruehdr`, `encoderHasRife`) are normalized from multiple possible keys
 * - `settings` is merged with `DEFAULT_STATUS.settings`
 */
export function normalizeStatus(status?: StatusLike): Status {
    const rawSessions = Array.isArray(status?.sessions) ? status.sessions : Array.isArray(status?.preview_sessions) ? status.preview_sessions : []
    const sessions = rawSessions
        .map((session) => normalizeSession(session as Partial<Session> & Record<string, unknown>))
        .filter((session): session is Session => session !== null)
    const primarySession = normalizeSession(((status?.primarySession ?? status?.primary_session) as Partial<Session> & Record<string, unknown>) ?? null)
    const activeSessions =
        typeof status?.activeSessions === 'number'
            ? status.activeSessions
            : typeof status?.active_sessions === 'number'
              ? status.active_sessions
              : sessions.length
    const gpuReady =
        typeof status?.gpuReady === 'boolean' ? status.gpuReady : typeof status?.gpu_ready === 'boolean' ? status.gpu_ready : DEFAULT_STATUS.gpuReady
    const gpuName = typeof status?.gpuName === 'string' ? status.gpuName : typeof status?.gpu_name === 'string' ? status.gpu_name : null
    const gpuVendor = typeof status?.gpuVendor === 'string' ? status.gpuVendor : typeof status?.gpu_vendor === 'string' ? status.gpu_vendor : null
    const gpuUtilization =
        typeof status?.gpuUtilization === 'number' ? status.gpuUtilization : typeof status?.gpu_utilization === 'number' ? status.gpu_utilization : null
    const encoderBackend =
        typeof status?.encoderBackend === 'string' ? status.encoderBackend : typeof status?.encoder_backend === 'string' ? status.encoder_backend : null
    const selectedExecutor =
        typeof status?.selectedExecutor === 'string'
            ? status.selectedExecutor
            : typeof status?.selected_executor === 'string'
              ? status.selected_executor
              : (primarySession?.executor ?? sessions[0]?.executor ?? null)
    const nvidiaAiAvailable =
        typeof status?.nvidiaAiAvailable === 'boolean'
            ? status.nvidiaAiAvailable
            : typeof status?.nvidia_ai_available === 'boolean'
              ? status.nvidia_ai_available
              : null
    const amdAiAvailable =
        typeof status?.amdAiAvailable === 'boolean' ? status.amdAiAvailable : typeof status?.amd_ai_available === 'boolean' ? status.amd_ai_available : null
    const encoderHasFramegen = readBoolean(status?.encoderHasFramegen) ?? readBoolean(status?.encoder_has_framegen)
    const encoderHasTruehdr = readBoolean(status?.encoderHasTruehdr) ?? readBoolean(status?.encoder_has_truehdr)
    const encoderHasRife = readBoolean(status?.encoderHasRife) ?? readBoolean(status?.encoder_has_rife)

    return {
        ...DEFAULT_STATUS,
        ...status,
        gpuReady,
        gpuName,
        gpuVendor,
        gpuUtilization,
        encoderBackend,
        selectedExecutor,
        nvidiaAiAvailable,
        amdAiAvailable,
        activeSessions,
        sessions,
        primarySession: primarySession ?? sessions[0] ?? null,
        settings: {
            ...DEFAULT_STATUS.settings,
            ...status?.settings,
        },
        encoderHasFramegen,
        encoderHasTruehdr,
        encoderHasRife,
    }
}

/**
 * Determine the settings UI lock state.
 *
 * @returns `'locked'` when the UI should be locked, `'editable'` otherwise.
 */
function getSettingsUiLockState(locked: boolean): SettingsUiLockState {
    return locked ? 'locked' : 'editable'
}

/**
 * Normalize a GPU vendor string to a canonical identifier.
 *
 * Trims and lowercases the input; maps "nvidia" → "nvidia", "amd" → "amd", and returns the normalized string for other inputs.
 *
 * @param value - The vendor name to normalize, or `null`/`undefined`.
 * @returns `'nvidia'` or `'amd'` for those vendors, the lowercased trimmed input for other non-empty strings, or `null` when `value` is missing.
 */
function normalizeGpuVendor(value?: string | null) {
    if (!value) {
        return null
    }

    const normalized = value.trim().toLowerCase()
    if (normalized === 'nvidia') return 'nvidia'
    if (normalized === 'amd') return 'amd'
    return normalized
}

export function isSupportedGpuVendor(value?: string | null) {
    const normalized = normalizeGpuVendor(value)
    return normalized === 'nvidia' || normalized === 'amd'
}

export function requiresRelayForLocalHardware(status: Status, setupState?: { gpu: { vendor: string } | null } | null) {
    const vendor = normalizeGpuVendor(status.gpuVendor) ?? normalizeGpuVendor(setupState?.gpu?.vendor)
    return Boolean(vendor && !isSupportedGpuVendor(vendor))
}

/**
 * Determines whether any hardware detection signals are present in the provided status.
 *
 * @param status - The status object to inspect for GPU/encoder detection signals.
 * @returns `true` if at least one GPU or encoder detection field (e.g., GPU name, vendor, encoder backend, selected executor, or any of the AI/framegen/HDR/RIFE availability flags) is present or set (not `null`), `false` otherwise.
 */
function hasHardwareDetectionSignal(status: Status) {
    return (
        Boolean(status.gpuName) ||
        Boolean(status.gpuVendor) ||
        Boolean(status.encoderBackend) ||
        Boolean(status.selectedExecutor) ||
        status.nvidiaAiAvailable !== null ||
        status.amdAiAvailable !== null ||
        status.encoderHasFramegen !== null ||
        status.encoderHasTruehdr !== null ||
        status.encoderHasRife !== null
    )
}

/**
 * Builds a SettingsUiFeatureSupport object representing availability and disabled state for a UI feature.
 *
 * @param support - The canonical support kind for the feature (`'native' | 'fallback' | 'unavailable'`).
 * @param lockState - The UI lock state (`'locked' | 'editable'`) that can disable the feature when locked.
 * @param sourceLabel - Optional label describing the detected source of the support (or `null` when none).
 * @param reason - Optional human-readable reason explaining availability or lack thereof (or `null`).
 * @returns A feature support state containing `support`, `lockState`, `available` (`true` when `support !== 'unavailable'`), `disabled` (`true` when `lockState === 'locked'` or not available), and the provided `sourceLabel` and `reason`.
 */
function createFeatureSupportState({
    support,
    lockState,
    sourceLabel,
    reason,
}: {
    support: SettingsUiSupportKind
    lockState: SettingsUiLockState
    sourceLabel: string | null
    reason: string | null
}): SettingsUiFeatureSupport {
    const available = support !== 'unavailable'
    return {
        support,
        lockState,
        available,
        disabled: lockState === 'locked' || !available,
        sourceLabel,
        reason,
    }
}

/**
 * Resolve which processing path (renderer) should be considered active based on settings and runtime status.
 *
 * @param settings - Current user settings that influence executor preference and quality decisions
 * @param status - Runtime status reporting the selected executor, GPU vendor, and capability flags
 * @param detectionPending - True when hardware/renderer detection is still in progress and no resolved path is reported
 * @returns The resolved UI path object containing `kind`, `label`, and a human-readable `reason` explaining the resolution
 */
function deriveResolvedPath(settings: Settings, status: Status, detectionPending: boolean): SettingsUiResolvedPath {
    const activeEngine = normalizeActiveProcessingEngine(status.selectedExecutor)
    const gpuVendor = normalizeGpuVendor(status.gpuVendor)
    const amdNativeLikely = gpuVendor === 'amd' && status.amdAiAvailable === true && (settings.executorPreference === 'amdAi' || activeEngine === 'universal')

    if (activeEngine === 'nvidiaAi') {
        return {
            kind: 'nvidiaNative',
            label: 'NVIDIA® Native',
            reason: 'Resolved through the NVIDIA® specialized runtime path.',
        }
    }

    if (activeEngine === 'amdAi') {
        return {
            kind: 'amdNative',
            label: 'AMD® Native',
            reason: 'Resolved through the AMD® runtime path.',
        }
    }

    if (amdNativeLikely) {
        return {
            kind: 'amdNative',
            label: 'AMD® Native / Universal Graph',
            reason: 'AMD® acceleration is available even though the graph-level executor reports Universal.',
        }
    }

    if (activeEngine === 'universal') {
        return {
            kind: 'universal',
            label: 'Universal',
            reason: 'Resolved through the broad compatibility path.',
        }
    }

    if (activeEngine === 'cpu') {
        return {
            kind: 'cpu',
            label: 'CPU Fallback',
            reason: 'GPU acceleration is unavailable, so processing falls back to local CPU work.',
        }
    }

    if (detectionPending) {
        return {
            kind: 'pending',
            label: 'Detecting Hardware',
            reason: 'The renderer has not reported a resolved path yet.',
        }
    }

    return {
        kind: 'universal',
        label: 'Universal',
        reason: 'No specialized path was reported, so the portable path is assumed.',
    }
}

/**
 * Determine the availability state and human-readable reason for a renderer preference option.
 *
 * @param option - The renderer preference being evaluated (e.g., `auto`, `nvidiaAi`, `amdAi`, `universal`).
 * @param status - Current system/status signals used to decide native availability (e.g., `nvidiaAiAvailable`, `amdAiAvailable`).
 * @param resolvedPath - The computed resolved renderer path that influences `auto` and `universal` outcomes.
 * @param detectionPending - `true` when hardware detection is still pending and should prevent fallback decisions.
 * @returns An object with `availability` set to `native`, `fallback`, or `unavailable`, and a `reason` string explaining the decision.
 */
function getRendererOptionAvailability({
    option,
    status,
    resolvedPath,
    detectionPending,
}: {
    option: ExecutorPreference
    status: Status
    resolvedPath: SettingsUiResolvedPath
    detectionPending: boolean
}) {
    if (option === 'nvidiaAi') {
        if (status.nvidiaAiAvailable === true) {
            return {
                availability: 'native' as const,
                reason: 'Native NVIDIA® processing is available.',
            }
        }

        if (!detectionPending) {
            return {
                availability: 'fallback' as const,
                reason: 'Unavailable natively here; the backend falls back to Universal compatibility.',
            }
        }
    }

    if (option === 'amdAi') {
        if (status.amdAiAvailable === true) {
            return {
                availability: 'native' as const,
                reason: 'Native AMD® processing is available.',
            }
        }

        if (!detectionPending) {
            return {
                availability: 'fallback' as const,
                reason: 'Unavailable natively here; the backend falls back to the portable path.',
            }
        }
    }

    if (option === 'universal') {
        if (resolvedPath.kind === 'pending') {
            return {
                availability: 'unavailable' as const,
                reason: 'Awaiting hardware detection.',
            }
        }

        return {
            availability: 'fallback' as const,
            reason: 'Portable compatibility path.',
        }
    }

    if (option === 'auto') {
        if (resolvedPath.kind === 'pending') {
            return {
                availability: 'unavailable' as const,
                reason: 'Awaiting hardware detection.',
            }
        }

        return {
            availability: resolvedPath.kind === 'nvidiaNative' || resolvedPath.kind === 'amdNative' ? ('native' as const) : ('fallback' as const),
            reason: resolvedPath.reason,
        }
    }

    return {
        availability: 'unavailable' as const,
        reason: 'Not available on this device.',
    }
}

/**
 * Builds a renderer preference option for the settings UI.
 *
 * Determines availability, selectable/disabled state, and reason based on the provided
 * system status, resolved renderer path, detection progress, and lock state.
 *
 * @param status - Current normalized system/status information used to evaluate availability
 * @param resolvedPath - Resolved renderer path and kind used to prefer native or fallback support
 * @param detectionPending - `true` when hardware detection is still pending and availability may be provisional
 * @param lockState - UI lock state (`'locked'` or `'editable'`) influencing disabled/selected behavior
 * @param locked - Explicit boolean indicating whether the overall settings UI is locked
 * @returns An option object containing `value`, `label`, and `description`, plus:
 * - `availability`: `'native' | 'fallback' | 'unavailable'` describing support level
 * - `lockState`: the provided lock state
 * - `selectable`: `true` when the option can be selected
 * - `disabled`: `true` when the option should be disabled
 * - `reason`: human-readable reason when availability is not `native`
 */
function createRendererPreferenceOption({
    value,
    label,
    description,
    status,
    resolvedPath,
    detectionPending,
    lockState,
    locked,
}: {
    value: ExecutorPreference
    label: string
    description: string
    status: Status
    resolvedPath: SettingsUiResolvedPath
    detectionPending: boolean
    lockState: SettingsUiLockState
    locked: boolean
}) {
    const option = getRendererOptionAvailability({
        option: value,
        status,
        resolvedPath,
        detectionPending,
    })

    return {
        value,
        label,
        description,
        availability: option.availability,
        lockState,
        selectable: option.availability !== 'unavailable' && !locked,
        disabled: option.availability === 'unavailable' || locked,
        reason: option.reason,
    }
}

/**
 * Produce the UI feature support state for upscaling based on the resolved execution path, lock state, and detection status.
 *
 * @param resolvedPath - The resolved renderer path indicating native/vendor, universal, cpu, or pending resolution
 * @param lockState - The current UI lock state that may disable features
 * @param detectionPending - Whether hardware detection is still in progress
 * @returns A SettingsUiFeatureSupport describing upscale availability: `native` for vendor-native paths, `fallback` for universal/cpu paths, or `unavailable` when no path is derived (reason explains detection state); includes `sourceLabel` and `reason`
 */
function getUpscaleSupport(resolvedPath: SettingsUiResolvedPath, lockState: SettingsUiLockState, detectionPending: boolean) {
    if (resolvedPath.kind === 'nvidiaNative') {
        return createFeatureSupportState({
            support: 'native',
            lockState,
            sourceLabel: 'NVIDIA® native upscale',
            reason: 'Uses the NVIDIA® specialized upscale path.',
        })
    }

    if (resolvedPath.kind === 'amdNative') {
        return createFeatureSupportState({
            support: 'native',
            lockState,
            sourceLabel: 'AMD® native upscale',
            reason: 'Uses the AMD® runtime upscale path when available.',
        })
    }

    if (resolvedPath.kind === 'universal' || resolvedPath.kind === 'cpu') {
        return createFeatureSupportState({
            support: 'fallback',
            lockState,
            sourceLabel: resolvedPath.kind === 'cpu' ? 'Portable local upscale' : 'Universal / shader upscale',
            reason: 'Uses the portable compatibility path instead of a native accelerator.',
        })
    }

    return createFeatureSupportState({
        support: 'unavailable',
        lockState,
        sourceLabel: null,
        reason: detectionPending ? 'Awaiting hardware detection.' : 'No upscale path was derived.',
    })
}

/**
 * Derives the UI feature support state for frame generation (framegen).
 *
 * @param status - Current system status containing encoder capability flags
 * @param resolvedPath - Resolved renderer path used to determine native support
 * @param lockState - Current UI lock state
 * @param detectionPending - Whether hardware detection is still pending
 * @returns A SettingsUiFeatureSupport describing availability (`native`, `fallback`, or `unavailable`), whether the feature is disabled, an optional `sourceLabel`, and a human-readable `reason`
 */
function getFramegenSupport(status: Status, resolvedPath: SettingsUiResolvedPath, lockState: SettingsUiLockState, detectionPending: boolean) {
    if (resolvedPath.kind === 'nvidiaNative' && status.encoderHasFramegen === true) {
        return createFeatureSupportState({
            support: 'native',
            lockState,
            sourceLabel: 'NVIDIA® FRUC',
            reason: 'Boost frames with NVIDIA® Optical Flow-based frame generation.',
        })
    }

    if (status.encoderHasRife === true) {
        return createFeatureSupportState({
            support: 'fallback',
            lockState,
            sourceLabel: 'RIFE interpolation',
            reason: 'Boost frames with RIFE AI-powered frame interpolation.',
        })
    }

    return createFeatureSupportState({
        support: 'unavailable',
        lockState,
        sourceLabel: null,
        reason: detectionPending ? 'Awaiting hardware detection.' : 'No native or fallback interpolation path was reported.',
    })
}

/**
 * Derives the UI feature support state for HDR based on detected hardware and resolved execution path.
 *
 * @param status - Current status containing encoder capability flags (e.g., `encoderHasTruehdr`)
 * @param resolvedPath - Derived executor/path resolution used to determine native vs fallback support
 * @param lockState - UI lock state that may disable the feature
 * @param detectionPending - Whether hardware detection is still in progress
 * @returns A SettingsUiFeatureSupport object: `native` when NVIDIA TrueHDR is available, `fallback` when a non-pending path can provide HDR mapping, or `unavailable` with a reason when detection is pending or no path is available
 */
function getHdrSupport(status: Status, resolvedPath: SettingsUiResolvedPath, lockState: SettingsUiLockState, detectionPending: boolean) {
    if (resolvedPath.kind === 'nvidiaNative' && status.encoderHasTruehdr === true) {
        return createFeatureSupportState({
            support: 'native',
            lockState,
            sourceLabel: 'NVIDIA® TrueHDR',
            reason: 'Convert SDR content to HDR with NVIDIA® RTX™ HDR.',
        })
    }

    if (resolvedPath.kind !== 'pending') {
        return createFeatureSupportState({
            support: 'fallback',
            lockState,
            sourceLabel: 'REFEREE HDR mapping',
            reason: 'Convert SDR content to HDR with REFEREE HDR mapping.',
        })
    }

    return createFeatureSupportState({
        support: 'unavailable',
        lockState,
        sourceLabel: null,
        reason: detectionPending ? 'Awaiting hardware detection.' : 'No HDR path was derived.',
    })
}

/**
 * Derives UI-facing capability and availability metadata for settings and features.
 *
 * Builds a complete SettingsUiCapabilities object describing renderer preference options,
 * the resolved execution path, per-feature support (upscale, frame generation, HDR),
 * quality limits, and advanced encoding metadata based on the current settings, detected
 * status, and whether UI settings are locked.
 *
 * @param settings - Current user settings used to derive quality limits and preference hints
 * @param status - Current system/status information used to determine hardware and encoder capabilities
 * @param locked - Whether UI settings are locked, which influences availability and disabled state
 * @returns The resolved SettingsUiCapabilities describing renderer options, feature support, and related UI state
 */
export function deriveSettingsUiCapabilities(settings: Settings, status: Status, locked: boolean): SettingsUiCapabilities {
    const lockState = getSettingsUiLockState(locked)
    const detectionPending = !hasHardwareDetectionSignal(status)
    const resolvedPath = deriveResolvedPath(settings, status, detectionPending)

    return {
        lockState,
        rendererPreferenceOptions: [
            createRendererPreferenceOption({
                value: 'auto',
                label: 'Recommended',
                description: 'Chooses the best available path for this device.',
                status,
                resolvedPath,
                detectionPending,
                lockState,
                locked,
            }),
            createRendererPreferenceOption({
                value: 'nvidiaAi',
                label: 'NVIDIA® AI',
                description: 'Prefers native NVIDIA® acceleration when supported.',
                status,
                resolvedPath,
                detectionPending,
                lockState,
                locked,
            }),
            createRendererPreferenceOption({
                value: 'amdAi',
                label: 'AMD® AI',
                description: 'Prefers native AMD® acceleration when supported.',
                status,
                resolvedPath,
                detectionPending,
                lockState,
                locked,
            }),
            createRendererPreferenceOption({
                value: 'universal',
                label: 'Universal',
                description: 'Uses the broad compatibility path.',
                status,
                resolvedPath,
                detectionPending,
                lockState,
                locked,
            }),
        ],
        resolvedPath,
        features: {
            upscale: getUpscaleSupport(resolvedPath, lockState, detectionPending),
            framegen: getFramegenSupport(status, resolvedPath, lockState, detectionPending),
            hdr: getHdrSupport(status, resolvedPath, lockState, detectionPending),
        },
        quality: {
            max: getMaxQualityForResolution(settings.resolution),
            lockState,
        },
        advancedEncoding: {
            lockState,
            profileShape: resolvedPath.kind === 'nvidiaNative' ? 'nvenc' : 'generic',
            reason:
                resolvedPath.kind === 'nvidiaNative'
                    ? 'The current advanced controls still reflect NVENC-style tuning.'
                    : 'Advanced controls should be normalized before they drive non-NVIDIA paths.',
        },
    }
}

export function formatResolutionLabel(resolution?: string | null) {
    if (!resolution) {
        return null
    }

    const resolutionMatch = resolution.match(/(\d{3,5})\s*x\s*(\d{3,5})/i)
    if (!resolutionMatch) {
        return resolution.toUpperCase()
    }

    const height = Number(resolutionMatch[2])
    if (!Number.isFinite(height)) {
        return resolution.toUpperCase()
    }

    if (height >= 2160) return '4K'
    if (height >= 1440) return '1440P'
    if (height >= 1080) return '1080P'
    if (height >= 720) return '720P'
    if (height >= 480) return '480P'
    if (height >= 360) return '360P'
    return `${height}P`
}

export function formatFrameRate(value?: number | null) {
    const normalizedValue = Number(value)
    if (!Number.isFinite(normalizedValue) || normalizedValue <= 0) {
        return null
    }

    const roundedValue = Math.round(normalizedValue * 10) / 10
    return Number.isInteger(roundedValue) ? `${roundedValue} FPS` : `${roundedValue.toFixed(1)} FPS`
}

export function formatGpuUtilization(value?: number | null) {
    const normalizedValue = Number(value)
    if (!Number.isFinite(normalizedValue) || normalizedValue < 0) {
        return null
    }

    return `${Math.round(normalizedValue)}% Load`
}

/**
 * Format a GPU vendor identifier into a user-facing brand string.
 *
 * @param value - Vendor name (e.g., `"nvidia"`, `"amd"`); may be `null` or `undefined`
 * @returns `'NVIDIA®'` for NVIDIA, `'AMD®'` for AMD, `'Unknown'` when `value` is missing, otherwise the normalized value in uppercase
 */
export function formatGpuVendor(value?: string | null) {
    if (!value) {
        return 'Unknown'
    }

    const normalized = value.trim().toLowerCase()
    if (normalized === 'nvidia') return 'NVIDIA®'
    if (normalized === 'amd') return 'AMD®'
    return normalized.toUpperCase()
}

export function formatEncoderBackend(value?: string | null) {
    if (!value) {
        return 'Not Detected'
    }

    const normalized = value.trim().toLowerCase()
    if (normalized === 'nvenc') return 'NVENC'
    if (normalized === 'vceenc') return 'VCE'
    return normalized.toUpperCase()
}

/**
 * Formats an executor identifier into a human-readable label.
 *
 * @param value - Executor identifier (e.g., `"nvidia"`, `"universal"`, `"cpu"`) or null/undefined
 * @returns `'NVIDIA® AI'`, `'Universal'`, or `'CPU'` for recognized identifiers; `'Not Selected'` when `value` is missing; otherwise returns the original `value`
 */
export function formatExecutorKind(value?: string | null) {
    if (!value) {
        return 'Not Selected'
    }

    const normalized = value
        .trim()
        .replace(/[\s_-]/g, '')
        .replace(/executor$/i, '')
        .toLowerCase()
    if (normalized === 'nvidiaspecialized' || normalized === 'nvidia') return 'NVIDIA® AI'
    if (normalized === 'universal') return 'Universal'
    if (normalized === 'cpu') return 'CPU'
    return value
}

export function formatSourceContentKind(value?: string | null) {
    if (!value) {
        return null
    }

    const normalized = value
        .trim()
        .replace(/[\s_-]/g, '')
        .toLowerCase()
    if (normalized === 'animated') return 'Animated'
    if (normalized === 'liveaction') return 'Live Action'
    if (normalized === 'unknown') return 'Unknown'
    return value
}

/**
 * Formats an upscaler identifier into a human-friendly label.
 *
 * @param value - An optional upscaler identifier or name to normalize
 * @returns `null` if `value` is missing; otherwise a display label, mapping:
 * - `anime4k2x` → `Anime4K2x`
 * - `artcnn` → `ArtCNN`
 * - `libplacebo` → `Placebo`
 * - `nvidiaai` → `NVIDIA® AI`
 * - otherwise returns the original `value`
 */
export function formatUpscalerLabel(value?: string | null) {
    if (!value) {
        return null
    }

    const normalized = value
        .trim()
        .replace(/[\s_-]/g, '')
        .toLowerCase()
    if (normalized === 'anime4k2x') return 'Anime4K2x'
    if (normalized === 'artcnn') return 'ArtCNN'
    if (normalized === 'libplacebo') return 'Placebo'
    if (normalized === 'nvidiaai') return 'NVIDIA® AI'
    return value
}

export function normalizeSessionTextValue(value?: string | null) {
    return typeof value === 'string' ? value.trim() : ''
}

export function getActiveSession(status?: Status | null): Session | null {
    const sessionCount = getSessionCount(status)
    if (!status || sessionCount <= 0) {
        return null
    }

    const primarySession = status.primarySession
    if (primarySession && (primarySession.id || primarySession.appName || primarySession.streamTitle)) {
        return primarySession
    }

    return status.sessions.find((session) => Boolean(session && typeof session === 'object')) ?? null
}

export function isSessionLive(session?: Session | null) {
    return Boolean(session?.startupComplete)
}

export function getSessionCount(status?: Status | null) {
    if (!status) {
        return 0
    }

    return Math.max(Number(status.activeSessions) || 0, status.sessions.length, status.primarySession ? 1 : 0)
}

export function getLiveSessionCount(status?: Status | null) {
    const sessions = status?.sessions ?? []
    const primarySession = status?.primarySession
    const liveCount = sessions.reduce((count, session) => count + (isSessionLive(session) ? 1 : 0), 0)

    if (liveCount > 0) {
        return liveCount
    }

    return isSessionLive(primarySession) ? 1 : 0
}

export function hasLiveSession(status?: Status | null) {
    return getLiveSessionCount(status) > 0
}

export function hasPendingSession(status?: Status | null) {
    return getSessionCount(status) > 0 && !hasLiveSession(status)
}

export function getStandbyFrameRateLabel(settings: Settings) {
    return settings.framegen ? 'Up to 60 FPS' : 'Native'
}

export function getFrameRateTone(frameRateValue: number | null | undefined, hasActiveStream: boolean) {
    const normalizedValue = Number(frameRateValue)
    if (!Number.isFinite(normalizedValue) || normalizedValue <= 0) {
        return hasActiveStream ? 'accent' : 'idle'
    }

    return normalizedValue < 30 ? 'alert' : 'accent'
}

export function getGpuTone(gpuUtilizationValue: number | null | undefined, hasActiveStream: boolean) {
    const normalizedValue = Number(gpuUtilizationValue)
    if (!Number.isFinite(normalizedValue) || normalizedValue < 0) {
        return hasActiveStream ? 'accent' : 'idle'
    }

    return normalizedValue >= 90 ? 'alert' : 'accent'
}

export function getActiveStreamBody(session: Session | null, fallbackResolution: string) {
    const appName = normalizeSessionTextValue(session?.appName)
    const sourceLabel = formatResolutionLabel(session?.sourceResolution) || 'AUTO'
    const targetLabel = formatResolutionLabel(session?.outputResolution || fallbackResolution) || 'TARGET'

    if (appName) {
        return `REFEREE is actively upscaling the ${sourceLabel} source stream for ${appName} to ${targetLabel}.`
    }

    return `REFEREE is actively upscaling the ${sourceLabel} source stream to ${targetLabel}.`
}
