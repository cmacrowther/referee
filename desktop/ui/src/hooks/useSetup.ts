import { useCallback, useEffect, useRef, useState } from 'react';

import {
    DEFAULT_SETUP_STATE,
    type AppView,
    type RendererApi,
    type SetupProgress,
    type SetupReadyResponse,
    type SetupState
} from '@/lib/types';

const MIN_PHASE_MS = 750;

function getFallbackSetupProgress(gpuVendor?: string | null): SetupProgress {
    const detail = gpuVendor === 'nvidia'
        ? 'Preparing NVEncC download...'
        : gpuVendor === 'amd'
            ? 'Preparing VCEEncC download...'
            : 'Preparing required binaries...';

    return {
        phase: 'encoder',
        percent: 0,
        detail
    };
}

function mergeSetupProgress(current: SetupProgress | null, next: SetupProgress | null): SetupProgress | null {
    if (!next) {
        return current;
    }

    if (!current || next.phase === 'done') {
        return next;
    }

    if (current.phase === 'done') {
        return current;
    }

    if (current.phase === next.phase && next.percent < current.percent) {
        return current;
    }

    return next;
}

function isUnsupportedLocalHardware(vendor?: string | null) {
    const normalizedVendor = vendor?.trim().toLowerCase();
    return Boolean(normalizedVendor && normalizedVendor !== 'nvidia' && normalizedVendor !== 'amd');
}

/**
 * Manages and exposes UI-ready setup state for the application, including throttled phase transitions, event subscriptions, polling fallback, and retry/response handlers.
 *
 * Subscribes to renderer setup events, optionally polls the renderer when events may be missed, enforces a minimum display duration for each distinct setup phase, and exposes actions to retry setup and to handle a manual setup-ready response.
 *
 * @param api - Renderer API used to receive setup events, query setup state, and initiate retries
 * @param setView - Callback to switch the application view (e.g., 'setup' or 'status')
 * @returns An object with:
 *  - `setupState`: the throttled state suitable for rendering the setup UI,
 *  - `retrySetup`: a function that triggers a setup retry,
 *  - `handleSetupResponse`: a function to apply a `SetupReadyResponse` (from polling or manual invocation) to the hook's state.
 */
