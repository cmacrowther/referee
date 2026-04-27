import { CircleAlert } from 'lucide-react';

interface SetupErrorProps {
    error: string | null;
    onRetry: () => void;
}

export function SetupError({ error, onRetry }: SetupErrorProps) {
    if (!error) {
        return null;
    }

    return (
        <div className="flex w-full flex-col items-center gap-3">
            <div className="w-full rounded-lg border border-[#3a2020] bg-[#2a1a1a] p-3 text-[11px] leading-relaxed text-[#ff6b6b]">
                <div className="flex items-start gap-2">
                    <CircleAlert className="mt-0.5 size-[14px] shrink-0" />
                    <span>{error}</span>
                </div>
            </div>
            <button
                type="button"
                onClick={onRetry}
                className="rounded-lg bg-[#FF6B35] px-4 py-2 text-[12px] font-semibold uppercase tracking-wider text-white transition-colors hover:bg-[#e55a2b]"
            >
                Retry
            </button>
        </div>
    );
}
