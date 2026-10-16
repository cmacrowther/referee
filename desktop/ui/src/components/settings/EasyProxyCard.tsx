import { Lock } from 'lucide-react';
import { type KeyboardEvent, useEffect, useId, useState } from 'react';

import { RadioDot, SettingsSection, ToggleRow } from '@/components/settings/SettingsPrimitives';
import { Button } from '@/components/ui/button';
import {
    DEFAULT_EASY_PROXY_URL,
    type EasyProxyMode,
    type EasyProxySettings,
    type ManagedEasyProxyStatus,
    type RendererApi,
    type Settings,
} from '@/lib/types';
import { cn } from '@/lib/utils';

const EASY_PROXY_REPO_URL = 'https://github.com/realbestia1/EasyProxy';
const STATUS_POLL_MS = 4000;
const INSTALL_POLL_MS = 1000;

const INPUT_CLASS =
    'w-full rounded-md border border-white/[0.08] bg-white/[0.04] px-2.5 py-1.5 text-[11px] text-white placeholder-white/25 outline-none transition-colors focus:border-referee-accent/40 focus:bg-referee-accent/[0.04]';

const FIELD_LABEL_CLASS = 'text-[10px] font-medium uppercase tracking-wider text-referee-muted';

const SMALL_BUTTON_CLASS = 'h-7 shrink-0 rounded-md px-2.5 text-[10px]';

const MODES: { id: EasyProxyMode; label: string; description: string }[] = [
    { id: 'managed', label: 'Run by REFEREE', description: 'Installs EasyProxy and starts it when a stream needs it' },
    { id: 'external', label: 'External server', description: 'An EasyProxy you run yourself' },
];

type CheckState =
    | { kind: 'idle' }
    | { kind: 'checking' }
    | { kind: 'ok'; version: string | null }
    | { kind: 'error'; message: string };

interface EasyProxyCardProps {
    api: RendererApi;
    settings: Settings;
    locked: boolean;
    /** Set while streams relay to a peer, which applies its own EasyProxy setting. */
    relayPeerName?: string | null;
    onChange: (patch: Partial<Settings>) => void;
}

function blankToNull(value: string) {
    const trimmed = value.trim();
    return trimmed ? trimmed : null;
}

function commitOnEnter(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'Enter') {
        event.currentTarget.blur();
    }
}

function errorMessage(error: unknown, fallback: string) {
    return error instanceof Error ? error.message : String(error || fallback);
}

function CheckResult({ check }: { check: CheckState }) {
    return (
        <p
            role="status"
            className={cn(
                'min-w-0 text-[11px] leading-[15px]',
                check.kind === 'ok' && 'text-referee-green',
                check.kind === 'error' && 'text-red-300',
                (check.kind === 'idle' || check.kind === 'checking') && 'text-referee-muted'
            )}
        >
            {check.kind === 'ok' && (check.version ? `Connected to EasyProxy ${check.version}` : 'Connected to EasyProxy')}
            {check.kind === 'error' && check.message}
        </p>
    );
}

/** Polls the managed server's status, faster while it installs or starts. */
function useManagedStatus(api: RendererApi) {
    const [status, setStatus] = useState<ManagedEasyProxyStatus | null>(null);
    const [refreshToken, setRefreshToken] = useState(0);

    useEffect(() => {
        let cancelled = false;
        let timer: number | undefined;
        const poll = () => {
            api.getEasyProxyStatus()
                .then(next => {
                    if (cancelled) {
                        return;
                    }
                    setStatus(next);
                    timer = window.setTimeout(poll, next.installing || next.starting ? INSTALL_POLL_MS : STATUS_POLL_MS);
                })
                .catch(() => {
                    if (!cancelled) {
                        timer = window.setTimeout(poll, STATUS_POLL_MS);
                    }
                });
        };
        poll();
        return () => {
            cancelled = true;
            window.clearTimeout(timer);
        };
    }, [api, refreshToken]);

    return { status, refresh: () => setRefreshToken(token => token + 1) };
}

