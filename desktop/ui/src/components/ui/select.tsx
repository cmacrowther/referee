import { ChevronDown } from 'lucide-react';
import type { SelectHTMLAttributes } from 'react';

import { cn } from '@/lib/utils';

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
    return (
        <div className="relative">
            <select
                className={cn(
                    'h-9 w-full appearance-none rounded-xl border border-white/[0.08] bg-white/[0.06] px-3 pr-9 text-[12px] font-medium text-white outline-none transition hover:bg-white/[0.08] focus:border-referee-accent',
                    className
                )}
                {...props}
            >
                {children}
            </select>
            <ChevronDown className="pointer-events-none absolute right-3 top-1/2 size-4 -translate-y-1/2 text-referee-text-muted" />
        </div>
    );
}
