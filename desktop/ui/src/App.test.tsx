import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import App from './App'
import { DEFAULT_STATUS, type ConsentRequest, type Session, type Status } from '@/lib/types'
import { makeMockApi } from '@/test/mocks'

const rendererApiState = vi.hoisted(() => ({
    currentApi: null as ReturnType<typeof makeMockApi>['api'] | null,
}))

vi.mock('@/hooks/useRendererApi', () => ({
    useRendererApi: () => {
        if (!rendererApiState.currentApi) {
            throw new Error('Mock renderer API was not initialized for this test')
        }

        return rendererApiState.currentApi
    },
}))

vi.mock('@/dev/renderer-debug', () => ({
    useRendererDebugPanel: ({ currentView, status, setupState }: { currentView: string; status: Status; setupState: unknown }) => ({
        effectiveView: currentView,
        effectiveStatus: status,
        effectiveSetupState: setupState,
        settingsPanel: null,
        isSimulating: false,
    }),
}))

function makeSession(overrides: Partial<Session> = {}): Session {
    return {
        id: 'session-1',
        sourceUrl: null,
        outputUrl: 'http://out.m3u8',
        appName: null,
        streamTitle: null,
        sourceContentKind: null,
        upscaler: null,
        sourceResolution: null,
        outputResolution: null,
        sourceFps: null,
        targetFps: null,
        framegenEnabled: false,
        hdrEnabled: false,
        qualityLevel: null,
        executor: null,
        encoderBackend: null,
        startupComplete: true,
        retryingStartup: false,
        ...overrides,
    }
}

function makeLiveStatus(sessionOverrides: Partial<Session> = {}): Partial<Status> {
    const session = makeSession(sessionOverrides)

    return {
        ...DEFAULT_STATUS,
        activeSessions: 1,
        sessions: [session],
        primarySession: session,
    }
}

function makeConsentRequest(overrides: Partial<ConsentRequest> = {}): ConsentRequest {
    return {
        nonce: 'consent-1',
        origin: 'referee-relay://peer-1',
        appName: 'REFEREE Relay',
        ...overrides,
    }
}

describe('App - external player auto-open', () => {
    beforeEach(() => {
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    })

    afterEach(() => {
        rendererApiState.currentApi = null
        vi.unstubAllGlobals()
        vi.clearAllMocks()
        vi.useRealTimers()
        delete (window as any).__TAURI_INTERNALS__
        window.history.replaceState(null, '', '/')
    })

    it('launches the selected external player once when a stream becomes live', async () => {
        const mockApi = makeMockApi()
        rendererApiState.currentApi = mockApi.api
        vi.mocked(mockApi.api.getInitialSettings).mockResolvedValue({
            player: {
                enabled: true,
                selectedPlayer: 'vlc',
                customPath: null,
            },
        })

        render(<App />)

        await waitFor(() => {
            expect(mockApi.api.getInitialSettings).toHaveBeenCalled()
        })

        act(() => {
            mockApi.emit.statusUpdate(makeLiveStatus({ outputUrl: 'http://output.m3u8' }))
        })

        await waitFor(() => {
            expect(mockApi.api.launchPlayer).toHaveBeenCalledWith('http://output.m3u8')
        })

        act(() => {
            mockApi.emit.statusUpdate(makeLiveStatus({ outputUrl: 'http://output.m3u8' }))
        })

        await waitFor(() => {
            expect(mockApi.api.launchPlayer).toHaveBeenCalledTimes(1)
        })
    })

    it('launches again for a new stream after the previous one ends', async () => {
        const mockApi = makeMockApi()
        rendererApiState.currentApi = mockApi.api
        vi.mocked(mockApi.api.getInitialSettings).mockResolvedValue({
            player: {
                enabled: true,
                selectedPlayer: 'mpv',
                customPath: null,
            },
        })

        render(<App />)

        await waitFor(() => {
            expect(mockApi.api.getInitialSettings).toHaveBeenCalled()
        })

        act(() => {
            mockApi.emit.statusUpdate(makeLiveStatus({ id: 'session-1', outputUrl: 'http://output-1.m3u8' }))
        })

        await waitFor(() => {
            expect(mockApi.api.launchPlayer).toHaveBeenCalledWith('http://output-1.m3u8')
        })

        act(() => {
            mockApi.emit.statusUpdate(DEFAULT_STATUS)
        })

        act(() => {
            mockApi.emit.statusUpdate(makeLiveStatus({ id: 'session-2', outputUrl: 'http://output-2.m3u8' }))
        })

        await waitFor(() => {
            expect(mockApi.api.launchPlayer).toHaveBeenCalledTimes(2)
        })

        expect(mockApi.api.launchPlayer).toHaveBeenNthCalledWith(2, 'http://output-2.m3u8')
    })
})

describe('App - consent request fallback', () => {
    beforeEach(() => {
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    })

    afterEach(() => {
        rendererApiState.currentApi = null
        vi.unstubAllGlobals()
        vi.clearAllMocks()
        vi.useRealTimers()
        delete (window as any).__TAURI_INTERNALS__
        window.history.replaceState(null, '', '/')
    })

    it('polls pending consent so a missed Tauri event appears without clicking the window', async () => {
        vi.useFakeTimers()
        Object.defineProperty(window, '__TAURI_INTERNALS__', {
            configurable: true,
            value: {},
        })

        const mockApi = makeMockApi()
        let pendingConsent: ConsentRequest | null = null
        vi.mocked(mockApi.api.getPendingConsent).mockImplementation(() => Promise.resolve(pendingConsent))
        rendererApiState.currentApi = mockApi.api

        render(<App />)

        await act(async () => {
            await Promise.resolve()
        })

        pendingConsent = makeConsentRequest()

        await act(async () => {
            vi.advanceTimersByTime(1000)
            await Promise.resolve()
        })

        expect(screen.getByText('Access Request')).not.toBeNull()
    })
})

describe('App - unsupported hardware Relay prompt', () => {
    beforeEach(() => {
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
    })

    afterEach(() => {
        rendererApiState.currentApi = null
        vi.unstubAllGlobals()
        vi.clearAllMocks()
        vi.useRealTimers()
        delete (window as any).__TAURI_INTERNALS__
        window.history.replaceState(null, '', '/')
    })

    it('opens the Relay settings card from the standby prompt', async () => {
        const mockApi = makeMockApi()
        rendererApiState.currentApi = mockApi.api

        render(<App />)

        await waitFor(() => {
            expect(mockApi.api.getInitialSettings).toHaveBeenCalled()
        })

        act(() => {
            mockApi.emit.statusUpdate({ gpuVendor: 'intel' })
        })

        await screen.findByText('Unsupported Hardware')

        fireEvent.click(document.querySelector('#status-view .stream-message-outline-action')!)

        await waitFor(() => {
            expect(document.getElementById('app-shell')?.getAttribute('data-view')).toBe('settings')
        })

        expect(screen.getByText('Route new streams locally or through Relay.')).not.toBeNull()
    })
})
