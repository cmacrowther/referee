import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { RelayCard } from './RelayCard';
import { DEFAULT_RELAY_LINK_STATUS, DEFAULT_SETTINGS } from '@/lib/types';
import { makeMockApi } from '@/test/mocks';

describe('RelayCard', () => {
    it('renders discovered peer metadata after scanning', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        vi.mocked(api.discoverLanPeers).mockResolvedValue([
            {
                instanceId: 'peer-1',
                ip: '192.168.1.25',
                hostname: 'media-box',
                version: '1.2.3',
                gpuReady: true,
                gpuVendor: 'nvidia',
                gpuName: 'RTX 4080',
            },
        ]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        await userEvent.click(screen.getByRole('button', { name: 'Scan' }));

        await waitFor(() => {
            expect(api.discoverLanPeers).toHaveBeenCalled();
        });

        expect(screen.getByText('media-box')).not.toBeNull();
        expect(screen.getByText('192.168.1.25 - v1.2.3')).not.toBeNull();
    });

    it('calls the link API for a discovered peer', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        const peer = {
            instanceId: 'peer-1',
            ip: '192.168.1.25',
            hostname: 'media-box',
            version: '1.2.3',
            gpuReady: true,
            gpuVendor: 'nvidia',
            gpuName: 'RTX 4080',
        };

        vi.mocked(api.discoverLanPeers).mockResolvedValue([peer]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        await userEvent.click(screen.getByRole('button', { name: 'Scan' }));
        await userEvent.click(await screen.findByRole('button', { name: 'Link' }));

        await waitFor(() => {
            expect(api.linkRelayPeer).toHaveBeenCalledWith(peer);
        });
    });

    it('renders the linked peer summary and allows unlinking', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(
            <RelayCard
                api={api}
                settings={{
                    ...DEFAULT_SETTINGS,
                    relay: {
                        enabled: true,
                        linkedPeerId: 'peer-1',
                        linkedPeerHostname: 'media-box',
                        linkedPeerIp: '192.168.1.25',
                        remoteToken: 'relay-secret',
                        lastKnownPeer: {
                            instanceId: 'peer-1',
                            hostname: 'media-box',
                            ip: '192.168.1.25',
                            version: '1.2.3',
                            gpuReady: true,
                            gpuVendor: 'nvidia',
                            gpuName: 'RTX 4080',
                        },
                    },
                }}
                relayLinkStatus={{
                    linked: true,
                    available: true,
                    peer: {
                        instanceId: 'peer-1',
                        hostname: 'media-box',
                        ip: '192.168.1.25',
                        version: '1.2.3',
                        gpuReady: true,
                        gpuVendor: 'nvidia',
                        gpuName: 'RTX 4080',
                    },
                    reason: null,
                }}
                onChange={onChange}
            />
        );

        await waitFor(() => {
            expect(api.getApprovedOrigins).toHaveBeenCalled();
        });

        expect(screen.getByText('Trusted for Relay')).not.toBeNull();
        expect(screen.getByText('Run streams on the linked REFEREE.')).not.toBeNull();
        expect(screen.getAllByText('media-box').length).toBeGreaterThan(0);
        expect(screen.getByText('192.168.1.25 - v1.2.3')).not.toBeNull();
        expect(screen.getAllByText('Online').length).toBeGreaterThan(0);

        await userEvent.click(screen.getByRole('button', { name: 'Unlink' }));

        await waitFor(() => {
            expect(api.unlinkRelayPeer).toHaveBeenCalledTimes(1);
        });
    });

    it('shows the empty-state message when no peers are found', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        vi.mocked(api.discoverLanPeers).mockResolvedValue([]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        await userEvent.click(screen.getByRole('button', { name: 'Scan' }));

        await waitFor(() => {
            expect(screen.getByText('No other REFEREE instances found on this network.')).not.toBeNull();
        });
    });

    it('opens network peers by default and hides local routing when no relay is linked', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        vi.mocked(api.discoverLanPeers).mockResolvedValue([]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        expect(screen.queryByRole('radio', { name: /This REFEREE/i })).toBeNull();

        await waitFor(() => {
            expect(screen.getByText('No other REFEREE instances found on this network.')).not.toBeNull();
        });
    });

    it('shows trusted peer approvals for incoming relay access', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        vi.mocked(api.getApprovedOrigins).mockResolvedValue([
            {
                origin: 'https://peer-1234abcd.referee.invalid',
                appName: 'REFEREE Relay (media-box)',
                approvedAt: '1700000000',
            },
        ]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        await waitFor(() => {
            expect(screen.getByText('Trusted for Relay')).not.toBeNull();
        });

        expect(screen.getByText('REFEREE Relay (media-box)')).not.toBeNull();
    });

    it('does not show trusted peers in the network peers list', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();
        vi.mocked(api.getApprovedOrigins).mockResolvedValue([
            {
                origin: 'https://peer-peer-1.referee.invalid',
                appName: 'REFEREE Relay (media-box)',
                approvedAt: '1700000000',
            },
        ]);
        vi.mocked(api.discoverLanPeers).mockResolvedValue([
            {
                instanceId: 'peer-1',
                ip: '192.168.1.25',
                hostname: 'media-box',
                version: '1.2.3',
                gpuReady: true,
                gpuVendor: 'nvidia',
                gpuName: 'RTX 4080',
            },
        ]);

        render(<RelayCard api={api} settings={DEFAULT_SETTINGS} relayLinkStatus={DEFAULT_RELAY_LINK_STATUS} onChange={onChange} />);

        await waitFor(() => {
            expect(api.discoverLanPeers).toHaveBeenCalled();
        });

        expect(screen.getByText('Trusted for Relay')).not.toBeNull();
        expect(screen.getByText('No other REFEREE instances found on this network.')).not.toBeNull();
        expect(screen.queryByRole('button', { name: 'Link' })).toBeNull();
    });

    it('lets the user choose whether streams run locally or through the linked peer', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(
            <RelayCard
                api={api}
                settings={{
                    ...DEFAULT_SETTINGS,
                    relay: {
                        enabled: false,
                        linkedPeerId: 'peer-1',
                        linkedPeerHostname: 'media-box',
                        linkedPeerIp: '192.168.1.25',
                        remoteToken: 'relay-secret',
                        lastKnownPeer: {
                            instanceId: 'peer-1',
                            hostname: 'media-box',
                            ip: '192.168.1.25',
                            version: '1.2.3',
                            gpuReady: true,
                            gpuVendor: 'nvidia',
                            gpuName: 'RTX 4080',
                        },
                    },
                }}
                relayLinkStatus={DEFAULT_RELAY_LINK_STATUS}
                onChange={onChange}
            />
        );

        expect(screen.getByRole('radio', { name: /This REFEREE/i })).not.toBeNull();
        expect(screen.getByRole('radio', { name: /media-box/i })).not.toBeNull();

        await userEvent.click(screen.getByRole('radio', { name: /media-box/i }));

        expect(onChange).toHaveBeenCalledWith({
            relay: {
                enabled: true,
                linkedPeerId: 'peer-1',
                linkedPeerHostname: 'media-box',
                linkedPeerIp: '192.168.1.25',
                remoteToken: 'relay-secret',
                lastKnownPeer: {
                    instanceId: 'peer-1',
                    hostname: 'media-box',
                    ip: '192.168.1.25',
                    version: '1.2.3',
                    gpuReady: true,
                    gpuVendor: 'nvidia',
                    gpuName: 'RTX 4080',
                },
            },
        });
    });

    it('disables local routing when relay is required by unsupported hardware', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(
            <RelayCard
                api={api}
                settings={{
                    ...DEFAULT_SETTINGS,
                    relay: {
                        enabled: false,
                        linkedPeerId: 'peer-1',
                        linkedPeerHostname: 'media-box',
                        linkedPeerIp: '192.168.1.25',
                        remoteToken: 'relay-secret',
                        lastKnownPeer: {
                            instanceId: 'peer-1',
                            hostname: 'media-box',
                            ip: '192.168.1.25',
                            version: '1.2.3',
                            gpuReady: true,
                            gpuVendor: 'nvidia',
                            gpuName: 'RTX 4080',
                        },
                    },
                }}
                relayLinkStatus={DEFAULT_RELAY_LINK_STATUS}
                requireRelayRoute
                onChange={onChange}
            />
        );

        await waitFor(() => {
            expect(api.getApprovedOrigins).toHaveBeenCalled();
        });

        expect((screen.getByRole('radio', { name: /This REFEREE/i }) as HTMLButtonElement).disabled).toBe(true);
        expect(screen.getByRole('radio', { name: /media-box/i }).getAttribute('aria-checked')).toBe('true');
        expect(screen.getByText('Unavailable on this hardware')).not.toBeNull();

        await userEvent.click(screen.getByRole('radio', { name: /This REFEREE/i }));

        expect(onChange).not.toHaveBeenCalled();
    });
});
