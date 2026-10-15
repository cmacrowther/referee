import { ChevronDown } from 'lucide-react';
import { type ReactNode, useState } from 'react';

import {
    AiBadge,
    CONTROL_CLASS,
    SelectField,
    SettingRow,
    SettingsGroup,
    ToggleRow,
} from '@/components/settings/SettingsPrimitives';
import {
    deriveSettingsUiCapabilities,
    formatResolutionLabel,
    FRAMEGEN_TARGET_OPTIONS,
    getFramegenEngineOptions,
    getMaxQualityForResolution,
    normalizeFramegenEngine,
    normalizeFramegenTarget,
} from '@/lib/stream';
import {
    DEFAULT_ENCODING_PROFILES,
    type EncodingProfile,
    type RelayPeerMetadata,
    type Settings,
    type SettingsUiFeatureSupport,
    type SettingsUiRendererPreferenceOption,
    type Status
} from '@/lib/types';
import { cn } from '@/lib/utils';

interface UpscalerSettingsCardProps {
    settings: Settings;
    locked: boolean;
    relayLocked?: boolean;
    relayRemote?: boolean;
    relayRemoteOffline?: boolean;
    relayUpdating?: boolean;
    relayUpdateError?: string | null;
    relayRequired?: boolean;
    relayPeerName?: string | null;
    relayPeer?: RelayPeerMetadata | null;
    status: Status;
    engineSettings: boolean;
    onOpenRelaySettings?: () => void;
    onChange: (patch: Partial<Settings>) => void;
}

/**
 * A compact select row nested under Motion Boost. The label stays on one line and the
 * select takes the remaining width, truncating long option names rather than wrapping.
 */
function SubSettingSelect({
    label,
    description,
    value,
    disabled,
    onChange,
    children,
}: {
    label: string;
    description: string | null;
    value: string;
    disabled: boolean;
    onChange: (value: string) => void;
    children: ReactNode;
}) {
    return (
        <div className="py-1 pl-4 pr-3">
            <div className="flex items-center gap-3">
                <span className="w-[72px] shrink-0 whitespace-nowrap text-[12px] font-medium text-white/85">{label}</span>
                <SelectField
                    aria-label={label}
                    value={value}
                    disabled={disabled}
                    onChange={event => {
                        onChange(event.currentTarget.value);
                    }}
                    className="flex-1"
                >
                    {children}
                </SelectField>
            </div>
            {description && <p className="mt-1 text-[11px] leading-[15px] text-referee-muted">{description}</p>}
        </div>
    );
}

function NumberField({
    label,
    value,
    min,
    max,
    step,
    onChange,
}: {
    label: string;
    value: number;
    min: number;
    max?: number;
    step?: number;
    onChange: (value: number) => void;
}) {
    return (
        <label className="flex min-w-0 flex-col gap-1">
            <span className="truncate text-[10px] text-referee-muted">{label}</span>
            <input
                type="number"
                min={min}
                max={max}
                step={step}
                value={value}
                onChange={event => onChange(Number(event.currentTarget.value))}
                className={cn(CONTROL_CLASS, 'w-full px-2 py-1 text-right')}
            />
        </label>
    );
}

/** Frame Rate select value for native doubling (NVIDIA Framegen or AMD Fluid Motion), which doubles instead of targeting a rate. */
const FRAMEGEN_DOUBLE_OPTION = 'nativeDouble';

const NVENC_PRESETS = ['p1', 'p2', 'p3', 'p4', 'p5', 'p6', 'p7'] as const;
const ENCODING_PROFILE_RESOLUTIONS = Object.keys(DEFAULT_ENCODING_PROFILES);

interface CapabilityState {
    label: string;
    support: SettingsUiFeatureSupport['support'];
}

