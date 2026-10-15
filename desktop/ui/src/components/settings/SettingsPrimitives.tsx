import { ChevronDown, Sparkles } from 'lucide-react';
import { type ReactNode, type SelectHTMLAttributes, useId } from 'react';

import { cn } from '@/lib/utils';

/**
 * Shared building blocks for the settings tabs. The window is a fixed 300x500, so every
 * tab is built from the same compact grouped-list pattern: an optional section label,
 * a bordered group, and hairline-separated rows.
 */

export const SECTION_LABEL_CLASS = 'text-[10px] font-bold uppercase tracking-widest text-referee-muted';

export const CONTROL_CLASS =
    'rounded border border-transparent bg-[#282828] text-xs font-medium text-white transition-colors hover:border-white/10 hover:bg-[#333333] focus:border-referee-green focus:outline-none focus:ring-1 focus:ring-referee-green disabled:cursor-not-allowed disabled:opacity-40';

export function SettingsSection({
    title,
    aside,
    footer,
    id,
    groupClassName,
    children,
}: {
    title?: ReactNode;
    aside?: ReactNode;
    footer?: ReactNode;
    id?: string;
    groupClassName?: string;
    children: ReactNode;
}) {
    return (
        <section id={id} className="flex flex-col">
            {(title || aside) && (
                <div className="mb-1.5 flex min-h-5 items-center justify-between gap-2 px-1">
                    {title && <h3 className={SECTION_LABEL_CLASS}>{title}</h3>}
                    {aside}
                </div>
            )}
            <SettingsGroup className={groupClassName}>{children}</SettingsGroup>
            {footer && <p className="mt-1.5 px-1 text-[10px] leading-[14px] text-referee-muted/70">{footer}</p>}
        </section>
    );
}

export function SettingsGroup({ className, children }: { className?: string; children: ReactNode }) {
    return (
        <div
            className={cn(
                'flex flex-col divide-y divide-white/[0.05] overflow-hidden rounded-lg border border-referee-border bg-referee-card transition-colors duration-300',
                className
            )}
        >
            {children}
        </div>
    );
}

function RowText({
    label,
    description,
    badge,
    muted = false,
    descriptionId,
}: {
    label: ReactNode;
    description?: ReactNode;
    badge?: ReactNode;
    muted?: boolean;
    descriptionId?: string;
}) {
    return (
        <span className="flex min-w-0 flex-col">
            <span className={cn('flex items-center gap-1.5 text-[13px] font-medium leading-[18px]', muted ? 'text-referee-muted' : 'text-white')}>
                {label}
                {badge}
            </span>
            {description && (
                <span id={descriptionId} className="text-[11px] leading-[15px] text-referee-muted">
                    {description}
                </span>
            )}
        </span>
    );
}

/** A label/description on the left with an arbitrary control on the right. */
export function SettingRow({
    label,
    description,
    badge,
    control,
    muted,
    className,
}: {
    label: ReactNode;
    description?: ReactNode;
    badge?: ReactNode;
    control?: ReactNode;
    muted?: boolean;
    className?: string;
}) {
    return (
        <div className={cn('flex items-center justify-between gap-3 px-3 py-2', className)}>
            <RowText label={label} description={description} badge={badge} muted={muted} />
            {control && <div className="flex shrink-0 items-center">{control}</div>}
        </div>
    );
}

export function ToggleSwitch({
    checked,
    disabled = false,
    label,
    describedBy,
    onChange,
}: {
    checked: boolean;
    disabled?: boolean;
    label: string;
    describedBy?: string;
    onChange: (checked: boolean) => void;
}) {
    return (
        <span className={cn('relative inline-flex shrink-0 items-center', disabled && 'opacity-30')}>
            <input
                type="checkbox"
                role="switch"
                aria-label={label}
                aria-describedby={describedBy}
                className="peer sr-only toggle-checkbox"
                checked={checked}
                disabled={disabled}
                onChange={event => {
                    onChange(event.currentTarget.checked);
                }}
            />
            <span className="toggle-label block h-[20px] w-[36px] rounded-full bg-[#535353] after:absolute after:left-[2px] after:top-[2px] after:h-[16px] after:w-[16px] after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all after:content-[''] hover:bg-[#666666] peer-focus-visible:ring-2 peer-focus-visible:ring-referee-accent/60" />
        </span>
    );
}

/** A whole-row toggle: clicking the label or description flips the switch too. */
export function ToggleRow({
    label,
    description,
    badge,
    checked,
    disabled = false,
    muted,
    onChange,
}: {
    label: string;
    description?: ReactNode;
    badge?: ReactNode;
    checked: boolean;
    disabled?: boolean;
    muted?: boolean;
    onChange: (checked: boolean) => void;
}) {
    const descriptionId = useId();

    return (
        <label
            className={cn(
                // Only animate the hover background: rows gain a divider border when siblings
                // appear below them, and animating that border color would flash white.
                'flex items-center justify-between gap-3 px-3 py-2 transition-[background-color]',
                disabled ? 'cursor-not-allowed' : 'cursor-pointer hover:bg-white/[0.02]'
            )}
        >
            <RowText
                label={label}
                description={description}
                badge={badge}
                muted={muted ?? disabled}
                descriptionId={description ? descriptionId : undefined}
            />
            <ToggleSwitch
                checked={checked}
                disabled={disabled}
                label={label}
                describedBy={description ? descriptionId : undefined}
                onChange={onChange}
            />
        </label>
    );
}

export function SelectField({
    className,
    selectClassName,
    children,
    ...props
}: SelectHTMLAttributes<HTMLSelectElement> & { selectClassName?: string }) {
    return (
        <div className={cn('group relative min-w-0', className)}>
            <select
                {...props}
                className={cn(CONTROL_CLASS, 'w-full cursor-pointer appearance-none truncate py-1 pl-2.5 pr-7', selectClassName)}
            >
                {children}
            </select>
            <ChevronDown className="pointer-events-none absolute right-2 top-1/2 size-3 -translate-y-1/2 text-referee-muted group-hover:text-white" />
        </div>
    );
}

export function RadioDot({ selected }: { selected: boolean }) {
    return (
        <span
            aria-hidden="true"
            className={cn(
                'flex size-3.5 shrink-0 items-center justify-center rounded-full border transition-colors',
                selected ? 'border-referee-accent bg-referee-accent' : 'border-white/25'
            )}
        >
            {selected && <span className="size-1.5 rounded-full bg-white" />}
        </span>
    );
}

export function AiBadge() {
    return (
        <span className="inline-flex items-center gap-0.5 rounded bg-referee-green/10 px-1 py-px text-[9px] font-bold leading-3 tracking-wide text-referee-green">
            <Sparkles className="size-2.5" />
            AI
        </span>
    );
}
