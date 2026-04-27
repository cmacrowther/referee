import { LockKeyhole, RadioTower } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';

import { AboutCard } from '@/components/settings/AboutCard';
import { AccessCard } from '@/components/settings/AccessCard';
import { ApplicationSettingsCard } from '@/components/settings/ApplicationSettingsCard';
import { HardwareDetectionCard } from '@/components/settings/HardwareDetectionCard';
import { PlayerSettingsCard } from '@/components/settings/PlayerSettingsCard';
import { RelayCard } from '@/components/settings/RelayCard';
import { UpscalerSettingsCard } from '@/components/settings/UpscalerSettingsCard';
import type { RelayLinkStatus, RendererApi, Settings, Status } from '@/lib/types';

interface SettingsViewProps {
    api: RendererApi;
    settings: Settings;
    status: Status;
    relayLinkStatus: RelayLinkStatus;
    requireRelayRoute?: boolean;
    bootSetting: boolean;
    onChangeSettings: (patch: Partial<Settings>) => void;
    onChangeStreamSettings?: (patch: Partial<Settings>) => void;
    onChangeBootSetting: (value: boolean) => void;
    onOpenRelaySettings?: () => void;
    isSavingRelayStreamSettings?: boolean;
    relayStreamSettingsError?: string | null;
    focusRequest?: SettingsFocusRequest | null;
    debugPanel?: ReactNode;
    isActive?: boolean;
}

const TABS = [
    { id: 'pipeline', label: 'Stream' },
    { id: 'app',      label: 'App' },
    { id: 'about',    label: 'About' },
] as const;

type SettingsTab = typeof TABS[number]['id'];

export interface SettingsFocusRequest {
    tab: SettingsTab;
    target?: 'relay';
    nonce: number;
}

/**
 * Renders the settings UI with tabbed navigation and conditional content panels.
 *
 * @param api - Renderer API instance passed to cards that need it (e.g., player, about)
 * @param settings - Current settings object applied to all settings cards
 * @param status - Runtime status used to determine locked state and provided to cards that require it
 * @param bootSetting - Initial/system boot flag passed to the system (Application) settings card
 * @param onChangeSettings - Callback invoked with a partial settings patch to apply updates
 * @param onChangeBootSetting - Callback invoked to update the boot setting
 * @param debugPanel - Optional React node rendered alongside the System settings card when that tab is active
 * @returns The settings view element
 */
export function SettingsView({
    api,
    settings,
    status,
    relayLinkStatus,
    requireRelayRoute = false,
    bootSetting,
    onChangeSettings,
    onChangeStreamSettings,
    onChangeBootSetting,
    onOpenRelaySettings,
    isSavingRelayStreamSettings = false,
    relayStreamSettingsError = null,
    focusRequest,
    debugPanel,
    isActive = true
}: SettingsViewProps) {
    const locked = status.activeSessions > 0;
    const [activeTab, setActiveTab] = useState<SettingsTab>('pipeline');
    const relayRouteEnabled = settings.relay.enabled
        && Boolean(
            settings.relay.linkedPeerId
            || settings.relay.linkedPeerIp
            || settings.relay.remoteToken
            || settings.relay.lastKnownPeer
        );
    const relayRoutePeerName = settings.relay.lastKnownPeer?.hostname
        ?? settings.relay.linkedPeerHostname
        ?? settings.relay.lastKnownPeer?.ip
        ?? settings.relay.linkedPeerIp
        ?? null;
    const relayRoutePeer = relayLinkStatus.peer ?? settings.relay.lastKnownPeer ?? null;
    const relayRouteOffline = relayRouteEnabled && relayLinkStatus.linked && relayLinkStatus.available === false;

    useEffect(() => {
        if (!focusRequest) {
            return;
        }

        setActiveTab(focusRequest.tab);
    }, [focusRequest]);

    useEffect(() => {
        if (!focusRequest || focusRequest.target !== 'relay' || activeTab !== 'app') {
            return;
        }

        const animationFrame = window.requestAnimationFrame(() => {
            document.getElementById('relay-settings-card')?.scrollIntoView?.({
                block: 'start',
                behavior: 'smooth',
            });
        });

        return () => {
            window.cancelAnimationFrame(animationFrame);
        };
    }, [activeTab, focusRequest]);

    return (
        <section id="settings-view" className="view-pane !p-0 flex flex-col overflow-hidden">
            <div className="shrink-0 px-2.5 pt-3 pb-3">
                <div className="flex rounded-lg bg-white/[0.04] p-0.5">
                {TABS.map(tab => {
                    const active = activeTab === tab.id;
                    const relayRemoteTab = tab.id === 'pipeline' && relayRouteEnabled;
                    const relayLockedTab = tab.id === 'pipeline' && ((requireRelayRoute && !relayRouteEnabled) || relayRouteOffline);
                    return (
                        <button
                            key={tab.id}
                            type="button"
                            onClick={() => setActiveTab(tab.id)}
                            className={[
                                'relative flex flex-1 items-center justify-center rounded-md py-1.5 transition-colors',
                                active
                                    ? 'bg-referee-accent/20 text-referee-accent shadow-sm'
                                    : relayLockedTab || relayRemoteTab
                                      ? 'text-[#FFB36B] hover:text-[#FFD7AE]'
                                    : 'text-referee-muted hover:text-white/60',
                            ].join(' ')}
                        >
                            <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-widest">
                                {relayLockedTab ? <LockKeyhole className="size-3" /> : null}
                                {relayRemoteTab ? <RadioTower className="size-3" /> : null}
                                {tab.label}
                            </span>
                        </button>
                    );
                })}
                </div>
            </div>

            <div className="custom-scrollbar flex-1 overflow-y-auto">
                <div className="flex flex-col gap-3 px-2.5 pb-3">
                    {isActive && activeTab === 'pipeline' && (
                        <UpscalerSettingsCard
                            settings={settings}
                            locked={locked}
                            relayLocked={requireRelayRoute && !relayRouteEnabled}
                            relayRemote={relayRouteEnabled}
                            relayRemoteOffline={relayRouteOffline}
                            relayUpdating={isSavingRelayStreamSettings}
                            relayUpdateError={relayStreamSettingsError}
                            relayRequired={requireRelayRoute}
                            relayPeerName={relayRoutePeerName}
                            relayPeer={relayRoutePeer}
                            status={status}
                            onChange={onChangeStreamSettings ?? onChangeSettings}
                            onOpenRelaySettings={onOpenRelaySettings}
                            engineSettings={!!debugPanel}
                        />
                    )}

                    {isActive && activeTab === 'app' && (
                        <>
                            <PlayerSettingsCard api={api} settings={settings} locked={locked} onChange={onChangeSettings} />
                            <ApplicationSettingsCard
                                settings={settings}
                                bootSetting={bootSetting}
                                onChange={onChangeSettings}
                                onBootChange={onChangeBootSetting}
                            />
                            <AccessCard api={api} />
                            <RelayCard
                                api={api}
                                settings={settings}
                                relayLinkStatus={relayLinkStatus}
                                requireRelayRoute={requireRelayRoute}
                                onChange={onChangeSettings}
                            />
                            {debugPanel}
                        </>
                    )}

                    {isActive && activeTab === 'about' && (
                        <>
                            <HardwareDetectionCard status={status} />
                            <AboutCard api={api} />
                        </>
                    )}
                </div>
            </div>
        </section>
    );
}
