import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ConsentModal } from './ConsentModal'
import type { ConsentRequest } from '@/lib/types'

function makeRequest(overrides: Partial<ConsentRequest> = {}): ConsentRequest {
    return {
        nonce: 'nonce-123',
        origin: 'https://example.com',
        appName: null,
        ...overrides,
    }
}

describe('ConsentModal — display', () => {
    it('renders the origin without the protocol prefix', () => {
        render(<ConsentModal request={makeRequest({ origin: 'https://app.example.com' })} onRespond={vi.fn()} />)
        expect(screen.getByText('app.example.com')).not.toBeNull()
    })

    it('strips http:// prefix too', () => {
        render(<ConsentModal request={makeRequest({ origin: 'http://localhost:3000' })} onRespond={vi.fn()} />)
        expect(screen.getByText('localhost:3000')).not.toBeNull()
    })

    it('renders the appName when provided', () => {
        render(<ConsentModal request={makeRequest({ appName: 'MyApp' })} onRespond={vi.fn()} />)
        expect(screen.getByText(/MyApp/)).not.toBeNull()
    })

    it('does not render "(null)" when appName is null', () => {
        const { container } = render(<ConsentModal request={makeRequest({ appName: null })} onRespond={vi.fn()} />)
        expect(container.textContent).not.toContain('null')
    })

    it('renders both action buttons', () => {
        render(<ConsentModal request={makeRequest()} onRespond={vi.fn()} />)
        expect(screen.getByRole('button', { name: /^allow$/i })).not.toBeNull()
        expect(screen.getByRole('button', { name: /^deny$/i })).not.toBeNull()
    })
})

describe('ConsentModal — callbacks', () => {
    it('calls onRespond(nonce, true, true) for "Allow"', () => {
        const onRespond = vi.fn()
        render(<ConsentModal request={makeRequest()} onRespond={onRespond} />)
        fireEvent.click(screen.getByRole('button', { name: /^allow$/i }))
        expect(onRespond).toHaveBeenCalledWith('nonce-123', true, true)
    })

    it('calls onRespond(nonce, false, false) for "Deny"', () => {
        const onRespond = vi.fn()
        render(<ConsentModal request={makeRequest()} onRespond={onRespond} />)
        fireEvent.click(screen.getByText(/Deny/i))
        expect(onRespond).toHaveBeenCalledWith('nonce-123', false, false)
    })
})
