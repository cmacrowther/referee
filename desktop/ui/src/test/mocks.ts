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
    const updateProgress = makeChannel<number>()
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
        checkForUpdate: vi.fn().mockResolvedValue({ currentVersion: 'test', hasUpdate: false }),
        downloadAndInstallUpdate: vi.fn().mockResolvedValue(undefined),
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
        onUpdateProgress: updateProgress.listen,
        respondToConsent: vi.fn(),
        getPendingConsent: vi.fn().mockResolvedValue(null),
        onConsentRequest: consentRequest.listen,
        getApprovedOrigins: vi.fn().mockResolvedValue([]),
        revokeApprovedOrigin: vi.fn().mockResolvedValue(undefined),
        discoverLanPeers: vi.fn().mockResolvedValue([]),
        getRelayLinkStatus: vi.fn().mockResolvedValue(DEFAULT_RELAY_LINK_STATUS),
        linkRelayPeer: vi.fn().mockResolvedValue(undefined),
        unlinkRelayPeer: vi.fn().mockResolvedValue(undefined),
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
            updateProgress: updateProgress.emit,
            consentRequest: consentRequest.emit,
        },
    }
}
