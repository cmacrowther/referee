import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { TitleBar } from './TitleBar'

vi.mock('@tauri-apps/api/window', () => ({
    getCurrentWindow: vi.fn(() => ({ startDragging: vi.fn() })),
}))

const DEFAULT_PROPS = {
    view: 'status' as const,
    pinned: false,
    onToggleView: vi.fn(),
    onTogglePin: vi.fn(),
    onMinimize: vi.fn(),
    onClose: vi.fn(),
}

describe('TitleBar — nav button', () => {
    it('has title "Open Settings" when view is "status"', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} view="status" />)
        expect(container.querySelector('#nav-button')?.getAttribute('title')).toBe('Open Settings')
    })

    it('has title "Back to Status" when view is "settings"', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} view="settings" />)
        expect(container.querySelector('#nav-button')?.getAttribute('title')).toBe('Back to Status')
    })

    it('calls onToggleView when nav button is clicked', () => {
        const onToggleView = vi.fn()
        const { container } = render(<TitleBar {...DEFAULT_PROPS} onToggleView={onToggleView} />)
        fireEvent.click(container.querySelector('#nav-button')!)
        expect(onToggleView).toHaveBeenCalledTimes(1)
    })
})

describe('TitleBar — pin button', () => {
    it('has aria-pressed="false" when not pinned', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} pinned={false} />)
        expect(container.querySelector('#pin-button')?.getAttribute('aria-pressed')).toBe('false')
    })

    it('has aria-pressed="true" when pinned', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} pinned={true} />)
        expect(container.querySelector('#pin-button')?.getAttribute('aria-pressed')).toBe('true')
    })

    it('has title "Keep REFEREE on top" when not pinned', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} pinned={false} />)
        expect(container.querySelector('#pin-button')?.getAttribute('title')).toBe('Keep REFEREE on top')
    })

    it('has title "Disable Always on Top" when pinned', () => {
        const { container } = render(<TitleBar {...DEFAULT_PROPS} pinned={true} />)
        expect(container.querySelector('#pin-button')?.getAttribute('title')).toBe('Disable Always on Top')
    })

    it('calls onTogglePin when pin button is clicked', () => {
        const onTogglePin = vi.fn()
        const { container } = render(<TitleBar {...DEFAULT_PROPS} onTogglePin={onTogglePin} />)
        fireEvent.click(container.querySelector('#pin-button')!)
        expect(onTogglePin).toHaveBeenCalledTimes(1)
    })
})

describe('TitleBar — window controls', () => {
    it('calls onMinimize when minimize button is clicked', () => {
        const onMinimize = vi.fn()
        const { container } = render(<TitleBar {...DEFAULT_PROPS} onMinimize={onMinimize} />)
        fireEvent.click(container.querySelector('#minimize-button')!)
        expect(onMinimize).toHaveBeenCalledTimes(1)
    })

    it('calls onClose when close button is clicked', () => {
        const onClose = vi.fn()
        const { container } = render(<TitleBar {...DEFAULT_PROPS} onClose={onClose} />)
        fireEvent.click(container.querySelector('#close-button')!)
        expect(onClose).toHaveBeenCalledTimes(1)
    })
})
