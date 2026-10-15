import { SiLinux, SiMacos } from '@icons-pack/react-simple-icons';
import { Monitor, Radio, RefreshCw } from 'lucide-react';
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';

import { RadioDot, SettingsSection } from '@/components/settings/SettingsPrimitives';
import type { ApprovedOrigin, RelayLinkStatus, RelayPeer, RelayPeerMetadata, RendererApi, Settings } from '@/lib/types';
import { cn } from '@/lib/utils';

const RELAY_APPROVAL_REFRESH_MS = 15000;
const RELAY_PEER_DISCOVERY_REFRESH_MS = 30000;

interface RelayCardProps {
    api: RendererApi;
    settings: Settings;
    relayLinkStatus: RelayLinkStatus;
    requireRelayRoute?: boolean;
    onChange: (patch: Partial<Settings>) => void;
}

function getPeerReadinessLabel(peer: RelayPeer | RelayPeerMetadata) {
    if (peer.gpuReady === true) {
        return 'Ready for relay';
    }

    if (peer.gpuReady === false) {
        return 'Setup incomplete';
    }

    return 'Readiness unknown';
}

function formatPeerGpuSummary(peer: RelayPeer | RelayPeerMetadata) {
    const vendor = peer.gpuVendor?.trim();
    const normalizedVendor = vendor ? vendor.toUpperCase() : null;
    const parts = [normalizedVendor, peer.gpuName].filter(Boolean);
    return parts.length > 0 ? parts.join(' - ') : 'GPU details unavailable';
}

function WindowsPlatformIcon({ className }: { className: string }) {
    return (
        <svg className={className} viewBox="0 0 24 24" aria-hidden="true" fill="currentColor" xmlns="http://www.w3.org/2000/svg">
            <path d="M3 5.4 10.6 4v7.2H3V5.4Zm8.6-1.6L21 2v9.2h-9.4V3.8ZM3 12.8h7.6V20L3 18.6v-5.8Zm8.6 0H21V22l-9.4-1.8v-7.4Z" />
        </svg>
    );
}

function RelayPeerPlatformIcon({ platform, className }: { platform?: string | null; className: string }) {
    const normalized = platform?.trim().toLowerCase();

    if (normalized === 'windows' || normalized === 'win32') {
        return <WindowsPlatformIcon className={className} />;
    }

    if (normalized === 'macos' || normalized === 'darwin') {
        return <SiMacos className={className} aria-hidden="true" />;
    }

    if (normalized === 'linux') {
        return <SiLinux className={className} aria-hidden="true" />;
    }

    return <Monitor className={className} aria-hidden="true" />;
}

function describeLinkedPeer(settings: Settings) {
    const peer = settings.relay.lastKnownPeer;
    const hostname = peer?.hostname ?? settings.relay.linkedPeerHostname;
    const ip = peer?.ip ?? settings.relay.linkedPeerIp;
    const version = peer?.version;
    const heading = hostname ?? ip ?? 'Saved relay peer';
    const details = [ip, version ? `v${version}` : null].filter(Boolean).join(' - ');

    return {
        heading,
        details: details || 'Relay credentials are saved locally.',
        metadata: peer,
    };
}

function describePeerMetadata(peer: RelayPeerMetadata) {
    const hostname = peer.hostname ?? peer.ip;
    const ip = peer.ip;
    const version = peer.version;
    const heading = hostname ?? 'Saved relay peer';
    const details = [ip, version ? `v${version}` : null].filter(Boolean).join(' - ');

    return {
        heading,
        details: details || 'Relay credentials are saved locally.',
        metadata: peer,
    };
}

function isPeerLinked(peer: RelayPeer, settings: Settings) {
    if (settings.relay.linkedPeerId && peer.instanceId) {
        return settings.relay.linkedPeerId === peer.instanceId;
    }

    return Boolean(settings.relay.linkedPeerIp) && settings.relay.linkedPeerIp === peer.ip;
}

function errorMessage(error: unknown) {
    if (typeof error === 'string') {
        return error;
    }

    if (error instanceof Error && error.message.trim()) {
        return error.message;
    }

    return 'Unable to update relay link right now.';
}

