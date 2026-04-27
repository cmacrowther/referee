import { describe, it, expect, vi } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { useSettings } from './useSettings'
import { makeMockApi } from '@/test/mocks'
import { DEFAULT_SETTINGS } from '@/lib/types'

describe('useSettings — initial state', () => {
    it('starts with DEFAULT_SETTINGS, bootSetting false, and isReady false', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        expect(result.current.settings).toEqual(DEFAULT_SETTINGS)
        expect(result.current.bootSetting).toBe(false)
        expect(result.current.isReady).toBe(false)
    })
})

describe('useSettings — async initialisation', () => {
    it('becomes ready after the initial API calls resolve', async () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
    })

    it('applies the settings returned by getInitialSettings', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getInitialSettings).mockResolvedValue({ resolution: '2560x1440' })
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        expect(result.current.settings.resolution).toBe('2560x1440')
    })

    it('applies the boot setting returned by getBootSetting', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getBootSetting).mockResolvedValue(true)
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.bootSetting).toBe(true))
    })

    it('falls back to DEFAULT_SETTINGS when getInitialSettings rejects', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getInitialSettings).mockRejectedValue(new Error('network error'))
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        expect(result.current.settings).toEqual(DEFAULT_SETTINGS)
    })

    it('falls back to false when getBootSetting rejects', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getBootSetting).mockRejectedValue(new Error('ipc error'))
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        expect(result.current.bootSetting).toBe(false)
    })
})

describe('useSettings — event subscriptions', () => {
    it('updates settings when onSettingsSync fires', async () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        act(() => emit.settingsSync({ resolution: '3840x2160' }))
        expect(result.current.settings.resolution).toBe('3840x2160')
    })

    it('normalises settings received via onSettingsSync', async () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        // Quality 99 should be clamped to the resolution max
        act(() => emit.settingsSync({ resolution: '1920x1080', quality: 99 }))
        expect(result.current.settings.quality).toBe(4)
    })

    it('updates bootSetting when onBootSettingSync fires', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        act(() => emit.bootSettingSync(true))
        expect(result.current.bootSetting).toBe(true)
    })

    it('coerces truthy values from onBootSettingSync to boolean', () => {
        const { api, emit } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        act(() => emit.bootSettingSync(true))
        act(() => emit.bootSettingSync(false))
        expect(result.current.bootSetting).toBe(false)
    })
})

describe('useSettings — saveSettings', () => {
    it('updates state and calls api.saveSettings with a patch object', async () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        act(() => result.current.saveSettings({ quality: 4 }))
        expect(result.current.settings.quality).toBe(4)
        expect(api.saveSettings).toHaveBeenCalledWith(
            expect.objectContaining({ quality: 4 })
        )
    })

    it('updates state and calls api.saveSettings with a patch function', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getInitialSettings).mockResolvedValue({ quality: 2 })
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        act(() => result.current.saveSettings(prev => ({ ...prev, quality: prev.quality + 1 })))
        expect(result.current.settings.quality).toBe(3)
        expect(api.saveSettings).toHaveBeenCalled()
    })

    it('normalises the settings before saving', async () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))
        // Quality 99 exceeds the max → should be clamped
        act(() => result.current.saveSettings({ resolution: '1920x1080', quality: 99 }))
        expect(result.current.settings.quality).toBe(4)
        expect(api.saveSettings).toHaveBeenCalledWith(
            expect.objectContaining({ quality: 4 })
        )
    })
})

describe('useSettings — saveStreamSettings', () => {
    it('uses local settings save when relay is not enabled', async () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))

        act(() => result.current.saveStreamSettings({ quality: 3 }))

        expect(api.saveSettings).toHaveBeenCalledWith(
            expect.objectContaining({ quality: 3 })
        )
        expect(api.saveStreamSettings).not.toHaveBeenCalled()
    })

    it('uses relay stream settings save when relay is enabled', async () => {
        const { api } = makeMockApi()
        vi.mocked(api.getInitialSettings).mockResolvedValue({
            relay: {
                enabled: true,
                linkedPeerId: 'peer-1',
                linkedPeerHostname: 'media-box',
                linkedPeerIp: '192.168.1.23',
                remoteToken: 'relay-secret',
                lastKnownPeer: null,
            },
        })
        const { result } = renderHook(() => useSettings(api))
        await waitFor(() => expect(result.current.isReady).toBe(true))

        act(() => result.current.saveStreamSettings({ quality: 3 }))

        expect(api.saveStreamSettings).toHaveBeenCalledWith(
            expect.objectContaining({ quality: 3 })
        )
    })
})

describe('useSettings — updateBootSetting', () => {
    it('updates the local bootSetting state', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        act(() => result.current.updateBootSetting(true))
        expect(result.current.bootSetting).toBe(true)
    })

    it('calls api.setBootSetting with the new value', () => {
        const { api } = makeMockApi()
        const { result } = renderHook(() => useSettings(api))
        act(() => result.current.updateBootSetting(true))
        expect(api.setBootSetting).toHaveBeenCalledWith(true)
    })
})

describe('useSettings — cleanup', () => {
    it('does not update state after unmount (avoids memory-leak warnings)', async () => {
        const { api } = makeMockApi()
        // Make the API call resolve after unmount
        let resolve!: (v: any) => void
        vi.mocked(api.getInitialSettings).mockReturnValue(new Promise(r => { resolve = r }))
        const { unmount } = renderHook(() => useSettings(api))
        unmount()
        // Resolving after unmount should not throw or cause a state-update warning
        await act(async () => { resolve({ resolution: '3840x2160' }) })
    })
})
