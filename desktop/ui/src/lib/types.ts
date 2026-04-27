export type AppView = 'status' | 'settings' | 'setup';
export type ExecutorPreference = 'auto' | 'nvidiaAi' | 'amdAi' | 'universal';

export interface EncodingProfile {
    bitrate: number;
    max_bitrate: number;
    preset: string;
    lookahead: number;
    bframes: number;
    hls_time: number;
}

export interface PlayerSettings {
    enabled: boolean;
    selectedPlayer: string | null;
    customPath: string | null;
}

export interface RelayPeerMetadata {
    instanceId: string | null;
    hostname: string | null;
    ip: string | null;
    version: string | null;
    platform?: string | null;
    gpuReady: boolean | null;
    gpuVendor: string | null;
    gpuName: string | null;
}

export interface RelaySettings {
    enabled: boolean;
    linkedPeerId: string | null;
    linkedPeerHostname: string | null;
    linkedPeerIp: string | null;
    remoteToken: string | null;
    lastKnownPeer: RelayPeerMetadata | null;
}

export interface RelayLinkStatus {
    linked: boolean;
    available: boolean | null;
    peer: RelayPeerMetadata | null;
    reason: string | null;
}

export interface DetectedPlayer {
    id: string;
    name: string;
    path: string | null;
    installed: boolean;
}

export interface Settings {
    resolution: string;
    quality: number;
    framegen: boolean;
    hdr: boolean;
    executorPreference: ExecutorPreference;
    alwaysOnTop: boolean;
    showOnProxyStart: boolean;
    minimizeToTray: boolean;
    closeToTray: boolean;
    notifications: boolean;
    encodingProfiles: Record<string, EncodingProfile>;
    player: PlayerSettings;
    instanceId: string | null;
    relay: RelaySettings;
}

export interface StatusSettings {
    resolution: string;
    quality: number;
    framegen: boolean;
    hdr: boolean;
}

export interface Session {
    id: string | null;
    sourceUrl: string | null;
    outputUrl: string | null;
    appName: string | null;
    streamTitle: string | null;
    sourceContentKind: string | null;
    upscaler: string | null;
    sourceResolution: string | null;
    outputResolution: string | null;
    sourceFps: number | null;
    targetFps: number | null;
    framegenEnabled: boolean;
    hdrEnabled: boolean;
    qualityLevel: number | null;
    executor: string | null;
    encoderBackend: string | null;
    startupComplete: boolean;
    retryingStartup: boolean;
}

export interface Status {
    gpuReady: boolean | null;
    gpuName: string | null;
    gpuVendor: string | null;
    gpuUtilization: number | null;
    encoderBackend: string | null;
    selectedExecutor: string | null;
    nvidiaAiAvailable: boolean | null;
    amdAiAvailable: boolean | null;
    activeSessions: number;
    sessions: Session[];
    primarySession: Session | null;
    settings: StatusSettings;
    encoderHasFramegen: boolean | null;
    encoderHasTruehdr: boolean | null;
    encoderHasRife: boolean | null;
}

export type SettingsUiSupportKind = 'native' | 'fallback' | 'unavailable';
export type SettingsUiLockState = 'editable' | 'locked';
export type SettingsUiResolvedPathKind = 'nvidiaNative' | 'amdNative' | 'universal' | 'cpu' | 'pending';

export interface SettingsUiFeatureSupport {
    support: SettingsUiSupportKind;
    lockState: SettingsUiLockState;
    available: boolean;
    disabled: boolean;
    sourceLabel: string | null;
    reason: string | null;
}

export interface SettingsUiRendererPreferenceOption {
    value: ExecutorPreference;
    label: string;
    description: string;
    availability: SettingsUiSupportKind;
    lockState: SettingsUiLockState;
    selectable: boolean;
    disabled: boolean;
    reason: string | null;
}

export interface SettingsUiResolvedPath {
    kind: SettingsUiResolvedPathKind;
    label: string;
    reason: string | null;
}

export interface SettingsUiCapabilities {
    lockState: SettingsUiLockState;
    rendererPreferenceOptions: SettingsUiRendererPreferenceOption[];
    resolvedPath: SettingsUiResolvedPath;
    features: {
        upscale: SettingsUiFeatureSupport;
        framegen: SettingsUiFeatureSupport;
        hdr: SettingsUiFeatureSupport;
    };
    quality: {
        max: number;
        lockState: SettingsUiLockState;
    };
    advancedEncoding: {
        lockState: SettingsUiLockState;
        profileShape: 'nvenc' | 'generic';
        reason: string | null;
    };
}

export interface SetupGpuInfo {
    vendor: string;
    name: string;
}

export interface SetupProgress {
    phase: string;
    percent: number;
    detail: string;
}

export interface SetupState {
    gpu: SetupGpuInfo | null;
    progress: SetupProgress | null;
    error: string | null;
    complete: boolean;
}

export interface UpdateInfo {
    currentVersion: string;
    latestVersion?: string;
    hasUpdate: boolean;
    downloadUrl?: string;
}

export type Unsubscribe = () => void;

export interface ConsentRequest {
    nonce: string;
    origin: string;
    appName: string | null;
}

export interface ApprovedOrigin {
    origin: string;
    appName: string | null;
    approvedAt: string;
}

export interface RelayPeer {
    instanceId: string | null;
    ip: string;
    hostname: string;
    version: string;
    platform?: string | null;
    gpuReady: boolean | null;
    gpuVendor: string | null;
    gpuName: string | null;
}

