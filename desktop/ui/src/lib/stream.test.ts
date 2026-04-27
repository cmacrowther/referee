import { describe, it, expect } from 'vitest'
import {
    getMaxQualityForResolution,
    clampQualityForResolution,
    normalizeActiveProcessingEngine,
    normalizeExecutorPreference,
    normalizeSettings,
    normalizeStatus,
    deriveSettingsUiCapabilities,
    formatResolutionLabel,
    formatFrameRate,
    formatGpuUtilization,
    formatGpuVendor,
    formatEncoderBackend,
    formatExecutorKind,
    formatSourceContentKind,
    formatUpscalerLabel,
    normalizeSessionTextValue,
    getActiveSession,
    isSessionLive,
    getSessionCount,
    getLiveSessionCount,
    hasLiveSession,
    hasPendingSession,
    getStandbyFrameRateLabel,
    getFrameRateTone,
    getGpuTone,
    getActiveStreamBody,
} from './stream'
import { DEFAULT_SETTINGS, DEFAULT_STATUS, type Session, type Status } from './types'

// ─── Fixtures ─────────────────────────────────────────────────────────────────

function makeSession(overrides: Partial<Session> = {}): Session {
    return {
        id: null,
        sourceUrl: null,
        outputUrl: null,
        appName: null,
        streamTitle: null,
        sourceContentKind: null,
        upscaler: null,
        sourceResolution: null,
        outputResolution: null,
        sourceFps: null,
        targetFps: null,
        framegenEnabled: false,
        hdrEnabled: false,
        qualityLevel: null,
        executor: null,
        encoderBackend: null,
        startupComplete: false,
        retryingStartup: false,
        ...overrides,
    }
}

function makeStatus(overrides: Partial<Status> = {}): Status {
    return { ...DEFAULT_STATUS, ...overrides }
}

// ─── getMaxQualityForResolution ────────────────────────────────────────────────

describe('getMaxQualityForResolution', () => {
    it('returns 4 for 4K', () => {
        expect(getMaxQualityForResolution('3840x2160')).toBe(4)
    })

    it('returns 4 for non-4K resolutions', () => {
        expect(getMaxQualityForResolution('1920x1080')).toBe(4)
        expect(getMaxQualityForResolution('2560x1440')).toBe(4)
    })
})

// ─── clampQualityForResolution ─────────────────────────────────────────────────

describe('clampQualityForResolution', () => {
    it('passes through valid in-range integers', () => {
        expect(clampQualityForResolution('1920x1080', 2)).toBe(2)
        expect(clampQualityForResolution('1920x1080', 4)).toBe(4)
    })

    it('clamps values below minimum to 1', () => {
        expect(clampQualityForResolution('1920x1080', 0)).toBe(1)
        expect(clampQualityForResolution('1920x1080', -5)).toBe(1)
    })

    it('clamps values above maximum to the resolution max', () => {
        expect(clampQualityForResolution('1920x1080', 99)).toBe(4)
        expect(clampQualityForResolution('3840x2160', 99)).toBe(4)
    })

    it('rounds fractional values', () => {
        expect(clampQualityForResolution('1920x1080', 2.7)).toBe(3)
        expect(clampQualityForResolution('1920x1080', 1.2)).toBe(1)
    })

    it('parses numeric strings', () => {
        expect(clampQualityForResolution('1920x1080', '3')).toBe(3)
    })

    it('defaults non-finite values to 1', () => {
        expect(clampQualityForResolution('1920x1080', null)).toBe(1)
        expect(clampQualityForResolution('1920x1080', undefined)).toBe(1)
        expect(clampQualityForResolution('1920x1080', NaN)).toBe(1)
        expect(clampQualityForResolution('1920x1080', 'abc')).toBe(1)
    })
})

// ─── normalizeActiveProcessingEngine ─────────────────────────────────────────

describe('normalizeActiveProcessingEngine', () => {
    it('maps nvidia aliases to nvidiaAi', () => {
        expect(normalizeActiveProcessingEngine('NvidiaSpecialized')).toBe('nvidiaAi')
        expect(normalizeActiveProcessingEngine('nvidia')).toBe('nvidiaAi')
        expect(normalizeActiveProcessingEngine('NvidiaSpecializedExecutor')).toBe('nvidiaAi')
    })

    it('maps amd aliases to amdAi', () => {
        expect(normalizeActiveProcessingEngine('amd')).toBe('amdAi')
        expect(normalizeActiveProcessingEngine('amdAi')).toBe('amdAi')
        expect(normalizeActiveProcessingEngine('AMD')).toBe('amdAi')
    })

    it('maps universal', () => {
        expect(normalizeActiveProcessingEngine('universal')).toBe('universal')
        expect(normalizeActiveProcessingEngine('  universal  ')).toBe('universal')
    })

    it('maps cpu', () => {
        expect(normalizeActiveProcessingEngine('cpu')).toBe('cpu')
    })

    it('returns null for null, undefined, and empty string', () => {
        expect(normalizeActiveProcessingEngine(null)).toBe(null)
        expect(normalizeActiveProcessingEngine(undefined)).toBe(null)
        expect(normalizeActiveProcessingEngine('')).toBe(null)
    })

    it('returns null for unrecognized values', () => {
        expect(normalizeActiveProcessingEngine('intel')).toBe(null)
    })
})

