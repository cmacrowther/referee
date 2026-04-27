import type { Settings } from '@/lib/types';

interface ApplicationSettingsCardProps {
    settings: Settings;
    bootSetting: boolean;
    onChange: (patch: Partial<Settings>) => void;
    onBootChange: (value: boolean) => void;
}

function ToggleField({
    checked,
    onChange
}: {
    checked: boolean;
    onChange: (checked: boolean) => void;
}) {
    return (
        <label className="relative inline-flex cursor-pointer items-center">
            <input
                type="checkbox"
                className="sr-only toggle-checkbox"
                checked={checked}
                onChange={event => {
                    onChange(event.currentTarget.checked);
                }}
            />
            <div className="toggle-label h-[20px] w-[36px] rounded-full bg-[#535353] after:absolute after:left-[2px] after:top-[2px] after:h-[16px] after:w-[16px] after:rounded-full after:border after:border-gray-300 after:bg-white after:transition-all after:content-[''] hover:bg-[#666666]" />
        </label>
    );
}

function SettingRow({
    title,
    description,
    checked,
    onChange
}: {
    title: string;
    description: string;
    checked: boolean;
    onChange: (checked: boolean) => void;
}) {
    return (
        <div className="flex items-center justify-between gap-3">
            <div className="flex min-w-0 flex-col pr-2">
                <span className="text-[13px] font-medium text-white">{title}</span>
                <span className="text-[11px] text-referee-muted">{description}</span>
            </div>
            <ToggleField checked={checked} onChange={onChange} />
        </div>
    );
}

export function ApplicationSettingsCard({
    settings,
    bootSetting,
    onChange,
    onBootChange
}: ApplicationSettingsCardProps) {
    return (
        <div className="rounded-lg border border-referee-border bg-referee-card p-3.5">
            <div className="mb-3 text-[10px] font-bold uppercase tracking-widest text-referee-muted">Application</div>

            <div className="flex flex-col gap-4">
                <SettingRow
                    title="Start on Boot"
                    description="Launch quietly on startup"
                    checked={bootSetting}
                    onChange={onBootChange}
                />
                <SettingRow
                    title="Minimize to Tray"
                    description="Hide rather than minimize"
                    checked={settings.minimizeToTray}
                    onChange={checked => {
                        onChange({ minimizeToTray: checked });
                    }}
                />
                <SettingRow
                    title="Show on Proxy Start"
                    description="Open REFEREE when a stream starts"
                    checked={settings.showOnProxyStart}
                    onChange={checked => {
                        onChange({ showOnProxyStart: checked });
                    }}
                />
                <SettingRow
                    title="Close to Tray"
                    description="Keep running in background"
                    checked={settings.closeToTray}
                    onChange={checked => {
                        onChange({ closeToTray: checked });
                    }}
                />
                <SettingRow
                    title="Stream Notifications"
                    description="Notify when stream status changes"
                    checked={settings.notifications}
                    onChange={checked => {
                        onChange({ notifications: checked });
                    }}
                />
            </div>
        </div>
    );
}
