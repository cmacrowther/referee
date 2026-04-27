import { ChevronDown, LockKeyhole, Sparkles } from 'lucide-react';
import { useState } from 'react';

import { deriveSettingsUiCapabilities, formatResolutionLabel, getMaxQualityForResolution } from '@/lib/stream';
import {
    DEFAULT_ENCODING_PROFILES,
    type EncodingProfile,
    type RelayPeerMetadata,
    type Settings,
    type SettingsUiFeatureSupport,
    type SettingsUiRendererPreferenceOption,
    type Status
} from '@/lib/types';

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

function ToggleField({
    checked,
    disabled = false,
    onChange
}: {
    checked: boolean;
    disabled?: boolean;
    onChange: (checked: boolean) => void;
}) {
    return (
        <label className={`relative inline-flex items-center ${disabled ? 'cursor-not-allowed opacity-30' : 'cursor-pointer'}`}>
            <input
                type="checkbox"
                className="sr-only toggle-checkbox"
                checked={checked}
                disabled={disabled}
                onChange={event => {
                    onChange(event.currentTarget.checked);
                }}
            />
            <div className="toggle-label h-[20px] w-[36px] rounded-full bg-[#535353] after:absolute after:left-[2px] after:top-[2px] after:h-[16px] after:w-[16px] after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all after:content-[''] hover:bg-[#666666]" />
        </label>
    );
}

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
 * Choose the label text color class for a feature toggle based on availability and lock state.
 *
 * @param feature - Object describing the feature's availability and lock state
 * @returns `'text-white'` when the feature is available and not locked, `'text-referee-muted'` otherwise
 */
function getFeatureToggleLabelClass(feature: SettingsUiFeatureSupport) {
    return feature.available && feature.lockState !== 'locked' ? 'text-white' : 'text-referee-muted';
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
        };
    }

    return status;
}

