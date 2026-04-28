import type { ConsentRequest } from '@/lib/types';
import { Button } from '@/components/ui/button';
import { ShieldCheck } from 'lucide-react';

interface ConsentModalProps {
    request: ConsentRequest;
    onRespond: (nonce: string, approved: boolean, alwaysAllow: boolean) => void;
}

export function ConsentModal({ request, onRespond }: ConsentModalProps) {
    const displayOrigin = request.origin.replace(/^https?:\/\//, '');
    const displayAppName = request.appName?.trim();

    return (
        <div className="absolute inset-0 z-50 flex flex-col justify-end">
            {/* Scrim */}
            <div className="animate-fade-in absolute inset-0 bg-black/40 backdrop-blur-[0.25px]" />

            {/* Drawer */}
            <div className="animate-slide-up relative border border-b-0 border-white/[0.08] bg-[#161616] shadow-2xl">
                <div className="flex flex-col gap-4 px-4 pb-5 pt-3">
                    {/* Header */}
                    <div className="flex flex-col items-center gap-2 pt-1">
                        <div className="flex h-10 w-10 items-center justify-center rounded-full bg-referee-accent/15 ring-1 ring-referee-accent/20">
                            <ShieldCheck className="h-4 w-4 text-referee-accent" />
                        </div>
                        <div className="flex flex-col items-center gap-1">
                            <p className="text-[9.5px] pt-2 font-bold uppercase tracking-widest text-referee-accent leading-none">
                                Access Request
                            </p>
                            {displayAppName ? (
                                <p className="max-w-full truncate text-[12px] font-semibold text-white leading-none">
                                    {displayAppName}
                                </p>
                            ) : null}
                            <p className="font-mono text-[11px] font-medium text-white/70 leading-none">
                                {displayOrigin}
                            </p>
                        </div>
                    </div>

                    {/* Divider */}
                    <div className="h-px bg-white/[0.06]" />

                    {/* Description */}
                    <p className="text-[11.5px] leading-relaxed text-center text-white/45">
                        This site is requesting permission to initiate REFEREE streams on this device.
                    </p>

                    {/* Actions */}
                    <div className="flex flex-col gap-1.5">
                        <Button
                            variant="primary"
                            size="sm"
                            className="w-full"
                            onClick={() => onRespond(request.nonce, true, true)}
                        >
                            Allow
                        </Button>
                        <Button
                            variant="ghost"
                            size="sm"
                            className="w-full"
                            onClick={() => onRespond(request.nonce, false, false)}
                        >
                            Deny
                        </Button>
                    </div>
                </div>
            </div>
        </div>
    );
}