function ManagedServer({ api, locked }: { api: RendererApi; locked: boolean }) {
    const { status, refresh } = useManagedStatus(api);
    const [check, setCheck] = useState<CheckState>({ kind: 'idle' });
    const [actionError, setActionError] = useState<string | null>(null);

    if (!status) {
        return <p className="text-[11px] text-referee-muted">Checking EasyProxy...</p>;
    }

    function install() {
        setActionError(null);
        api.installEasyProxy()
            .catch(error => setActionError(errorMessage(error, 'Unable to install EasyProxy.')))
            .finally(refresh);
    }

    function remove() {
        setActionError(null);
        setCheck({ kind: 'idle' });
        api.removeEasyProxy()
            .catch(error => setActionError(errorMessage(error, 'Unable to remove EasyProxy.')))
            .finally(refresh);
    }

    function runCheck() {
        setCheck({ kind: 'checking' });
        api.checkEasyProxy('managed', null, null)
            .then(result => setCheck({ kind: 'ok', version: result.version }))
            .catch(error => setCheck({ kind: 'error', message: errorMessage(error, 'EasyProxy check failed.') }))
            .finally(refresh);
    }

    if (status.installState === 'unsupported') {
        return (
            <p className="text-[11px] leading-[15px] text-referee-muted">
                REFEREE can run EasyProxy on Windows and Linux x86_64 only. Use an external server instead.
            </p>
        );
    }

    if (status.installing) {
        const percent = status.progress?.percent ?? 0;
        return (
            <div className="setup-progress flex flex-col gap-1" data-state="active">
                <div className="flex items-center justify-between text-[11px]">
                    <span className="text-white">{status.installState === 'outdated' ? 'Updating EasyProxy' : 'Installing EasyProxy'}</span>
                    <span className="text-referee-muted">{percent}%</span>
                </div>
                <div
                    className="setup-progress-track"
                    role="progressbar"
                    aria-label="EasyProxy install"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={percent}
                >
                    <div className="setup-progress-fill" style={{ width: `${percent}%` }} />
                </div>
                <p className="text-[10px] text-referee-muted">{status.progress?.detail ?? 'Starting...'}</p>
            </div>
        );
    }

    const installed = status.installState === 'current';
    const stateLabel = status.starting ? 'Starting' : status.running ? 'Running' : 'Stopped until a stream needs it';
    const error = actionError ?? status.error;

    return (
        <div className="flex flex-col gap-2">
            {installed ? (
                <p className="text-[11px] leading-[15px] text-referee-muted">
                    <span className="text-white">EasyProxy {status.version}</span>
                    {' '}({status.commit}) · {stateLabel}
                </p>
            ) : (
                <p className="text-[11px] leading-[15px] text-referee-muted">
                    {status.installState === 'outdated'
                        ? `This REFEREE version uses EasyProxy ${status.version}.`
                        : 'Downloads Python and EasyProxy, about 230 MB installed.'}
                </p>
            )}

            {error && (
                <p role="alert" className="text-[11px] leading-[15px] text-red-300">
                    {error}
                </p>
            )}

            <div className="flex items-center gap-2">
                {installed ? (
                    <>
                        <Button
                            type="button"
                            variant="secondary"
                            size="sm"
                            className={SMALL_BUTTON_CLASS}
                            disabled={check.kind === 'checking' || locked}
                            onClick={runCheck}
                        >
                            {check.kind === 'checking' ? 'Testing…' : 'Test'}
                        </Button>
                        <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className={SMALL_BUTTON_CLASS}
                            disabled={locked}
                            onClick={remove}
                        >
                            Remove
                        </Button>
                        <CheckResult check={check} />
                    </>
                ) : (
                    <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        className={SMALL_BUTTON_CLASS}
                        disabled={locked}
                        onClick={install}
                    >
                        {status.error ? 'Retry' : status.installState === 'outdated' ? 'Update' : 'Install'}
                    </Button>
                )}
            </div>
        </div>
    );
}

