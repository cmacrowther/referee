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

/** `managed`: REFEREE installs and runs EasyProxy. `external`: a server the user runs, at `url`. */
export type EasyProxyMode = 'managed' | 'external';

/** Routing of stream sources through an EasyProxy server (github.com/realbestia1/EasyProxy). */
export interface EasyProxySettings {
    enabled: boolean;
    mode: EasyProxyMode;
    /** External mode: base URL of the server. `null` means {@link DEFAULT_EASY_PROXY_URL}. */
    url: string | null;
    /** External mode: the server's API_PASSWORD, if it sets one. */
    apiPassword: string | null;
}

export interface EasyProxyStatus {
    version: string | null;
}

export type EasyProxyInstallState = 'unsupported' | 'missing' | 'outdated' | 'current';

/** State of the EasyProxy server REFEREE manages. */
export interface ManagedEasyProxyStatus {
    installState: EasyProxyInstallState;
    installing: boolean;
    progress: SetupProgress | null;
    error: string | null;
    running: boolean;
    starting: boolean;
    /** Pinned EasyProxy version and short commit. */
    version: string;
    commit: string;
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

export type FramegenTarget = '30fps' | '60fps' | 'matchDisplay';
/** `nativeDouble` doubles the source rate with the GPU's own frame generation (NVIDIA Framegen or AMD Fluid Motion). */
export type FramegenEngine = 'opticalFlow' | 'rife' | 'nativeDouble';

export interface Settings {
    resolution: string;
    quality: number;
    framegen: boolean;
    framegenTarget: FramegenTarget;
    framegenEngine: FramegenEngine;
    hdr: boolean;
    executorPreference: ExecutorPreference;
    alwaysOnTop: boolean;
    showOnProxyStart: boolean;
    minimizeToTray: boolean;
    closeToTray: boolean;
    notifications: boolean;
    /** Download new REFEREE releases and install them while no stream runs. */
    autoUpdate: boolean;
    encodingProfiles: Record<string, EncodingProfile>;
    player: PlayerSettings;
    instanceId: string | null;
    relay: RelaySettings;
    easyProxy: EasyProxySettings;
}

export interface StatusSettings {
    resolution: string;
    quality: number;
    framegen: boolean;
    framegenTarget: FramegenTarget;
    framegenEngine: FramegenEngine;
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
    framegenEngine: string | null;
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
    encoderHasNativeDouble: boolean | null;
    encoderHasTruehdr: boolean | null;
    encoderHasHdrTonemap: boolean | null;
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

/** `ready`: downloaded and verified, waiting to install. */
export type UpdatePhase = 'idle' | 'checking' | 'downloading' | 'ready' | 'installing';

/** REFEREE's own updates from its GitHub releases. */
export interface UpdateStatus {
    currentVersion: string;
    phase: UpdatePhase;
    /** The newest release above this one, from the last check. */
    latestVersion: string | null;
    releaseUrl: string | null;
    /** 0–100 while downloading. */
    progress: number | null;
    error: string | null;
    /** Unix time in milliseconds of the last check that reached GitHub. */
    lastChecked: number | null;
    /** Why this installation cannot install updates itself; `null` when it can. */
    unsupportedReason: string | null;
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
    getUpdateStatus(): Promise<UpdateStatus>;
    /** Checks GitHub now; a failed check is reported in the returned status. */
    checkForUpdate(): Promise<UpdateStatus>;
    /**
     * Downloads and installs the update found by the last check, then REFEREE exits so
     * the installer can replace it. Rejects with a readable message when it cannot start.
     */
    installUpdate(): Promise<void>;
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
    onUpdateStatus(cb: (status: UpdateStatus) => void): Unsubscribe;
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
    /**
     * Rejects with a readable message when the server is unreachable or refuses the password.
     * Managed mode starts REFEREE's server first; `url` and `apiPassword` are for external mode.
     */
    checkEasyProxy(mode: EasyProxyMode, url: string | null, apiPassword: string | null): Promise<EasyProxyStatus>;
    getEasyProxyStatus(): Promise<ManagedEasyProxyStatus>;
    /** Starts installing or updating the managed EasyProxy; progress comes from getEasyProxyStatus. */
    installEasyProxy(): Promise<void>;
    removeEasyProxy(): Promise<void>;
    /**
     * Development builds only: starts a stream for `url` and opens playback.
     * Resolves once the output is ready; rejects with a readable message.
     */
    startDebugStream(url: string): Promise<void>;
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

export const DEFAULT_EASY_PROXY_URL = 'http://127.0.0.1:7860';

export const DEFAULT_EASY_PROXY_SETTINGS: EasyProxySettings = {
    enabled: false,
    mode: 'managed',
    url: null,
    apiPassword: null,
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
    framegenTarget: '60fps',
    framegenEngine: 'opticalFlow',
    hdr: true,
    executorPreference: 'auto',
    alwaysOnTop: false,
    showOnProxyStart: false,
    minimizeToTray: false,
    closeToTray: true,
    notifications: false,
    autoUpdate: true,
    encodingProfiles: DEFAULT_ENCODING_PROFILES,
    player: DEFAULT_PLAYER_SETTINGS,
    instanceId: null,
    relay: DEFAULT_RELAY_SETTINGS,
    easyProxy: DEFAULT_EASY_PROXY_SETTINGS,
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
        framegenTarget: DEFAULT_SETTINGS.framegenTarget,
        framegenEngine: DEFAULT_SETTINGS.framegenEngine,
        hdr: DEFAULT_SETTINGS.hdr,
    },
    encoderHasFramegen: null,
    encoderHasNativeDouble: null,
    encoderHasTruehdr: null,
    encoderHasHdrTonemap: null,
    encoderHasRife: null,
};

export const DEFAULT_SETUP_STATE: SetupState = {
    gpu: null,
    progress: null,
    error: null,
    complete: false,
};
