import { DEFAULT_RELAY_LINK_STATUS, DEFAULT_SETTINGS, DEFAULT_STATUS, type ApprovedOrigin, type ConsentRequest, type RelayPeer, type RendererApi, type Unsubscribe } from '@/lib/types';

const noopSubscription = () => () => {};

const mockApi: RendererApi = {
    isElectron: false,
    async ready() {
        return { setupNeeded: false, gpuVendor: 'unknown', gpuName: null };
    },
    async getSetupState() {
        return { setupNeeded: false, gpuVendor: 'unknown', gpuName: null };
    },
    async getInitialSettings() {
        return DEFAULT_SETTINGS;
    },
    saveSettings() {},
    async saveStreamSettings() {},
    async getBootSetting() {
        return false;
    },
    setBootSetting() {},
    async getAppVersion() {
        return 'dev';
    },
    async checkForUpdate() {
        return {
            currentVersion: 'dev',
            hasUpdate: false
        };
    },
    async downloadAndInstallUpdate() {},
    async stopStream() {
        return DEFAULT_STATUS;
    },
    async retrySetup() {},
    async detectPlayers() {
        return [
            {
                id: 'builtin',
                name: 'REFEREE Built-in Player',
                path: null,
                installed: true,
            },
            {
                id: 'custom',
                name: 'Custom Player',
                path: null,
                installed: false,
            },
        ];
    },
    async launchPlayer() {},
    openExternal(url: string) {
        if (typeof window !== 'undefined') {
            window.open(url, '_blank', 'noopener,noreferrer');
        }
    },
    openGithub() {
        if (typeof window !== 'undefined') {
            window.open('https://github.com/cmacrowther/referee', '_blank', 'noopener,noreferrer');
        }
    },
    minimizeWindow() {},
    closeWindow() {},
    togglePin() {},
    onNavigateView: noopSubscription,
    onStatusUpdate: noopSubscription,
    onSettingsSync: noopSubscription,
    onBootSettingSync: noopSubscription,
    onSetupGpuDetected: noopSubscription,
    onSetupProgress: noopSubscription,
    onSetupComplete: noopSubscription,
    onSetupError: noopSubscription,
    onUpdateProgress: noopSubscription,
    respondToConsent() {},
    async getPendingConsent() { return null; },
    onConsentRequest: noopSubscription,
    async getApprovedOrigins() { return []; },
    async revokeApprovedOrigin() {},
    async discoverLanPeers() { return []; },
    async getRelayLinkStatus() { return DEFAULT_RELAY_LINK_STATUS; },
    async linkRelayPeer(_peer: RelayPeer) {},
    async unlinkRelayPeer() {},
};

export function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// Pre-fetch the listen function once so the dynamic import is resolved
// before any subscription is attempted.
const tauriListenPromise = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
    ? import('@tauri-apps/api/event').then(m => m.listen).catch(() => null)
    : Promise.resolve(null);

function createTauriEventSubscription(eventName: string): (cb: (data: any) => void) => Unsubscribe {
    return (cb: (data: any) => void) => {
        let unlisten: (() => void) | null = null;
        let cancelled = false;

        tauriListenPromise.then((listen) => {
            if (cancelled || !listen) return;
            return listen(eventName, (event: { payload: any }) => {
                cb(event.payload);
            });
        }).then((fn) => {
            if (!fn) return;
            if (cancelled) {
                fn();
            } else {
                unlisten = fn;
            }
        }).catch((err) => {
            console.error(`[Tauri] Failed to subscribe to "${eventName}":`, err);
        });

        return () => {
            cancelled = true;
            unlisten?.();
        };
    };
}

function createTauriApi(): RendererApi {
    const invoke = async (cmd: string, args?: Record<string, unknown>) => {
        const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
        return tauriInvoke(cmd, args);
    };

    return {
        isElectron: false,

        async ready() {
            return invoke('renderer_ready') as any;
        },

        async getSetupState() {
            return invoke('get_setup_state') as any;
        },

        async getInitialSettings() {
            return invoke('get_initial_settings') as any;
        },

        saveSettings(settings) {
            invoke('save_settings', { settings });
        },

        async saveStreamSettings(settings) {
            await invoke('save_stream_settings', { settings });
        },

        async getBootSetting() {
            return invoke('get_boot_setting') as Promise<boolean>;
        },

        setBootSetting(enable: boolean) {
            invoke('set_boot_setting', { enable });
        },

        async getAppVersion() {
            return invoke('get_app_version') as Promise<string>;
        },

        async checkForUpdate() {
            return invoke('check_for_update') as any;
        },

        async downloadAndInstallUpdate(downloadUrl: string) {
            await invoke('download_and_install_update', { downloadUrl });
        },

        async stopStream(sessionId: string) {
            return invoke('stop_stream', { sessionId }) as any;
        },

        async retrySetup() {
            await invoke('retry_setup');
        },

        async detectPlayers() {
            return invoke('detect_players') as any;
        },

        async launchPlayer(url: string) {
            await invoke('launch_player', { url });
        },

        openExternal(url: string) {
            invoke('open_external', { url });
        },

        openGithub() {
            invoke('open_github');
        },

        minimizeWindow() {
            invoke('minimize_window');
        },

        closeWindow() {
            invoke('close_window');
        },

        togglePin(enable: boolean) {
            invoke('toggle_pin', { enable });
        },

        onNavigateView: createTauriEventSubscription('navigate-view'),
        onStatusUpdate: createTauriEventSubscription('status-update'),
        onSettingsSync: createTauriEventSubscription('settings-sync'),
        onBootSettingSync: createTauriEventSubscription('boot-setting-sync'),
        onSetupGpuDetected: createTauriEventSubscription('setup-gpu-detected'),
        onSetupProgress: createTauriEventSubscription('setup-progress'),
        onSetupComplete: createTauriEventSubscription('setup-complete'),
        onSetupError: createTauriEventSubscription('setup-error'),
        onUpdateProgress: createTauriEventSubscription('update-progress'),

        respondToConsent(nonce: string, approved: boolean, alwaysAllow: boolean) {
            invoke('respond_to_consent', { nonce, approved, alwaysAllow });
        },

        async getPendingConsent() {
            return invoke('get_pending_consent') as Promise<ConsentRequest | null>;
        },

        onConsentRequest: createTauriEventSubscription('consent-request'),

        async getApprovedOrigins() {
            return invoke('get_approved_origins') as Promise<ApprovedOrigin[]>;
        },

        async revokeApprovedOrigin(origin: string) {
            await invoke('revoke_approved_origin', { origin });
        },

        async discoverLanPeers() {
            return invoke('discover_lan_peers') as Promise<RelayPeer[]>;
        },

        async getRelayLinkStatus() {
            return invoke('get_linked_relay_status') as any;
        },

        async linkRelayPeer(peer: RelayPeer) {
            await invoke('link_relay_peer', { peer });
        },

        async unlinkRelayPeer() {
            await invoke('unlink_relay_peer');
        },
    };
}

export function getRendererApi(): RendererApi {
    if (isTauri()) {
        return createTauriApi();
    }

    if (typeof window !== 'undefined' && window.referee) {
        return window.referee;
    }

    return mockApi;
}
