import { useCallback, useEffect, useRef, useState } from 'react';

import { normalizeStatus } from '@/lib/stream';
import { isTauri } from '@/lib/renderer-api';
import { DEFAULT_STATUS, type RendererApi, type Status } from '@/lib/types';

const STATUS_ENDPOINT = 'http://127.0.0.1:14002/v1/status';
const BROWSER_STATUS_POLL_MS = 2000;
const TAURI_FALLBACK_STATUS_POLL_MS = 5000;
const RECENT_TAURI_EVENT_MS = 5000;

function getStatusSignature(status: Status) {
    return JSON.stringify(status);
}

export function useStatus(api: RendererApi) {
    const [status, setStatus] = useState<Status>(DEFAULT_STATUS);
    const statusSignatureRef = useRef(getStatusSignature(DEFAULT_STATUS));
    const lastTauriEventAtRef = useRef(0);
    const tauriRuntimeRef = useRef(isTauri());

    const commitStatus = useCallback((nextStatus: Partial<Status>) => {
        const normalizedStatus = normalizeStatus(nextStatus);
        const nextSignature = getStatusSignature(normalizedStatus);
        if (nextSignature === statusSignatureRef.current) {
            return;
        }

        statusSignatureRef.current = nextSignature;
        setStatus(normalizedStatus);
    }, []);

    useEffect(() => {
        return api.onStatusUpdate(nextStatus => {
            lastTauriEventAtRef.current = Date.now();
            commitStatus(nextStatus);
        });
    }, [api, commitStatus]);

    useEffect(() => {
        let disposed = false;

        // Poll the HTTP status endpoint as a reliable fallback for Tauri events.
        // This ensures the UI stays in sync even if the IPC event channel has
        // a delivery gap (e.g. after a window destroy/recreate cycle).
        const syncStatus = async () => {
            if (
                tauriRuntimeRef.current
                && Date.now() - lastTauriEventAtRef.current < RECENT_TAURI_EVENT_MS
            ) {
                return;
            }

            try {
                const response = await fetch(STATUS_ENDPOINT, {
                    headers: {
                        Accept: 'application/json'
                    }
                });

                if (!response.ok || disposed) {
                    return;
                }

                const nextStatus = await response.json();
                if (disposed) {
                    return;
                }

                commitStatus(nextStatus);
            } catch {
                // The local status server may not be ready yet during startup.
            }
        };

        syncStatus();
        const interval = setInterval(
            syncStatus,
            tauriRuntimeRef.current ? TAURI_FALLBACK_STATUS_POLL_MS : BROWSER_STATUS_POLL_MS
        );

        return () => {
            disposed = true;
            clearInterval(interval);
        };
    }, [commitStatus]);

    return {
        status,
        setStatus: commitStatus
    };
}
