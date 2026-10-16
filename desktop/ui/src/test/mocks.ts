import { vi } from 'vitest'
import {
    DEFAULT_RELAY_LINK_STATUS,
    DEFAULT_STATUS,
    type AppView,
    type ConsentRequest,
    type RendererApi,
    type SetupGpuInfo,
    type SetupProgress,
    type Settings,
    type Status,
    type UpdateStatus,
} from '@/lib/types'

// ─── Generic event channel ────────────────────────────────────────────────────

function makeChannel<T>() {
    let cb: ((data: T) => void) | null = null
    return {
        listen: (handler: (data: T) => void) => {
            cb = handler
            return () => { cb = null }
        },
        emit: (data: T) => { cb?.(data) },
    }
}

function makeVoidChannel() {
    let cb: (() => void) | null = null
    return {
        listen: (handler: () => void) => {
            cb = handler
            return () => { cb = null }
        },
        emit: () => { cb?.() },
    }
}

// ─── Mock API factory ─────────────────────────────────────────────────────────

/** Checked a minute ago; no newer release. */
export const UP_TO_DATE: UpdateStatus = {
    currentVersion: 'test',
    phase: 'idle',
    latestVersion: null,
    releaseUrl: null,
    progress: null,
    error: null,
    lastChecked: Date.now() - 60_000,
    unsupportedReason: null,
}

/**
 * Creates a mock RendererApi suitable for hook tests. All `on*` subscriptions
 * store their callback and return a real unsubscribe function. An `emit` object
 * is returned alongside the api so tests can fire events directly.
 */
export function makeMockApi() {
    const navigateView = makeChannel<AppView>()
    const statusUpdate = makeChannel<Partial<Status>>()
    const settingsSync = makeChannel<Partial<Settings>>()
    const bootSettingSync = makeChannel<boolean>()
    const setupGpuDetected = makeChannel<SetupGpuInfo>()
    const setupProgress = makeChannel<SetupProgress>()
    const setupComplete = makeVoidChannel()
    const setupError = makeChannel<{ message?: string }>()
    const updateStatus = makeChannel<UpdateStatus>()
    const consentRequest = makeChannel<ConsentRequest>()

    const api: RendererApi = {
        isElectron: false,
        ready: vi.fn().mockResolvedValue({ setupNeeded: false, gpuVendor: 'unknown', gpuName: null }),
        getSetupState: vi.fn().mockResolvedValue({ setupNeeded: false, gpuVendor: 'unknown', gpuName: null }),
        getInitialSettings: vi.fn().mockResolvedValue({}),
        saveSettings: vi.fn(),
        saveStreamSettings: vi.fn().mockResolvedValue(undefined),
        getBootSetting: vi.fn().mockResolvedValue(false),
        setBootSetting: vi.fn(),
        getAppVersion: vi.fn().mockResolvedValue('test'),
        getUpdateStatus: vi.fn().mockResolvedValue(UP_TO_DATE),
        checkForUpdate: vi.fn().mockResolvedValue(UP_TO_DATE),
        installUpdate: vi.fn().mockResolvedValue(undefined),
        stopStream: vi.fn().mockResolvedValue(DEFAULT_STATUS),
        retrySetup: vi.fn().mockResolvedValue(undefined),
        detectPlayers: vi.fn().mockResolvedValue([
            {
                id: 'builtin',
                name: 'REFEREE Built-in Player',
                path: null,
                installed: true,
            },
            {
                id: 'custom',
                name: 'Custom Player',
                path: null,
                installed: false,
            },
        ]),
        launchPlayer: vi.fn().mockResolvedValue(undefined),
        openExternal: vi.fn(),
        openGithub: vi.fn(),
        minimizeWindow: vi.fn(),
        closeWindow: vi.fn(),
        togglePin: vi.fn(),
        onNavigateView: navigateView.listen,
        onStatusUpdate: statusUpdate.listen,
        onSettingsSync: settingsSync.listen,
        onBootSettingSync: bootSettingSync.listen,
        onSetupGpuDetected: setupGpuDetected.listen,
        onSetupProgress: setupProgress.listen,
        onSetupComplete: setupComplete.listen,
        onSetupError: setupError.listen,
        onUpdateStatus: updateStatus.listen,
        respondToConsent: vi.fn(),
        getPendingConsent: vi.fn().mockResolvedValue(null),
        onConsentRequest: consentRequest.listen,
        getApprovedOrigins: vi.fn().mockResolvedValue([]),
        revokeApprovedOrigin: vi.fn().mockResolvedValue(undefined),
        discoverLanPeers: vi.fn().mockResolvedValue([]),
        getRelayLinkStatus: vi.fn().mockResolvedValue(DEFAULT_RELAY_LINK_STATUS),
        linkRelayPeer: vi.fn().mockResolvedValue(undefined),
        unlinkRelayPeer: vi.fn().mockResolvedValue(undefined),
        checkEasyProxy: vi.fn().mockResolvedValue({ version: null }),
        getEasyProxyStatus: vi.fn().mockResolvedValue({
            installState: 'missing',
            installing: false,
            progress: null,
            error: null,
            running: false,
            starting: false,
            version: '2.13.35',
            commit: '1d4ad76',
        }),
        installEasyProxy: vi.fn().mockResolvedValue(undefined),
        removeEasyProxy: vi.fn().mockResolvedValue(undefined),
        startDebugStream: vi.fn().mockResolvedValue(undefined),
    }

    return {
        api,
        emit: {
            navigateView: navigateView.emit,
            statusUpdate: statusUpdate.emit,
            settingsSync: settingsSync.emit,
            bootSettingSync: bootSettingSync.emit,
            setupGpuDetected: setupGpuDetected.emit,
            setupProgress: setupProgress.emit,
            setupComplete: setupComplete.emit,
            setupError: setupError.emit,
            updateStatus: updateStatus.emit,
            consentRequest: consentRequest.emit,
        },
    }
}
