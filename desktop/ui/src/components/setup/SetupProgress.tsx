import type { SetupState } from '@/lib/types';

// Maps technical backend detail strings to user-friendly equivalents.
/**
 * Map a raw backend progress detail string to a concise, user-facing message.
 *
 * This normalizes the input and converts known technical prefixes (e.g., NVENC/NVENC-c, VCE/AMF, ffmpeg, pip, setup complete)
 * into branded, UI-friendly text; if no known mapping exists, the original `detail` is returned unchanged.
 *
 * @param detail - Raw progress detail text from the backend
 * @returns A user-facing progress message for recognized detail prefixes, or the original `detail` if unmapped
 */
function friendlyDetail(detail: string): string {
    const d = detail.trim().toLowerCase();

    if (d.startsWith('preparing nvencc') || d.startsWith('preparing nvenc')) {
        return 'Preparing NVIDIA® acceleration libraries...';
    }
    if (d.startsWith('preparing vceencc') || d.startsWith('preparing vceenc')) {
        return 'Preparing AMD® acceleration libraries...';
    }
    if (d.startsWith('preparing required binaries') || d.startsWith('preparing required')) {
        return 'Preparing pipeline components...';
    }
    if (d.startsWith('downloading nvencc') || d.startsWith('downloading nvenc')) {
        return 'Downloading NVIDIA® acceleration libraries...';
    }
    if (d.startsWith('downloading vceencc') || d.startsWith('downloading vceenc')) {
        return 'Downloading AMD® acceleration libraries...';
    }
    if (d.startsWith('downloading ffmpeg')) {
        return 'Downloading REFEREE pipeline libraries...';
    }
    if (d.startsWith('downloading rife')) {
        return 'Downloading frame interpolation engine...';
    }
    if (d.startsWith('downloading pip') || d.startsWith('downloading pip bootstrap')) {
        return 'Downloading AI runtime components...';
    }
    if (d.startsWith('extracting embedded python')) {
        return 'Preparing AI runtime...';
    }
    if (d.startsWith('bootstrapping pip') || d.startsWith('configuring pip')) {
        return 'Configuring AI runtime...';
    }
    if (d.startsWith('setup complete')) {
        return 'Setup complete';
    }

    return detail;
}

interface SetupProgressProps {
    setupState: SetupState;
}

/**
 * Selects the primary user-facing label for the current setup progress.
 *
 * @param setupState - The current setup state; its `progress.phase`, `complete`, `error`, and `gpu?.vendor` fields determine the chosen label.
 * @returns One of the user-facing progress labels: `"Ready"` when setup is complete, `"Setup Paused"` when an error occurred, or a phase-specific label such as `"Checking GPU"`, `"Installing NVIDIA® Libraries"`, `"Installing AMD® Libraries"`, `"Installing Video Libraries"`, `"Retrying"`, `"Setting Up Pipeline"`, or `"Preparing"`.
 */
function getProgressLabel(setupState: SetupState) {
    const phase = setupState.progress?.phase;

    if (setupState.complete || phase === 'done') {
        return 'Ready';
    }

    if (setupState.error) {
        return 'Setup Paused';
    }

    if (phase === 'detect') {
        return 'Checking GPU';
    }

    if (phase === 'encoder') {
        if (setupState.gpu?.vendor === 'nvidia') {
            return 'Installing NVIDIA® Libraries';
        }

        if (setupState.gpu?.vendor === 'amd') {
            return 'Installing AMD® Libraries';
        }

        return 'Installing Video Libraries';
    }

    if (phase === 'retry') {
        return 'Retrying';
    }

    if (phase === 'install') {
        return 'Setting Up Pipeline';
    }

    return 'Preparing';
}

/**
 * Compute a user-facing secondary progress message for the provided setup state.
 *
 * Chooses a live backend detail when present (normalized for display) and otherwise falls back
 * to phase-specific or terminal messages such as "Setup complete" or "Retry to continue".
 *
 * @param setupState - The current setup state from which to derive the detail message
 * @returns A human-friendly string describing the current setup progress detail
 */
function getProgressDetail(setupState: SetupState) {
    const phase = setupState.progress?.phase;
    const liveDetail = setupState.progress?.detail?.trim();

    if (setupState.complete || phase === 'done') {
        return 'Setup complete';
    }

    if (setupState.error) {
        return 'Retry to continue';
    }

    if (liveDetail) {
        return friendlyDetail(liveDetail);
    }

    if (phase === 'detect') {
        return 'Hardware scan';
    }

    if (phase === 'encoder') {
        return 'Preparing acceleration libraries';
    }

    if (phase === 'retry') {
        return 'Starting again';
    }

    if (phase === 'install') {
        return 'Preparing pipeline components';
    }

    return 'Getting ready';
}

export function SetupProgress({ setupState }: SetupProgressProps) {
    if (setupState.gpu?.vendor === 'unknown') {
        return null;
    }

    const rawProgress = setupState.progress?.percent ?? 0;
    const progressValue = setupState.progress?.phase === 'done'
        ? 100
        : Math.max(0, Math.min(100, rawProgress));
    const progressLabel = getProgressLabel(setupState);
    const progressDetail = getProgressDetail(setupState);
    const progressState = setupState.complete || setupState.progress?.phase === 'done'
        ? 'complete'
        : setupState.error
            ? 'error'
            : 'active';
    const isLateStage = progressState === 'active' && progressValue >= 88;

    return (
        <div className="setup-progress" data-state={progressState} data-late-stage={isLateStage ? 'true' : 'false'}>
            <div className="setup-progress-header">
                <span className="setup-progress-label">{progressLabel}</span>
                <span className="setup-progress-value">{progressValue}%</span>
            </div>
            <div className="setup-progress-track">
                <div
                    className="setup-progress-fill"
                    style={{ width: `${progressValue}%` }}
                />
            </div>
            <div className="setup-progress-detail">{progressDetail}</div>
        </div>
    );
}
