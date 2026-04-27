import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { StreamInfoBadges, StreamInfoSegments } from './StreamInfoBadges'
import { DEFAULT_STATUS, DEFAULT_SETTINGS } from '@/lib/types'
import type { Status, Session, Settings } from '@/lib/types'

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

// ─── StreamInfoBadges ─────────────────────────────────────────────────────────

describe('StreamInfoBadges — stream state label', () => {
    it('shows "Standby" when there are no sessions', () => {
        render(<StreamInfoBadges status={makeStatus()} settings={DEFAULT_SETTINGS} />)
        expect(screen.getByText('Standby')).not.toBeNull()
    })

    it('shows "RELAY" when there is one live session', () => {
        const liveSession = makeSession({ startupComplete: true })
        render(
            <StreamInfoBadges
                status={makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession })}
                settings={DEFAULT_SETTINGS}
            />
        )
        expect(screen.getByText('RELAY')).not.toBeNull()
    })

    it('shows "Starting" when there is a pending (not yet live) session', () => {
        const pending = makeSession({ startupComplete: false })
        render(
            <StreamInfoBadges
                status={makeStatus({ sessions: [pending], activeSessions: 1, primarySession: pending })}
                settings={DEFAULT_SETTINGS}
            />
        )
        expect(screen.getByText('Starting')).not.toBeNull()
    })
})

describe('StreamInfoBadges — upscaler label', () => {
    it('shows upscaler label when session has one', () => {
        // formatUpscalerLabel returns the raw value for unmapped upscalers like 'rife'
        const liveSession = makeSession({ upscaler: 'rife', startupComplete: true })
        render(
            <StreamInfoBadges
                status={makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession })}
                settings={DEFAULT_SETTINGS}
            />
        )
        expect(screen.getByText('rife')).not.toBeNull()
    })
})

// ─── StreamInfoSegments ───────────────────────────────────────────────────────

describe('StreamInfoSegments — resolution', () => {
    it('shows target resolution from settings when no session', () => {
        // formatResolutionLabel('2560x1440') → '1440P' (uppercase P)
        render(
            <StreamInfoSegments
                status={makeStatus()}
                settings={{ ...DEFAULT_SETTINGS, resolution: '2560x1440' }}
            />
        )
        expect(screen.getByText('1440P')).not.toBeNull()
    })
})

describe('StreamInfoSegments — frame gen', () => {
    it('shows "OFF" for the FG segment when framegen is disabled', () => {
        // Both FG and HDR segments can show 'OFF' — query the specific FG segment
        const liveSession = makeSession({ framegenEnabled: false })
        const { container } = render(
            <StreamInfoSegments
                status={makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession, encoderHasFramegen: true })}
                settings={DEFAULT_SETTINGS}
            />
        )
        const segments = container.querySelectorAll('.stream-info-segment')
        // segment order: [0] Res, [1] FG, [2] HDR, [3] GPU
        expect(segments[1].querySelector('.stream-info-value')?.textContent).toBe('OFF')
    })

    it('shows "ON" for the FG segment when framegen is enabled and supported', () => {
        const liveSession = makeSession({ framegenEnabled: true })
        const { container } = render(
            <StreamInfoSegments
                status={makeStatus({ sessions: [liveSession], activeSessions: 1, primarySession: liveSession, encoderHasFramegen: true })}
                settings={DEFAULT_SETTINGS}
            />
        )
        const segments = container.querySelectorAll('.stream-info-segment')
        expect(segments[1].querySelector('.stream-info-value')?.textContent).toBe('ON')
    })

    it('shows "N/A" for the FG segment when framegen is not supported', () => {
        const { container } = render(
            <StreamInfoSegments
                status={makeStatus({ encoderHasFramegen: false })}
                settings={DEFAULT_SETTINGS}
            />
        )
        const segments = container.querySelectorAll('.stream-info-segment')
        expect(segments[1].querySelector('.stream-info-value')?.textContent).toBe('N/A')
    })
})

describe('StreamInfoSegments — GPU label', () => {
    it('shows "0%" when no sessions and gpuUtilization is null (null coerces to 0)', () => {
        // formatGpuUtilization(null): Number(null)=0 → '0% Load'; compact → '0%'
        const { container } = render(
            <StreamInfoSegments
                status={makeStatus({ gpuUtilization: null })}
                settings={DEFAULT_SETTINGS}
            />
        )
        const segments = container.querySelectorAll('.stream-info-segment')
        expect(segments[3].querySelector('.stream-info-value')?.textContent).toBe('0%')
    })

    it('shows compact GPU utilization when set', () => {
        const { container } = render(
            <StreamInfoSegments
                status={makeStatus({ gpuUtilization: 75 })}
                settings={DEFAULT_SETTINGS}
            />
        )
        // formatGpuUtilization(75) = '75% Load'; compact removes ' Load' → '75%'
        const segments = container.querySelectorAll('.stream-info-segment')
        expect(segments[3].querySelector('.stream-info-value')?.textContent).toBe('75%')
    })
})
