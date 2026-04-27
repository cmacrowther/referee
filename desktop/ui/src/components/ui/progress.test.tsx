import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { Progress } from './progress'

function fillEl(container: HTMLElement) {
    // container → track div → fill div
    return container.firstElementChild?.firstElementChild as HTMLElement
}

describe('Progress', () => {
    it('sets fill width to the given value', () => {
        const { container } = render(<Progress value={60} />)
        expect(fillEl(container).style.width).toBe('60%')
    })

    it('clamps to 0% for negative values', () => {
        const { container } = render(<Progress value={-10} />)
        expect(fillEl(container).style.width).toBe('0%')
    })

    it('clamps to 100% for values over 100', () => {
        const { container } = render(<Progress value={150} />)
        expect(fillEl(container).style.width).toBe('100%')
    })

    it('renders 0% for value 0', () => {
        const { container } = render(<Progress value={0} />)
        expect(fillEl(container).style.width).toBe('0%')
    })

    it('renders 100% for value 100', () => {
        const { container } = render(<Progress value={100} />)
        expect(fillEl(container).style.width).toBe('100%')
    })

    it('merges className onto the track', () => {
        const { container } = render(<Progress value={50} className="my-class" />)
        expect(container.querySelector('div')?.className).toContain('my-class')
    })
})
