import type { ButtonHTMLAttributes, ReactNode } from 'react';

import { cn } from '@/lib/utils';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';
type ButtonSize = 'sm' | 'md' | 'icon';

const variantClasses: Record<ButtonVariant, string> = {
    primary: 'bg-referee-accent text-white hover:bg-referee-accent-strong',
    secondary: 'bg-white/[0.08] text-white hover:bg-white/[0.12]',
    ghost: 'bg-transparent text-referee-text-muted hover:bg-white/[0.06] hover:text-white',
    danger: 'bg-referee-danger/[0.15] text-referee-danger hover:bg-referee-danger/[0.25]'
};

const sizeClasses: Record<ButtonSize, string> = {
    sm: 'h-8 px-3 text-[11px]',
    md: 'h-10 px-4 text-[12px]',
    icon: 'size-9'
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
    variant?: ButtonVariant;
    size?: ButtonSize;
    children: ReactNode;
}

export function Button({
    className,
    variant = 'primary',
    size = 'md',
    children,
    ...props
}: ButtonProps) {
    return (
        <button
            className={cn(
                'inline-flex items-center justify-center gap-2 rounded-xl font-semibold uppercase tracking-[0.16em] transition disabled:cursor-not-allowed disabled:opacity-50',
                variantClasses[variant],
                sizeClasses[size],
                className
            )}
            {...props}
        >
            {children}
        </button>
    );
}
