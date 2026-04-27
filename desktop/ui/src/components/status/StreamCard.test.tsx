import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'

import { DEFAULT_SETTINGS, DEFAULT_STATUS } from '@/lib/types'
import type { Session, Settings, Status } from '@/lib/types'

import { StreamCard } from './StreamCard'

function makeSession(overrides: Partial<Session> = {}): Session {
    return {
        id: 'session-1',
        sourceUrl: null,
        outputUrl: 'http://out.m3u8',
        appName: null,
        streamTitle: null,
        sourceContentKind: null,
        upscaler: null,
        sourceResolution: null,
        outputResolution: null,
        sourceFps: null,
        targetFps: null,
        framegenEnabled: false,
        hdrEnabled: false,
        qualityLevel: null,
        executor: null,
        encoderBackend: null,
        startupComplete: true,
        retryingStartup: false,
        ...overrides,
    }
}

function makeStatus(overrides: Partial<Status> = {}): Status {
    return { ...DEFAULT_STATUS, ...overrides }
}

function makeSettings(overrides: Partial<Settings> = {}): Settings {
    return { ...DEFAULT_SETTINGS, ...overrides }
}

const DEFAULT_PROPS = {
    status: makeStatus(),
    settings: makeSettings(),
    isStopping: false,
    onStop: vi.fn(),
}

describe('StreamCard - idle state (no sessions)', () => {
    it('shows "Awaiting Signal" title', () => {
        render(<StreamCard {...DEFAULT_PROPS} />)
        expect(screen.getByText('Awaiting Signal')).not.toBeNull()
    })

    it('shows "Standby" kicker', () => {
        render(<StreamCard {...DEFAULT_PROPS} />)
        expect(screen.getByText('Standby')).not.toBeNull()
    })

    it('stop button is disabled when there is no session', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} />)
        expect(container.querySelector('.stream-message-stop')?.hasAttribute('disabled')).toBe(true)
    })

    it('does not render StreamInfoSegments when there are no sessions', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} />)
        expect(container.querySelector('.stream-info-segments')).toBeNull()
    })

    it('shows unsupported hardware copy and a Relay settings button when relay setup is required', () => {
        const onOpenRelaySettings = vi.fn()

        render(
            <StreamCard
                {...DEFAULT_PROPS}
                relaySetupRequired
                onOpenRelaySettings={onOpenRelaySettings}
            />
        )

        expect(screen.getByText('Relay Required')).not.toBeNull()
        expect(screen.getByText('REFEREE is not compatible with your hardware, REFEREE Relay will be required to enhance streams on this device. Please set up REFEREE Relay to continue.')).not.toBeNull()

        const relaySettingsButton = screen.getByRole('button', { name: 'Open Relay Settings' })

        expect(relaySettingsButton.querySelector('.stream-message-button-icon')).toBeNull()

        fireEvent.click(relaySettingsButton)

        expect(onOpenRelaySettings).toHaveBeenCalledTimes(1)
    })

    it('shows relay offline copy when the linked relay peer is unavailable', () => {
        const onOpenRelaySettings = vi.fn()

        render(
            <StreamCard
                {...DEFAULT_PROPS}
                relayPeerOffline
                onOpenRelaySettings={onOpenRelaySettings}
            />
        )

        expect(screen.getByText('Standby')).not.toBeNull()
        expect(screen.getByText('Relay Offline')).not.toBeNull()
        expect(screen.getByText('You have a relay peer linked, but the peer is offline. Streams cannot be started until the peer comes online.')).not.toBeNull()
        expect(screen.queryByText('Awaiting Signal')).toBeNull()

        fireEvent.click(screen.getByRole('button', { name: 'Open Relay Settings' }))

        expect(onOpenRelaySettings).toHaveBeenCalledTimes(1)
    })
})

describe('StreamCard - starting state (pending session)', () => {
    const pendingSession = makeSession({ startupComplete: false })
    const startingStatus = makeStatus({ sessions: [pendingSession], activeSessions: 1, primarySession: pendingSession })

    it('shows "Preparing Stream" title', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={startingStatus} />)
        expect(screen.getByText('Preparing Stream')).not.toBeNull()
    })

    it('shows "Pipeline Starting" kicker', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={startingStatus} />)
        expect(screen.getByText('Pipeline Starting')).not.toBeNull()
    })

    it('sets data-preview-state to "loading"', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={startingStatus} />)
        expect(container.querySelector('#stream-panel-card')?.getAttribute('data-preview-state')).toBe('loading')
    })
})

describe('StreamCard - active state (live session)', () => {
    const liveSession = makeSession({ startupComplete: true })
    const activeStatus = makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession })

    it('shows "Enhancing Stream" title', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} />)
        expect(screen.getByText('Enhancing Stream')).not.toBeNull()
    })

    it('shows "Pipeline Active" kicker', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} />)
        expect(screen.getByText('Pipeline Active')).not.toBeNull()
    })

    it('sets data-preview-state to "ready"', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} />)
        expect(container.querySelector('#stream-panel-card')?.getAttribute('data-preview-state')).toBe('ready')
    })

    it('renders StreamInfoSegments when the session is active', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} />)
        expect(container.querySelector('.stream-info-segments')).not.toBeNull()
    })

    it('enables the stop button when the session has an id', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} />)
        expect(container.querySelector('.stream-message-stop')?.hasAttribute('disabled')).toBe(false)
    })

    it('calls onStop when the stop button is clicked', () => {
        const onStop = vi.fn()
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} onStop={onStop} />)
        fireEvent.click(container.querySelector('.stream-message-stop')!)
        expect(onStop).toHaveBeenCalledTimes(1)
    })
})

describe('StreamCard - isStopping', () => {
    const liveSession = makeSession()
    const activeStatus = makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession })

    it('disables the stop button when isStopping is true', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} isStopping={true} />)
        expect(container.querySelector('.stream-message-stop')?.hasAttribute('disabled')).toBe(true)
    })

    it('shows "Stopping" when isStopping is true', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} isStopping={true} />)
        expect(screen.getByText('Stopping')).not.toBeNull()
    })

    it('sets data-stopping to "true" when isStopping is true', () => {
        const { container } = render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} isStopping={true} />)
        expect(container.querySelector('#stream-panel-card')?.getAttribute('data-stopping')).toBe('true')
    })
})

describe('StreamCard - external player actions', () => {
    const liveSession = makeSession({ outputUrl: 'http://output.m3u8' })
    const activeStatus = makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession })
    const playerSettings = makeSettings({ player: { enabled: true, selectedPlayer: 'vlc', customPath: null } })

    it('does not render an "Open" button even when external player auto-open is enabled', () => {
        render(<StreamCard {...DEFAULT_PROPS} status={activeStatus} settings={playerSettings} />)
        expect(screen.queryByText('Open')).toBeNull()
    })
})