// ─── normalizeExecutorPreference ─────────────────────────────────────────────

describe('normalizeExecutorPreference', () => {
    // Vitest runs in dev mode (import.meta.env.DEV === true), so the function's
    // full normalization logic runs rather than the non-DEV short-circuit.
    it('maps nvidiaAi', () => {
        expect(normalizeExecutorPreference('nvidiaAi')).toBe('nvidiaAi')
        expect(normalizeExecutorPreference('nvidia')).toBe('nvidiaAi')
        expect(normalizeExecutorPreference('nvidiaSpecialized')).toBe('nvidiaAi')
    })

    it('maps amdAi', () => {
        expect(normalizeExecutorPreference('amdAi')).toBe('amdAi')
        expect(normalizeExecutorPreference('amd')).toBe('amdAi')
    })

    it('maps universal', () => {
        expect(normalizeExecutorPreference('universal')).toBe('universal')
    })

    it('returns the default for unrecognized values', () => {
        expect(normalizeExecutorPreference('unknown')).toBe(DEFAULT_SETTINGS.executorPreference)
        expect(normalizeExecutorPreference(null)).toBe(DEFAULT_SETTINGS.executorPreference)
    })
})

// ─── normalizeSettings ────────────────────────────────────────────────────────

describe('normalizeSettings', () => {
    it('returns full defaults when called with no arguments', () => {
        const result = normalizeSettings()
        expect(result.resolution).toBe(DEFAULT_SETTINGS.resolution)
        expect(result.quality).toBe(DEFAULT_SETTINGS.quality)
        expect(result.framegen).toBe(DEFAULT_SETTINGS.framegen)
        expect(result.hdr).toBe(DEFAULT_SETTINGS.hdr)
        expect(result.alwaysOnTop).toBe(false)
        expect(result.instanceId).toBe(null)
        expect(result.relay).toEqual(DEFAULT_SETTINGS.relay)
    })

    it('applies provided overrides', () => {
        const result = normalizeSettings({ resolution: '2560x1440', quality: 3 })
        expect(result.resolution).toBe('2560x1440')
        expect(result.quality).toBe(3)
    })

    it('clamps quality to the max for the given resolution', () => {
        expect(normalizeSettings({ resolution: '1920x1080', quality: 99 }).quality).toBe(4)
    })

    it('coerces boolean fields from truthy/falsy values', () => {
        const result = normalizeSettings({
            framegen: 0 as any,
            hdr: 1 as any,
            alwaysOnTop: 'yes' as any,
        })
        expect(result.framegen).toBe(false)
        expect(result.hdr).toBe(true)
        expect(result.alwaysOnTop).toBe(true)
    })

    it('merges encodingProfiles with defaults, preserving unspecified profiles', () => {
        const customProfile = {
            bitrate: 99999,
            max_bitrate: 999,
            preset: 'p1',
            lookahead: 0,
            bframes: 0,
            hls_time: 2,
        }
        const result = normalizeSettings({
            encodingProfiles: { '1920x1080': customProfile },
        })
        expect(result.encodingProfiles['1920x1080']).toEqual(customProfile)
        expect(result.encodingProfiles['2560x1440']).toBeDefined()
    })

    it('populates player fields from defaults when not provided', () => {
        const result = normalizeSettings({})
        expect(result.player.enabled).toBe(false)
        expect(result.player.selectedPlayer).toBe(null)
        expect(result.player.customPath).toBe(null)
    })

    it('preserves provided player fields', () => {
        const result = normalizeSettings({
            player: { enabled: true, selectedPlayer: 'vlc', customPath: null },
        })
        expect(result.player.enabled).toBe(true)
        expect(result.player.selectedPlayer).toBe('vlc')
    })

    it('normalizes instanceId and relay metadata fields', () => {
        const result = normalizeSettings({
            instanceId: '  instance-123  ',
            relay: {
                enabled: true,
                linkedPeerId: ' peer-1 ',
                linkedPeerHostname: ' media-box ',
                linkedPeerIp: ' 192.168.1.23 ',
                remoteToken: ' relay-secret ',
                lastKnownPeer: {
                    instanceId: ' peer-1 ',
                    hostname: ' media-box ',
                    ip: ' 192.168.1.23 ',
                    version: ' 1.2.3 ',
                    platform: ' linux ',
                    gpuReady: true,
                    gpuVendor: ' nvidia ',
                    gpuName: ' RTX 4080 ',
                },
            },
        })

        expect(result.instanceId).toBe('instance-123')
        expect(result.relay.enabled).toBe(true)
        expect(result.relay.linkedPeerId).toBe('peer-1')
        expect(result.relay.linkedPeerHostname).toBe('media-box')
        expect(result.relay.linkedPeerIp).toBe('192.168.1.23')
        expect(result.relay.remoteToken).toBe('relay-secret')
        expect(result.relay.lastKnownPeer).toEqual({
            instanceId: 'peer-1',
            hostname: 'media-box',
            ip: '192.168.1.23',
            version: '1.2.3',
            platform: 'linux',
            gpuReady: true,
            gpuVendor: 'nvidia',
            gpuName: 'RTX 4080',
        })
    })

    it('drops empty relay peer metadata objects back to null', () => {
        const result = normalizeSettings({
            relay: {
                enabled: true,
                linkedPeerId: null,
                linkedPeerHostname: null,
                linkedPeerIp: null,
                remoteToken: null,
                lastKnownPeer: {
                    instanceId: '   ',
                    hostname: null,
                    ip: null,
                    version: null,
                    platform: null,
                    gpuReady: null,
                    gpuVendor: null,
                    gpuName: null,
                },
            },
        } as any)

        expect(result.relay.lastKnownPeer).toBe(null)
    })
})

