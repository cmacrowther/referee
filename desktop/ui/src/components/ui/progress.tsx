import { cn } from '@/lib/utils';

interface ProgressProps {
    value: number;
    className?: string;
}

export function Progress({ value, className }: ProgressProps) {
    const clampedValue = Math.max(0, Math.min(100, value));

    return (
        <div className={cn('h-2 w-full overflow-hidden rounded-full bg-white/[0.08]', className)}>
            <div
                className="h-full rounded-full bg-referee-accent transition-[width] duration-300 ease-out"
                style={{ width: `${clampedValue}%` }}
            />
        </div>
    );
}
