import { ShieldCheck, Trash2 } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { SettingsSection } from '@/components/settings/SettingsPrimitives';
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
        <SettingsSection
            title="Site Access"
            footer={'Sites set to "Always Allow" skip the approval prompt. Remove one to require approval again.'}
        >
            {origins.length === 0 ? (
                <div className="flex items-center gap-2.5 px-3 py-2.5">
                    <ShieldCheck className="size-4 shrink-0 text-referee-muted/50" />
                    <p className="text-[11px] leading-[15px] text-referee-muted">
                        No sites have permanent access yet.
                    </p>
                </div>
            ) : (
                origins.map(({ origin, appName }) => {
                    const displayOrigin = origin.replace(/^https?:\/\//, '');
                    const isRevoking = revoking === origin;
                    return (
                        <div key={origin} className="flex items-center justify-between gap-2 py-2 pl-3 pr-2">
                            <div className="flex min-w-0 flex-col">
                                <span className="truncate text-[12px] font-medium leading-4 text-white">{displayOrigin}</span>
                                {appName && (
                                    <span className="truncate text-[11px] leading-[15px] text-referee-muted">{appName}</span>
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
                        </div>
                    );
                })
            )}
        </SettingsSection>
    );
}
