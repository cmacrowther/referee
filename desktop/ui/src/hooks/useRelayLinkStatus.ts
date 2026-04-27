import { useCallback, useEffect, useRef, useState } from 'react';

import {
    DEFAULT_RELAY_LINK_STATUS,
    type RelayLinkStatus,
    type RelayPeerMetadata,
    type RendererApi,
    type Settings,
} from '@/lib/types';

const RELAY_LINK_STATUS_POLL_MS = 10000;

function getRelayLinkStatusSignature(status: RelayLinkStatus) {
    return JSON.stringify(status);
}

function buildSavedRelayPeer(settings: Settings): RelayPeerMetadata | null {
    const peer = settings.relay.lastKnownPeer;
    const savedPeer: RelayPeerMetadata = {
        instanceId: peer?.instanceId ?? settings.relay.linkedPeerId,
        hostname: peer?.hostname ?? settings.relay.linkedPeerHostname,
        ip: peer?.ip ?? settings.relay.linkedPeerIp,
        version: peer?.version ?? null,
        gpuReady: peer?.gpuReady ?? null,
        gpuVendor: peer?.gpuVendor ?? null,
        gpuName: peer?.gpuName ?? null,
    };

    return Object.values(savedPeer).some(value => value !== null) ? savedPeer : null;
}

function hasLinkedRelay(settings: Settings) {
    return settings.relay.enabled && buildSavedRelayPeer(settings) !== null;
}

export function useRelayLinkStatus(api: RendererApi, settings: Settings) {
    const [relayLinkStatus, setRelayLinkStatus] = useState<RelayLinkStatus>(DEFAULT_RELAY_LINK_STATUS);
    const relayLinkStatusSignatureRef = useRef(getRelayLinkStatusSignature(DEFAULT_RELAY_LINK_STATUS));

    const commitRelayLinkStatus = useCallback((nextStatus: RelayLinkStatus) => {
        const nextSignature = getRelayLinkStatusSignature(nextStatus);
        if (nextSignature === relayLinkStatusSignatureRef.current) {
            return;
        }

        relayLinkStatusSignatureRef.current = nextSignature;
        setRelayLinkStatus(nextStatus);
    }, []);

    useEffect(() => {
        let disposed = false;
        const savedPeer = buildSavedRelayPeer(settings);

        if (!hasLinkedRelay(settings) || !savedPeer) {
            commitRelayLinkStatus(DEFAULT_RELAY_LINK_STATUS);
            return;
        }

        const syncRelayStatus = async () => {
            try {
                const nextStatus = await api.getRelayLinkStatus();
                if (disposed) {
                    return;
                }

                commitRelayLinkStatus({
                    linked: nextStatus.linked,
                    available: nextStatus.available,
                    peer: nextStatus.peer ?? savedPeer,
                    reason: nextStatus.reason,
                });
            } catch {
                if (disposed) {
                    return;
                }

                commitRelayLinkStatus({
                    linked: true,
                    available: false,
                    peer: savedPeer,
                    reason: 'Unable to refresh relay status right now.',
                });
            }
        };

        commitRelayLinkStatus({
            linked: true,
            available: null,
            peer: savedPeer,
            reason: null,
        });

        syncRelayStatus();
        const interval = window.setInterval(syncRelayStatus, RELAY_LINK_STATUS_POLL_MS);

        return () => {
            disposed = true;
            window.clearInterval(interval);
        };
    }, [
        api,
        settings.relay.enabled,
        settings.relay.linkedPeerId,
        settings.relay.linkedPeerHostname,
        settings.relay.linkedPeerIp,
        settings.relay.lastKnownPeer?.instanceId,
        settings.relay.lastKnownPeer?.hostname,
        settings.relay.lastKnownPeer?.ip,
        settings.relay.lastKnownPeer?.version,
        settings.relay.lastKnownPeer?.gpuReady,
        settings.relay.lastKnownPeer?.gpuVendor,
        settings.relay.lastKnownPeer?.gpuName,
        commitRelayLinkStatus,
    ]);

    return relayLinkStatus;
}