/**
 * Render a compact badge indicating a capability's support state.
 *
 * The badge displays `capability.label` and applies visual styles depending on
 * `capability.support`: `"native"` for a green/native style, `"fallback"` for an
 * amber/fallback style, and any other value for a muted/unavailable style.
 *
 * @param capability - Resolved capability state containing:
 *   - `label`: human-readable text to show inside the badge
 *   - `support`: `"native"`, `"fallback"`, or other values (treated as unavailable)
 * @returns A span element showing the capability label with styling that reflects support.
 */
function CapabilityChip({ capability }: { capability: CapabilityState }) {
    return (
        <span
            className={[
                'rounded border px-2 py-1 text-[10px] font-medium leading-none',
                capability.support === 'native'
                    ? 'border-referee-green/30 bg-referee-green/10 text-referee-green'
                    : capability.support === 'fallback'
                        ? 'border-[#FFB36B]/30 bg-[#FFB36B]/10 text-[#FFB36B]'
                        : 'border-white/[0.06] bg-white/[0.03] text-referee-muted',
            ].join(' ')}
        >
            {capability.label}
        </span>
    );
}

/**
 * Choose the description text for a feature toggle.
 *
 * @param feature - The resolved UI support state for the feature; may include `lockState` and `reason`
 * @param enabledDescription - Fallback description to use when `feature.reason` is not provided
 * @returns `'Locked while a stream is active'` when `feature.lockState === 'locked'`, otherwise `feature.reason` if present, else `enabledDescription`
 */
function getFeatureToggleDescription(
    feature: SettingsUiFeatureSupport,
    enabledDescription: string,
) {
    if (feature.lockState === 'locked') {
        return 'Locked while a stream is active';
    }

    return feature.reason ?? enabledDescription;
}

/**
 * Whether a feature toggle's label should render muted.
 *
 * @param feature - Object describing the feature's availability and lock state
 * @returns `false` when the feature is available and not locked, `true` otherwise
 */
function isFeatureToggleMuted(feature: SettingsUiFeatureSupport) {
    return !feature.available || feature.lockState === 'locked';
}

/**
 * Choose the badge text that represents a renderer preference option's lock and availability state.
 *
 * @param option - The renderer preference option whose `lockState` and `availability` determine the badge
 * @returns `Locked` if `option.lockState === "locked"`, `Native` if `option.availability === "native"`, `Fallback Available` if `option.availability === "fallback"`, `Unavailable` otherwise
 */
function getRendererOptionBadgeText(option: SettingsUiRendererPreferenceOption) {
    if (option.lockState === 'locked') {
        return 'Locked';
    }

    if (option.availability === 'native') {
        return 'Native';
    }

    if (option.availability === 'fallback') {
        return 'Fallback Available';
    }

    return 'Unavailable';
}

/**
 * Selects the text color CSS class for a renderer preference option badge based on its lock state and availability.
 *
 * @param option - The renderer preference option whose lock state and availability determine the badge color
 * @returns A CSS text color class: `text-referee-green` for native availability, `text-[#FFB36B]` for fallback availability, and `text-[#FF6B35]` when locked or otherwise unavailable
 */
function getRendererOptionBadgeClass(option: SettingsUiRendererPreferenceOption) {
    if (option.lockState === 'locked') {
        return 'text-[#FF6B35]';
    }

    if (option.availability === 'native') {
        return 'text-referee-green';
    }

    if (option.availability === 'fallback') {
        return 'text-[#FFB36B]';
    }

    return 'text-[#FF6B35]';
}

function normalizeRelayGpuVendor(peer?: RelayPeerMetadata | null) {
    const vendor = peer?.gpuVendor?.trim().toLowerCase();

    if (vendor === 'nvidia' || vendor === 'amd') {
        return vendor;
    }

    return null;
}

