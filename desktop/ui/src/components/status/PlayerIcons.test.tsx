import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { PlayerButtonIcon } from './PlayerIcons'

describe('PlayerButtonIcon', () => {
    it('renders an svg for null playerId (fallback Play icon)', () => {
        const { container } = render(<PlayerButtonIcon playerId={null} />)
        expect(container.querySelector('svg')).not.toBeNull()
    })

    it('renders an svg for an unknown playerId', () => {
        const { container } = render(<PlayerButtonIcon playerId="unknown-player" />)
        expect(container.querySelector('svg')).not.toBeNull()
    })

    it('renders an svg for vlc', () => {
        const { container } = render(<PlayerButtonIcon playerId="vlc" />)
        expect(container.querySelector('svg')).not.toBeNull()
    })

    it('renders an svg for mpv', () => {
        const { container } = render(<PlayerButtonIcon playerId="mpv" />)
        expect(container.querySelector('svg')).not.toBeNull()
    })

    it('renders a blue-rect svg for mpc-hc', () => {
        const { container } = render(<PlayerButtonIcon playerId="mpc-hc" />)
        // MpcIcon has a rect with fill="#1a73e8"
        expect(container.querySelector('rect[fill="#1a73e8"]')).not.toBeNull()
    })

    it('renders a green-circle svg for potplayer', () => {
        const { container } = render(<PlayerButtonIcon playerId="potplayer" />)
        // PotPlayerIcon has a circle with fill="#1fa83c"
        expect(container.querySelector('circle[fill="#1fa83c"]')).not.toBeNull()
    })

    it('applies the className prop', () => {
        const { container } = render(<PlayerButtonIcon playerId={null} className="size-4" />)
        expect(container.querySelector('svg')?.getAttribute('class')).toContain('size-4')
    })
})
