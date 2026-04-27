import { TriangleAlert } from 'lucide-react';

import { SetupError } from '@/components/setup/SetupError';
import { SetupProgress } from '@/components/setup/SetupProgress';
import type { SetupState } from '@/lib/types';

interface SetupViewProps {
    setupState: SetupState;
    onRetry: () => void;
}

interface SetupCopyState {
    title: string;
    body: string;
}

/**
 * Selects UI title and body copy appropriate to the current setup state.
 *
 * The returned copy reflects hardware detection, progress phase, errors, and completion so the UI can display a concise message to the user.
 *
 * @param setupState - Current setup state containing progress, GPU info, completion, and error indicators
 * @returns An object with `title` and `body` strings to display in the setup UI
 */
function getSetupCopyState(setupState: SetupState): SetupCopyState {
    const phase = setupState.progress?.phase;

    if (setupState.gpu?.vendor === 'unknown') {
        return {
            title: 'Unknown Hardware',
            body: 'No compatible GPU was detected. REFEREE requires dedicated NVIDIA® or AMD® hardware.'
        };
    }

    if (setupState.complete) {
        return {
            title: 'Setup Complete',
            body: 'REFEREE is ready for incoming streams.'
        };
    }

    if (setupState.error) {
        return {
            title: 'Retry Setup',
            body: 'An error occurred during setup. Please click the button below to try again.'
        };
    }

    if (phase === 'detect') {
        return {
            title: 'Checking Hardware',
            body: 'Detecting compatible NVIDIA® or AMD® GPU and hardware encoder support.'
        };
    }

    if (phase === 'encoder') {
        return {
            title: 'Getting Ready',
            body: 'Downloading the components needed to power up your stream for the first time.'
        };
    }

    if (phase === 'retry') {
        return {
            title: 'Retrying Setup',
            body: 'Attempting to resolve the issue and continue setup.'
        };
    }

    return {
        title: 'Getting Ready',
        body: 'Preparing to launch for the first time. This will only take a moment.'
    };
}

function Wordmark() {
    return (
        <span className="setup-message-wordmark">
            <span>R</span>
            <span>E</span>
            <span className="setup-message-wordmark-accent">
                F
            </span>
            <span>E</span>
            <span>R</span>
            <span>E</span>
            <span>E</span>
        </span>
    );
}

export function SetupView({ setupState, onRetry }: SetupViewProps) {
    const copyState = getSetupCopyState(setupState);

    return (
        <section id="setup-view" className="view-pane custom-scrollbar overflow-y-auto">
            <div className="flex min-h-full flex-col gap-3">
                <div id="setup-panel-card" className="stream-message-card">
                    <div className="stream-message-shell" aria-live="polite">
                        <div className="stream-message-copy">
                            <div className="stream-message-copy-content setup-message-content">
                                <div className="setup-message-header">
                                    <Wordmark />
                                    <div className="stream-message-copy-stack">
                                        <div className="stream-message-copy-panel">
                                            <span className="stream-message-kicker mb-8">Initial Setup</span>
                                            <h2 className="stream-message-title mt-2">{copyState.title}</h2>
                                            <div className="stream-message-body setup-message-body">{copyState.body}</div>
                                        </div>
                                    </div>
                                </div>

                                <div className="setup-message-panel">
                                    <SetupProgress setupState={setupState} />
                                    <SetupError error={setupState.error} onRetry={onRetry} />
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </section>
    );
}