// ─── normalizeStatus ──────────────────────────────────────────────────────────

describe('normalizeStatus', () => {
    it('returns DEFAULT_STATUS shape when called with no arguments', () => {
        const result = normalizeStatus()
        expect(result.gpuReady).toBe(DEFAULT_STATUS.gpuReady)
        expect(result.activeSessions).toBe(0)
        expect(result.sessions).toEqual([])
        expect(result.primarySession).toBe(null)
    })

    it('accepts camelCase GPU keys', () => {
        const result = normalizeStatus({
            gpuReady: true,
            gpuName: 'RTX 4080',
            gpuVendor: 'nvidia',
        })
        expect(result.gpuReady).toBe(true)
        expect(result.gpuName).toBe('RTX 4080')
        expect(result.gpuVendor).toBe('nvidia')
    })

    it('accepts snake_case GPU keys', () => {
        const result = normalizeStatus({
            gpu_ready: true,
            gpu_name: 'RX 7900',
            gpu_vendor: 'amd',
        } as any)
        expect(result.gpuReady).toBe(true)
        expect(result.gpuName).toBe('RX 7900')
        expect(result.gpuVendor).toBe('amd')
    })

    it('prefers camelCase over snake_case for GPU fields', () => {
        const result = normalizeStatus({
            gpuName: 'camelCase GPU',
            gpu_name: 'snake_case GPU',
        } as any)
        expect(result.gpuName).toBe('camelCase GPU')
    })

    it('normalizes sessions array and derives activeSessions from array length', () => {
        const result = normalizeStatus({
            sessions: [{ id: 'a', startupComplete: true } as any, { id: 'b', startupComplete: false } as any],
        })
        expect(result.sessions).toHaveLength(2)
        expect(result.activeSessions).toBe(2)
    })

    it('prefers explicit activeSessions count over sessions.length', () => {
        const result = normalizeStatus({ activeSessions: 5, sessions: [] })
        expect(result.activeSessions).toBe(5)
    })

    it('falls back to preview_sessions when sessions is absent', () => {
        const result = normalizeStatus({ preview_sessions: [{ id: 'p1' }] } as any)
        expect(result.sessions).toHaveLength(1)
        expect(result.sessions[0].id).toBe('p1')
    })

    it('normalizes snake_case session fields', () => {
        const result = normalizeStatus({
            sessions: [
                {
                    app_name: 'VLC',
                    source_url: 'rtmp://x',
                    startup_complete: true,
                } as any,
            ],
        })
        const session = result.sessions[0]
        expect(session.appName).toBe('VLC')
        expect(session.sourceUrl).toBe('rtmp://x')
        expect(session.startupComplete).toBe(true)
    })

    it('treats startup_stage "ready" as startupComplete: true', () => {
        const result = normalizeStatus({
            sessions: [{ startup_stage: 'ready' } as any],
        })
        expect(result.sessions[0].startupComplete).toBe(true)
    })

    it('filters null sessions from the array', () => {
        const result = normalizeStatus({
            sessions: [null, { id: 'valid' }] as any,
        })
        expect(result.sessions).toHaveLength(1)
        expect(result.sessions[0].id).toBe('valid')
    })

    it('falls back primarySession to sessions[0] when primarySession is absent', () => {
        const result = normalizeStatus({
            sessions: [{ id: 'first-session' }] as any,
        })
        expect(result.primarySession?.id).toBe('first-session')
    })

    it('normalizes encoder feature flags from camelCase', () => {
        const result = normalizeStatus({
            encoderHasFramegen: true,
            encoderHasTruehdr: false,
            encoderHasRife: true,
        })
        expect(result.encoderHasFramegen).toBe(true)
        expect(result.encoderHasTruehdr).toBe(false)
        expect(result.encoderHasRife).toBe(true)
    })

    it('normalizes encoder feature flags from snake_case', () => {
        const result = normalizeStatus({
            encoder_has_framegen: true,
            encoder_has_truehdr: true,
        } as any)
        expect(result.encoderHasFramegen).toBe(true)
        expect(result.encoderHasTruehdr).toBe(true)
    })

    it('falls back selectedExecutor to primarySession executor', () => {
        const result = normalizeStatus({
            primarySession: { executor: 'NvidiaSpecialized' } as any,
        })
        expect(result.selectedExecutor).toBe('NvidiaSpecialized')
    })

    it('merges settings with DEFAULT_STATUS.settings', () => {
        const result = normalizeStatus({
            settings: { resolution: '3840x2160' } as any,
        })
        expect(result.settings.resolution).toBe('3840x2160')
        expect(result.settings.quality).toBe(DEFAULT_STATUS.settings.quality)
    })
})