function getEffectiveSettingsStatus({
    relayRemote,
    relayPeer,
    status,
}: {
    relayRemote: boolean;
    relayPeer?: RelayPeerMetadata | null;
    status: Status;
}): Status {
    const relayGpuVendor = relayRemote ? normalizeRelayGpuVendor(relayPeer) : null;

    if (relayGpuVendor === 'nvidia') {
        return {
            ...status,
            gpuReady: relayPeer?.gpuReady ?? status.gpuReady,
            gpuName: relayPeer?.gpuName ?? status.gpuName,
            gpuVendor: 'nvidia',
            selectedExecutor: status.selectedExecutor ?? 'NvidiaSpecialized',
            nvidiaAiAvailable: true,
            encoderHasFramegen: true,
            // The peer's GPU generation is unknown here; a peer without NVIDIA
            // Framegen uses optical flow instead.
            encoderHasNativeDouble: status.encoderHasNativeDouble ?? true,
            encoderHasRife: status.encoderHasRife ?? true,
        };
    }

    if (relayGpuVendor === 'amd') {
        return {
            ...status,
            gpuReady: relayPeer?.gpuReady ?? status.gpuReady,
            gpuName: relayPeer?.gpuName ?? status.gpuName,
            gpuVendor: 'amd',
            selectedExecutor: status.selectedExecutor ?? 'amdAi',
            amdAiAvailable: true,
            // As above: a peer without AMF FRC interpolates instead.
            encoderHasNativeDouble: status.encoderHasNativeDouble ?? true,
            encoderHasRife: status.encoderHasRife ?? true,
        };
    }

    return status;
}

/**
 * Banner shown above the stream controls when they are routed to, or locked by, REFEREE Relay.
 */
function RelayNotice({
    relayRemote,
    relayRemoteOffline,
    relayUpdating,
    relayUpdateError,
    relayRequired,
    relayPeerName,
    onOpenRelaySettings,
}: {
    relayRemote: boolean;
    relayRemoteOffline: boolean;
    relayUpdating: boolean;
    relayUpdateError: string | null;
    relayRequired: boolean;
    relayPeerName: string | null;
    onOpenRelaySettings?: () => void;
}) {
    const peerName = relayPeerName ?? 'the linked REFEREE instance';
    const amber = relayRemote && !relayRemoteOffline;
    const statusError = relayRemoteOffline || Boolean(relayUpdateError);

    return (
        <section
            className={cn(
                'rounded-lg border px-3 py-2.5',
                amber ? 'border-[#FFB36B]/25 bg-[#FFB36B]/[0.08]' : 'border-[#FF6B35]/20 bg-[#FF6B35]/[0.08]'
            )}
        >
            <span className="text-[9px] font-bold uppercase tracking-widest text-[#FFD8B8]/80">
                {relayRemote ? 'Remote Relay Settings' : 'Stream Settings Locked'}
            </span>
            <p className="mt-0.5 text-[13px] font-medium leading-[18px] text-white">
                {relayRemoteOffline
                    ? `${relayPeerName ?? 'The linked REFEREE instance'} is offline`
                    : relayRemote
                    ? `Updating ${peerName}`
                    : relayRequired
                    ? 'This device requires REFEREE Relay'
                    : 'You have REFEREE Relay enabled.'}
            </p>
            <p className="mt-0.5 text-[11px] leading-[15px] text-white/70">
                {relayRemoteOffline
                    ? `Remote Relay settings are locked until ${peerName} comes online.`
                    : relayRemote
                    ? `Changes here are saved to ${peerName} for new relay streams.`
                    : relayRequired
                    ? relayPeerName
                        ? `New streams are routed through ${relayPeerName}. Stream Settings are managed on the relay device.`
                        : 'No supported graphics hardware detected. Link a REFEREE Relay peer to route streams.'
                    : `New streams are routed through ${peerName}. Switch routing back to This REFEREE in Relay settings to edit these controls here.`}
            </p>
            <div className="mt-2 flex items-center justify-between gap-2">
                {relayRemote ? (
                    <p
                        role="status"
                        aria-live="polite"
                        className={cn(
                            'flex min-w-0 items-center gap-1.5 text-[11px] font-medium leading-[15px]',
                            statusError ? 'text-[#FF6B35]' : 'text-[#FFD8B8]'
                        )}
                    >
                        <span aria-hidden="true" className={cn('size-1.5 shrink-0 rounded-full', statusError ? 'bg-[#FF6B35]' : 'bg-[#FFB36B]')} />
                        <span className="min-w-0 break-words">
                            {relayRemoteOffline
                                ? 'Remote relay offline'
                                : relayUpdateError
                                ? relayUpdateError
                                : relayUpdating
                                    ? 'Updating remote relay...'
                                    : 'Remote relay ready'}
                        </span>
                    </p>
                ) : (
                    <span />
                )}
                <button
                    type="button"
                    className="shrink-0 rounded-md border border-white/15 px-2 py-1 text-[11px] font-medium text-white/80 transition-colors hover:border-white/30 hover:bg-white/[0.04] hover:text-white"
                    onClick={onOpenRelaySettings}
                >
                    Open Relay Settings
                </button>
            </div>
        </section>
    );
}

