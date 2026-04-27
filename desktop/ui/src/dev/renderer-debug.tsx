import type { ReactNode } from 'react';
import { useMemo, useState } from 'react';

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';
import type { AppView, Session, SetupState, Status } from '@/lib/types';

export type DevUiSimulation = 'live' | 'setup' | 'standby' | 'active';

interface DevSimulationSnapshot {
    label: string;
    description: string;
    view: AppView;
    status: Status | null;
    setupState: SetupState | null;
}

interface DevStatePanelProps {
    simulation: DevUiSimulation;
    onChange: (simulation: DevUiSimulation) => void;
}

interface UseRendererDebugPanelOptions {
    currentView: AppView;
    status: Status;
    setupState: SetupState;
}

const DEV_ACTIVE_SESSION: Session = {
    id: null,
    sourceUrl: 'https://streams.dev/referee/source.m3u8',
    outputUrl: 'http://127.0.0.1:18990/mock-output.m3u8',
    appName: 'Developer Mock',
    streamTitle: 'Referee QA Feed',
    sourceContentKind: 'animated',
    upscaler: 'Anime4K2x',
    sourceResolution: '1920x1080',
    outputResolution: '3840x2160',
    sourceFps: 30,
    targetFps: 60,
    framegenEnabled: true,
    hdrEnabled: true,
    qualityLevel: 3,
    executor: 'nvidiaSpecialized',
    encoderBackend: 'nvenc',
    startupComplete: true,
    retryingStartup: false
};

export const DEV_SIMULATIONS: Record<Exclude<DevUiSimulation, 'live'>, DevSimulationSnapshot> = {
    setup: {
        label: 'Setup',
        description: 'Shows the first-run setup flow.',
        view: 'setup',
        status: null,
        setupState: {
            gpu: {
                vendor: 'nvidia',
                name: 'NVIDIA GeForce RTX 4080'
            },
            progress: {
                phase: 'install',
                percent: 62,
                detail: 'Installing encoder binaries'
            },
            error: null,
            complete: false
        }
    },
    standby: {
        label: 'Standby',
        description: 'Shows the idle status card with no routed stream.',
        view: 'status',
        status: {
            gpuReady: true,
            gpuName: 'NVIDIA GeForce RTX 4080',
            gpuVendor: 'nvidia',
            gpuUtilization: 6,
            encoderBackend: 'nvenc',
            selectedExecutor: 'nvidiaSpecialized',
            nvidiaAiAvailable: true,
            activeSessions: 0,
            sessions: [],
            primarySession: null,
            settings: {
                quality: 2,
                framegen: true,
                hdr: true
            },
            encoderHasFramegen: true,
            encoderHasTruehdr: true
        },
        setupState: null
    },
    active: {
        label: 'Active',
        description: 'Shows the live stream state with routed output badges.',
        view: 'status',
        status: {
            gpuReady: true,
            gpuName: 'NVIDIA GeForce RTX 4080',
            gpuVendor: 'nvidia',
            gpuUtilization: 68,
            encoderBackend: 'nvenc',
            selectedExecutor: 'nvidiaSpecialized',
            nvidiaAiAvailable: true,
            activeSessions: 1,
            sessions: [DEV_ACTIVE_SESSION],
            primarySession: DEV_ACTIVE_SESSION,
            settings: {
                resolution: '3840x2160',
                quality: 3,
                framegen: true,
                hdr: true
            },
            encoderHasFramegen: true,
            encoderHasTruehdr: true
        },
        setupState: null
    }
};

const PANEL_OPTIONS: Array<{ value: DevUiSimulation; label: string }> = [
    { value: 'live', label: 'Live' },
    { value: 'setup', label: 'Setup' },
    { value: 'standby', label: 'Standby' },
    { value: 'active', label: 'Active' }
];

/**
 * Compute effective view, status, and setup state for the renderer debug panel, optionally overriding live values with a selected simulation snapshot.
 *
 * @param currentView - The live UI view to use when no simulation overrides it
 * @param status - The live renderer `Status` to use when not overridden by a simulation
 * @param setupState - The live `SetupState` to use when not overridden by a simulation
 * @returns An object containing:
 * - `effectiveView`: The view to render (forced to `'settings'` when `currentView` is `'settings'`, otherwise the active simulation view or `currentView`).
 * - `effectiveStatus`: The `Status` to use (simulation `status` when present, otherwise the provided `status`).
 * - `effectiveSetupState`: The `SetupState` to use (simulation `setupState` when present, otherwise the provided `setupState`).
 * - `settingsPanel`: A React node for the debug panel UI that allows selecting the active simulation.
 * - `isSimulating`: `true` when a simulation other than `'live'` is active, `false` otherwise.
 */
export function useRendererDebugPanel({ currentView, status, setupState }: UseRendererDebugPanelOptions): {
    effectiveView: AppView;
    effectiveStatus: Status;
    effectiveSetupState: SetupState;
    settingsPanel: ReactNode;
    isSimulating: boolean;
} {
    const [simulation, setSimulation] = useState<DevUiSimulation>('live');

    const snapshot = useMemo(() => {
        return simulation === 'live' ? null : DEV_SIMULATIONS[simulation];
    }, [simulation]);

    return {
        effectiveView: currentView === 'settings' ? 'settings' : snapshot?.view ?? currentView,
        effectiveStatus: snapshot?.status ?? status,
        effectiveSetupState: snapshot?.setupState ?? setupState,
        settingsPanel: <DevStatePanel simulation={simulation} onChange={setSimulation} />,
        isSimulating: simulation !== 'live'
    };
}

/**
 * Renders a debug "View Simulator" panel with controls to choose a renderer simulation mode.
 *
 * @param simulation - The currently selected simulation mode (`'live' | 'setup' | 'standby' | 'active'`).
 * @param onChange - Callback invoked with the newly selected simulation mode when a button is clicked.
 * @returns A React element containing the simulator header, explanatory text, and buttons for each simulation option.
 */
export function DevStatePanel({ simulation, onChange }: DevStatePanelProps) {
    return (
        <section className={'rounded-lg border bg-referee-card p-3.5 transition-colors duration-300 border-referee-green/30 bg-referee-green/10'}>
            <div className="mb-3 flex items-center justify-between">
                <span className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">View Simulator</span>
                <span className="ml-2">
                    <span className="rounded border px-2 py-1 text-[10px] font-medium leading-none border-referee-green/30 bg-referee-green/10 text-referee-green">DEBUG ONLY</span>
                </span>
            </div>

            <div className="flex flex-col">
                <span className="text-[13px] font-medium text-white">View Selector</span>
                <span className="text-[11px] text-referee-muted">Choose the view to simulate. Live will follow real renderer events from the Tauri backend.</span>
            </div>

            <div className="grid grid-cols-2 mt-4 gap-2">
                {PANEL_OPTIONS.map(option => {
                    const isActive = simulation === option.value;

                    return (
                        <Button
                            key={option.value}
                            type="button"
                            size="sm"
                            variant={isActive ? 'secondary' : 'ghost'}
                            className={cn(
                                'h-9 rounded-xl border border-white/10 px-2 text-[10px] tracking-[0.18em]',
                                isActive && 'border-[#ff6b35]/40 bg-[#ff6b35]/[0.18] text-white hover:bg-[#ff6b35]/[0.24]'
                            )}
                            onClick={() => {
                                onChange(option.value);
                            }}
                        >
                            {option.label}
                        </Button>
                    );
                })}
            </div>
        </section>
    );
}
