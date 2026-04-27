import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useStatus } from './useStatus'
import { makeMockApi } from '@/test/mocks'
import { DEFAULT_STATUS } from '@/lib/types'

// ─── Fetch mock helpers ───────────────────────────────────────────────────────

function mockFetchOk(body: object) {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
        ok: true,
        json: () => Promise.resolve(body),
    }))
}

function mockFetchError() {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('network error')))
}

function mockFetchNotOk() {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false }))
}

// ─── Initial state ────────────────────────────────────────────────────────────

describe('useStatus — initial state', () => {
    it('starts with DEFAULT_STATUS', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useStatus(api))
        expect(result.current.status).toEqual(DEFAULT_STATUS)
    })
})

// ─── onStatusUpdate subscription ─────────────────────────────────────────────

describe('useStatus — onStatusUpdate', () => {
    it('updates status when the event fires', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useStatus(api))
        act(() => emit.statusUpdate({ gpuReady: true, gpuName: 'RTX 4080', gpuVendor: 'nvidia' }))
        expect(result.current.status.gpuReady).toBe(true)
        expect(result.current.status.gpuName).toBe('RTX 4080')
    })

    it('normalises the incoming status payload', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useStatus(api))
        // snake_case keys should be normalised
        act(() => emit.statusUpdate({ gpu_name: 'RX 7900', gpu_vendor: 'amd' } as any))
        expect(result.current.status.gpuName).toBe('RX 7900')
        expect(result.current.status.gpuVendor).toBe('amd')
    })

    it('merges partial updates with DEFAULT_STATUS fields', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useStatus(api))
        act(() => emit.statusUpdate({ gpuReady: true }))
        // activeSessions should still be 0 (from DEFAULT_STATUS)
        expect(result.current.status.activeSessions).toBe(0)
    })
})

// ─── HTTP polling ─────────────────────────────────────────────────────────────

describe('useStatus — HTTP polling', () => {
    beforeEach(() => { vi.useFakeTimers() })
    afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals() })
    it('polls the status endpoint on mount and updates state', async () => {
        const { api } = makeMockApi()
        mockFetchOk({ gpuReady: true, gpuName: 'RTX 4080' })
        const { result } = renderHook(() => useStatus(api))
        // Advance 100ms — enough for the initial fetch promise chain to settle
        // without triggering the 2-second polling interval.
        await act(async () => { await vi.advanceTimersByTimeAsync(100) })
        expect(result.current.status.gpuReady).toBe(true)
    })

    it('polls the correct endpoint URL', async () => {
        const { api } = makeMockApi()
        mockFetchOk({})
        renderHook(() => useStatus(api))
        await act(async () => { await Promise.resolve() })
        expect(vi.mocked(global.fetch)).toHaveBeenCalledWith(
            'http://127.0.0.1:14002/v1/status',
            expect.objectContaining({ headers: { Accept: 'application/json' } })
        )
    })

    it('polls again after the 2-second interval', async () => {
        const { api } = makeMockApi()
        mockFetchOk({ gpuReady: true })
        renderHook(() => useStatus(api))
        await act(async () => { await Promise.resolve() })
        const firstCallCount = vi.mocked(global.fetch).mock.calls.length
        await act(async () => {
            vi.advanceTimersByTime(2000)
            await Promise.resolve()
            await Promise.resolve()
        })
        expect(vi.mocked(global.fetch).mock.calls.length).toBeGreaterThan(firstCallCount)
    })

    it('silently ignores fetch errors', async () => {
        const { api } = makeMockApi()
        mockFetchError()
        const { result } = renderHook(() => useStatus(api))
        await act(async () => { await Promise.resolve() })
        // State should remain at the default — no crash, no error thrown
        expect(result.current.status).toEqual(DEFAULT_STATUS)
    })

    it('silently ignores non-ok responses', async () => {
        const { api } = makeMockApi()
        mockFetchNotOk()
        const { result } = renderHook(() => useStatus(api))
        await act(async () => { await Promise.resolve() })
        expect(result.current.status).toEqual(DEFAULT_STATUS)
    })

    it('stops polling after unmount', async () => {
        const { api } = makeMockApi()
        mockFetchOk({})
        const { unmount } = renderHook(() => useStatus(api))
        await act(async () => { await Promise.resolve() })
        const callsBeforeUnmount = vi.mocked(global.fetch).mock.calls.length
        unmount()
        await act(async () => {
            vi.advanceTimersByTime(4000)
            await Promise.resolve()
        })
        expect(vi.mocked(global.fetch).mock.calls.length).toBe(callsBeforeUnmount)
    })
})

// ─── setStatus ────────────────────────────────────────────────────────────────

describe('useStatus — setStatus', () => {
    it('updates status directly when called', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useStatus(api))
        act(() => result.current.setStatus({ ...DEFAULT_STATUS, gpuReady: true }))
        expect(result.current.status.gpuReady).toBe(true)
    })
})
