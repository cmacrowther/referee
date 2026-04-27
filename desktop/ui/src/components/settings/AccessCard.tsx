import { ShieldCheck, Trash2 } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import type { ApprovedOrigin, RendererApi } from '@/lib/types';

const ACCESS_REFRESH_MS = 15000;

interface AccessCardProps {
    api: RendererApi;
}

export function AccessCard({ api }: AccessCardProps) {
    const [origins, setOrigins] = useState<ApprovedOrigin[]>([]);
    const [revoking, setRevoking] = useState<string | null>(null);

    const load = useCallback(() => {
        api.getApprovedOrigins()
            .then(nextOrigins => {
                setOrigins(previousOrigins => (
                    JSON.stringify(previousOrigins) === JSON.stringify(nextOrigins)
                        ? previousOrigins
                        : nextOrigins
                ));
            })
            .catch(() => {});
    }, [api]);

    useEffect(() => {
        load();
        const interval = window.setInterval(load, ACCESS_REFRESH_MS);
        window.addEventListener('focus', load);

        return () => {
            window.clearInterval(interval);
            window.removeEventListener('focus', load);
        };
    }, [load]);

    const handleRevoke = useCallback(
        async (origin: string) => {
            setRevoking(origin);
            try {
                await api.revokeApprovedOrigin(origin);
                setOrigins(prev => prev.filter(o => o.origin !== origin));
            } finally {
                setRevoking(null);
            }
        },
        [api],
    );

    return (
        <div className="rounded-lg border border-referee-border bg-referee-card p-3.5">
            <div className="mb-3 text-[10px] font-bold uppercase tracking-widest text-referee-muted">
                Access
            </div>

            {origins.length === 0 ? (
                <div className="flex flex-col items-center gap-2 py-3 text-center">
                    <ShieldCheck className="size-5 text-referee-muted/50" />
                    <p className="text-[11px] text-referee-muted">
                        No sites have been granted permanent access. Choose "Always Allow" to add them.
                    </p>
                </div>
            ) : (
                <ul className="flex flex-col divide-y divide-white/[0.04]">
                    {origins.map(({ origin, appName }) => {
                        const displayOrigin = origin.replace(/^https?:\/\//, '');
                        const isRevoking = revoking === origin;
                        return (
                            <li key={origin} className="flex items-center justify-between gap-2 py-2 first:pt-0 last:pb-0">
                                <div className="flex min-w-0 flex-col">
                                    <span className="truncate text-[12px] font-medium text-white">{displayOrigin}</span>
                                    {appName && (
                                        <span className="truncate text-[11px] text-referee-muted">{appName}</span>
                                    )}
                                </div>
                                <button
                                    type="button"
                                    disabled={isRevoking}
                                    onClick={() => handleRevoke(origin)}
                                    className="shrink-0 rounded p-1 text-referee-muted transition-colors hover:bg-white/[0.06] hover:text-red-400 disabled:cursor-not-allowed disabled:opacity-40"
                                    aria-label={`Revoke access for ${displayOrigin}`}
                                >
                                    <Trash2 className="size-3.5" />
                                </button>
                            </li>
                        );
                    })}
                </ul>
            )}

            <p className="mt-3 text-[10px] leading-relaxed text-referee-muted/70">
                Sites listed here were granted "Always Allow" access and will not prompt for
                authorization again. Removing a site will require re-approval on their next request.
            </p>
        </div>
    );
}
