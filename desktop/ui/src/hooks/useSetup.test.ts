import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useSetup } from './useSetup'
import { makeMockApi } from '@/test/mocks'
import { DEFAULT_SETUP_STATE } from '@/lib/types'

// MIN_PHASE_MS is 750 — the minimum time each distinct setup phase is shown.
const MIN_PHASE_MS = 750

/** Advance fake timers by MIN_PHASE_MS so queued phase transitions are applied. */
function advancePhase() {
    act(() => { vi.advanceTimersByTime(MIN_PHASE_MS) })
}

// ─── Initial state ────────────────────────────────────────────────────────────

describe('useSetup — initial state', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('starts with DEFAULT_SETUP_STATE', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        expect(result.current.setupState).toEqual(DEFAULT_SETUP_STATE)
        expect(setView).not.toHaveBeenCalled()
    })
})

// ─── handleSetupResponse ──────────────────────────────────────────────────────

describe('useSetup — handleSetupResponse', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('does nothing when response is null or undefined', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => { result.current.handleSetupResponse(null) })
        act(() => { result.current.handleSetupResponse(undefined) })
        expect(setView).not.toHaveBeenCalled()
        expect(result.current.setupState).toEqual(DEFAULT_SETUP_STATE)
    })

    it('navigates to "status" when setupNeeded is false and vendor is not unknown', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: false,
                gpuVendor: 'nvidia',
                gpuName: 'RTX 4080',
            })
        })
        expect(setView).toHaveBeenCalledWith('status')
    })

    it('navigates to "status" when setupNeeded is false and vendor is unknown', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: false,
                gpuVendor: 'unknown',
                gpuName: null,
            })
        })
        expect(setView).toHaveBeenCalledWith('status')
    })

    it('treats unsupported local hardware as relay-ready instead of blocking setup', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: true,
                gpuVendor: 'unknown',
                gpuName: null,
            })
        })
        expect(setView).toHaveBeenCalledWith('status')
        advancePhase()
        expect(result.current.setupState.complete).toBe(true)
    })

    it('marks setup as complete after phase transition when setupNeeded is false', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: false,
                gpuVendor: 'nvidia',
                gpuName: 'RTX 4080',
            })
        })
        advancePhase()
        expect(result.current.setupState.complete).toBe(true)
    })

    it('navigates to "setup" and sets gpu when setupNeeded is true', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: true,
                gpuVendor: 'nvidia',
                gpuName: 'RTX 4080',
            })
        })
        expect(setView).toHaveBeenCalledWith('setup')
        advancePhase()
        expect(result.current.setupState.gpu?.vendor).toBe('nvidia')
    })

    it('remains in setup view when setupInProgress is true', () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => {
            result.current.handleSetupResponse({
                setupNeeded: false,
                gpuVendor: 'nvidia',
                gpuName: null,
                setupInProgress: true,
            })
        })
        expect(setView).toHaveBeenCalledWith('setup')
    })
})

// ─── Event subscriptions ──────────────────────────────────────────────────────

describe('useSetup — onSetupGpuDetected', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('calls setView("setup") immediately', () => {
        const { api, emit } = makeMockApi()
        const setView = vi.fn()
        renderHook(() => useSetup(api, setView))
        act(() => emit.setupGpuDetected({ vendor: 'nvidia', name: 'RTX 4080' }))
        expect(setView).toHaveBeenCalledWith('setup')
    })

    it('sets gpu on the displayed state after the phase transition', () => {
        const { api, emit } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        act(() => emit.setupGpuDetected({ vendor: 'amd', name: 'RX 7900' }))
        advancePhase()
        expect(result.current.setupState.gpu?.vendor).toBe('amd')
        expect(result.current.setupState.gpu?.name).toBe('RX 7900')
    })

    it('creates a vendor-specific fallback progress entry', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        act(() => emit.setupGpuDetected({ vendor: 'nvidia', name: 'RTX 4080' }))
        advancePhase()
        expect(result.current.setupState.progress?.detail).toContain('NVEncC')
    })
})

describe('useSetup — onSetupProgress', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('calls setView("setup")', () => {
        const { api, emit } = makeMockApi()
        const setView = vi.fn()
        renderHook(() => useSetup(api, setView))
        act(() => emit.setupProgress({ phase: 'encoder', percent: 30, detail: 'Downloading...' }))
        expect(setView).toHaveBeenCalledWith('setup')
    })

    it('updates progress after the phase transition', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        act(() => emit.setupProgress({ phase: 'encoder', percent: 50, detail: 'Half done' }))
        advancePhase()
        expect(result.current.setupState.progress?.percent).toBe(50)
    })

    it('marks setup complete when phase is "done"', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        // First advance to 'encoder' phase
        act(() => emit.setupProgress({ phase: 'encoder', percent: 0, detail: '' }))
        advancePhase()
        // Then send 'done'
        act(() => emit.setupProgress({ phase: 'done', percent: 100, detail: 'Done' }))
        advancePhase()
        expect(result.current.setupState.complete).toBe(true)
    })
})

describe('useSetup — onSetupComplete', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('sets complete to true after phase transition', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        act(() => emit.setupComplete())
        advancePhase()
        expect(result.current.setupState.complete).toBe(true)
        expect(result.current.setupState.error).toBeNull()
    })
})

describe('useSetup — onSetupError', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('calls setView("setup")', () => {
        const { api, emit } = makeMockApi()
        const setView = vi.fn()
        renderHook(() => useSetup(api, setView))
        act(() => emit.setupError({ message: 'Download failed' }))
        expect(setView).toHaveBeenCalledWith('setup')
    })

    it('sets the error message immediately (same phase — no throttle)', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        // Error does not change progress.phase → same-phase update → immediate
        act(() => emit.setupError({ message: 'Download failed' }))
        expect(result.current.setupState.error).toBe('Download failed')
    })

    it('falls back to "Setup failed" when error has no message', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        act(() => emit.setupError({}))
        expect(result.current.setupState.error).toBe('Setup failed')
    })

    it('sets complete to false on error', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        act(() => emit.setupError({ message: 'Error' }))
        expect(result.current.setupState.complete).toBe(false)
    })
})

// ─── retrySetup ───────────────────────────────────────────────────────────────

describe('useSetup — retrySetup', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers() })
    it('calls setView("setup")', async () => {
        const { api } = makeMockApi()
        const setView = vi.fn()
        const { result } = renderHook(() => useSetup(api, setView))
        await act(async () => { await result.current.retrySetup() })
        expect(setView).toHaveBeenCalledWith('setup')
    })

    it('calls api.retrySetup', async () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        await act(async () => { await result.current.retrySetup() })
        expect(api.retrySetup).toHaveBeenCalled()
    })

    it('sets error when api.retrySetup rejects', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.retrySetup).mockRejectedValue(new Error('retry failed'))
        // Prevent the shouldPollSetup effect from calling getSetupState and
        // overwriting the error state before we can assert on it.
        vi.mocked(api.getSetupState).mockRejectedValue(new Error())
        const { result } = renderHook(() => useSetup(api, vi.fn()))
        await act(async () => { await result.current.retrySetup() })
        // retrySetup changes phase null→'retry', which is throttled.
        // The rejection then sets error on the 'retry' phase state (replacing
        // it in the queue), so advancing the timer reveals the error.
        advancePhase()
        expect(result.current.setupState.error).toBe('retry failed')
    })
})