export function useSetup(api: RendererApi, setView: (view: AppView) => void) {
    const [setupState, setSetupState] = useState<SetupState>(DEFAULT_SETUP_STATE);
    const shouldPollSetup = !setupState.complete && (setupState.gpu !== null || setupState.progress !== null || setupState.error !== null);

    // Throttled display state — each distinct phase is shown for at least MIN_PHASE_MS
    const [displayedSetupState, setDisplayedSetupState] = useState<SetupState>(DEFAULT_SETUP_STATE);
    const displayedRef = useRef<SetupState>(DEFAULT_SETUP_STATE);
    const phaseQueue = useRef<SetupState[]>([]);
    const phaseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const phaseShownAt = useRef<number>(0);

    const advancePhaseQueue = useCallback(() => {
        phaseTimer.current = null;
        if (phaseQueue.current.length === 0) return;
        const next = phaseQueue.current.shift()!;
        displayedRef.current = next;
        setDisplayedSetupState(next);
        phaseShownAt.current = Date.now();
        if (phaseQueue.current.length > 0) {
            phaseTimer.current = setTimeout(advancePhaseQueue, MIN_PHASE_MS);
        }
    }, []);

    useEffect(() => {
        const newPhase = setupState.progress?.phase ?? null;
        const currentPhase = displayedRef.current.progress?.phase ?? null;

        if (newPhase === currentPhase) {
            // Same phase — update displayed state immediately (progress %, detail text, etc.)
            displayedRef.current = setupState;
            setDisplayedSetupState(setupState);
            return;
        }

        // Phase changed — queue the transition
        const existingIdx = phaseQueue.current.findIndex(s => (s.progress?.phase ?? null) === newPhase);
        if (existingIdx >= 0) {
            phaseQueue.current[existingIdx] = setupState;
        } else {
            phaseQueue.current.push(setupState);
        }

        if (phaseTimer.current === null) {
            const elapsed = Date.now() - phaseShownAt.current;
            phaseTimer.current = setTimeout(advancePhaseQueue, Math.max(0, MIN_PHASE_MS - elapsed));
        }
    }, [setupState, advancePhaseQueue]);

    useEffect(() => {
        if (displayedSetupState.complete) {
            setView('status');
        }
    }, [displayedSetupState.complete, setView]);

    useEffect(() => {
        return () => {
            if (phaseTimer.current !== null) {
                clearTimeout(phaseTimer.current);
            }
        };
    }, []);

    useEffect(() => {
        const unsubscribers = [
            api.onSetupGpuDetected(gpu => {
                setView('setup');
                setSetupState(previous => ({
                    ...previous,
                    gpu,
                    progress: previous.progress ?? getFallbackSetupProgress(gpu.vendor)
                }));
            }),
            api.onSetupProgress(progress => {
                setView('setup');
                setSetupState(previous => ({
                    ...previous,
                    progress,
                    error: null,
                    complete: progress.phase === 'done'
                }));
            }),
            api.onSetupComplete(() => {
                setSetupState(previous => ({
                    ...previous,
                    complete: true,
                    error: null,
                    progress: {
                        phase: 'done',
                        percent: 100,
                        detail: 'Setup complete'
                    }
                }));
            }),
            api.onSetupError(error => {
                setView('setup');
                setSetupState(previous => ({
                    ...previous,
                    error: error.message || 'Setup failed',
                    complete: false
                }));
            })
        ];

        return () => {
            unsubscribers.forEach(unsubscribe => unsubscribe());
        };
    }, [api, setView]);

    const handleSetupResponse = useCallback((response: SetupReadyResponse | null | undefined) => {
        if (!response) {
            return;
        }

        const gpu = {
            vendor: response.gpuVendor,
            name: response.gpuName ?? ''
        };

        if (!response.setupNeeded) {
            const inProgress = response.setupInProgress === true;
            setSetupState(previous => ({
                ...previous,
                gpu,
                error: response.setupError ?? null,
                complete: response.setupComplete ?? !inProgress,
                progress: response.setupProgress ?? {
                    phase: 'done',
                    percent: 100,
                    detail: 'Setup complete'
                }
            }));
            if (!inProgress) {
                setView('status');
            } else {
                setView('setup');
            }
            return;
        }

        if (isUnsupportedLocalHardware(response.gpuVendor) && !response.setupInProgress) {
            setSetupState(previous => ({
                ...previous,
                gpu,
                error: null,
                complete: true,
                progress: response.setupProgress ?? {
                    phase: 'done',
                    percent: 100,
                    detail: 'Relay mode ready'
                }
            }));
            setView('status');
            return;
        }

        setView('setup');
        setSetupState(previous => ({
            ...previous,
            gpu,
            error: response.setupError ?? null,
            complete: Boolean(response.setupComplete),
            progress: mergeSetupProgress(
                previous.progress,
                response.setupProgress ?? previous.progress ?? getFallbackSetupProgress(response.gpuVendor)
            )
        }));
    }, [setView]);

    useEffect(() => {
        if (!shouldPollSetup) {
            return;
        }

        let disposed = false;

        const syncSetup = async () => {
            try {
                const nextSetupState = await api.getSetupState();
                if (disposed) {
                    return;
                }

                handleSetupResponse(nextSetupState);
            } catch {
                // Setup state polling is only a fallback when event delivery misses updates.
            }
        };

        syncSetup();
        const intervalId = window.setInterval(syncSetup, 1000);

        return () => {
            disposed = true;
            window.clearInterval(intervalId);
        };
    }, [api, handleSetupResponse, shouldPollSetup]);

    async function retrySetup() {
        setView('setup');
        setSetupState(previous => ({
            ...previous,
            error: null,
            complete: false,
            progress: {
                phase: 'retry',
                percent: 0,
                detail: 'Retrying setup...'
            }
        }));

        try {
            await api.retrySetup();
        } catch (error) {
            setSetupState(previous => ({
                ...previous,
                error: error instanceof Error ? error.message : 'Setup failed',
                complete: false
            }));
        }
    }

    return {
        setupState: displayedSetupState,
        retrySetup,
        handleSetupResponse
    };
}
