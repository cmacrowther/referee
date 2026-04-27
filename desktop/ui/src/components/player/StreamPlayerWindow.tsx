import { AlertCircle, LoaderCircle } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import type Hls from 'hls.js';

import { useRendererApi } from '@/hooks/useRendererApi';
import { useStatus } from '@/hooks/useStatus';
import { getActiveSession } from '@/lib/stream';

type PlaybackState = 'idle' | 'loading' | 'ready' | 'blocked' | 'error';

export function StreamPlayerWindow() {
    const api = useRendererApi();
    const { status } = useStatus(api);
    const videoRef = useRef<HTMLVideoElement | null>(null);
    const hlsRef = useRef<Hls | null>(null);
    const fatalNetworkErrorsRef = useRef(0);
    const playbackStartedRef = useRef(false);
    const [isHoveringPlayer, setIsHoveringPlayer] = useState(false);
    const [playbackState, setPlaybackState] = useState<PlaybackState>('idle');
    const [errorMessage, setErrorMessage] = useState<string | null>(null);

    const activeSession = useMemo(() => getActiveSession(status), [status]);
    const streamUrl = activeSession?.startupComplete ? activeSession.outputUrl : null;

    async function attemptPlayback() {
        const video = videoRef.current;
        if (!video) {
            return;
        }

        try {
            video.muted = false;
            await video.play();
            playbackStartedRef.current = true;
            setPlaybackState('ready');
            setErrorMessage(null);
        } catch {
            try {
                video.muted = true;
                await video.play();
                playbackStartedRef.current = true;
                setPlaybackState('ready');
                setErrorMessage(null);
            } catch {
                setPlaybackState('blocked');
                setErrorMessage(null);
            }
        }
    }

    useEffect(() => {
        const video = videoRef.current;
        if (!video) {
            return;
        }

        let active = true;

        const teardown = () => {
            if (hlsRef.current) {
                hlsRef.current.destroy();
                hlsRef.current = null;
            }

            video.pause();
            video.removeAttribute('src');
            video.load();
        };

        if (!streamUrl) {
            fatalNetworkErrorsRef.current = 0;
            playbackStartedRef.current = false;
            setPlaybackState('idle');
            setErrorMessage(null);
            teardown();
            return;
        }

        setPlaybackState('loading');
        setErrorMessage(null);
        fatalNetworkErrorsRef.current = 0;
        playbackStartedRef.current = false;

        const handlePlayable = () => {
            if (playbackStartedRef.current) {
                return;
            }

            fatalNetworkErrorsRef.current = 0;
            void attemptPlayback();
        };

        const handlePlaying = () => {
            if (!active) {
                return;
            }

            setPlaybackState('ready');
            setErrorMessage(null);
            playbackStartedRef.current = true;
        };

        const attachStream = async () => {
            const HlsModule = await import('hls.js');
            if (!active) {
                return;
            }

            const HlsCtor = HlsModule.default;
            if (HlsCtor.isSupported()) {
                const hls = new HlsCtor({
                    // Keep HLS transmuxing off the renderer thread; tauri.conf.json allows blob workers.
                    enableWorker: true,
                    lowLatencyMode: false,
                    backBufferLength: 0,
                    liveSyncDurationCount: 3,
                    liveMaxLatencyDurationCount: 10,
                    maxLiveSyncPlaybackRate: 1.05,
                });

                hlsRef.current = hls;
                hls.loadSource(streamUrl);
                hls.attachMedia(video);

                hls.on(HlsCtor.Events.MANIFEST_PARSED, handlePlayable);
                hls.on(HlsCtor.Events.LEVEL_LOADED, handlePlayable);
                hls.on(HlsCtor.Events.ERROR, (_event, data) => {
                    if (!active || !data.fatal) {
                        return;
                    }

                    if (data.type === HlsCtor.ErrorTypes.NETWORK_ERROR) {
                        fatalNetworkErrorsRef.current += 1;
                        if (fatalNetworkErrorsRef.current <= 2) {
                            hls.startLoad();
                            return;
                        }

                        setPlaybackState('error');
                        setErrorMessage('The live relay stopped responding. Retry once the stream is back.');
                        return;
                    }

                    if (data.type === HlsCtor.ErrorTypes.MEDIA_ERROR) {
                        hls.recoverMediaError();
                        return;
                    }

                    hls.destroy();
                    if (hlsRef.current === hls) {
                        hlsRef.current = null;
                    }

                    setPlaybackState('error');
                    setErrorMessage('The built-in player could not decode this stream.');
                });
                return;
            }

            if (video.canPlayType('application/vnd.apple.mpegurl')) {
                video.src = streamUrl;
                video.load();
                return;
            }

            setPlaybackState('error');
            setErrorMessage('This runtime cannot play HLS streams in the built-in player.');
        };

        video.addEventListener('loadedmetadata', handlePlayable);
        video.addEventListener('canplay', handlePlayable);
        video.addEventListener('playing', handlePlaying);

        void attachStream().catch(() => {
            if (!active) {
                return;
            }

            setPlaybackState('error');
            setErrorMessage('The built-in player could not connect to the HLS output.');
        });

        return () => {
            active = false;
            video.removeEventListener('loadedmetadata', handlePlayable);
            video.removeEventListener('canplay', handlePlayable);
            video.removeEventListener('playing', handlePlaying);
            teardown();
        };
    }, [streamUrl]);

    return (
        <div
            className="fixed inset-0 overflow-hidden bg-black text-white"
            onPointerEnter={() => {
                setIsHoveringPlayer(true);
            }}
            onPointerLeave={() => {
                setIsHoveringPlayer(false);
            }}
        >
            <video
                ref={videoRef}
                controls={isHoveringPlayer}
                playsInline
                autoPlay
                preload="auto"
                className="absolute inset-0 h-full w-full bg-black object-contain"
            />

            {(playbackState !== 'ready' || !streamUrl) && (
                <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center bg-black/72 px-6 text-center">
                    <div className="max-w-md">
                        {playbackState === 'loading' && (
                            <LoaderCircle className="mx-auto mb-4 h-8 w-8 animate-spin text-white/80" />
                        )}
                        {(playbackState === 'blocked' || playbackState === 'error') && (
                            <AlertCircle className="mx-auto mb-4 h-8 w-8 text-white/80" />
                        )}

                        <p className="text-base font-medium text-white">
                            {!streamUrl && 'Waiting for a live stream'}
                            {streamUrl && playbackState === 'loading' && 'Connecting to the live output'}
                            {streamUrl && playbackState === 'blocked' && 'Use the player controls to start playback'}
                            {streamUrl && playbackState === 'error' && 'Playback interrupted'}
                        </p>

                        {streamUrl && (playbackState === 'loading' || playbackState === 'blocked' || playbackState === 'error') && (
                            <p className="mt-2 text-sm leading-6 text-white/60">
                                {playbackState === 'loading' && 'Loading the HLS manifest and preparing playback.'}
                                {playbackState === 'blocked' && 'Autoplay was blocked by the runtime, so playback needs to be started from the native controls.'}
                                {playbackState === 'error' && errorMessage}
                            </p>
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
