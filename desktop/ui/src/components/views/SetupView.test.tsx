import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { SetupView } from './SetupView'
import type { SetupState } from '@/lib/types'
import { DEFAULT_SETUP_STATE } from '@/lib/types'

function makeState(overrides: Partial<SetupState> = {}): SetupState {
    return { ...DEFAULT_SETUP_STATE, ...overrides }
}

describe('SetupView — copy text', () => {
    it('renders "Unknown Hardware" when gpu.vendor is "unknown"', () => {
        render(<SetupView setupState={makeState({ gpu: { vendor: 'unknown', name: '' } })} onRetry={vi.fn()} />)
        expect(screen.getByText('Unknown Hardware')).not.toBeNull()
    })

    it('renders "Setup Complete" when complete', () => {
        render(<SetupView setupState={makeState({ complete: true })} onRetry={vi.fn()} />)
        expect(screen.getByText('Setup Complete')).not.toBeNull()
    })

    it('renders "Retry Setup" when there is an error', () => {
        render(<SetupView setupState={makeState({ error: 'Oops' })} onRetry={vi.fn()} />)
        expect(screen.getByText('Retry Setup')).not.toBeNull()
    })

    it('renders "Checking Hardware" when phase is "detect"', () => {
        render(
            <SetupView
                setupState={makeState({ progress: { phase: 'detect', percent: 0, detail: '' } })}
                onRetry={vi.fn()}
            />
        )
        expect(screen.getByText('Checking Hardware')).not.toBeNull()
    })

    it('renders "Getting Ready" for encoder phase', () => {
        render(
            <SetupView
                setupState={makeState({ progress: { phase: 'encoder', percent: 0, detail: '' } })}
                onRetry={vi.fn()}
            />
        )
        expect(screen.getByText('Getting Ready')).not.toBeNull()
    })

    it('renders "Retrying Setup" when phase is "retry"', () => {
        render(
            <SetupView
                setupState={makeState({ progress: { phase: 'retry', percent: 0, detail: '' } })}
                onRetry={vi.fn()}
            />
        )
        expect(screen.getByText('Retrying Setup')).not.toBeNull()
    })
})

describe('SetupView — SetupError delegation', () => {
    it('renders the error message via SetupError', () => {
        render(<SetupView setupState={makeState({ error: 'Download failed' })} onRetry={vi.fn()} />)
        expect(screen.getByText('Download failed')).not.toBeNull()
    })

    it('renders the Retry button when there is an error', () => {
        render(<SetupView setupState={makeState({ error: 'Error' })} onRetry={vi.fn()} />)
        expect(screen.getByText('Retry')).not.toBeNull()
    })

    it('calls onRetry when the Retry button is clicked', () => {
        const onRetry = vi.fn()
        render(<SetupView setupState={makeState({ error: 'Error' })} onRetry={onRetry} />)
        fireEvent.click(screen.getByText('Retry'))
        expect(onRetry).toHaveBeenCalledTimes(1)
    })

    it('does not render Retry button when there is no error', () => {
        render(<SetupView setupState={makeState()} onRetry={vi.fn()} />)
        expect(screen.queryByText('Retry')).toBeNull()
    })
})

describe('SetupView — section id', () => {
    it('renders the setup-view section', () => {
        const { container } = render(<SetupView setupState={makeState()} onRetry={vi.fn()} />)
        expect(container.querySelector('#setup-view')).not.toBeNull()
    })
})
