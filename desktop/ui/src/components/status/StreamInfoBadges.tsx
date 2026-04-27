import {
    formatEncoderBackend,
    formatGpuUtilization,
    formatResolutionLabel,
    formatSourceContentKind,
    formatUpscalerLabel,
    getActiveSession,
    getGpuTone,
    getLiveSessionCount,
    getSessionCount,
    hasPendingSession
} from '@/lib/stream';
import type { Settings, Status } from '@/lib/types';

interface StreamInfoBadgesProps {
    status: Status;
    settings: Settings;
}

export function StreamInfoBadges({ status, settings }: StreamInfoBadgesProps) {
    const session = getActiveSession(status);
    const hasStreamSession = getSessionCount(status) > 0;
    const liveSessionCount = getLiveSessionCount(status);
    const hasLiveStream = liveSessionCount > 0;
    const isStreamStarting = hasPendingSession(status);
    const targetResolution = formatResolutionLabel(session?.outputResolution || settings.resolution) || 'TARGET';
    const gpuLabel = formatGpuUtilization(status.gpuUtilization) || (hasStreamSession ? 'Warming Up' : 'Ready');
    const streamStateLabel = hasLiveStream ? 'RELAY' : isStreamStarting ? 'Starting' : 'Standby';
    const framegenEnabled = session?.framegenEnabled ?? false;
    const hdrEnabled = session?.hdrEnabled ?? false;
    const framegenSupported = status.encoderHasFramegen !== false;
    const hdrSupported = status.encoderHasTruehdr !== false;
    const encoderLabel = formatEncoderBackend(status.encoderBackend) || 'not detected';
    const sourceContentLabel = formatSourceContentKind(session?.sourceContentKind) || null;
    const upscalerLabel = formatUpscalerLabel(session?.upscaler) || null;
    const sourceName = session?.appName || null;
    const statusSummary = `${streamStateLabel}, upscaled output ${targetResolution}${upscalerLabel ? ` via ${upscalerLabel}` : ''}${sourceContentLabel ? `, detected as ${sourceContentLabel}` : ''}, frame gen ${framegenSupported ? (framegenEnabled ? 'on' : 'off') : 'not supported'}, HDR ${hdrSupported ? (hdrEnabled ? 'on' : 'off') : 'not supported'}, GPU ${gpuLabel}, encoder ${encoderLabel}`;

    return (
        <div className="stream-info-badges" role="group" aria-label={statusSummary}>
            <span className="stream-info-status" data-tone={hasStreamSession ? 'accent' : 'idle'}>
                <span className="stream-info-status-dot" aria-hidden="true" />
                <span className="stream-info-status-label">{streamStateLabel}</span>
                {upscalerLabel && <span className="stream-info-source">{upscalerLabel}</span>}
                {sourceName && <span className="stream-info-source">{sourceName}</span>}
            </span>
        </div>
    );
}

export function StreamInfoSegments({ status, settings }: StreamInfoBadgesProps) {
    const session = getActiveSession(status);
    const hasStreamSession = getSessionCount(status) > 0;
    const targetResolution = formatResolutionLabel(session?.outputResolution || settings.resolution) || 'TARGET';
    const gpuLabel = formatGpuUtilization(status.gpuUtilization) || (hasStreamSession ? 'Warming Up' : 'Ready');
    const compactGpuLabel = gpuLabel.replace(' Load', '');
    const gpuTone = getGpuTone(status.gpuUtilization, hasStreamSession);
    const framegenEnabled = session?.framegenEnabled ?? settings.framegen;
    const hdrEnabled = session?.hdrEnabled ?? false;
    const framegenSupported = status.encoderHasFramegen !== false;
    const hdrSupported = status.encoderHasTruehdr !== false;
    const encoderLabel = formatEncoderBackend(status.encoderBackend) || '—';
    const upscalerLabel = formatUpscalerLabel(session?.upscaler) || 'AUTO';

    return (
        <div className="stream-info-segments">
            <span className="stream-info-segment stream-info-segment-path">
                <span className="stream-info-meta">Res</span>
                <span className="stream-info-value stream-path-value" title={`Upscaled output ${targetResolution}`}>
                    <span className="stream-path-segment">{targetResolution}</span>
                </span>
            </span>

            <span className="stream-info-divider" aria-hidden="true" />

            <span className="stream-info-segment" data-tone={framegenEnabled && framegenSupported ? 'accent' : 'idle'}>
                <span className="stream-info-meta">FG</span>
                <span className="stream-info-value">
                    {!framegenSupported ? 'N/A' : framegenEnabled ? 'ON' : 'OFF'}
                </span>
            </span>

            <span className="stream-info-divider" aria-hidden="true" />

            <span className="stream-info-segment" data-tone={hdrEnabled && hdrSupported ? 'accent' : 'idle'}>
                <span className="stream-info-meta">HDR</span>
                <span className="stream-info-value">
                    {!hdrSupported ? 'N/A' : hdrEnabled ? 'ON' : 'OFF'}
                </span>
            </span>

            <span className="stream-info-divider" aria-hidden="true" />

            <span className="stream-info-segment" data-tone={gpuTone}>
                <span className="stream-info-meta">GPU</span>
                <span className="stream-info-value" title={gpuLabel}>
                    {compactGpuLabel}
                </span>
            </span>
        </div>
    );
}
