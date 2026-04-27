import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { StreamWhistle } from './StreamWhistle'

function whistle(container: HTMLElement) {
    return container.querySelector('.stream-whistle') as HTMLElement
}

describe('StreamWhistle — state attribute', () => {
    it('defaults to data-whistle-state="inactive"', () => {
        const { container } = render(<StreamWhistle />)
        expect(whistle(container).getAttribute('data-whistle-state')).toBe('inactive')
    })

    it('sets data-whistle-state="active" when state="active"', () => {
        const { container } = render(<StreamWhistle state="active" />)
        expect(whistle(container).getAttribute('data-whistle-state')).toBe('active')
    })

    it('sets data-whistle-state="inactive" explicitly', () => {
        const { container } = render(<StreamWhistle state="inactive" />)
        expect(whistle(container).getAttribute('data-whistle-state')).toBe('inactive')
    })
})

describe('StreamWhistle — badge state', () => {
    it('defaults to data-badge-state="hidden"', () => {
        const { container } = render(<StreamWhistle />)
        expect(container.querySelector('.stream-whistle-badge')?.getAttribute('data-badge-state')).toBe('hidden')
    })

    it('sets data-badge-state="starting"', () => {
        const { container } = render(<StreamWhistle badgeState="starting" />)
        expect(container.querySelector('.stream-whistle-badge')?.getAttribute('data-badge-state')).toBe('starting')
    })

    it('sets data-badge-state="live"', () => {
        const { container } = render(<StreamWhistle badgeState="live" />)
        expect(container.querySelector('.stream-whistle-badge')?.getAttribute('data-badge-state')).toBe('live')
    })
})

describe('StreamWhistle — aria visibility', () => {
    it('inactive layer is not aria-hidden when state is "inactive"', () => {
        const { container } = render(<StreamWhistle state="inactive" />)
        const inactive = container.querySelector('.stream-whistle-layer-inactive')
        expect(inactive?.getAttribute('aria-hidden')).toBe('false')
    })

    it('active layer is aria-hidden when state is "inactive"', () => {
        const { container } = render(<StreamWhistle state="inactive" />)
        const active = container.querySelector('.stream-whistle-layer-active')
        expect(active?.getAttribute('aria-hidden')).toBe('true')
    })

    it('inactive layer is aria-hidden when state is "active"', () => {
        const { container } = render(<StreamWhistle state="active" />)
        const inactive = container.querySelector('.stream-whistle-layer-inactive')
        expect(inactive?.getAttribute('aria-hidden')).toBe('true')
    })

    it('active layer is not aria-hidden when state is "active"', () => {
        const { container } = render(<StreamWhistle state="active" />)
        const active = container.querySelector('.stream-whistle-layer-active')
        expect(active?.getAttribute('aria-hidden')).toBe('false')
    })
})

describe('StreamWhistle — className', () => {
    it('merges className onto the root element', () => {
        const { container } = render(<StreamWhistle className="my-class" />)
        expect(whistle(container).className).toContain('my-class')
    })
})
