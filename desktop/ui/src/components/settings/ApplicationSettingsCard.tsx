import { SettingsSection, ToggleRow } from '@/components/settings/SettingsPrimitives';
import type { Settings } from '@/lib/types';

interface ApplicationSettingsCardProps {
    settings: Settings;
    bootSetting: boolean;
    onChange: (patch: Partial<Settings>) => void;
    onBootChange: (value: boolean) => void;
}

export function ApplicationSettingsCard({
    settings,
    bootSetting,
    onChange,
    onBootChange
}: ApplicationSettingsCardProps) {
    return (
        <SettingsSection title="Application">
            <ToggleRow
                label="Start on Boot"
                description="Launch quietly on startup"
                checked={bootSetting}
                onChange={onBootChange}
            />
            <ToggleRow
                label="Minimize to Tray"
                description="Hide rather than minimize"
                checked={settings.minimizeToTray}
                onChange={checked => {
                    onChange({ minimizeToTray: checked });
                }}
            />
            <ToggleRow
                label="Show on Proxy Start"
                description="Open REFEREE when a stream starts"
                checked={settings.showOnProxyStart}
                onChange={checked => {
                    onChange({ showOnProxyStart: checked });
                }}
            />
            <ToggleRow
                label="Close to Tray"
                description="Keep running in background"
                checked={settings.closeToTray}
                onChange={checked => {
                    onChange({ closeToTray: checked });
                }}
            />
            <ToggleRow
                label="Stream Notifications"
                description="Notify when stream status changes"
                checked={settings.notifications}
                onChange={checked => {
                    onChange({ notifications: checked });
                }}
            />
        </SettingsSection>
    );
}