export interface SetupReadyResponse {
    setupNeeded: boolean;
    gpuVendor: string;
    gpuName: string | null;
    setupInProgress?: boolean;
    setupComplete?: boolean;
    setupProgress?: SetupProgress | null;
    setupError?: string | null;
}

export interface RendererApi {
    isElectron: boolean;
    ready(): Promise<SetupReadyResponse>;
    getSetupState(): Promise<SetupReadyResponse>;
    getInitialSettings(): Promise<Partial<Settings>>;
    saveSettings(settings: Settings): void;
    saveStreamSettings(settings: Settings): Promise<void>;
    getBootSetting(): Promise<boolean>;
    setBootSetting(enable: boolean): void;
    getAppVersion(): Promise<string>;
    checkForUpdate(): Promise<UpdateInfo>;
    downloadAndInstallUpdate(downloadUrl: string): Promise<void>;
    stopStream(sessionId: string): Promise<Status>;
    retrySetup(): Promise<void>;
    openExternal(url: string): void;
    openGithub(): void;
    minimizeWindow(): void;
    closeWindow(): void;
    togglePin(enable: boolean): void;
    onNavigateView(cb: (view: AppView) => void): Unsubscribe;
    onStatusUpdate(cb: (status: Partial<Status>) => void): Unsubscribe;
    onSettingsSync(cb: (settings: Partial<Settings>) => void): Unsubscribe;
    onBootSettingSync(cb: (isEnabled: boolean) => void): Unsubscribe;
    onSetupGpuDetected(cb: (gpu: SetupGpuInfo) => void): Unsubscribe;
    onSetupProgress(cb: (progress: SetupProgress) => void): Unsubscribe;
    onSetupComplete(cb: () => void): Unsubscribe;
    onSetupError(cb: (error: { message?: string }) => void): Unsubscribe;
    onUpdateProgress(cb: (percent: number) => void): Unsubscribe;
    detectPlayers(): Promise<DetectedPlayer[]>;
    launchPlayer(url: string): Promise<void>;
    respondToConsent(nonce: string, approved: boolean, alwaysAllow: boolean): void;
    getPendingConsent(): Promise<ConsentRequest | null>;
    onConsentRequest(cb: (request: ConsentRequest) => void): Unsubscribe;
    getApprovedOrigins(): Promise<ApprovedOrigin[]>;
    revokeApprovedOrigin(origin: string): Promise<void>;
    discoverLanPeers(): Promise<RelayPeer[]>;
    getRelayLinkStatus(): Promise<RelayLinkStatus>;
    linkRelayPeer(peer: RelayPeer): Promise<void>;
    unlinkRelayPeer(): Promise<void>;
}

export const DEFAULT_ENCODING_PROFILES: Record<string, EncodingProfile> = {
    '1920x1080': {
        bitrate: 25000,
        max_bitrate: 37500,
        preset: 'p4',
        lookahead: 8,
        bframes: 3,
        hls_time: 1,
    },
    '2560x1440': {
        bitrate: 35000,
        max_bitrate: 52500,
        preset: 'p4',
        lookahead: 12,
        bframes: 3,
        hls_time: 1,
    },
    '3840x2160': {
        bitrate: 50000,
        max_bitrate: 75000,
        preset: 'p4',
        lookahead: 8,
        bframes: 3,
        hls_time: 1,
    },
};

export const DEFAULT_PLAYER_SETTINGS: PlayerSettings = {
    enabled: false,
    selectedPlayer: null,
    customPath: null,
};

export const DEFAULT_RELAY_SETTINGS: RelaySettings = {
    enabled: false,
    linkedPeerId: null,
    linkedPeerHostname: null,
    linkedPeerIp: null,
    remoteToken: null,
    lastKnownPeer: null,
};

export const DEFAULT_RELAY_LINK_STATUS: RelayLinkStatus = {
    linked: false,
    available: null,
    peer: null,
    reason: null,
};

export const DEFAULT_SETTINGS: Settings = {
    resolution: '1920x1080',
    quality: 2,
    framegen: true,
    hdr: true,
    executorPreference: 'auto',
    alwaysOnTop: false,
    showOnProxyStart: false,
    minimizeToTray: false,
    closeToTray: true,
    notifications: false,
    encodingProfiles: DEFAULT_ENCODING_PROFILES,
    player: DEFAULT_PLAYER_SETTINGS,
    instanceId: null,
    relay: DEFAULT_RELAY_SETTINGS,
};

export const DEFAULT_STATUS: Status = {
    gpuReady: false,
    gpuName: null,
    gpuVendor: null,
    gpuUtilization: null,
    encoderBackend: null,
    selectedExecutor: null,
    nvidiaAiAvailable: null,
    amdAiAvailable: null,
    activeSessions: 0,
    sessions: [],
    primarySession: null,
    settings: {
        resolution: DEFAULT_SETTINGS.resolution,
        quality: DEFAULT_SETTINGS.quality,
        framegen: DEFAULT_SETTINGS.framegen,
        hdr: DEFAULT_SETTINGS.hdr,
    },
    encoderHasFramegen: null,
    encoderHasTruehdr: null,
    encoderHasRife: null,
};

export const DEFAULT_SETUP_STATE: SetupState = {
    gpu: null,
    progress: null,
    error: null,
    complete: false,
};
