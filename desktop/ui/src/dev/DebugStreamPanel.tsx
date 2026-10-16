import { Play } from 'lucide-react';
import { type FormEvent, useId, useState } from 'react';

import { Button } from '@/components/ui/button';
import type { RendererApi } from '@/lib/types';

const LAST_URL_STORAGE_KEY = 'referee.debugStreamUrl';

function readLastUrl() {
    try {
        return window.localStorage.getItem(LAST_URL_STORAGE_KEY) ?? '';
    } catch {
        return '';
    }
}

function rememberUrl(url: string) {
    try {
        window.localStorage.setItem(LAST_URL_STORAGE_KEY, url);
    } catch {
        // Storage can be unavailable; the field just starts empty next time.
    }
}

function isHttpUrl(value: string) {
    try {
        const url = new URL(value);
        return url.protocol === 'http:' || url.protocol === 'https:';
    } catch {
        return false;
    }
}

/**
 * Debug-only standby panel: starts a stream from a pasted source URL and
 * opens playback, without a web client.
 */
export function DebugStreamPanel({ api }: { api: RendererApi }) {
    const inputId = useId();
    const [url, setUrl] = useState(readLastUrl);
    const [starting, setStarting] = useState(false);
    const [error, setError] = useState<string | null>(null);

    async function handleSubmit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();
        const trimmed = url.trim();
        if (!isHttpUrl(trimmed)) {
            setError('Enter an http:// or https:// stream URL.');
            return;
        }

        rememberUrl(trimmed);
        setError(null);
        setStarting(true);
        try {
            await api.startDebugStream(trimmed);
        } catch (startError) {
            setError(startError instanceof Error ? startError.message : String(startError || 'The stream did not start.'));
        } finally {
            setStarting(false);
        }
    }

    return (
        <form
            onSubmit={handleSubmit}
            className="flex shrink-0 flex-col gap-2 rounded-lg border border-referee-green/30 bg-referee-green/10 p-2.5"
        >
            <div className="flex items-center justify-between">
                <label htmlFor={inputId} className="text-[10px] font-bold uppercase tracking-widest text-referee-muted">
                    Test Stream
                </label>
                <span className="rounded border border-referee-green/30 bg-referee-green/10 px-1.5 py-0.5 text-[9px] font-medium leading-none text-referee-green">
                    DEBUG ONLY
                </span>
            </div>
            <div className="flex items-center gap-2">
                <input
                    id={inputId}
                    type="url"
                    spellCheck={false}
                    autoComplete="off"
                    value={url}
                    disabled={starting}
                    placeholder="https://example.com/stream.m3u8"
                    onChange={event => {
                        setUrl(event.currentTarget.value);
                        setError(null);
                    }}
                    className="min-w-0 flex-1 rounded-md border border-white/[0.08] bg-white/[0.04] px-2.5 py-1.5 text-[11px] text-white placeholder-white/25 outline-none transition-colors focus:border-referee-accent/40 focus:bg-referee-accent/[0.04] disabled:opacity-50"
                />
                <Button
                    type="submit"
                    variant="secondary"
                    size="sm"
                    className="h-7 shrink-0 rounded-md px-2.5 text-[10px]"
                    disabled={starting || !url.trim()}
                >
                    <Play className="size-3 fill-current" aria-hidden="true" />
                    {starting ? 'Starting…' : 'Play'}
                </Button>
            </div>
            {error && (
                <p role="alert" className="text-[11px] leading-[15px] text-red-300">
                    {error}
                </p>
            )}
        </form>
    );
}
