import type { InputHTMLAttributes } from 'react';

import { cn } from '@/lib/utils';

export function Slider({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
    return <input type="range" className={cn('referee-slider w-full', className)} {...props} />;
}
