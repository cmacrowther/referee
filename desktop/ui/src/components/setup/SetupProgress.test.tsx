import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { SetupProgress } from './SetupProgress'
import type { SetupState } from '@/lib/types'
import { DEFAULT_SETUP_STATE } from '@/lib/types'

function makeState(overrides: Partial<SetupState> = {}): SetupState {
    return { ...DEFAULT_SETUP_STATE, ...overrides }
}

function root(container: HTMLElement) {
    return container.querySelector('.setup-progress') as HTMLElement
}

describe('SetupProgress — hidden when vendor unknown', () => {
    it('renders nothing when gpu.vendor is "unknown"', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ gpu: { vendor: 'unknown', name: '' } })} />
        )
        expect(container.firstChild).toBeNull()
    })
})

describe('SetupProgress — progress label', () => {
    it('shows "Preparing" when no phase set', () => {
        const { container } = render(<SetupProgress setupState={makeState()} />)
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Preparing')
    })

    it('shows "Checking GPU" for detect phase', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'detect', percent: 0, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Checking GPU')
    })

    it('shows "Installing NVIDIA® Libraries" for encoder phase with nvidia', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({
                gpu: { vendor: 'nvidia', name: 'RTX 4080' },
                progress: { phase: 'encoder', percent: 20, detail: '' },
            })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Installing NVIDIA® Libraries')
    })

    it('shows "Installing AMD® Libraries" for encoder phase with amd', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({
                gpu: { vendor: 'amd', name: 'RX 7900' },
                progress: { phase: 'encoder', percent: 20, detail: '' },
            })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Installing AMD® Libraries')
    })

    it('shows "Installing Video Libraries" for encoder phase with unknown vendor', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({
                progress: { phase: 'encoder', percent: 0, detail: '' },
            })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Installing Video Libraries')
    })

    it('shows "Retrying" for retry phase', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'retry', percent: 0, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Retrying')
    })

    it('shows "Setting Up Pipeline" for install phase', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'install', percent: 50, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Setting Up Pipeline')
    })

    it('shows "Ready" when complete', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ complete: true })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Ready')
    })

    it('shows "Ready" when phase is "done"', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'done', percent: 100, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Ready')
    })

    it('shows "Setup Paused" on error', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ error: 'Download failed' })} />
        )
        expect(container.querySelector('.setup-progress-label')?.textContent).toBe('Setup Paused')
    })
})

describe('SetupProgress — progress value', () => {
    it('displays the percentage value', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'encoder', percent: 42, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-value')?.textContent).toBe('42%')
    })

    it('shows 100% when phase is done', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'done', percent: 0, detail: '' } })} />
        )
        expect(container.querySelector('.setup-progress-value')?.textContent).toBe('100%')
    })
})

describe('SetupProgress — data-state attribute', () => {
    it('data-state is "active" during normal progress', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ progress: { phase: 'encoder', percent: 50, detail: '' } })} />
        )
        expect(root(container).getAttribute('data-state')).toBe('active')
    })

    it('data-state is "complete" when done', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ complete: true })} />
        )
        expect(root(container).getAttribute('data-state')).toBe('complete')
    })

    it('data-state is "error" when there is an error', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ error: 'Oops' })} />
        )
        expect(root(container).getAttribute('data-state')).toBe('error')
    })
})

describe('SetupProgress — detail text', () => {
    it('shows "Setup complete" when complete', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ complete: true })} />
        )
        expect(container.querySelector('.setup-progress-detail')?.textContent).toBe('Setup complete')
    })

    it('shows "Retry to continue" on error', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({ error: 'Error' })} />
        )
        expect(container.querySelector('.setup-progress-detail')?.textContent).toBe('Retry to continue')
    })

    it('normalises "Preparing NVEncC..." to user-friendly text', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({
                progress: { phase: 'encoder', percent: 0, detail: 'Preparing NVEncC download...' },
            })} />
        )
        expect(container.querySelector('.setup-progress-detail')?.textContent).toBe(
            'Preparing NVIDIA® acceleration libraries...'
        )
    })

    it('normalises "Downloading ffmpeg..." to user-friendly text', () => {
        const { container } = render(
            <SetupProgress setupState={makeState({
                progress: { phase: 'install', percent: 0, detail: 'Downloading ffmpeg binaries' },
            })} />
        )
        expect(container.querySelector('.setup-progress-detail')?.textContent).toBe(
            'Downloading REFEREE pipeline libraries...'
        )
    })
})