// ─── formatResolutionLabel ────────────────────────────────────────────────────

describe('formatResolutionLabel', () => {
    it('returns null for missing input', () => {
        expect(formatResolutionLabel(null)).toBe(null)
        expect(formatResolutionLabel(undefined)).toBe(null)
        expect(formatResolutionLabel('')).toBe(null)
    })

    it('identifies 4K (height >= 2160)', () => {
        expect(formatResolutionLabel('3840x2160')).toBe('4K')
    })

    it('identifies 1440P (height >= 1440)', () => {
        expect(formatResolutionLabel('2560x1440')).toBe('1440P')
    })

    it('identifies 1080P', () => {
        expect(formatResolutionLabel('1920x1080')).toBe('1080P')
    })

    it('identifies 720P', () => {
        expect(formatResolutionLabel('1280x720')).toBe('720P')
    })

    it('identifies 480P and 360P', () => {
        expect(formatResolutionLabel('854x480')).toBe('480P')
        expect(formatResolutionLabel('640x360')).toBe('360P')
    })

    it('returns Np for unlabelled heights', () => {
        expect(formatResolutionLabel('426x240')).toBe('240P')
    })

    it('uppercases strings that do not match the resolution pattern', () => {
        expect(formatResolutionLabel('unknown')).toBe('UNKNOWN')
    })
})

// ─── formatFrameRate ──────────────────────────────────────────────────────────

describe('formatFrameRate', () => {
    it('returns null for non-finite and non-positive values', () => {
        expect(formatFrameRate(null)).toBe(null)
        expect(formatFrameRate(undefined)).toBe(null)
        expect(formatFrameRate(0)).toBe(null)
        expect(formatFrameRate(-1)).toBe(null)
        expect(formatFrameRate(NaN)).toBe(null)
    })

    it('formats integer fps', () => {
        expect(formatFrameRate(60)).toBe('60 FPS')
        expect(formatFrameRate(30)).toBe('30 FPS')
    })

    it('rounds near-integer fps to integer format', () => {
        expect(formatFrameRate(29.97)).toBe('30 FPS')
        expect(formatFrameRate(23.976)).toBe('24 FPS')
    })

    it('formats non-integer fps to 1 decimal place', () => {
        expect(formatFrameRate(59.94)).toBe('59.9 FPS')
    })
})

// ─── formatGpuUtilization ─────────────────────────────────────────────────────

describe('formatGpuUtilization', () => {
    it('returns null for non-finite or negative values', () => {
        // Number(null) === 0 (valid), Number(undefined) === NaN (non-finite)
        expect(formatGpuUtilization(undefined)).toBe(null)
        expect(formatGpuUtilization(-1)).toBe(null)
        expect(formatGpuUtilization(NaN)).toBe(null)
    })

    it('treats null as 0% (Number(null) === 0)', () => {
        expect(formatGpuUtilization(null)).toBe('0% Load')
    })

    it('formats 0% correctly', () => {
        expect(formatGpuUtilization(0)).toBe('0% Load')
    })

    it('rounds and formats utilization', () => {
        expect(formatGpuUtilization(75.4)).toBe('75% Load')
        expect(formatGpuUtilization(100)).toBe('100% Load')
    })
})

