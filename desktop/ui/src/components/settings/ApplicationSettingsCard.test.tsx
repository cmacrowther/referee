import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { ApplicationSettingsCard } from './ApplicationSettingsCard'
import { DEFAULT_SETTINGS } from '@/lib/types'
import type { Settings } from '@/lib/types'

const BASE_SETTINGS: Settings = {
    ...DEFAULT_SETTINGS,
    minimizeToTray: false,
    showOnProxyStart: false,
    closeToTray: false,
    notifications: false,
}

function getCheckboxes(container: HTMLElement) {
    return container.querySelectorAll('input[type="checkbox"]') as NodeListOf<HTMLInputElement>
}

// Checkbox order matches SettingRow declaration order in the component:
// [0] Start on Boot, [1] Minimize to Tray, [2] Show on Proxy Start, [3] Close to Tray, [4] Stream Notifications

describe('ApplicationSettingsCard — rendering', () => {
    it('renders "Start on Boot" row', () => {
        render(<ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={vi.fn()} onBootChange={vi.fn()} />)
        expect(screen.getByText('Start on Boot')).not.toBeNull()
    })

    it('renders "Minimize to Tray" row', () => {
        render(<ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={vi.fn()} onBootChange={vi.fn()} />)
        expect(screen.getByText('Minimize to Tray')).not.toBeNull()
    })

    it('renders "Stream Notifications" row', () => {
        render(<ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={vi.fn()} onBootChange={vi.fn()} />)
        expect(screen.getByText('Stream Notifications')).not.toBeNull()
    })
})

describe('ApplicationSettingsCard — checked state', () => {
    it('Start on Boot checkbox is checked when bootSetting is true', () => {
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={true} onChange={vi.fn()} onBootChange={vi.fn()} />
        )
        expect(getCheckboxes(container)[0].checked).toBe(true)
    })

    it('Start on Boot checkbox is unchecked when bootSetting is false', () => {
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={vi.fn()} onBootChange={vi.fn()} />
        )
        expect(getCheckboxes(container)[0].checked).toBe(false)
    })

    it('Minimize to Tray checkbox reflects settings.minimizeToTray', () => {
        const { container } = render(
            <ApplicationSettingsCard
                settings={{ ...BASE_SETTINGS, minimizeToTray: true }}
                bootSetting={false}
                onChange={vi.fn()}
                onBootChange={vi.fn()}
            />
        )
        expect(getCheckboxes(container)[1].checked).toBe(true)
    })
})

describe('ApplicationSettingsCard — onChange callbacks', () => {
    it('calls onBootChange(true) when Start on Boot is enabled', async () => {
        const onBootChange = vi.fn()
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={vi.fn()} onBootChange={onBootChange} />
        )
        await userEvent.click(getCheckboxes(container)[0])
        expect(onBootChange).toHaveBeenCalledWith(true)
    })

    it('calls onChange({ minimizeToTray: true }) when Minimize to Tray is toggled', async () => {
        const onChange = vi.fn()
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={onChange} onBootChange={vi.fn()} />
        )
        await userEvent.click(getCheckboxes(container)[1])
        expect(onChange).toHaveBeenCalledWith({ minimizeToTray: true })
    })

    it('calls onChange({ showOnProxyStart: true }) when Show on Proxy Start is toggled', async () => {
        const onChange = vi.fn()
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={onChange} onBootChange={vi.fn()} />
        )
        await userEvent.click(getCheckboxes(container)[2])
        expect(onChange).toHaveBeenCalledWith({ showOnProxyStart: true })
    })

    it('calls onChange({ closeToTray: true }) when Close to Tray is toggled', async () => {
        const onChange = vi.fn()
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={onChange} onBootChange={vi.fn()} />
        )
        await userEvent.click(getCheckboxes(container)[3])
        expect(onChange).toHaveBeenCalledWith({ closeToTray: true })
    })

    it('calls onChange({ notifications: true }) when Stream Notifications is toggled', async () => {
        const onChange = vi.fn()
        const { container } = render(
            <ApplicationSettingsCard settings={BASE_SETTINGS} bootSetting={false} onChange={onChange} onBootChange={vi.fn()} />
        )
        await userEvent.click(getCheckboxes(container)[4])
        expect(onChange).toHaveBeenCalledWith({ notifications: true })
    })
})