function getAvailabilityPresentation(relayLinkStatus: RelayLinkStatus) {
    if (relayLinkStatus.available === true) {
        return {
            label: 'Online',
            pillClassName: 'border-emerald-500/30 bg-emerald-500/12 text-emerald-200',
            dotClassName: 'bg-emerald-400',
        };
    }

    if (relayLinkStatus.available === false) {
        return {
            label: 'Offline',
            pillClassName: 'border-red-500/30 bg-red-500/12 text-red-200',
            dotClassName: 'bg-red-400',
        };
    }

    return {
        label: 'Checking',
        pillClassName: 'border-white/[0.08] bg-white/[0.03] text-referee-muted',
        dotClassName: 'bg-referee-muted',
    };
}

function isRelayPeerOrigin(origin: string) {
    return /^https:\/\/peer-[a-z0-9-]+\.referee\.invalid$/i.test(origin);
}

function relayOriginDisplayName(origin: ApprovedOrigin) {
    if (origin.appName?.trim()) {
        return origin.appName.trim();
    }

    return origin.origin.replace(/^https?:\/\//, '');
}

function relayOriginPeerId(origin: ApprovedOrigin) {
    const match = origin.origin.match(/^https:\/\/peer-([a-z0-9-]+)\.referee\.invalid$/i);
    return match?.[1] ?? null;
}

function normalizeTrustToken(value: string | null | undefined) {
    return value?.trim().toLowerCase() ?? '';
}

function isPeerTrusted(peer: RelayPeer, settings: Settings, relayApprovals: ApprovedOrigin[]) {
    if (isPeerLinked(peer, settings)) {
        return true;
    }

    const peerId = normalizeTrustToken(peer.instanceId);
    const peerIdWithoutPrefix = peerId.replace(/^peer-/, '');
    const hostname = normalizeTrustToken(peer.hostname);
    const ip = normalizeTrustToken(peer.ip);

    return relayApprovals.some((origin) => {
        const originPeerId = normalizeTrustToken(relayOriginPeerId(origin));
        const displayName = normalizeTrustToken(relayOriginDisplayName(origin));

        return Boolean(
            (peerId && (originPeerId === peerId || originPeerId === peerIdWithoutPrefix))
            || (hostname && (displayName === hostname || displayName.includes(`(${hostname})`)))
            || (ip && displayName.includes(ip))
        );
    });
}

function relayApprovalPeerPlatform(origin: ApprovedOrigin, peers: RelayPeer[] | null) {
    if (!peers) {
        return null;
    }

    const originPeerId = normalizeTrustToken(relayOriginPeerId(origin));
    const displayName = normalizeTrustToken(relayOriginDisplayName(origin));

    return peers.find((peer) => {
        const peerId = normalizeTrustToken(peer.instanceId);
        const peerIdWithoutPrefix = peerId.replace(/^peer-/, '');
        const hostname = normalizeTrustToken(peer.hostname);
        const ip = normalizeTrustToken(peer.ip);

        return Boolean(
            (peerId && (originPeerId === peerId || originPeerId === peerIdWithoutPrefix))
            || (hostname && (displayName === hostname || displayName.includes(`(${hostname})`)))
            || (ip && displayName.includes(ip))
        );
    })?.platform ?? null;
}

function hasSavedRelayLink(settings: Settings) {
    return Boolean(
        settings.relay.linkedPeerId
        || settings.relay.linkedPeerIp
        || settings.relay.remoteToken
        || settings.relay.lastKnownPeer
    );
}

function RelayRouteOption({
    title,
    description,
    badge,
    hint,
    selected,
    disabled = false,
    onClick,
    statusPill,
}: {
    title: string;
    description: string;
    badge: string;
    hint?: string | null;
    selected: boolean;
    disabled?: boolean;
    onClick: () => void;
    statusPill?: { label: string; className: string } | null;
}) {
    const accessibilityLabel = [
        title,
        badge,
        statusPill?.label,
        description,
        hint,
    ].filter(Boolean).join('. ');

    return (
        <button
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={accessibilityLabel}
            disabled={disabled}
            onClick={onClick}
            className={cn(
                'flex w-full items-start gap-2.5 px-3 py-2 text-left transition-colors',
                disabled
                    ? 'cursor-not-allowed opacity-45'
                    : selected
                      ? 'bg-referee-accent/[0.08]'
                      : 'hover:bg-white/[0.03]'
            )}
        >
            <span className="mt-[2px]">
                <RadioDot selected={selected} />
            </span>
            <span className="min-w-0 flex-1">
                <span className="flex min-w-0 items-center justify-between gap-2">
                    <span className={cn('min-w-0 truncate text-[13px] font-medium leading-[18px]', selected ? 'text-white' : 'text-white/80')}>
                        {title}
                    </span>
                    {statusPill ? (
                        <span className={`shrink-0 rounded-full border px-1.5 py-px text-[9px] font-bold uppercase tracking-wider ${statusPill.className}`}>
                            {statusPill.label}
                        </span>
                    ) : (
                        <span className="shrink-0 text-[9px] font-bold uppercase tracking-wider text-referee-muted/80">
                            {badge}
                        </span>
                    )}
                </span>
                <span className="block text-[11px] leading-[15px] text-referee-muted">
                    {description}
                </span>
                {hint ? (
                    <span className="mt-0.5 block text-[10px] leading-[14px] text-referee-muted/80">
                        {hint}
                    </span>
                ) : null}
            </span>
        </button>
    );
}

const PEER_ACTION_CLASS =
    'shrink-0 rounded border border-white/[0.08] px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-referee-muted transition-colors hover:border-white/[0.16] hover:text-white disabled:cursor-not-allowed disabled:opacity-40';

function PeerRow({
    platform,
    name,
    details,
    action,
}: {
    platform?: string | null;
    name: string;
    details: string;
    action?: ReactNode;
}) {
    return (
        <div className="flex items-center gap-2.5 py-2 pl-3 pr-2">
            <RelayPeerPlatformIcon platform={platform} className="size-4 shrink-0 text-referee-muted/50" />
            <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-[12px] font-medium leading-4 text-white">{name}</span>
                <span className="truncate text-[11px] leading-[15px] text-referee-muted">{details}</span>
            </div>
            {action}
        </div>
    );
}

export function RelayCard({ api, settings, relayLinkStatus, requireRelayRoute = false, onChange }: RelayCardProps) {
    const [peers, setPeers] = useState<RelayPeer[] | null>(null);
    const [scanning, setScanning] = useState(false);
    const [linkingPeerKey, setLinkingPeerKey] = useState<string | null>(null);
    const [unlinking, setUnlinking] = useState(false);
    const [actionError, setActionError] = useState<string | null>(null);
    const [optimisticLinkedPeer, setOptimisticLinkedPeer] = useState<RelayPeerMetadata | null>(null);
    const [approvedPeerOrigins, setApprovedPeerOrigins] = useState<ApprovedOrigin[]>([]);
    const peerScanInFlightRef = useRef(false);

    const hasStoredLinkedPeer = hasSavedRelayLink(settings);
    const linkedPeer = hasStoredLinkedPeer
        ? describeLinkedPeer(settings)
        : optimisticLinkedPeer
          ? describePeerMetadata(optimisticLinkedPeer)
          : null;
    const relayEnabled = requireRelayRoute && hasStoredLinkedPeer
        ? true
        : hasStoredLinkedPeer
          ? settings.relay.enabled
          : Boolean(optimisticLinkedPeer);
    const liveLinkedPeer = relayLinkStatus.peer ?? linkedPeer?.metadata ?? optimisticLinkedPeer ?? null;
    const availability = getAvailabilityPresentation(relayLinkStatus);
    const relayApprovals = approvedPeerOrigins.filter(origin => isRelayPeerOrigin(origin.origin));
    const networkPeers = peers?.filter(peer => !isPeerTrusted(peer, settings, relayApprovals)) ?? null;
    const linkedPeerName = relayLinkStatus.peer?.hostname ?? linkedPeer?.heading ?? 'Linked REFEREE';
    const linkedPeerDetails = [
        relayLinkStatus.peer?.ip ?? linkedPeer?.metadata?.ip ?? settings.relay.linkedPeerIp,
        relayLinkStatus.peer?.version
            ? `v${relayLinkStatus.peer.version}`
            : linkedPeer?.metadata?.version
              ? `v${linkedPeer.metadata.version}`
              : null,
    ].filter(Boolean).join(' - ') || linkedPeer?.details || 'Relay credentials are saved locally.';
    const remoteRoutePill = relayEnabled
        ? { label: availability.label, className: availability.pillClassName }
        : { label: 'Standby', className: 'border-white/[0.08] bg-white/[0.03] text-referee-muted' };

    const loadApprovedPeers = useCallback(() => {
        api.getApprovedOrigins()
            .then(nextOrigins => {
                setApprovedPeerOrigins(previousOrigins => (
                    JSON.stringify(previousOrigins) === JSON.stringify(nextOrigins)
                        ? previousOrigins
                        : nextOrigins
                ));
            })
            .catch(() => {});
    }, [api]);

    useEffect(() => {
        loadApprovedPeers();
        const interval = window.setInterval(loadApprovedPeers, RELAY_APPROVAL_REFRESH_MS);
        window.addEventListener('focus', loadApprovedPeers);

        return () => {
            window.clearInterval(interval);
            window.removeEventListener('focus', loadApprovedPeers);
        };
    }, [loadApprovedPeers]);

    const refreshPeers = useCallback(async ({
        showIndicator = false,
        showError = false,
    }: {
        showIndicator?: boolean;
        showError?: boolean;
    } = {}) => {
        if (peerScanInFlightRef.current) {
            return;
        }

        peerScanInFlightRef.current = true;
        if (showIndicator) {
            setScanning(true);
        }
        if (showError) {
            setActionError(null);
        }

        try {
            const found = await api.discoverLanPeers();
            setPeers(previousPeers => (
                JSON.stringify(previousPeers) === JSON.stringify(found)
                    ? previousPeers
                    : found
            ));
        } catch {
            if (showError) {
                setPeers(previousPeers => previousPeers?.length === 0 ? previousPeers : []);
                setActionError('Unable to scan the local network right now.');
            }
        } finally {
            if (showIndicator) {
                setScanning(false);
            }
            peerScanInFlightRef.current = false;
        }
    }, [api]);

    useEffect(() => {
        void refreshPeers();
        const interval = window.setInterval(() => {
            void refreshPeers();
        }, RELAY_PEER_DISCOVERY_REFRESH_MS);

        return () => {
            window.clearInterval(interval);
        };
    }, [refreshPeers]);

    const scan = useCallback(async () => {
        await refreshPeers({ showIndicator: true, showError: true });
    }, [refreshPeers]);

    const linkPeer = useCallback(async (peer: RelayPeer) => {
        const peerKey = peer.instanceId ?? peer.ip;
        setLinkingPeerKey(peerKey);
        setActionError(null);

        try {
            await api.linkRelayPeer(peer);
            setOptimisticLinkedPeer({
                instanceId: peer.instanceId,
                hostname: peer.hostname,
                ip: peer.ip,
                version: peer.version,
                gpuReady: peer.gpuReady,
                gpuVendor: peer.gpuVendor,
                gpuName: peer.gpuName,
                platform: peer.platform,
            });
        } catch (error) {
            setActionError(errorMessage(error));
        } finally {
            setLinkingPeerKey(null);
        }
    }, [api]);

    const unlinkPeer = useCallback(async () => {
        setUnlinking(true);
        setActionError(null);

        try {
            await api.unlinkRelayPeer();
            setOptimisticLinkedPeer(null);
        } catch (error) {
            setActionError(errorMessage(error));
        } finally {
            setUnlinking(false);
        }
    }, [api]);

    const selectRelayRoute = useCallback((enabled: boolean) => {
        if (requireRelayRoute && !enabled) {
            return;
        }

        setActionError(null);
        onChange({
            relay: {
                ...settings.relay,
                enabled,
            },
        });
    }, [onChange, requireRelayRoute, settings.relay]);

    return (
        <div id="relay-settings" className="flex flex-col gap-3">
            <p className="px-1 text-[11px] leading-[15px] text-referee-muted">
                Route new streams locally or through Relay.
            </p>

            {linkedPeer ? (
                <SettingsSection title="Route">
                    <div role="radiogroup" aria-label="Relay route selection" className="flex flex-col divide-y divide-white/[0.05]">
                        <RelayRouteOption
                            title="This REFEREE"
                            badge="Local"
                            description={requireRelayRoute
                                ? 'Unavailable on this hardware'
                                : 'Run streams here; Stream settings stay editable.'}
                            hint={requireRelayRoute ? 'Stream settings are managed by the relay device.' : null}
                            selected={!relayEnabled}
                            disabled={requireRelayRoute}
                            onClick={() => selectRelayRoute(false)}
                        />

                        <RelayRouteOption
                            title={linkedPeerName}
                            badge="Linked"
                            description="Run streams on the linked REFEREE."
                            hint={relayEnabled && relayLinkStatus.available === false ? relayLinkStatus.reason : null}
                            statusPill={remoteRoutePill}
                            selected={relayEnabled}
                            onClick={() => selectRelayRoute(true)}
                        />
                    </div>
                </SettingsSection>
            ) : null}

            {linkedPeer || relayApprovals.length > 0 ? (
                <SettingsSection
                    title="Trusted for Relay"
                    aside={
                        <span className="shrink-0 rounded-full border border-white/[0.08] bg-white/[0.04] px-1.5 py-px text-[9px] font-semibold uppercase tracking-wider text-referee-muted">
                            {relayApprovals.length > 0
                                ? `${relayApprovals.length} Saved`
                                : 'Linked'}
                        </span>
                    }
                >
                    {linkedPeer ? (
                        <PeerRow
                            platform={liveLinkedPeer?.platform}
                            name={linkedPeerName}
                            details={linkedPeerDetails}
                            action={
                                <button
                                    type="button"
                                    onClick={unlinkPeer}
                                    disabled={unlinking || scanning || linkingPeerKey !== null}
                                    className={PEER_ACTION_CLASS}
                                >
                                    {unlinking ? 'Unlinking...' : 'Unlink'}
                                </button>
                            }
                        />
                    ) : null}

                    {relayApprovals.slice(0, 3).map((origin) => (
                        <PeerRow
                            key={origin.origin}
                            platform={relayApprovalPeerPlatform(origin, peers)}
                            name={relayOriginDisplayName(origin)}
                            details="Approved for relay control"
                        />
                    ))}
                </SettingsSection>
            ) : null}

            {actionError ? (
                <p role="alert" className="rounded-md border border-red-500/20 bg-red-500/10 px-2.5 py-2 text-[11px] text-red-100/90">
                    {actionError}
                </p>
            ) : null}

            <SettingsSection
                title="Network Peers"
                aside={
                    <button
                        type="button"
                        aria-label="Scan"
                        title="Scan"
                        onClick={() => {
                            void scan();
                        }}
                        disabled={scanning || unlinking || linkingPeerKey !== null}
                        className="-my-1 rounded p-1 text-referee-muted transition-colors hover:bg-white/[0.06] hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                    >
                        <RefreshCw className={`size-3 ${scanning ? 'animate-spin' : ''}`} />
                    </button>
                }
            >
                {networkPeers === null ? (
                    <p className="px-3 py-2.5 text-[11px] leading-[15px] text-referee-muted">
                        Searching this network...
                    </p>
                ) : networkPeers.length === 0 ? (
                    <div className="flex items-center gap-2.5 px-3 py-2.5">
                        <Radio className="size-4 shrink-0 text-referee-muted/50" />
                        <p className="text-[11px] leading-[15px] text-referee-muted">
                            No other REFEREE instances found on this network.
                        </p>
                    </div>
                ) : (
                    networkPeers.map((peer) => {
                        const peerKey = peer.instanceId ?? peer.ip;
                        const peerLinked = isPeerLinked(peer, settings);
                        const peerLinking = linkingPeerKey === peerKey;
                        const label = peerLinked ? 'Linked' : hasStoredLinkedPeer ? 'Replace Link' : 'Link';

                        return (
                            <PeerRow
                                key={peerKey}
                                platform={peer.platform}
                                name={peer.hostname}
                                details={`${peer.ip} - v${peer.version}`}
                                action={
                                    <button
                                        type="button"
                                        onClick={() => linkPeer(peer)}
                                        disabled={peerLinked || unlinking || scanning || linkingPeerKey !== null}
                                        className={PEER_ACTION_CLASS}
                                    >
                                        {peerLinking ? 'Linking...' : label}
                                    </button>
                                }
                            />
                        );
                    })
                )}
            </SettingsSection>
        </div>
    );
}
