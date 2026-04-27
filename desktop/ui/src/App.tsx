import { useEffect, useMemo, useRef, useState } from 'react';

import { AppShell } from '@/components/layout/AppShell';
import { TitleBar } from '@/components/layout/TitleBar';
import { ConsentModal } from '@/components/ConsentModal';
import { SettingsView } from '@/components/views/SettingsView';
import { SetupView } from '@/components/views/SetupView';
import { StatusView } from '@/components/views/StatusView';
import type { SettingsFocusRequest } from '@/components/views/SettingsView';
import { useRendererApi } from '@/hooks/useRendererApi';
import { useRelayLinkStatus } from '@/hooks/useRelayLinkStatus';
import { isTauri } from '@/lib/renderer-api';
import { useSettings } from '@/hooks/useSettings';
import { useSetup } from '@/hooks/useSetup';
import { useStatus } from '@/hooks/useStatus';
import { useViewRouter } from '@/hooks/useViewRouter';
import { useRendererDebugPanel } from '@/dev/renderer-debug';
import { getActiveSession, requiresRelayForLocalHardware } from '@/lib/stream';
import type { AppView, ConsentRequest, Settings } from '@/lib/types';

export default function App() {
    const api = useRendererApi();
    const { currentView, setView } = useViewRouter(api);
    const { status, setStatus } = useStatus(api);
    const {
        settings,
        bootSetting,
        isReady,
        isSavingRelayStreamSettings,
        relayStreamSettingsError,
        saveSettings,
        saveStreamSettings,
        updateBootSetting
    } = useSettings(api);
    const relayLinkStatus = useRelayLinkStatus(api, settings);
    const { setupState, retrySetup, handleSetupResponse } = useSetup(api, setView);
    const rendererReadyRef = useRef(false);
    const lastContentViewRef = useRef<AppView>('status');
    const autoLaunchAttemptRef = useRef<string | null>(null);
    const [isStoppingStream, setIsStoppingStream] = useState(false);
    const [settingsFocusRequest, setSettingsFocusRequest] = useState<SettingsFocusRequest | null>(null);

    const activeSession = useMemo(() => getActiveSession(status), [status]);
    const relayRequiredForLocalHardware = useMemo(
        () => requiresRelayForLocalHardware(status, setupState),
        [setupState, status]
    );
    const hasSavedRelayPeer = Boolean(
        settings.relay.linkedPeerId
        || settings.relay.linkedPeerIp
        || settings.relay.remoteToken
        || settings.relay.lastKnownPeer
    );
    const autoLaunchPlayerKey = useMemo(() => {
        if (!settings.player.enabled || !activeSession?.startupComplete || !activeSession.outputUrl) {
            return null;
        }

        const selectedPlayer = settings.player.selectedPlayer;
        if (!selectedPlayer) {
            return null;
        }

        if (selectedPlayer === 'custom') {
            const customPath = settings.player.customPath?.trim();
            if (!customPath) {
                return null;
            }

            return `${activeSession.id ?? activeSession.outputUrl}:${activeSession.outputUrl}:custom:${customPath}`;
        }

        return `${activeSession.id ?? activeSession.outputUrl}:${activeSession.outputUrl}:${selectedPlayer}`;
    }, [
        activeSession?.id,
        activeSession?.outputUrl,
        activeSession?.startupComplete,
        settings.player.customPath,
        settings.player.enabled,
        settings.player.selectedPlayer
    ]);
    const { effectiveView, effectiveStatus, effectiveSetupState, settingsPanel, isSimulating } = useRendererDebugPanel({
        currentView,
        status,
        setupState
    });
    const [consentRequest, setConsentRequest] = useState<ConsentRequest | null>(null);
    const hasConsentRequestRef = useRef(false);
    const dismissedConsentNonceRef = useRef<string | null>(null);

    useEffect(() => {
        hasConsentRequestRef.current = consentRequest !== null;
    }, [consentRequest]);

    useEffect(() => {
        const unsub = api.onConsentRequest(request => {
            if (request.nonce === dismissedConsentNonceRef.current) return;
            setConsentRequest(previousRequest => (
                previousRequest?.nonce === request.nonce ? previousRequest : request
            ));
        });

        let mounted = true;
        const tauriRuntime = isTauri();
        const checkPending = () => {
            if (!tauriRuntime || hasConsentRequestRef.current) return;
            api.getPendingConsent().then(request => {
                if (!mounted || !request || request.nonce === dismissedConsentNonceRef.current) return;
                setConsentRequest(previousRequest => (
                    previousRequest?.nonce === request.nonce ? previousRequest : request
                ));
            }).catch(() => {});
        };

        // Poll for a pending consent shortly after mount to close the race where the
        // Tauri "consent-request" event fires before the async listener is registered.
        // We retry every 150 ms for up to 2 s, then stop. The interval is cleared early
        // if the event fires through the normal listener path (consentRequest becomes
        // non-null) or if the component unmounts.
        let pollCount = 0;
        const MAX_POLLS = 14; // ~2 s at 150 ms
        const pollTimer = setInterval(() => {
            pollCount++;
            checkPending();
            if (pollCount >= MAX_POLLS) clearInterval(pollTimer);
        }, 150);
        const pendingConsentTimer = tauriRuntime
            ? setInterval(checkPending, 5000)
            : null;

        // Keep a lightweight fallback poll and re-check on focus. This catches missed
        // Tauri events when REFEREE is raised from the tray or unminimized without a
        // DOM focus event, so the consent drawer appears without requiring a click.
        window.addEventListener('focus', checkPending);

        return () => {
            mounted = false;
            clearInterval(pollTimer);
            if (pendingConsentTimer !== null) clearInterval(pendingConsentTimer);
            unsub();
            window.removeEventListener('focus', checkPending);
        };
    }, [api]);

    function handleConsentRespond(nonce: string, approved: boolean, alwaysAllow: boolean) {
        dismissedConsentNonceRef.current = nonce;
        api.respondToConsent(nonce, approved, alwaysAllow);
        setConsentRequest(null);
    }

    useEffect(() => {
        if (!isReady || rendererReadyRef.current) {
            return;
        }

        rendererReadyRef.current = true;
        if (isTauri()) {
            api.ready().then(result => {
                handleSetupResponse(result);
            });
        }
    }, [api, isReady, handleSetupResponse]);

    useEffect(() => {
        if (status.activeSessions === 0) {
            setIsStoppingStream(false);
        }
    }, [status.activeSessions]);

    useEffect(() => {
        if (!relayRequiredForLocalHardware || !hasSavedRelayPeer || settings.relay.enabled) {
            return;
        }

        saveSettings({
            relay: {
                ...settings.relay,
                enabled: true,
            },
        });
    }, [hasSavedRelayPeer, relayRequiredForLocalHardware, saveSettings, settings.relay]);

    useEffect(() => {
        if (!autoLaunchPlayerKey || !activeSession?.outputUrl) {
            autoLaunchAttemptRef.current = null;
            return;
        }

        if (autoLaunchAttemptRef.current === autoLaunchPlayerKey) {
            return;
        }

        autoLaunchAttemptRef.current = autoLaunchPlayerKey;
        api.launchPlayer(activeSession.outputUrl).catch(err => {
            console.error('[Player] Failed to launch player:', err);
        });
    }, [activeSession?.outputUrl, api, autoLaunchPlayerKey]);

    useEffect(() => {
        if (currentView !== 'settings') {
            lastContentViewRef.current = currentView;
        }
    }, [currentView]);

    async function handleStopStream() {
        if (!activeSession?.id || isStoppingStream) {
            return;
        }

        setIsStoppingStream(true);
        try {
            const nextStatus = await api.stopStream(activeSession.id);
            setStatus(nextStatus);
        } catch {
            setIsStoppingStream(false);
        }
    }

    function handleChangeSettings(patch: Partial<Settings>) {
        saveSettings(patch);
    }

    function handleChangeStreamSettings(patch: Partial<Settings>) {
        saveStreamSettings(patch);
    }

    function handleToggleView() {
        if (currentView === 'settings') {
            setView(lastContentViewRef.current === 'setup' ? 'setup' : 'status');
            return;
        }

        setView('settings');
    }

    function handleOpenRelaySettings() {
        setSettingsFocusRequest(previousRequest => ({
            tab: 'app',
            target: 'relay',
            nonce: (previousRequest?.nonce ?? 0) + 1,
        }));
        setView('settings');
    }

    return (
        <AppShell
            view={effectiveView}
            debugAccessEnabled={isSimulating && effectiveView === 'setup'}
            overlay={consentRequest && (
                <ConsentModal request={consentRequest} onRespond={handleConsentRespond} />
            )}
            titleBar={
                <TitleBar
                    view={effectiveView}
                    pinned={settings.alwaysOnTop}
                    onToggleView={handleToggleView}
                    onTogglePin={() => {
                        saveSettings(previous => ({
                            ...previous,
                            alwaysOnTop: !previous.alwaysOnTop
                        }));
                    }}
                    onMinimize={() => {
                        api.minimizeWindow();
                    }}
                    onClose={() => {
                        api.closeWindow();
                    }}
                />
            }
        >
            <StatusView
                status={effectiveStatus}
                settings={settings}
                relayLinkStatus={relayLinkStatus}
                requireRelayRoute={relayRequiredForLocalHardware}
                isStopping={isStoppingStream}
                onOpenRelaySettings={handleOpenRelaySettings}
                onStop={handleStopStream}
            />
            <SettingsView
                api={api}
                settings={settings}
                status={status}
                relayLinkStatus={relayLinkStatus}
                requireRelayRoute={relayRequiredForLocalHardware}
                bootSetting={bootSetting}
                onChangeSettings={handleChangeSettings}
                onChangeStreamSettings={handleChangeStreamSettings}
                onChangeBootSetting={updateBootSetting}
                onOpenRelaySettings={handleOpenRelaySettings}
                isSavingRelayStreamSettings={isSavingRelayStreamSettings}
                relayStreamSettingsError={relayStreamSettingsError}
                focusRequest={settingsFocusRequest}
                debugPanel={settingsPanel}
                isActive={effectiveView === 'settings'}
            />
            <SetupView setupState={effectiveSetupState} onRetry={retrySetup} />
        </AppShell>
    );
}