// ─── formatGpuVendor ──────────────────────────────────────────────────────────

describe('formatGpuVendor', () => {
    it('returns "Unknown" for missing input', () => {
        expect(formatGpuVendor(null)).toBe('Unknown')
        expect(formatGpuVendor(undefined)).toBe('Unknown')
        expect(formatGpuVendor('')).toBe('Unknown')
    })

    it('formats nvidia (case-insensitive)', () => {
        expect(formatGpuVendor('nvidia')).toBe('NVIDIA®')
        expect(formatGpuVendor('NVIDIA')).toBe('NVIDIA®')
    })

    it('formats amd (case-insensitive)', () => {
        expect(formatGpuVendor('amd')).toBe('AMD®')
        expect(formatGpuVendor('AMD')).toBe('AMD®')
    })

    it('uppercases other vendor strings', () => {
        expect(formatGpuVendor('intel')).toBe('INTEL')
    })
})

// ─── formatEncoderBackend ─────────────────────────────────────────────────────

describe('formatEncoderBackend', () => {
    it('returns "Not Detected" for missing input', () => {
        expect(formatEncoderBackend(null)).toBe('Not Detected')
        expect(formatEncoderBackend(undefined)).toBe('Not Detected')
        expect(formatEncoderBackend('')).toBe('Not Detected')
    })

    it('formats nvenc', () => {
        expect(formatEncoderBackend('nvenc')).toBe('NVENC')
        expect(formatEncoderBackend('NVENC')).toBe('NVENC')
    })

    it('formats vceenc as VCE', () => {
        expect(formatEncoderBackend('vceenc')).toBe('VCE')
    })

    it('uppercases unrecognized backends', () => {
        expect(formatEncoderBackend('h264')).toBe('H264')
    })
})

// ─── formatExecutorKind ───────────────────────────────────────────────────────

describe('formatExecutorKind', () => {
    it('returns "Not Selected" for missing input', () => {
        expect(formatExecutorKind(null)).toBe('Not Selected')
        expect(formatExecutorKind(undefined)).toBe('Not Selected')
        expect(formatExecutorKind('')).toBe('Not Selected')
    })

    it('maps nvidia aliases to "NVIDIA® AI"', () => {
        expect(formatExecutorKind('nvidia')).toBe('NVIDIA® AI')
        expect(formatExecutorKind('NvidiaSpecialized')).toBe('NVIDIA® AI')
        expect(formatExecutorKind('NvidiaSpecializedExecutor')).toBe('NVIDIA® AI')
    })

    it('maps universal', () => {
        expect(formatExecutorKind('universal')).toBe('Universal')
    })

    it('maps cpu', () => {
        expect(formatExecutorKind('cpu')).toBe('CPU')
    })

    it('returns unrecognized values as-is', () => {
        expect(formatExecutorKind('CustomExecutor')).toBe('CustomExecutor')
    })
})

// ─── formatSourceContentKind ──────────────────────────────────────────────────

describe('formatSourceContentKind', () => {
    it('returns null for missing input', () => {
        expect(formatSourceContentKind(null)).toBe(null)
        expect(formatSourceContentKind(undefined)).toBe(null)
    })

    it('formats known kinds', () => {
        expect(formatSourceContentKind('animated')).toBe('Animated')
        expect(formatSourceContentKind('liveaction')).toBe('Live Action')
        expect(formatSourceContentKind('live_action')).toBe('Live Action')
        expect(formatSourceContentKind('live-action')).toBe('Live Action')
        expect(formatSourceContentKind('unknown')).toBe('Unknown')
    })

    it('returns unrecognized values as-is', () => {
        expect(formatSourceContentKind('sports')).toBe('sports')
    })
})

// ─── formatUpscalerLabel ──────────────────────────────────────────────────────

describe('formatUpscalerLabel', () => {
    it('returns null for missing input', () => {
        expect(formatUpscalerLabel(null)).toBe(null)
        expect(formatUpscalerLabel(undefined)).toBe(null)
    })

    it('maps known identifiers', () => {
        expect(formatUpscalerLabel('anime4k2x')).toBe('Anime4K2x')
        expect(formatUpscalerLabel('artcnn')).toBe('ArtCNN')
        expect(formatUpscalerLabel('libplacebo')).toBe('Placebo')
        expect(formatUpscalerLabel('nvidiaai')).toBe('NVIDIA® AI')
    })

    it('returns unrecognized values as-is', () => {
        expect(formatUpscalerLabel('custom-upscaler')).toBe('custom-upscaler')
    })
})

// ─── normalizeSessionTextValue ────────────────────────────────────────────────

