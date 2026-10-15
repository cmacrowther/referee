import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { makeMockApi } from '@/test/mocks';
import { DEFAULT_RELAY_LINK_STATUS, DEFAULT_SETTINGS, DEFAULT_STATUS } from '@/lib/types';
import type { RelayLinkStatus, Settings } from '@/lib/types';

import { SettingsView } from './SettingsView';

const linkedRelaySettings: Settings = {
    ...DEFAULT_SETTINGS,
    relay: {
        enabled: true,
        linkedPeerId: 'relay-1',
        linkedPeerHostname: 'media-box',
        linkedPeerIp: '192.168.1.50',
        remoteToken: 'relay-secret',
        lastKnownPeer: {
            instanceId: 'relay-1',
            hostname: 'media-box',
            ip: '192.168.1.50',
            version: '1.0.0',
            platform: 'linux',
            gpuReady: true,
            gpuVendor: 'nvidia',
            gpuName: 'RTX',
        },
    },
};

const offlineRelayStatus: RelayLinkStatus = {
    ...DEFAULT_RELAY_LINK_STATUS,
    linked: true,
    available: false,
    peer: linkedRelaySettings.relay.lastKnownPeer,
    reason: 'Could not reach media-box.',
};

const onlineRelayStatus: RelayLinkStatus = {
    ...DEFAULT_RELAY_LINK_STATUS,
    linked: true,
    available: true,
    peer: linkedRelaySettings.relay.lastKnownPeer,
    reason: null,
};

describe('SettingsView - remote relay settings', () => {
    it('enables Motion Boost settings for an online NVIDIA relay peer', () => {
        const { api } = makeMockApi();
        render(
            <SettingsView
                api={api}
                settings={linkedRelaySettings}
                status={{ ...DEFAULT_STATUS, encoderHasFramegen: false }}
                relayLinkStatus={onlineRelayStatus}
                bootSetting={false}
                onChangeSettings={vi.fn()}
                onChangeBootSetting={vi.fn()}
            />
        );

        const motionBoostToggle = screen.getByRole('switch', { name: 'Motion Boost' }) as HTMLInputElement;

        expect(motionBoostToggle.disabled).toBe(false);
    });

    it('locks stream settings when the selected relay peer is offline', () => {
        const { api } = makeMockApi();
        const { container } = render(
            <SettingsView
                api={api}
                settings={linkedRelaySettings}
                status={DEFAULT_STATUS}
                relayLinkStatus={offlineRelayStatus}
                bootSetting={false}
                onChangeSettings={vi.fn()}
                onChangeBootSetting={vi.fn()}
            />
        );

        expect(screen.getByText('Remote Relay Settings')).not.toBeNull();
        expect(screen.getByText('media-box is offline')).not.toBeNull();
        expect((container.querySelector('#upscaler-controls') as HTMLFieldSetElement | null)?.disabled).toBe(true);
    });
});

describe('SettingsView - tabs', () => {
    it('switches to the Relay tab from the remote relay notice', async () => {
        const { api } = makeMockApi();
        render(
            <SettingsView
                api={api}
                settings={linkedRelaySettings}
                status={DEFAULT_STATUS}
                relayLinkStatus={onlineRelayStatus}
                bootSetting={false}
                onChangeSettings={vi.fn()}
                onChangeBootSetting={vi.fn()}
            />
        );

        expect(screen.getByRole('tab', { name: 'Stream' }).getAttribute('aria-selected')).toBe('true');

        fireEvent.click(screen.getByRole('button', { name: 'Open Relay Settings' }));

        expect(screen.getByRole('tab', { name: 'Relay' }).getAttribute('aria-selected')).toBe('true');
        expect(await screen.findByText('Route new streams locally or through Relay.')).not.toBeNull();
        expect(screen.queryByRole('switch', { name: 'Motion Boost' })).toBeNull();
    });
});