/**
 * Render the Upscaler settings card with stream controls, advanced encoder tuning, and optional engine-only debug controls.
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
    const hdrFeature = uiCapabilities.features.hdr;
    const controlsLocked = locked || relayLocked || relayRemoteOffline;
    const resolvedPathCapabilities: CapabilityState[] = [
        { label: `Upscale ${uiCapabilities.features.upscale.support === 'native' ? 'Native' : uiCapabilities.features.upscale.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: uiCapabilities.features.upscale.support },
        { label: `Motion ${framegenFeature.support === 'native' ? 'Native' : framegenFeature.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: framegenFeature.support },
        { label: `HDR ${hdrFeature.support === 'native' ? 'Native' : hdrFeature.support === 'fallback' ? 'Fallback' : 'Unavailable'}`, support: hdrFeature.support },
    ];
    const cardClass = `rounded-lg border bg-referee-card p-3.5 transition-colors duration-300 ${
        controlsLocked ? 'border-[#FF6B35]/20' : relayRemote ? 'border-[#FFB36B]/25' : 'border-referee-border'
    }`;
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
        <div id="settings-card" className="flex flex-col gap-3">
            {(relayLocked || relayRemote) && (
                <section className={`rounded-lg border p-3.5 ${
                    relayRemoteOffline
                        ? 'border-[#FF6B35]/20 bg-[#FF6B35]/10'
                        : relayRemote
                        ? 'border-[#FFB36B]/25 bg-[#FFB36B]/10'
                        : 'border-[#FF6B35]/20 bg-[#FF6B35]/10'
                }`}>
                    <div className="flex items-start gap-3">
                        <div>
                            <span className="text-[10px] font-bold uppercase tracking-widest text-[#FFD8B8]">
                                {relayRemote ? 'Remote Relay Settings' : 'Stream Settings Locked'}
                            </span>
                            <p className="mt-1 text-[13px] font-medium text-white">
                                {relayRemoteOffline
                                    ? `${relayPeerName ?? 'The linked REFEREE instance'} is offline`
                                    : relayRemote
                                    ? `Updating ${relayPeerName ?? 'the linked REFEREE instance'}`
                                    : relayRequired
                                    ? 'This device requires REFEREE Relay'
                                    : 'You have REFEREE Relay enabled.'}
                            </p>
                            <p className="mt-1 text-[11px] leading-relaxed text-white/75">
                                {relayRemoteOffline
                                    ? `Remote Relay settings are locked until ${relayPeerName ?? 'the linked REFEREE instance'} comes online.`
                                    : relayRemote
                                    ? `Changes here are saved to ${relayPeerName ?? 'the linked REFEREE instance'} for new relay streams.`
                                    : relayRequired
                                    ? relayPeerName
                                        ? `New streams are routed through ${relayPeerName}. Stream Settings are managed on the relay device.`
                                        : 'No supported graphics hardware detected. Link a REFEREE Relay peer to route streams.'
                                    : `New streams are routed through ${relayPeerName ?? 'the linked REFEREE instance'}. Switch routing back to This REFEREE in Relay settings to edit these controls here.`}
                            </p>
                            {relayRemote && (
                                <p
                                    role="status"
                                    aria-live="polite"
                                    className={`mt-2 text-[10px] font-bold uppercase tracking-wider ${
                                        relayRemoteOffline || relayUpdateError ? 'text-[#FF6B35]' : 'text-[#FFD8B8]'
                                    }`}
                                >
                                    {relayRemoteOffline
                                        ? 'Remote relay offline'
                                        : relayUpdateError
                                        ? relayUpdateError
                                        : relayUpdating
                                            ? 'Updating remote relay...'
                                            : 'Remote relay ready'}
                                </p>
                            )}
                            <button
                                type="button"
                                className="mt-2 rounded-md border border-white/15 px-2.5 py-1.5 text-[10px] font-bold uppercase tracking-wider text-white/75 transition-colors hover:border-white/30 hover:bg-white/[0.04] hover:text-white"
                                onClick={onOpenRelaySettings}
                            >
                                Open Relay Settings
                            </button>
                        </div>
                    </div>
                </section>
            )}

            <fieldset
                disabled={controlsLocked}
                aria-disabled={controlsLocked}
                id="upscaler-controls"
                className={`m-0 flex min-w-0 flex-col gap-3 border-0 p-0 transition-all duration-300 ${controlsLocked ? 'pointer-events-none opacity-30' : ''}`}
            >
                <section className={`${cardClass} flex flex-col gap-3.5`}>
                    <div className="flex flex-col">
                        <span className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">Stream Settings</span>
                    </div>

                    <div className="group flex items-center justify-between gap-3">
                        <div className="flex min-w-0 flex-col pr-2">
                            <span className="flex items-center gap-0 text-[13px] font-medium text-white">
                                Upscale Target
                                <span className="ml-1.5 inline-flex items-center gap-0.5 rounded bg-referee-green/10 px-1.5 py-0.5 text-[9px] font-bold tracking-wide text-referee-green">
                                    <Sparkles className="size-2.5" />
                                    AI
                                </span>
                            </span>
                            <span className="text-[11px] text-referee-muted">
                                {uiCapabilities.resolvedPath.kind === 'nvidiaNative'
                                    ? 'Target for NVIDIA® AI-based upscaling'
                                    : uiCapabilities.resolvedPath.kind === 'amdNative'
                                        ? 'Target for AMD® AI-based upscaling'
                                        : uiCapabilities.resolvedPath.kind === 'pending'
                                            ? 'Target output resolution'
                                            : 'Target for REFEREE upscaling'}
                            </span>
                        </div>
                        <div className="relative shrink-0">
                            <select
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
                                className="cursor-pointer appearance-none rounded border border-transparent bg-[#282828] py-1.5 pl-3 pr-7 text-xs font-medium text-white transition-colors hover:border-white/10 hover:bg-[#333333] focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                            >
                                <option value="1920x1080">1080p</option>
                                <option value="2560x1440">1440p</option>
                                <option value="3840x2160">4K</option>
                            </select>
                            <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2 text-referee-muted group-hover:text-white">
                                <ChevronDown className="size-3" />
                            </div>
                        </div>
                    </div>

                    <div className="group flex flex-col gap-3 border-y border-white/[0.03] py-3">
                        <div className="flex items-end justify-between">
                            <div className="flex flex-col">
                                <span className="text-[13px] font-medium text-white">Upscaling Quality</span>
                                <span className="text-[11px] text-referee-muted">Sharper image at higher GPU cost</span>
                            </div>
                            <span className="min-w-[32px] rounded bg-referee-green/10 px-2.5 py-0.5 text-center text-[16px] font-bold text-referee-green">
                                {settings.quality}
                            </span>
                        </div>
                        <div className="mt-1 px-1">
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
                            />
                            <div className="mt-2 flex justify-between text-[9px] font-bold uppercase tracking-wider text-referee-muted">
                                <span>Fast</span>
                                <span>Quality</span>
                            </div>
                        </div>
                    </div>

                    <div className="flex items-center justify-between gap-3">
                        <div className="flex min-w-0 flex-col pr-2">
                            <span className={`flex items-center gap-0 text-[13px] font-medium ${getFeatureToggleLabelClass(framegenFeature)}`}>
                                Motion Boost
                                <span className="ml-1.5 inline-flex items-center gap-0.5 rounded bg-referee-green/10 px-1.5 py-0.5 text-[9px] font-bold tracking-wide text-referee-green">
                                    <Sparkles className="size-2.5" />
                                    AI
                                </span>
                            </span>
                            <span className="text-[11px] text-referee-muted">
                                {getFeatureToggleDescription(framegenFeature, 'Smooths motion by generating extra frames')}
                            </span>
                        </div>
                        <ToggleField
                            checked={framegenFeature.support === 'unavailable' ? false : settings.framegen}
                            disabled={framegenFeature.disabled}
                            onChange={checked => {
                                onChange({ framegen: checked });
                            }}
                        />
                    </div>

                    <div className="flex items-center justify-between gap-3">
                        <div className="flex min-w-0 flex-col pr-2">
                            <span className={`flex items-center gap-0 text-[13px] font-medium ${getFeatureToggleLabelClass(hdrFeature)}`}>
                                HDR Mapping
                                <span className="ml-1.5 inline-flex items-center gap-0.5 rounded bg-referee-green/10 px-1.5 py-0.5 text-[9px] font-bold tracking-wide text-referee-green">
                                    <Sparkles className="size-2.5" />
                                    AI
                                </span>
                            </span>
                            <span className="text-[11px] text-referee-muted">
                                {getFeatureToggleDescription(hdrFeature, 'Maps SDR video for HDR displays')}
                            </span>
                        </div>
                        <ToggleField
                            checked={hdrFeature.support === 'unavailable' ? false : settings.hdr}
                            disabled={hdrFeature.disabled}
                            onChange={checked => {
                                onChange({ hdr: checked });
                            }}
                        />
                    </div>
                </section>

                <section className={cardClass}>
                    <div className="mb-3 flex flex-col">
                        <span className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">Advanced Settings</span>
                    </div>

                    <div className="flex items-center justify-between gap-3">
                        <div className="flex flex-col pr-2">
                            <span className="text-[13px] font-medium text-white">Show Advanced</span>
                            <span className="text-[11px] leading-4 text-referee-muted">Most users can leave this off</span>
                        </div>
                        <ToggleField
                            checked={advancedSettingsEnabled}
                            onChange={setAdvancedSettingsEnabled}
                        />
                    </div>

                    {advancedSettingsEnabled && (
                        <div
                            className="mt-3 border-l border-[#FF6B35]/30 pl-3"
                            aria-label="Advanced encoder tuning controls"
                        >
                            <div className="flex flex-col gap-3">
                                <p className="text-[11px] leading-4 text-referee-muted">
                                    Fine-tune bitrate, encoder preset, lookahead, B-frames, and HLS segment length.
                                </p>
                                {uiCapabilities.advancedEncoding.reason && (
                                    <p className="text-[10px] leading-4 text-referee-muted">
                                        {uiCapabilities.advancedEncoding.reason}
                                    </p>
                                )}

                                <div className="flex items-center justify-between gap-3">
                                    <span className="text-[13px] font-medium text-white">Encoding Profile</span>
                                    <div className="relative shrink-0">
                                        <select
                                            aria-label="Encoding Profile"
                                            value={profileResolution}
                                            onChange={event => {
                                                setProfileResolution(event.currentTarget.value);
                                            }}
                                            className="cursor-pointer appearance-none rounded border border-transparent bg-[#282828] py-1.5 pl-3 pr-7 text-xs font-medium text-white transition-colors hover:border-white/10 hover:bg-[#333333] focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        >
                                            {ENCODING_PROFILE_RESOLUTIONS.map(resolution => (
                                                <option key={resolution} value={resolution}>
                                                    {formatResolutionLabel(resolution)}
                                                </option>
                                            ))}
                                        </select>
                                        <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2 text-referee-muted">
                                            <ChevronDown className="size-3" />
                                        </div>
                                    </div>
                                </div>

                                <div className="flex flex-col gap-2 border-t border-white/[0.04] pt-3">
                                    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-2">
                                        <span className="text-[11px] text-referee-muted">Bitrate (kbps)</span>
                                        <input
                                            type="number"
                                            min={1000}
                                            step={500}
                                            value={currentProfile.bitrate}
                                            onChange={e => updateProfile({ bitrate: Number(e.currentTarget.value) })}
                                            className="w-[72px] rounded border border-transparent bg-[#282828] px-2 py-1 text-right text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        />

                                        <span className="text-[11px] text-referee-muted">Max Bitrate (kbps)</span>
                                        <input
                                            type="number"
                                            min={1000}
                                            step={500}
                                            value={currentProfile.max_bitrate}
                                            onChange={e => updateProfile({ max_bitrate: Number(e.currentTarget.value) })}
                                            className="w-[72px] rounded border border-transparent bg-[#282828] px-2 py-1 text-right text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        />

                                        <span className="text-[11px] text-referee-muted">Preset</span>
                                        <div className="relative">
                                            <select
                                                value={currentProfile.preset}
                                                onChange={e => updateProfile({ preset: e.currentTarget.value })}
                                                className="w-[72px] cursor-pointer appearance-none rounded border border-transparent bg-[#282828] py-1 pl-2 pr-6 text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                            >
                                                {NVENC_PRESETS.map(p => (
                                                    <option key={p} value={p}>{p}</option>
                                                ))}
                                            </select>
                                            <div className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-1.5 text-referee-muted">
                                                <ChevronDown className="size-3" />
                                            </div>
                                        </div>

                                        <span className="text-[11px] text-referee-muted">Lookahead</span>
                                        <input
                                            type="number"
                                            min={0}
                                            max={32}
                                            value={currentProfile.lookahead}
                                            onChange={e => updateProfile({ lookahead: Number(e.currentTarget.value) })}
                                            className="w-[72px] rounded border border-transparent bg-[#282828] px-2 py-1 text-right text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        />

                                        <span className="text-[11px] text-referee-muted">B-Frames</span>
                                        <input
                                            type="number"
                                            min={0}
                                            max={4}
                                            value={currentProfile.bframes}
                                            onChange={e => updateProfile({ bframes: Number(e.currentTarget.value) })}
                                            className="w-[72px] rounded border border-transparent bg-[#282828] px-2 py-1 text-right text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        />

                                        <span className="text-[11px] text-referee-muted">HLS Segment (s)</span>
                                        <input
                                            type="number"
                                            min={1}
                                            max={10}
                                            value={currentProfile.hls_time}
                                            onChange={e => updateProfile({ hls_time: Number(e.currentTarget.value) })}
                                            className="w-[72px] rounded border border-transparent bg-[#282828] px-2 py-1 text-right text-xs font-medium text-white transition-colors hover:border-white/10 focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green"
                                        />
                                    </div>

                                    <button
                                        type="button"
                                        onClick={resetProfile}
                                        className="mt-1 self-end text-[10px] font-bold uppercase tracking-wider text-referee-muted transition-colors hover:text-white"
                                    >
                                        Reset to Defaults
                                    </button>
                                </div>
                            </div>
                        </div>
                    )}
                </section>

                {import.meta.env.DEV && engineSettings && (
                    <section className={cardClass + ' border-referee-green/30 bg-referee-green/10'}>
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