describe('normalizeSessionTextValue', () => {
    it('trims surrounding whitespace', () => {
        expect(normalizeSessionTextValue('  hello  ')).toBe('hello')
    })

    it('returns empty string for null or undefined', () => {
        expect(normalizeSessionTextValue(null)).toBe('')
        expect(normalizeSessionTextValue(undefined)).toBe('')
    })

    it('preserves internal whitespace', () => {
        expect(normalizeSessionTextValue('foo bar')).toBe('foo bar')
    })
})

// ─── isSessionLive ────────────────────────────────────────────────────────────

describe('isSessionLive', () => {
    it('returns true when startupComplete is true', () => {
        expect(isSessionLive(makeSession({ startupComplete: true }))).toBe(true)
    })

    it('returns false when startupComplete is false', () => {
        expect(isSessionLive(makeSession({ startupComplete: false }))).toBe(false)
    })

    it('returns false for null or undefined', () => {
        expect(isSessionLive(null)).toBe(false)
        expect(isSessionLive(undefined)).toBe(false)
    })
})

// ─── getSessionCount ──────────────────────────────────────────────────────────

describe('getSessionCount', () => {
    it('returns 0 for null or undefined', () => {
        expect(getSessionCount(null)).toBe(0)
        expect(getSessionCount(undefined)).toBe(0)
    })

    it('uses activeSessions when it is the highest value', () => {
        expect(getSessionCount(makeStatus({ activeSessions: 5, sessions: [] }))).toBe(5)
    })

    it('falls back to sessions.length when activeSessions is 0', () => {
        const s = makeSession()
        expect(getSessionCount(makeStatus({ activeSessions: 0, sessions: [s, s] }))).toBe(2)
    })

    it('counts primarySession when sessions array is empty', () => {
        expect(
            getSessionCount(
                makeStatus({
                    activeSessions: 0,
                    sessions: [],
                    primarySession: makeSession(),
                }),
            ),
        ).toBe(1)
    })
})

// ─── getLiveSessionCount ──────────────────────────────────────────────────────

describe('getLiveSessionCount', () => {
    it('returns 0 for empty status', () => {
        expect(getLiveSessionCount(makeStatus())).toBe(0)
    })

    it('counts only live sessions in the sessions array', () => {
        const s = makeStatus({
            sessions: [makeSession({ startupComplete: true }), makeSession({ startupComplete: false }), makeSession({ startupComplete: true })],
        })
        expect(getLiveSessionCount(s)).toBe(2)
    })

    it('falls back to primarySession when sessions array has no live sessions', () => {
        const s = makeStatus({
            sessions: [],
            primarySession: makeSession({ startupComplete: true }),
        })
        expect(getLiveSessionCount(s)).toBe(1)
    })

    it('returns 0 when primarySession is not live', () => {
        const s = makeStatus({
            sessions: [],
            primarySession: makeSession({ startupComplete: false }),
        })
        expect(getLiveSessionCount(s)).toBe(0)
    })
})

// ─── hasLiveSession ───────────────────────────────────────────────────────────

describe('hasLiveSession', () => {
    it('returns false for empty status', () => {
        expect(hasLiveSession(makeStatus())).toBe(false)
    })

    it('returns true when a session is live', () => {
        expect(hasLiveSession(makeStatus({ primarySession: makeSession({ startupComplete: true }) }))).toBe(true)
    })

    it('returns false for null', () => {
        expect(hasLiveSession(null)).toBe(false)
    })
})

// ─── hasPendingSession ────────────────────────────────────────────────────────

describe('hasPendingSession', () => {
    it('returns false for empty status', () => {
        expect(hasPendingSession(makeStatus())).toBe(false)
    })

    it('returns true when there is a session count but no live sessions', () => {
        const s = makeStatus({
            activeSessions: 1,
            sessions: [makeSession({ startupComplete: false })],
        })
        expect(hasPendingSession(s)).toBe(true)
    })

    it('returns false when sessions are all live', () => {
        const s = makeStatus({
            activeSessions: 1,
            sessions: [makeSession({ startupComplete: true })],
        })
        expect(hasPendingSession(s)).toBe(false)
    })
})

// ─── getActiveSession ─────────────────────────────────────────────────────────

describe('getActiveSession', () => {
    it('returns null for null or undefined status', () => {
        expect(getActiveSession(null)).toBe(null)
        expect(getActiveSession(undefined)).toBe(null)
    })

    it('returns null when session count is 0', () => {
        expect(getActiveSession(makeStatus())).toBe(null)
    })

    it('returns primarySession when it has an id', () => {
        const primary = makeSession({ id: 'primary-id' })
        const s = makeStatus({ activeSessions: 1, primarySession: primary })
        expect(getActiveSession(s)).toBe(primary)
    })

    it('returns primarySession when it has an appName', () => {
        const primary = makeSession({ appName: 'VLC' })
        const s = makeStatus({ activeSessions: 1, primarySession: primary })
        expect(getActiveSession(s)).toBe(primary)
    })

    it('returns first sessions[] entry when primarySession has no identifying fields', () => {
        const session = makeSession({ id: 'first' })
        const s = makeStatus({
            activeSessions: 1,
            sessions: [session],
            primarySession: makeSession(),
        })
        expect(getActiveSession(s)?.id).toBe('first')
    })
})

