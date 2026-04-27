import type { HTMLAttributes, ReactNode } from 'react';

import { cn } from '@/lib/utils';

export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
    return (
        <div
            className={cn(
                'rounded-2xl border border-white/[0.06] bg-referee-card shadow-[0_18px_40px_rgba(0,0,0,0.28)]',
                className
            )}
            {...props}
        />
    );
}

export function CardHeader({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
    return <div className={cn('flex items-center justify-between gap-3 px-4 pt-4', className)} {...props} />;
}

export function CardTitle({ className, ...props }: HTMLAttributes<HTMLHeadingElement>) {
    return (
        <h3
            className={cn('text-[10px] font-bold uppercase tracking-[0.2em] text-referee-text-muted', className)}
            {...props}
        />
    );
}

export function CardDescription({ className, ...props }: HTMLAttributes<HTMLParagraphElement>) {
    return <p className={cn('text-[11px] text-referee-text-soft', className)} {...props} />;
}

export function CardContent({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
    return <div className={cn('px-4 pb-4 pt-3', className)} {...props} />;
}

export function CardAction({ children }: { children: ReactNode }) {
    return <div className="flex items-center gap-2">{children}</div>;
}
