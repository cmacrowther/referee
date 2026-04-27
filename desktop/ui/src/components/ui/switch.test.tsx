import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { Switch } from './switch'

function sw(container: HTMLElement) {
    return container.querySelector('button')!
}

describe('Switch — ARIA', () => {
    it('has role="switch"', () => {
        const { container } = render(<Switch checked={false} />)
        expect(sw(container).getAttribute('role')).toBe('switch')
    })

    it('has aria-checked="true" when checked', () => {
        const { container } = render(<Switch checked={true} />)
        expect(sw(container).getAttribute('aria-checked')).toBe('true')
    })

    it('has aria-checked="false" when unchecked', () => {
        const { container } = render(<Switch checked={false} />)
        expect(sw(container).getAttribute('aria-checked')).toBe('false')
    })
})

describe('Switch — interactions', () => {
    it('calls onCheckedChange(!checked) when clicked and unchecked', () => {
        const onChange = vi.fn()
        const { container } = render(<Switch checked={false} onCheckedChange={onChange} />)
        fireEvent.click(sw(container))
        expect(onChange).toHaveBeenCalledWith(true)
    })

    it('calls onCheckedChange(!checked) when clicked and checked', () => {
        const onChange = vi.fn()
        const { container } = render(<Switch checked={true} onCheckedChange={onChange} />)
        fireEvent.click(sw(container))
        expect(onChange).toHaveBeenCalledWith(false)
    })

    it('does not call onCheckedChange when disabled', () => {
        const onChange = vi.fn()
        const { container } = render(<Switch checked={false} disabled onCheckedChange={onChange} />)
        fireEvent.click(sw(container))
        expect(onChange).not.toHaveBeenCalled()
    })

    it('does not throw when onCheckedChange is not provided', () => {
        const { container } = render(<Switch checked={false} />)
        expect(() => fireEvent.click(sw(container))).not.toThrow()
    })
})

describe('Switch — disabled state', () => {
    it('sets the disabled attribute', () => {
        const { container } = render(<Switch checked={false} disabled />)
        expect(sw(container).disabled).toBe(true)
    })
})