// ─── getStandbyFrameRateLabel ─────────────────────────────────────────────────

describe('getStandbyFrameRateLabel', () => {
    it('returns "Up to 60 FPS" when framegen is on', () => {
        expect(getStandbyFrameRateLabel({ ...DEFAULT_SETTINGS, framegen: true })).toBe('Up to 60 FPS')
    })

    it('returns "Native" when framegen is off', () => {
        expect(getStandbyFrameRateLabel({ ...DEFAULT_SETTINGS, framegen: false })).toBe('Native')
    })
})

// ─── getFrameRateTone ─────────────────────────────────────────────────────────

describe('getFrameRateTone', () => {
    it('returns "idle" for non-finite value with no active stream', () => {
        expect(getFrameRateTone(null, false)).toBe('idle')
        expect(getFrameRateTone(undefined, false)).toBe('idle')
    })

    it('returns "accent" for non-finite value with an active stream', () => {
        expect(getFrameRateTone(null, true)).toBe('accent')
    })

    it('returns "alert" for fps below 30', () => {
        expect(getFrameRateTone(29, true)).toBe('alert')
        expect(getFrameRateTone(1, false)).toBe('alert')
    })

    it('returns "accent" for fps at or above 30', () => {
        expect(getFrameRateTone(30, true)).toBe('accent')
        expect(getFrameRateTone(60, false)).toBe('accent')
    })
})

// ─── getGpuTone ───────────────────────────────────────────────────────────────

describe('getGpuTone', () => {
    it('returns "idle" for non-finite value (undefined/NaN) with no active stream', () => {
        // Number(undefined) === NaN — genuinely missing data → idle/accent based on stream
        expect(getGpuTone(undefined, false)).toBe('idle')
        expect(getGpuTone(NaN, false)).toBe('idle')
    })

    it('treats null as 0% utilization (Number(null) === 0), returning accent', () => {
        // 0% GPU is a valid reading below the 90% alert threshold
        expect(getGpuTone(null, false)).toBe('accent')
        expect(getGpuTone(null, true)).toBe('accent')
    })

    it('returns "accent" for non-finite value with an active stream', () => {
        expect(getGpuTone(undefined, true)).toBe('accent')
    })

    it('returns "alert" for utilization at or above 90%', () => {
        expect(getGpuTone(90, true)).toBe('alert')
        expect(getGpuTone(100, false)).toBe('alert')
    })

    it('returns "accent" for utilization below 90%', () => {
        expect(getGpuTone(89, true)).toBe('accent')
        expect(getGpuTone(0, false)).toBe('accent')
    })
})

// ─── getActiveStreamBody ──────────────────────────────────────────────────────

describe('getActiveStreamBody', () => {
    it('includes app name and resolution labels when session is fully populated', () => {
        const session = makeSession({
            appName: 'VLC',
            sourceResolution: '1920x1080',
            outputResolution: '3840x2160',
        })
        const body = getActiveStreamBody(session, '1920x1080')
        expect(body).toContain('VLC')
        expect(body).toContain('1080P')
        expect(body).toContain('4K')
    })

    it('omits "for <appName>" when appName is absent', () => {
        const session = makeSession({
            sourceResolution: '1920x1080',
            outputResolution: '3840x2160',
        })
        const body = getActiveStreamBody(session, '1920x1080')
        expect(body).not.toMatch(/for \w/)
        expect(body).toContain('1080P')
        expect(body).toContain('4K')
    })

    it('uses fallback resolution when outputResolution is null', () => {
        const session = makeSession({ sourceResolution: '1920x1080' })
        expect(getActiveStreamBody(session, '3840x2160')).toContain('4K')
    })

    it('uses AUTO as source label when sourceResolution is null', () => {
        expect(getActiveStreamBody(null, '1920x1080')).toContain('AUTO')
    })
})

// ─── deriveSettingsUiCapabilities ─────────────────────────────────────────────

