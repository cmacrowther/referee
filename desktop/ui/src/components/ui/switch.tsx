import type { HTMLAttributes } from 'react';

import { cn } from '@/lib/utils';

interface SwitchProps extends Omit<HTMLAttributes<HTMLButtonElement>, 'onChange'> {
    checked: boolean;
    disabled?: boolean;
    onCheckedChange?: (checked: boolean) => void;
}

export function Switch({ checked, disabled, onCheckedChange, className, ...props }: SwitchProps) {
    return (
        <button
            type="button"
            role="switch"
            aria-checked={checked}
            disabled={disabled}
            onClick={() => {
                if (!disabled) {
                    onCheckedChange?.(!checked);
                }
            }}
            className={cn(
                'relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border border-transparent transition',
                checked ? 'bg-referee-accent' : 'bg-white/[0.14]',
                disabled ? 'opacity-50' : 'hover:brightness-110',
                className
            )}
            {...props}
        >
            <span
                className={cn(
                    'inline-block size-5 rounded-full border border-white/[0.2] bg-white transition-transform',
                    checked ? 'translate-x-5' : 'translate-x-0.5'
                )}
            />
        </button>
    );
}
