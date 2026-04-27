import { WifiOff } from 'lucide-react';

import type { RelayLinkStatus } from '@/lib/types';

export function RelayLinkCard({ relayLinkStatus }: { relayLinkStatus: RelayLinkStatus }) {
    if (!relayLinkStatus.linked) {
        return null;
    }

    const peer = relayLinkStatus.peer;
    const machineName = peer?.hostname ?? peer?.ip ?? 'Linked relay peer';
    const availabilityTone = relayLinkStatus.available === true
        ? 'online'
        : relayLinkStatus.available === false
            ? 'offline'
            : 'checking';
    const label = `RELAY TARGET ${machineName}`;

    return (
        <div className="stream-info-badges" role="group" aria-label={label}>
            <span className="stream-info-status" data-tone={availabilityTone}>
                <span className="stream-info-status-dot" aria-hidden="true" />
                <span className="stream-info-status-label">RELAY TARGET</span>
                <span className="stream-info-source relay-target-source">
                    {relayLinkStatus.available === false ? (
                        <WifiOff
                            className="relay-target-availability-icon"
                            aria-label="Offline"
                        />
                    ) : null}
                    <span>{machineName}</span>
                </span>
            </span>
        </div>
    );
}