function ExternalServer({
    api,
    easyProxy,
    onSave,
}: {
    api: RendererApi;
    easyProxy: EasyProxySettings;
    onSave: (patch: Partial<EasyProxySettings>) => void;
}) {
    const urlId = useId();
    const passwordId = useId();
    // Text fields save on blur or Enter, not per keystroke, so a settings sync
    // echoing an earlier save cannot overwrite what is being typed.
    const [urlDraft, setUrlDraft] = useState(easyProxy.url ?? '');
    const [passwordDraft, setPasswordDraft] = useState(easyProxy.apiPassword ?? '');
    const [check, setCheck] = useState<CheckState>({ kind: 'idle' });

    useEffect(() => {
        setUrlDraft(easyProxy.url ?? '');
    }, [easyProxy.url]);

    useEffect(() => {
        setPasswordDraft(easyProxy.apiPassword ?? '');
    }, [easyProxy.apiPassword]);

    function commitUrl() {
        const url = blankToNull(urlDraft);
        if (url !== easyProxy.url) {
            onSave({ url });
        }
    }

    function commitPassword() {
        const apiPassword = blankToNull(passwordDraft);
        if (apiPassword !== easyProxy.apiPassword) {
            onSave({ apiPassword });
        }
    }

    function runCheck() {
        setCheck({ kind: 'checking' });
        api.checkEasyProxy('external', blankToNull(urlDraft), blankToNull(passwordDraft))
            .then(status => setCheck({ kind: 'ok', version: status.version }))
            .catch(error => setCheck({ kind: 'error', message: errorMessage(error, 'EasyProxy check failed.') }));
    }

    return (
        <div className="flex flex-col gap-2">
            <div className="flex flex-col gap-1">
                <label htmlFor={urlId} className={FIELD_LABEL_CLASS}>Server URL</label>
                <input
                    id={urlId}
                    type="url"
                    spellCheck={false}
                    autoComplete="off"
                    value={urlDraft}
                    placeholder={DEFAULT_EASY_PROXY_URL}
                    onChange={event => {
                        setUrlDraft(event.currentTarget.value);
                        setCheck({ kind: 'idle' });
                    }}
                    onBlur={commitUrl}
                    onKeyDown={commitOnEnter}
                    className={INPUT_CLASS}
                />
            </div>

            <div className="flex flex-col gap-1">
                <label htmlFor={passwordId} className={FIELD_LABEL_CLASS}>API Password</label>
                <input
                    id={passwordId}
                    type="password"
                    autoComplete="off"
                    value={passwordDraft}
                    placeholder="Only if API_PASSWORD is set"
                    onChange={event => {
                        setPasswordDraft(event.currentTarget.value);
                        setCheck({ kind: 'idle' });
                    }}
                    onBlur={commitPassword}
                    onKeyDown={commitOnEnter}
                    className={INPUT_CLASS}
                />
            </div>

            <div className="flex items-center gap-2">
                <Button
                    type="button"
                    variant="secondary"
                    size="sm"
                    className={SMALL_BUTTON_CLASS}
                    disabled={check.kind === 'checking'}
                    onClick={runCheck}
                >
                    {check.kind === 'checking' ? 'Testing…' : 'Test'}
                </Button>
                <CheckResult check={check} />
            </div>
        </div>
    );
}

export function EasyProxyCard({ api, settings, locked, relayPeerName = null, onChange }: EasyProxyCardProps) {
    const easyProxy = settings.easyProxy;

    function save(patch: Partial<EasyProxySettings>) {
        onChange({ easyProxy: { ...easyProxy, ...patch } });
    }

    const footer = relayPeerName
        ? `Streams relay to ${relayPeerName}, which uses its own EasyProxy setting.`
        : (
            <button
                type="button"
                className="text-referee-accent underline-offset-2 hover:underline"
                onClick={() => api.openExternal(EASY_PROXY_REPO_URL)}
            >
                About EasyProxy
            </button>
        );

    return (
        <SettingsSection
            title="Source Proxy"
            aside={locked && (
                <span className="flex items-center gap-1 text-[#FF6B35] opacity-90">
                    <Lock className="size-[10px]" />
                    <span className="text-[9px] font-bold uppercase tracking-widest">Locked</span>
                </span>
            )}
            groupClassName={cn(locked && 'pointer-events-none border-[#FF6B35]/20 opacity-50')}
            footer={footer}
        >
            <ToggleRow
                label="EasyProxy"
                description="Fetch stream sources through EasyProxy, which finds the video on supported sites"
                checked={easyProxy.enabled}
                onChange={enabled => save({ enabled })}
            />

            {easyProxy.enabled && (
                <>
                    <div role="radiogroup" aria-label="EasyProxy server" className="flex flex-col py-1">
                        {MODES.map(mode => (
                            <button
                                key={mode.id}
                                type="button"
                                role="radio"
                                aria-checked={easyProxy.mode === mode.id}
                                onClick={() => save({ mode: mode.id })}
                                className="flex w-full items-center gap-2.5 px-3 py-1.5 text-left transition-colors hover:bg-white/[0.03]"
                            >
                                <RadioDot selected={easyProxy.mode === mode.id} />
                                <span className="flex min-w-0 flex-col">
                                    <span className={cn('text-[12px] font-medium', easyProxy.mode === mode.id ? 'text-white' : 'text-white/80')}>
                                        {mode.label}
                                    </span>
                                    <span className="text-[10px] leading-[14px] text-referee-muted">{mode.description}</span>
                                </span>
                            </button>
                        ))}
                    </div>

                    <div className="px-3 py-2">
                        {easyProxy.mode === 'managed'
                            ? <ManagedServer api={api} locked={locked} />
                            : <ExternalServer api={api} easyProxy={easyProxy} onSave={save} />}
                    </div>
                </>
            )}
        </SettingsSection>
    );
}
