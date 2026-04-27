import type { ReactNode } from 'react';

import type { AppView, SetupState, Status } from '@/lib/types';

interface UseRendererDebugPanelOptions {
    currentView: AppView;
    status: Status;
    setupState: SetupState;
}

export function useRendererDebugPanel({ currentView, status, setupState }: UseRendererDebugPanelOptions): {
    effectiveView: AppView;
    effectiveStatus: Status;
    effectiveSetupState: SetupState;
    settingsPanel: ReactNode;
    isSimulating: boolean;
} {
    return {
        effectiveView: currentView,
        effectiveStatus: status,
        effectiveSetupState: setupState,
        settingsPanel: null,
        isSimulating: false
    };
}
