import { ArrowUpCircle, ScrollText } from 'lucide-react';
import { type ReactNode, useEffect, useState } from 'react';

import { LinkRow } from '@/components/settings/AboutCard';
import { SettingRow, SettingsSection, ToggleRow } from '@/components/settings/SettingsPrimitives';
import { Button } from '@/components/ui/button';
import type { RendererApi, Settings, UpdateStatus } from '@/lib/types';

/** A check older than this runs again when the card opens. */
const STALE_CHECK_MS = 60 * 60 * 1000;

const SMALL_BUTTON_CLASS = 'h-7 shrink-0 gap-1 rounded-md px-2.5 text-[10px]';

interface UpdatesCardProps {
    api: RendererApi;
    settings: Settings;
    /** Streams are running, and installing would stop them. */
    locked: boolean;
    onChange: (patch: Partial<Settings>) => void;
}

function errorMessage(error: unknown) {
    return error instanceof Error ? error.message : String(error || 'The update could not start.');
}

export function checkedAgo(lastChecked: number, now = Date.now()) {
    const minutes = Math.floor((now - lastChecked) / 60_000);
    if (minutes < 1) {
        return 'checked just now';
    }
    if (minutes < 60) {
        return `checked ${minutes} min ago`;
    }
    const hours = Math.floor(minutes / 60);
    if (hours < 24) {
        return `checked ${hours} h ago`;
    }
    return `checked ${new Date(lastChecked).toLocaleDateString()}`;
}

function summary(status: UpdateStatus, autoUpdate: boolean) {
    const current = `v${status.currentVersion}`;
    const latest = `v${status.latestVersion}`;
    switch (status.phase) {
        case 'checking':
            return { label: 'Checking for updates', description: current };
        case 'downloading':
            return { label: `Downloading ${latest}`, description: `${status.progress ?? 0}%` };
        case 'ready':
            return {
                label: `${latest} ready`,
                // Automatic updates skip a version whose install failed until the next launch.
                description: autoUpdate && !status.error ? 'Installs when no stream is running' : 'Downloaded and verified',
            };
        case 'installing':
            return { label: `Installing ${latest}`, description: 'REFEREE restarts when it finishes' };
        case 'idle':
            if (status.latestVersion) {
                return { label: `${latest} available`, description: `You have ${current}` };
            }
            return status.lastChecked
                ? { label: 'Up to date', description: `${current} · ${checkedAgo(status.lastChecked)}` }
                : { label: 'Updates', description: `${current} · not checked yet` };
    }
}

/** Follows REFEREE's own updates, checking GitHub when the last check is stale. */
function useUpdateStatus(api: RendererApi) {
    const [status, setStatus] = useState<UpdateStatus | null>(null);

    useEffect(() => {
        let cancelled = false;
        const show = (next: UpdateStatus) => {
            if (!cancelled) {
                setStatus(next);
            }
        };
        const unsubscribe = api.onUpdateStatus(show);
        api.getUpdateStatus()
            .then(current => {
                show(current);
                const stale = current.lastChecked === null || Date.now() - current.lastChecked > STALE_CHECK_MS;
                if (current.phase === 'idle' && stale) {
                    return api.checkForUpdate().then(show);
                }
            })
            .catch(() => {});
        return () => {
            cancelled = true;
            unsubscribe();
        };
    }, [api]);

    return [status, setStatus] as const;
}

export function UpdatesCard({ api, settings, locked, onChange }: UpdatesCardProps) {
    const [status, setStatus] = useUpdateStatus(api);
    const [actionError, setActionError] = useState<string | null>(null);

    function check() {
        setActionError(null);
        api.checkForUpdate()
            .then(setStatus)
            .catch(error => setActionError(errorMessage(error)));
    }

    function install() {
        setActionError(null);
        // On success REFEREE exits for the installer; progress arrives as status events.
        api.installUpdate().catch(error => setActionError(errorMessage(error)));
    }

    const autoUpdateToggle = (canInstall: boolean) => (
        <ToggleRow
            label="Automatic Updates"
            description="Install new versions when idle"
            checked={settings.autoUpdate}
            disabled={!canInstall}
            onChange={checked => {
                onChange({ autoUpdate: checked });
            }}
        />
    );

    if (!status) {
        return (
            <SettingsSection title="Updates">
                <SettingRow label="Updates" description="Loading…" />
                {autoUpdateToggle(true)}
            </SettingsSection>
        );
    }

    const canInstall = status.unsupportedReason === null;
    const busy = status.phase === 'checking' || status.phase === 'downloading' || status.phase === 'installing';
    const { label, description } = summary(status, settings.autoUpdate);
    const error = actionError ?? status.error;
    const releaseUrl = status.latestVersion ? status.releaseUrl : null;

    let control: ReactNode;
    if (status.latestVersion && canInstall) {
        control = (
            <Button
                type="button"
                variant="primary"
                size="sm"
                className={SMALL_BUTTON_CLASS}
                disabled={busy || locked}
                onClick={install}
            >
                <ArrowUpCircle className="size-3" />
                {status.phase === 'ready' ? 'Install' : 'Update'}
            </Button>
        );
    } else if (releaseUrl) {
        control = (
            <Button
                type="button"
                variant="secondary"
                size="sm"
                className={SMALL_BUTTON_CLASS}
                onClick={() => {
                    api.openExternal(releaseUrl);
                }}
            >
                Download
            </Button>
        );
    } else {
        control = (
            <Button
                type="button"
                variant="secondary"
                size="sm"
                className={SMALL_BUTTON_CLASS}
                disabled={busy}
                onClick={check}
            >
                {status.phase === 'checking' ? 'Checking…' : 'Check'}
            </Button>
        );
    }

    let footer: ReactNode = null;
    if (error) {
        footer = <span role="alert" className="text-red-300">{error}</span>;
    } else if (status.latestVersion && canInstall && locked) {
        footer = 'Stop active streams to update.';
    } else if (!canInstall) {
        footer = status.unsupportedReason;
    }

    return (
        <SettingsSection title="Updates" footer={footer}>
            <SettingRow label={label} description={description} control={control} />
            {status.phase === 'downloading' && (
                <div className="px-3 py-2">
                    <div
                        className="setup-progress-track"
                        role="progressbar"
                        aria-label="Update download"
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={status.progress ?? 0}
                    >
                        <div className="setup-progress-fill" style={{ width: `${status.progress ?? 0}%` }} />
                    </div>
                </div>
            )}
            {releaseUrl && (
                <LinkRow
                    icon={<ScrollText className="size-4" />}
                    title={`What's new in v${status.latestVersion}`}
                    url={releaseUrl}
                    onClick={() => {
                        api.openExternal(releaseUrl);
                    }}
                />
            )}
            {autoUpdateToggle(canInstall)}
        </SettingsSection>
    );
}
