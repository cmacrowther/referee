import { Square } from 'lucide-react';
import { useEffect, useState } from 'react';

import { StreamInfoBadges, StreamInfoSegments } from '@/components/status/StreamInfoBadges';
import { StreamWhistle } from '@/components/status/StreamWhistle';
import { getActiveSession, getSessionCount, hasLiveSession, hasPendingSession } from '@/lib/stream';
import type { Settings, Status } from '@/lib/types';

interface StreamCardProps {
    status: Status;
    settings: Settings;
    isStopping: boolean;
    relaySetupRequired?: boolean;
    relayPeerOffline?: boolean;
    onOpenRelaySettings?: () => void;
    onStop: () => void;
}

interface StreamCopyState {
    key: string;
    kicker: string;
    title: string;
    body: string;
}

function StreamCopyPanel({
    copyState,
    animationClassName,
    ariaHidden = false
}: {
    copyState: StreamCopyState;
    animationClassName?: string;
    ariaHidden?: boolean;
}) {
    return (
        <div className={`stream-message-copy-panel${animationClassName ? ` ${animationClassName}` : ''}`} aria-hidden={ariaHidden}>
            <span className="stream-message-kicker">{copyState.kicker}</span>
            <h2 className="stream-message-title">{copyState.title}</h2>
            <div className="stream-message-body">{copyState.body}</div>
        </div>
    );
}

function StreamCopyTransition({ copyState }: { copyState: StreamCopyState }) {
    const [currentCopyState, setCurrentCopyState] = useState(copyState);
    const [outgoingCopyState, setOutgoingCopyState] = useState<StreamCopyState | null>(null);

    useEffect(() => {
        if (copyState.key === currentCopyState.key) {
            return;
        }

        setOutgoingCopyState(currentCopyState);
        setCurrentCopyState(copyState);

        const exitingKey = currentCopyState.key;
        const timeoutId = window.setTimeout(() => {
            setOutgoingCopyState(previousState => (previousState?.key === exitingKey ? null : previousState));
        }, 280);

        return () => window.clearTimeout(timeoutId);
    }, [copyState, currentCopyState]);

    return (
        <div className="stream-message-copy-stack">
            {outgoingCopyState ? (
                <StreamCopyPanel
                    copyState={outgoingCopyState}
                    animationClassName="stream-message-copy-panel-exit"
                    ariaHidden={true}
                />
            ) : null}
            <StreamCopyPanel
                copyState={currentCopyState}
                animationClassName={outgoingCopyState ? 'stream-message-copy-panel-enter' : undefined}
            />
        </div>
    );
}

export function StreamCard({
    status,
    settings,
    isStopping,
    relaySetupRequired = false,
    relayPeerOffline = false,
    onOpenRelaySettings,
    onStop
}: StreamCardProps) {
    const session = getActiveSession(status);
    const hasStreamSession = getSessionCount(status) > 0;
    const hasStartedStream = hasLiveSession(status);
    const isStreamStarting = hasPendingSession(status);
    const showRelaySetupPrompt = relaySetupRequired && !hasStreamSession;
    const showRelayOfflinePrompt = relayPeerOffline && !hasStreamSession && !showRelaySetupPrompt;
    const showRelaySettingsAction = showRelaySetupPrompt || showRelayOfflinePrompt;
    const whistleState = hasStartedStream ? 'active' : 'inactive';
    const whistleBadgeState = hasStartedStream ? 'live' : isStreamStarting ? 'starting' : 'hidden';
    const streamCopyState: StreamCopyState = hasStartedStream
        ? {
            key: 'active',
            kicker: 'Pipeline Active',
            title: 'Enhancing Stream',
            body: 'The whistle has blown - REFEREE is actively enhancing your content and relaying it back to the source player.'
        }
        : isStreamStarting
            ? {
                key: 'starting',
                kicker: 'Pipeline Starting',
                title: 'Preparing Stream',
                body: 'REFEREE has accepted the stream and is warming up the enhancement pipeline. Your output feed will be ready in a moment.'
            }
            : {
                key: showRelaySetupPrompt ? 'unsupported-hardware' : showRelayOfflinePrompt ? 'relay-offline' : 'idle',
                kicker: showRelaySetupPrompt ? 'Config Required' : 'Standby',
                title: showRelaySetupPrompt ? 'Relay Required' : showRelayOfflinePrompt ? 'Relay Offline' : 'Awaiting Signal',
                body: showRelaySetupPrompt
                    ? 'REFEREE is not compatible with your hardware, REFEREE Relay will be required to enhance streams on this device. Please set up REFEREE Relay to continue.'
                    : showRelayOfflinePrompt
                        ? 'You have a relay peer linked, but the peer is offline. Streams cannot be started until the peer comes online.'
                    : 'REFEREE is ready in the background and will jump into action as soon as the proxy sees incoming video.'
            };

    return (
        <>
            {hasStreamSession ? <StreamInfoSegments status={status} settings={settings} /> : null}
            <div
                id="stream-panel-card"
                className={`stream-message-card${hasStreamSession ? ' active' : ''}`}
                data-preview-state={hasStartedStream ? 'ready' : isStreamStarting ? 'loading' : 'idle'}
                data-relay-setup-required={showRelaySetupPrompt ? 'true' : 'false'}
                data-stopping={isStopping ? 'true' : 'false'}
            >
            {hasStreamSession ? <StreamInfoBadges status={status} settings={settings} /> : null}

            <div className="stream-message-shell" aria-live="polite">
                <div className="stream-message-copy">
                    <div className="stream-message-copy-content">
                        <StreamWhistle className="stream-message-whistle" state={whistleState} badgeState={whistleBadgeState} />
                        <StreamCopyTransition copyState={streamCopyState} />
                        {showRelaySettingsAction ? (
                            <button
                                type="button"
                                className="stream-message-outline-action"
                                onClick={onOpenRelaySettings}
                            >
                                Open Relay Settings
                            </button>
                        ) : null}
                    </div>
                </div>
            </div>
            <div className="stream-message-actions">
                <button
                    type="button"
                    className="stream-message-stop"
                    disabled={!session?.id || isStopping}
                    onClick={onStop}
                >
                    <span className="stream-message-button-icon" aria-hidden="true">
                        <Square className="size-3 fill-current" />
                    </span>
                    <span className="stream-message-button-label">{isStopping ? 'Stopping' : 'Stop'}</span>
                </button>
            </div>
        </div>
        </>
    );
}
