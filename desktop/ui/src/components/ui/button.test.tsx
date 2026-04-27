import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { Button } from './button'

function btn(container: HTMLElement) {
    return container.querySelector('button')!
}

describe('Button — rendering', () => {
    it('renders children', () => {
        const { container } = render(<Button>Save</Button>)
        expect(btn(container).textContent).toBe('Save')
    })

    it('renders as a <button> element', () => {
        const { container } = render(<Button>X</Button>)
        expect(btn(container).tagName).toBe('BUTTON')
    })

    it('merges additional className', () => {
        const { container } = render(<Button className="custom">X</Button>)
        expect(btn(container).className).toContain('custom')
    })

    it('forwards extra HTML attributes', () => {
        const { container } = render(<Button data-testid="my-btn">Btn</Button>)
        expect(container.querySelector('[data-testid="my-btn"]')).not.toBeNull()
    })

    it('is disabled when disabled prop is true', () => {
        const { container } = render(<Button disabled>Off</Button>)
        expect(btn(container).disabled).toBe(true)
    })
})

describe('Button — variants', () => {
    it('applies primary variant class by default', () => {
        const { container } = render(<Button>X</Button>)
        expect(btn(container).className).toContain('bg-referee-accent')
    })

    it('applies secondary variant class', () => {
        const { container } = render(<Button variant="secondary">X</Button>)
        expect(btn(container).className).toContain('bg-white/[0.08]')
    })

    it('applies danger variant class', () => {
        const { container } = render(<Button variant="danger">X</Button>)
        expect(btn(container).className).toContain('bg-referee-danger')
    })

    it('applies ghost variant class', () => {
        const { container } = render(<Button variant="ghost">X</Button>)
        expect(btn(container).className).toContain('bg-transparent')
    })
})

describe('Button — sizes', () => {
    it('applies md size class by default', () => {
        const { container } = render(<Button>X</Button>)
        expect(btn(container).className).toContain('h-10')
    })

    it('applies sm size class', () => {
        const { container } = render(<Button size="sm">X</Button>)
        expect(btn(container).className).toContain('h-8')
    })

    it('applies icon size class', () => {
        const { container } = render(<Button size="icon">X</Button>)
        expect(btn(container).className).toContain('size-9')
    })
})

describe('Button — interactions', () => {
    it('fires onClick when clicked', () => {
        const onClick = vi.fn()
        const { container } = render(<Button onClick={onClick}>Click</Button>)
        fireEvent.click(btn(container))
        expect(onClick).toHaveBeenCalledTimes(1)
    })
})