/**
 * Render the Stream settings tab: picture and motion controls, advanced encoder tuning, and optional engine-only debug controls.
 *
 * Renders controls driven by derived UI capabilities (quality limits, feature availability, renderer options) and emits partial Settings patches via `onChange`. Advanced encoder profile editing targets a local profile selection and updates the corresponding encoding profile in `settings.encodingProfiles`. When `import.meta.env.DEV` and `engineSettings` are true, a debug "Engine Settings" panel is shown.
 *
 * @param settings - Current settings state to display and patch
 * @param locked - When true, UI controls are visually disabled and interactions are prevented
 * @param status - Runtime status used to derive capability availability and limits
 * @param engineSettings - When true (and running in DEV), show engine/debug controls for renderer preference and resolved path
 * @param onChange - Callback invoked with a partial Settings patch when controls change
 * @returns The React element tree for the upscaler settings card
 */
export function UpscalerSettingsCard({
    settings,
    locked,
    relayLocked = false,
    relayRemote = false,
    relayRemoteOffline = false,
    relayUpdating = false,
    relayUpdateError = null,
    relayRequired = false,
    relayPeerName = null,
    relayPeer = null,
    status,
    engineSettings,
    onOpenRelaySettings,
    onChange
}: UpscalerSettingsCardProps) {
    const effectiveStatus = getEffectiveSettingsStatus({ relayRemote, relayPeer, status });
    const uiCapabilities = deriveSettingsUiCapabilities(settings, effectiveStatus, locked);
    const maxQuality = uiCapabilities.quality.max;
    const framegenFeature = uiCapabilities.features.framegen;
    const framegenEngineOptions = getFramegenEngineOptions(effectiveStatus, uiCapabilities.resolvedPath.kind);
    const nativeDoubleOption = framegenEngineOptions.find(option => option.value === 'nativeDouble');
    const interpolatorOptions = framegenEngineOptions.filter(option => option.value !== 'nativeDouble');
    const selectedInterpolator = interpolatorOptions.find(option => option.value === settings.framegenEngine);
    const fallbackInterpolator = interpolatorOptions.find(option => option.available && option !== selectedInterpolator);
    const unavailableNote = (option: { reason: string | null } | undefined) =>
        `${option?.reason ?? ''} ${fallbackInterpolator ? `Using ${fallbackInterpolator.label} instead.` : ''}`.trim();
    const framegenDoubling = nativeDoubleOption !== undefined && settings.framegenEngine === 'nativeDouble';
    const showFramegenOptions = settings.framegen && framegenFeature.available;
    const hdrFeature = uiCapabilities.features.hdr;
    const controlsLocked = locked || relayLocked || relayRemoteOffline;
    const resolvedPathCapabilities: CapabilityState[] = [
        { label: `Upscale ${uiCapabilities.features.upscale.support === 'native' ? 'Native' : uiCapabilities.features.upscale.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: uiCapabilities.features.upscale.support },
        { label: `Motion ${framegenFeature.support === 'native' ? 'Native' : framegenFeature.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: framegenFeature.support },
        { label: `HDR ${hdrFeature.support === 'native' ? 'Native' : hdrFeature.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: hdrFeature.support },
    ];
    const groupTone = controlsLocked ? 'border-[#FF6B35]/20' : relayRemote ? 'border-[#FFB36B]/25' : undefined;
    const [advancedSettingsEnabled, setAdvancedSettingsEnabled] = useState(false);
    const [profileResolution, setProfileResolution] = useState(settings.resolution);

    const currentProfile: EncodingProfile = settings.encodingProfiles?.[profileResolution]
        ?? DEFAULT_ENCODING_PROFILES[profileResolution];

    function updateProfile(patch: Partial<EncodingProfile>) {
        onChange({
            encodingProfiles: {
                ...settings.encodingProfiles,
                [profileResolution]: { ...currentProfile, ...patch },
            },
        });
    }

    function resetProfile() {
        const defaults = DEFAULT_ENCODING_PROFILES[profileResolution];
        onChange({
            encodingProfiles: {
                ...settings.encodingProfiles,
                [profileResolution]: { ...defaults },
            },
        });
    }

    return (
        <div id="settings-card" className="flex flex-col gap-2.5">
            {(relayLocked || relayRemote) && (
                <RelayNotice
                    relayRemote={relayRemote}
                    relayRemoteOffline={relayRemoteOffline}
                    relayUpdating={relayUpdating}
                    relayUpdateError={relayUpdateError}
                    relayRequired={relayRequired}
                    relayPeerName={relayPeerName}
                    onOpenRelaySettings={onOpenRelaySettings}
                />
            )}

            <fieldset
                disabled={controlsLocked}
                aria-disabled={controlsLocked}
                id="upscaler-controls"
                className={`m-0 flex min-w-0 flex-col gap-2.5 border-0 p-0 transition-all duration-300 ${controlsLocked ? 'pointer-events-none opacity-30' : ''}`}
            >
                <SettingsGroup className={groupTone}>
                    <SettingRow
                        label="Upscale Target"
                        badge={<AiBadge />}
                        description={uiCapabilities.resolvedPath.kind === 'nvidiaNative'
                            ? 'NVIDIA® AI upscaling'
                            : uiCapabilities.resolvedPath.kind === 'amdNative'
                                ? 'AMD® FSR upscaling'
                                : uiCapabilities.resolvedPath.kind === 'pending'
                                    ? 'Output resolution'
                                    : 'REFEREE upscaling'}
                        control={
                            <SelectField
                                aria-label="Resolution Target"
                                value={settings.resolution}
                                onChange={event => {
                                    const resolution = event.currentTarget.value;
                                    setProfileResolution(resolution);
                                    onChange({
                                        resolution,
                                        quality: Math.min(settings.quality, getMaxQualityForResolution(resolution))
                                    });
                                }}
                                className="w-[84px]"
                            >
                                <option value="1920x1080">1080p</option>
                                <option value="2560x1440">1440p</option>
                                <option value="3840x2160">4K</option>
                            </SelectField>
                        }
                    />

                    <div className="px-3 pb-2 pt-2">
                        <div className="flex items-center justify-between gap-3">
                            <span className="text-[13px] font-medium leading-[18px] text-white">Upscaling Quality</span>
                            <span className="min-w-[26px] rounded bg-referee-green/10 px-1.5 text-center text-[13px] font-bold leading-[18px] tabular-nums text-referee-green">
                                {settings.quality}
                            </span>
                        </div>
                        <input
                            type="range"
                            aria-label="Upscaling Quality"
                            min={1}
                            max={maxQuality}
                            value={settings.quality}
                            style={{ '--range-fill': `${((settings.quality - 1) / (maxQuality - 1)) * 100}%` } as React.CSSProperties}
                            onChange={event => {
                                onChange({ quality: Number(event.currentTarget.value) });
                            }}
                            className="mt-2 block"
                        />
                        <div className="mt-1 flex justify-between text-[9px] font-bold uppercase tracking-wider text-referee-muted/80">
                            <span>Faster</span>
                            <span>Sharper</span>
                        </div>
                    </div>

                    <ToggleRow
                        label="HDR Mapping"
                        badge={<AiBadge />}
                        description={getFeatureToggleDescription(hdrFeature, 'Maps SDR video for HDR displays')}
                        muted={isFeatureToggleMuted(hdrFeature)}
                        checked={hdrFeature.support === 'unavailable' ? false : settings.hdr}
                        disabled={hdrFeature.disabled}
                        onChange={checked => {
                            onChange({ hdr: checked });
                        }}
                    />
                </SettingsGroup>

                <SettingsGroup className={groupTone}>
                    <ToggleRow
                        label="Motion Boost"
                        badge={<AiBadge />}
                        description={getFeatureToggleDescription(framegenFeature, 'Smooths motion by generating extra frames')}
                        muted={isFeatureToggleMuted(framegenFeature)}
                        checked={framegenFeature.support === 'unavailable' ? false : settings.framegen}
                        disabled={framegenFeature.disabled}
                        onChange={checked => {
                            onChange({ framegen: checked });
                        }}
                    />

                    {showFramegenOptions && (
                        <div className="py-1">
                            <SubSettingSelect
                                label="Frame Rate"
                                description={
                                    framegenDoubling
                                        ? nativeDoubleOption?.available
                                            ? 'Doubles the source frame rate'
                                            : unavailableNote(nativeDoubleOption)
                                        : settings.framegenTarget === 'matchDisplay'
                                            ? "Matches the viewer's display refresh rate"
                                            : null
                                }
                                value={framegenDoubling ? FRAMEGEN_DOUBLE_OPTION : settings.framegenTarget}
                                disabled={framegenFeature.disabled}
                                onChange={value => {
                                    if (value === FRAMEGEN_DOUBLE_OPTION) {
                                        onChange({ framegenEngine: 'nativeDouble' });
                                        return;
                                    }
                                    // Picking a rate leaves native doubling, which only doubles.
                                    onChange({
                                        framegenTarget: normalizeFramegenTarget(value),
                                        ...(settings.framegenEngine === 'nativeDouble'
                                            ? { framegenEngine: 'opticalFlow' as const }
                                            : {}),
                                    });
                                }}
                            >
                                {FRAMEGEN_TARGET_OPTIONS.map(option => (
                                    <option key={option.value} value={option.value}>
                                        {option.label}
                                    </option>
                                ))}
                                {nativeDoubleOption && (
                                    <option
                                        value={FRAMEGEN_DOUBLE_OPTION}
                                        disabled={!nativeDoubleOption.available && !framegenDoubling}
                                    >
                                        {nativeDoubleOption.available
                                            ? nativeDoubleOption.label
                                            : `${nativeDoubleOption.label} (unavailable)`}
                                    </option>
                                )}
                            </SubSettingSelect>
                            {!framegenDoubling && (
                                <SubSettingSelect
                                    label="Interpolator"
                                    description={
                                        selectedInterpolator && !selectedInterpolator.available
                                            ? unavailableNote(selectedInterpolator)
                                            : null
                                    }
                                    value={settings.framegenEngine}
                                    disabled={framegenFeature.disabled}
                                    onChange={value => {
                                        onChange({ framegenEngine: normalizeFramegenEngine(value) });
                                    }}
                                >
                                    {interpolatorOptions.map(option => (
                                        <option
                                            key={option.value}
                                            value={option.value}
                                            disabled={!option.available && option.value !== settings.framegenEngine}
                                        >
                                            {option.available ? option.label : `${option.label} (unavailable)`}
                                        </option>
                                    ))}
                                </SubSettingSelect>
                            )}
                        </div>
                    )}
                </SettingsGroup>

                <SettingsGroup className={groupTone}>
                    <button
                        type="button"
                        aria-expanded={advancedSettingsEnabled}
                        onClick={() => setAdvancedSettingsEnabled(open => !open)}
                        className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left transition-[background-color] hover:bg-white/[0.02]"
                    >
                        <span className="flex min-w-0 flex-col">
                            <span className="text-[13px] font-medium leading-[18px] text-white">Advanced Encoding</span>
                            <span className="text-[11px] leading-[15px] text-referee-muted">Bitrate, preset, and HLS tuning</span>
                        </span>
                        <ChevronDown
                            className={cn('size-4 shrink-0 text-referee-muted transition-transform', advancedSettingsEnabled && 'rotate-180')}
                        />
                    </button>

                    {advancedSettingsEnabled && (
                        <div className="flex flex-col gap-2.5 px-3 pb-3 pt-2.5" aria-label="Advanced encoder tuning controls">
                            {uiCapabilities.advancedEncoding.reason && (
                                <p className="text-[10px] leading-[14px] text-referee-muted">
                                    {uiCapabilities.advancedEncoding.reason}
                                </p>
                            )}

                            <div className="flex items-center justify-between gap-3">
                                <span className="text-[12px] font-medium text-white/85">Encoding Profile</span>
                                <SelectField
                                    aria-label="Encoding Profile"
                                    value={profileResolution}
                                    onChange={event => {
                                        setProfileResolution(event.currentTarget.value);
                                    }}
                                    className="w-[84px]"
                                >
                                    {ENCODING_PROFILE_RESOLUTIONS.map(resolution => (
                                        <option key={resolution} value={resolution}>
                                            {formatResolutionLabel(resolution)}
                                        </option>
                                    ))}
                                </SelectField>
                            </div>

                            <div className="grid grid-cols-2 gap-x-2.5 gap-y-2">
                                <NumberField
                                    label="Bitrate (kbps)"
                                    min={1000}
                                    step={500}
                                    value={currentProfile.bitrate}
                                    onChange={bitrate => updateProfile({ bitrate })}
                                />
                                <NumberField
                                    label="Max Bitrate (kbps)"
                                    min={1000}
                                    step={500}
                                    value={currentProfile.max_bitrate}
                                    onChange={max_bitrate => updateProfile({ max_bitrate })}
                                />
                                <label className="flex min-w-0 flex-col gap-1">
                                    <span className="text-[10px] text-referee-muted">Preset</span>
                                    <SelectField
                                        value={currentProfile.preset}
                                        onChange={e => updateProfile({ preset: e.currentTarget.value })}
                                        selectClassName="py-1"
                                    >
                                        {NVENC_PRESETS.map(p => (
                                            <option key={p} value={p}>{p}</option>
                                        ))}
                                    </SelectField>
                                </label>
                                <NumberField
                                    label="Lookahead"
                                    min={0}
                                    max={32}
                                    value={currentProfile.lookahead}
                                    onChange={lookahead => updateProfile({ lookahead })}
                                />
                                <NumberField
                                    label="B-Frames"
                                    min={0}
                                    max={4}
                                    value={currentProfile.bframes}
                                    onChange={bframes => updateProfile({ bframes })}
                                />
                                <NumberField
                                    label="HLS Segment (s)"
                                    min={1}
                                    max={10}
                                    value={currentProfile.hls_time}
                                    onChange={hls_time => updateProfile({ hls_time })}
                                />
                            </div>

                            <button
                                type="button"
                                onClick={resetProfile}
                                className="self-end text-[10px] font-bold uppercase tracking-wider text-referee-muted transition-colors hover:text-white"
                            >
                                Reset to Defaults
                            </button>
                        </div>
                    )}
                </SettingsGroup>

                {import.meta.env.DEV && engineSettings && (
                    <section className="rounded-lg border border-referee-green/30 bg-referee-green/10 p-3">
                        <div className="mb-3 flex items-center justify-between">
                            <span className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">Engine Settings</span>
                            <span className="ml-2">
                                <span className="rounded border px-2 py-1 text-[10px] font-medium leading-none border-referee-green/30 bg-referee-green/10 text-referee-green">DEBUG ONLY</span>
                            </span>
                        </div>

                        <div className="flex flex-col">
                            <span className="text-[13px] font-medium text-white">Renderer Preference</span>
                            <span className="text-[11px] text-referee-muted">Choose your preferred processing path; the backend may still resolve a different runtime path.</span>
                        </div>

                        <div role="radiogroup" aria-label="Processing Mode" className="mt-3 flex flex-col gap-2">
                            {uiCapabilities.rendererPreferenceOptions.map(option => {
                                const selected = settings.executorPreference === option.value;
                                const disabled = option.disabled;

                                return (
                                    <button
                                        key={option.value}
                                        type="button"
                                        role="radio"
                                        aria-checked={selected}
                                        disabled={disabled}
                                        onClick={() => {
                                            onChange({ executorPreference: option.value });
                                        }}
                                        className={[
                                            'flex w-full items-start gap-3 rounded-lg border px-3 py-2.5 text-left transition-colors',
                                            selected
                                                ? 'border-[rgba(255,107,53,0.4)] bg-[rgba(255,107,53,0.08)]'
                                                : disabled
                                                    ? 'cursor-not-allowed border-[rgba(255,255,255,0.03)] bg-transparent opacity-40'
                                                    : 'border-[rgba(255,255,255,0.06)] bg-[rgba(255,255,255,0.03)] hover:border-[rgba(255,255,255,0.1)] hover:bg-[rgba(255,255,255,0.05)]',
                                        ].join(' ')}
                                    >
                                        <span
                                            className={[
                                                'mt-0.5 flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center rounded-full border',
                                                selected
                                                    ? 'border-[rgba(255,107,53,0.8)] bg-[rgba(255,107,53,0.8)]'
                                                    : 'border-[rgba(255,255,255,0.25)]',
                                            ].join(' ')}
                                            aria-hidden="true"
                                        >
                                            {selected && <span className="h-1.5 w-1.5 rounded-full bg-white" />}
                                        </span>

                                        <span className="min-w-0 flex-1">
                                            <span className="block text-[12px] font-medium leading-tight text-white">
                                                {option.label}
                                            </span>
                                            <span className="mt-0.5 block text-[11px] leading-4 text-referee-muted">
                                                {option.description}
                                            </span>

                                            <span className={`mt-0.5 block text-[10px] font-medium uppercase tracking-wider ${getRendererOptionBadgeClass(option)}`}>
                                                {getRendererOptionBadgeText(option)}
                                            </span>

                                            {option.reason && (
                                                <span className="mt-0.5 block text-[10px] leading-4 text-referee-muted">
                                                    {option.reason}
                                                </span>
                                            )}
                                        </span>
                                    </button>
                                );
                            })}
                        </div>

                        <div
                            role="status"
                            aria-live="polite"
                            className="mt-3 rounded-lg border border-white/[0.06] bg-white/[0.03] px-3 py-2.5"
                        >
                            <span className="mb-1 block text-[9px] font-bold uppercase tracking-wider text-white/30">Resolved Path</span>
                            <span className="block text-[12px] font-medium text-white">
                                {uiCapabilities.resolvedPath.label}
                            </span>
                            {uiCapabilities.resolvedPath.reason && (
                                <span className="mt-0.5 block text-[11px] leading-4 text-referee-muted">
                                    {uiCapabilities.resolvedPath.reason}
                                </span>
                            )}

                            <div className="mt-2.5 pt-0.5" aria-label="Resolved path capabilities">
                                <span className="mb-1.5 block text-[9px] font-bold uppercase tracking-wider text-white/30">Resolved Feature Support</span>
                                <div className="flex flex-wrap gap-1">
                                    {resolvedPathCapabilities.map(capability => (
                                        <CapabilityChip key={capability.label} capability={capability} />
                                    ))}
                                </div>
                            </div>
                        </div>
                    </section>
                )}
            </fieldset>
        </div>
    );
}
