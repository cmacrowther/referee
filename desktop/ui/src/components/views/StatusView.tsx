import type { ReactNode } from 'react';

import { RelayLinkCard } from '@/components/status/RelayLinkCard';
import { StreamCard } from '@/components/status/StreamCard';
import { getSessionCount, hasLiveSession } from '@/lib/stream';
import type { RelayLinkStatus, Settings, Status } from '@/lib/types';

interface StatusViewProps {
    status: Status;
    settings: Settings;
    relayLinkStatus: RelayLinkStatus;
    requireRelayRoute?: boolean;
    isStopping: boolean;
    onOpenRelaySettings?: () => void;
    onStop: () => void;
    /** Debug builds only: shown under the card until a stream is live. */
    debugPanel?: ReactNode;
}

function hasConfiguredRelayPeer(settings: Settings) {
    return Boolean(
        settings.relay.linkedPeerId
        || settings.relay.linkedPeerIp
        || settings.relay.remoteToken
        || settings.relay.lastKnownPeer
    );
}

export function StatusView({
    status,
    settings,
    relayLinkStatus,
    requireRelayRoute = false,
    isStopping,
    onOpenRelaySettings,
    onStop,
    debugPanel
}: StatusViewProps) {
    const hasOutboundRelayLink = settings.relay.enabled
        && Boolean(settings.relay.remoteToken)
        && Boolean(settings.relay.linkedPeerId || settings.relay.linkedPeerIp || settings.relay.lastKnownPeer);
    const showRelayLinkCard = getSessionCount(status) === 0 && hasOutboundRelayLink;
    const relayPeerOffline = showRelayLinkCard && relayLinkStatus.linked && relayLinkStatus.available === false;
    const relaySetupRequired = requireRelayRoute && !hasConfiguredRelayPeer(settings);

    return (
        <section id="status-view" className="view-pane custom-scrollbar overflow-y-auto">
            <div className="flex min-h-full flex-col gap-3">
                {showRelayLinkCard ? <RelayLinkCard relayLinkStatus={relayLinkStatus} /> : null}
                <StreamCard
                    status={status}
                    settings={settings}
                    isStopping={isStopping}
                    relaySetupRequired={relaySetupRequired}
                    relayPeerOffline={relayPeerOffline}
                    onOpenRelaySettings={onOpenRelaySettings}
                    onStop={onStop}
                />
                {/* Stays mounted while a started stream is still warming up, so a failed start can report its error. */}
                {debugPanel && !hasLiveSession(status) ? debugPanel : null}
            </div>
        </section>
    );
}
