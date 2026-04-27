import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { DEFAULT_RELAY_LINK_STATUS, DEFAULT_SETTINGS, DEFAULT_STATUS } from '@/lib/types'
import type { RelayLinkStatus, Session, Settings, Status } from '@/lib/types'

import { StatusView } from './StatusView'

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

function makeLiveStatus(): Status {
    const session = makeSession()

    return {
        ...DEFAULT_STATUS,
        activeSessions: 1,
        sessions: [session],
        primarySession: session,
    }
}

const hostRelaySettings: Settings = {
    ...DEFAULT_SETTINGS,
    relay: {
        enabled: true,
        linkedPeerId: 'relay-1',
        linkedPeerHostname: 'relay-box',
        linkedPeerIp: '192.168.1.50',
        remoteToken: 'relay-secret',
        lastKnownPeer: {
            instanceId: 'relay-1',
            hostname: 'relay-box',
            ip: '192.168.1.50',
            version: '1.0.0',
            gpuReady: true,
            gpuVendor: 'nvidia',
            gpuName: 'RTX',
        },
    },
}

const linkedRelayStatus: RelayLinkStatus = {
    ...DEFAULT_RELAY_LINK_STATUS,
    linked: true,
    available: true,
    peer: hostRelaySettings.relay.lastKnownPeer,
}

describe('StatusView — relay link card', () => {
    it('shows unsupported hardware standby copy when Relay is required but not configured', () => {
        const onOpenRelaySettings = vi.fn()

        render(
            <StatusView
                status={DEFAULT_STATUS}
                settings={DEFAULT_SETTINGS}
                relayLinkStatus={DEFAULT_RELAY_LINK_STATUS}
                requireRelayRoute
                isStopping={false}
                onOpenRelaySettings={onOpenRelaySettings}
                onStop={vi.fn()}
            />
        )

        expect(screen.getByText('Relay Required')).not.toBeNull()

        screen.getByRole('button', { name: 'Open Relay Settings' }).click()

        expect(onOpenRelaySettings).toHaveBeenCalledTimes(1)
    })

    it('shows normal standby copy once a Relay peer is configured', () => {
        render(
            <StatusView
                status={DEFAULT_STATUS}
                settings={hostRelaySettings}
                relayLinkStatus={linkedRelayStatus}
                requireRelayRoute
                isStopping={false}
                onStop={vi.fn()}
            />
        )

        expect(screen.getByText('Awaiting Signal')).not.toBeNull()
        expect(screen.queryByText('Relay Required')).toBeNull()
    })

    it('shows the Relay Link card on standby for the host machine', () => {
        render(
            <StatusView
                status={DEFAULT_STATUS}
                settings={hostRelaySettings}
                relayLinkStatus={linkedRelayStatus}
                isStopping={false}
                onStop={vi.fn()}
            />
        )

        const relayTarget = screen.getByRole('group', { name: 'RELAY TARGET relay-box' })

        expect(relayTarget).not.toBeNull()
        expect(screen.getByText('RELAY TARGET')).not.toBeNull()
        expect(screen.getByText('relay-box')).not.toBeNull()
        expect(screen.queryByLabelText('Offline')).toBeNull()
        expect(relayTarget.querySelector('.stream-info-status')?.getAttribute('data-tone')).toBe('online')
    })

    it('shows the Relay Link card as offline when the linked peer is unavailable', () => {
        render(
            <StatusView
                status={DEFAULT_STATUS}
                settings={hostRelaySettings}
                relayLinkStatus={{
                    ...linkedRelayStatus,
                    available: false,
                    reason: 'Connection refused.',
                }}
                isStopping={false}
                onStop={vi.fn()}
            />
        )

        const relayTarget = screen.getByRole('group', { name: 'RELAY TARGET relay-box' })

        expect(relayTarget).not.toBeNull()
        expect(screen.getByText('relay-box')).not.toBeNull()
        expect(screen.getByLabelText('Offline')).not.toBeNull()
        expect(relayTarget.querySelector('.stream-info-status')?.getAttribute('data-tone')).toBe('offline')
        expect(screen.getByText('Relay Offline')).not.toBeNull()
        expect(screen.getByText('You have a relay peer linked, but the peer is offline. Streams cannot be started until the peer comes online.')).not.toBeNull()
        expect(screen.queryByText('Awaiting Signal')).toBeNull()
    })

    it('hides the Relay Link card while a stream is active', () => {
        render(
            <StatusView
                status={makeLiveStatus()}
                settings={hostRelaySettings}
                relayLinkStatus={linkedRelayStatus}
                isStopping={false}
                onStop={vi.fn()}
            />
        )

        expect(screen.queryByRole('group', { name: 'RELAY TARGET relay-box' })).toBeNull()
    })

    it('hides the Relay Link card on the relay instance without an outbound token', () => {
        render(
            <StatusView
                status={DEFAULT_STATUS}
                settings={{
                    ...hostRelaySettings,
                    relay: {
                        ...hostRelaySettings.relay,
                        remoteToken: null,
                    },
                }}
                relayLinkStatus={linkedRelayStatus}
                isStopping={false}
                onStop={vi.fn()}
            />
        )

        expect(screen.queryByRole('group', { name: 'RELAY TARGET relay-box' })).toBeNull()
    })
})
