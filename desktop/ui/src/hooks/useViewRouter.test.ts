import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useViewRouter } from './useViewRouter'
import { makeMockApi } from '@/test/mocks'

function resetHash() {
    window.location.hash = ''
}

describe('useViewRouter — initial state', () => {
    beforeEach(resetHash)

    it('defaults to "status" when hash is empty', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        expect(result.current.currentView).toBe('status')
    })

    it('reads "settings" from the URL hash on mount', () => {
        window.location.hash = '#settings'
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        expect(result.current.currentView).toBe('settings')
    })

    it('falls back to "status" for an unrecognised hash fragment', () => {
        window.location.hash = '#unknown'
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        expect(result.current.currentView).toBe('status')
    })
})

describe('useViewRouter — setView', () => {
    beforeEach(resetHash)

    it('updates currentView', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => result.current.setView('settings'))
        expect(result.current.currentView).toBe('settings')
    })

    it('normalises unknown views to "status"', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => result.current.setView('unknown' as any))
        expect(result.current.currentView).toBe('status')
    })

    it('can set view to "setup"', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => result.current.setView('setup'))
        expect(result.current.currentView).toBe('setup')
    })
})

describe('useViewRouter — onNavigateView event', () => {
    beforeEach(resetHash)

    it('updates currentView when the backend emits a navigate event', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => emit.navigateView('settings'))
        expect(result.current.currentView).toBe('settings')
    })

    it('can navigate to "setup" via backend event', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => emit.navigateView('setup'))
        expect(result.current.currentView).toBe('setup')
    })

    it('normalises unrecognised event values to "status"', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => emit.navigateView('settings'))
        act(() => emit.navigateView('bad' as any))
        expect(result.current.currentView).toBe('status')
    })
})

describe('useViewRouter — hashchange events', () => {
    beforeEach(resetHash)

    it('updates view when the hash changes to #settings', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => {
            window.location.hash = '#settings'
            window.dispatchEvent(new HashChangeEvent('hashchange'))
        })
        expect(result.current.currentView).toBe('settings')
    })

    it('updates view when the hash is cleared', () => {
        window.location.hash = '#settings'
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => {
            window.location.hash = ''
            window.dispatchEvent(new HashChangeEvent('hashchange'))
        })
        expect(result.current.currentView).toBe('status')
    })

    it('does NOT navigate to "setup" via hashchange (setup is backend-only)', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useViewRouter(api))
        act(() => result.current.setView('settings'))
        act(() => {
            window.location.hash = '#setup'
            window.dispatchEvent(new HashChangeEvent('hashchange'))
        })
        // view must not have changed to 'setup' — stays on 'settings'
        expect(result.current.currentView).toBe('settings')
    })
})

describe('useViewRouter — cleanup', () => {
    beforeEach(resetHash)

    it('calls the unsubscribe function returned by onNavigateView when unmounted', () => {
        const { api } = makeMockApi()
        const unsubscribe = vi.fn()
        api.onNavigateView = vi.fn().mockReturnValue(unsubscribe)
        const { unmount } = renderHook(() => useViewRouter(api))
        unmount()
        expect(unsubscribe).toHaveBeenCalled()
    })

    it('removes the hashchange listener on unmount (no stale updates)', () => {
        const { api } = makeMockApi()
        const { result, unmount } = renderHook(() => useViewRouter(api))
        unmount()
        act(() => {
            window.location.hash = '#settings'
            window.dispatchEvent(new HashChangeEvent('hashchange'))
        })
        // result.current still reflects the pre-unmount state ('status')
        expect(result.current.currentView).toBe('status')
    })
})