describe('deriveSettingsUiCapabilities', () => {
    it('returns pending resolved path and unavailable features when no hardware signal is present', () => {
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, DEFAULT_STATUS, false)
        expect(caps.resolvedPath.kind).toBe('pending')
        expect(caps.features.upscale.support).toBe('unavailable')
        expect(caps.features.framegen.support).toBe('unavailable')
        expect(caps.features.hdr.support).toBe('unavailable')
    })

    it('resolves nvidiaNative path when selectedExecutor is NvidiaSpecialized', () => {
        const status = makeStatus({
            gpuName: 'RTX 4080',
            selectedExecutor: 'NvidiaSpecialized',
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.resolvedPath.kind).toBe('nvidiaNative')
        expect(caps.features.upscale.support).toBe('native')
        expect(caps.advancedEncoding.profileShape).toBe('nvenc')
    })

    it('resolves amdNative path when selectedExecutor is amdAi', () => {
        const status = makeStatus({
            gpuName: 'RX 7900',
            gpuVendor: 'amd',
            selectedExecutor: 'amdAi',
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.resolvedPath.kind).toBe('amdNative')
        expect(caps.features.upscale.support).toBe('native')
        expect(caps.advancedEncoding.profileShape).toBe('generic')
    })

    it('resolves universal path when selectedExecutor is universal', () => {
        const status = makeStatus({
            encoderBackend: 'generic',
            selectedExecutor: 'universal',
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.resolvedPath.kind).toBe('universal')
        expect(caps.features.upscale.support).toBe('fallback')
    })

    it('resolves cpu path when selectedExecutor is cpu', () => {
        const status = makeStatus({ selectedExecutor: 'cpu' })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.resolvedPath.kind).toBe('cpu')
        expect(caps.features.upscale.support).toBe('fallback')
        expect(caps.features.upscale.sourceLabel).toBe('Portable local upscale')
    })

    it('reports native framegen for NVIDIA when encoderHasFramegen is true', () => {
        const status = makeStatus({
            gpuName: 'RTX 4080',
            selectedExecutor: 'NvidiaSpecialized',
            encoderHasFramegen: true,
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.features.framegen.support).toBe('native')
        expect(caps.features.framegen.sourceLabel).toBe('NVIDIA® FRUC')
    })

    it('reports RIFE fallback for framegen when encoderHasRife is true', () => {
        const status = makeStatus({
            encoderBackend: 'generic',
            selectedExecutor: 'universal',
            encoderHasRife: true,
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.features.framegen.support).toBe('fallback')
        expect(caps.features.framegen.sourceLabel).toBe('RIFE interpolation')
    })

    it('reports native HDR for NVIDIA with encoderHasTruehdr', () => {
        const status = makeStatus({
            gpuName: 'RTX 4080',
            selectedExecutor: 'NvidiaSpecialized',
            encoderHasTruehdr: true,
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.features.hdr.support).toBe('native')
        expect(caps.features.hdr.sourceLabel).toBe('NVIDIA® TrueHDR')
    })

    it('reports fallback HDR for universal path', () => {
        const status = makeStatus({
            encoderBackend: 'generic',
            selectedExecutor: 'universal',
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        expect(caps.features.hdr.support).toBe('fallback')
        expect(caps.features.hdr.sourceLabel).toBe('REFEREE HDR mapping')
    })

    it('disables all features when locked is true', () => {
        const status = makeStatus({
            gpuName: 'RTX 4080',
            selectedExecutor: 'NvidiaSpecialized',
        })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, true)
        expect(caps.lockState).toBe('locked')
        expect(caps.features.upscale.disabled).toBe(true)
        expect(caps.features.framegen.disabled).toBe(true)
        expect(caps.features.hdr.disabled).toBe(true)
    })

    it('exposes four renderer preference options', () => {
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, DEFAULT_STATUS, false)
        const values = caps.rendererPreferenceOptions.map((o) => o.value)
        expect(values).toEqual(['auto', 'nvidiaAi', 'amdAi', 'universal'])
    })

    it('marks renderer options as unavailable when detection is pending', () => {
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, DEFAULT_STATUS, false)
        expect(caps.rendererPreferenceOptions.find((o) => o.value === 'auto')?.availability).toBe('unavailable')
        expect(caps.rendererPreferenceOptions.find((o) => o.value === 'universal')?.availability).toBe('unavailable')
    })

    it('marks nvidia option as native when nvidiaAiAvailable is true', () => {
        const status = makeStatus({ gpuName: 'RTX 4080', nvidiaAiAvailable: true })
        const caps = deriveSettingsUiCapabilities(DEFAULT_SETTINGS, status, false)
        const nvidiaOption = caps.rendererPreferenceOptions.find((o) => o.value === 'nvidiaAi')!
        expect(nvidiaOption.availability).toBe('native')
        expect(nvidiaOption.selectable).toBe(true)
    })

    it('returns correct max quality for the given resolution', () => {
        const settings4k = { ...DEFAULT_SETTINGS, resolution: '3840x2160' }
        const caps = deriveSettingsUiCapabilities(settings4k, DEFAULT_STATUS, false)
        expect(caps.quality.max).toBe(4)
    })
})
