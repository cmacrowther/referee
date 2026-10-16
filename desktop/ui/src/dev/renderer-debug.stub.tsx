import type { ReactNode } from 'react';

import type { AppView, RendererApi, SetupState, Status } from '@/lib/types';

interface UseRendererDebugPanelOptions {
    api: RendererApi;
    currentView: AppView;
    status: Status;
    setupState: SetupState;
}

export function useRendererDebugPanel({ currentView, status, setupState }: UseRendererDebugPanelOptions): {
    effectiveView: AppView;
    effectiveStatus: Status;
    effectiveSetupState: SetupState;
    settingsPanel: ReactNode;
    statusPanel: ReactNode;
    isSimulating: boolean;
} {
    return {
        effectiveView: currentView,
        effectiveStatus: status,
        effectiveSetupState: setupState,
        settingsPanel: null,
        statusPanel: null,
        isSimulating: false
    };
}
