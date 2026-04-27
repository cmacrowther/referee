import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { SetupError } from './SetupError'

describe('SetupError', () => {
    it('renders nothing when error is null', () => {
        const { container } = render(<SetupError error={null} onRetry={vi.fn()} />)
        expect(container.firstChild).toBeNull()
    })

    it('renders the error message', () => {
        render(<SetupError error="Download failed" onRetry={vi.fn()} />)
        expect(screen.getByText('Download failed')).not.toBeNull()
    })

    it('renders the Retry button', () => {
        render(<SetupError error="Oops" onRetry={vi.fn()} />)
        expect(screen.getByText('Retry')).not.toBeNull()
    })

    it('calls onRetry when Retry is clicked', () => {
        const onRetry = vi.fn()
        render(<SetupError error="Error" onRetry={onRetry} />)
        fireEvent.click(screen.getByText('Retry'))
        expect(onRetry).toHaveBeenCalledTimes(1)
    })
})
