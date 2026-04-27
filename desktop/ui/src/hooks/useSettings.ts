import { useCallback, useEffect, useRef, useState } from 'react';

import { normalizeSettings } from '@/lib/stream';
import { DEFAULT_SETTINGS, type RendererApi, type Settings } from '@/lib/types';

const SETTINGS_FALLBACK_POLL_MS = 15000;

function getSettingsSignature(settings: Settings) {
    return JSON.stringify(settings);
}

export function useSettings(api: RendererApi) {
    const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
    const [bootSetting, setBootSetting] = useState(false);
    const [isReady, setIsReady] = useState(false);
    const [isSavingRelayStreamSettings, setIsSavingRelayStreamSettings] = useState(false);
    const [relayStreamSettingsError, setRelayStreamSettingsError] = useState<string | null>(null);
    const settingsRef = useRef(DEFAULT_SETTINGS);
    const settingsSignatureRef = useRef(getSettingsSignature(DEFAULT_SETTINGS));
    const bootSettingRef = useRef(false);

    const commitSettings = useCallback((nextSettings: Partial<Settings>) => {
        const normalizedSettings = normalizeSettings(nextSettings);
        const nextSignature = getSettingsSignature(normalizedSettings);
        if (nextSignature === settingsSignatureRef.current) {
            return normalizedSettings;
        }

        settingsSignatureRef.current = nextSignature;
        settingsRef.current = normalizedSettings;
        setSettings(normalizedSettings);
        return normalizedSettings;
    }, []);

    const commitBootSetting = useCallback((nextValue: boolean) => {
        const normalizedValue = Boolean(nextValue);
        if (normalizedValue === bootSettingRef.current) {
            return;
        }

        bootSettingRef.current = normalizedValue;
        setBootSetting(normalizedValue);
    }, []);

    useEffect(() => {
        let isMounted = true;
        let disposed = false;

        const syncSettings = async () => {
            try {
                const nextSettings = await api.getInitialSettings();
                if (disposed) {
                    return;
                }

                commitSettings(nextSettings);
            } catch {
                if (!isMounted || disposed) {
                    return;
                }

                commitSettings(DEFAULT_SETTINGS);
            }
        };

        Promise.all([
            api.getInitialSettings().catch(() => DEFAULT_SETTINGS),
            api.getBootSetting().catch(() => false)
        ]).then(([initialSettings, initialBootSetting]) => {
            if (!isMounted || disposed) {
                return;
            }

            commitSettings(initialSettings);
            commitBootSetting(Boolean(initialBootSetting));
            setIsReady(true);
        });

        const interval = window.setInterval(syncSettings, SETTINGS_FALLBACK_POLL_MS);
        window.addEventListener('focus', syncSettings);

        return () => {
            isMounted = false;
            disposed = true;
            window.clearInterval(interval);
            window.removeEventListener('focus', syncSettings);
        };
    }, [api, commitBootSetting, commitSettings]);

    useEffect(() => {
        return api.onSettingsSync(nextSettings => {
            commitSettings(nextSettings);
        });
    }, [api, commitSettings]);

    useEffect(() => {
        return api.onBootSettingSync(nextValue => {
            commitBootSetting(Boolean(nextValue));
        });
    }, [api, commitBootSetting]);

    function saveSettings(patch: Partial<Settings> | ((previous: Settings) => Settings)) {
        const previousSettings = settingsRef.current;
        const nextSettings = normalizeSettings(
            typeof patch === 'function' ? patch(previousSettings) : { ...previousSettings, ...patch }
        );
        const nextSignature = getSettingsSignature(nextSettings);
        if (nextSignature === settingsSignatureRef.current) {
            return;
        }

        settingsSignatureRef.current = nextSignature;
        settingsRef.current = nextSettings;
        setSettings(nextSettings);
        api.saveSettings(nextSettings);
    }

    function saveStreamSettings(patch: Partial<Settings> | ((previous: Settings) => Settings)) {
        const previousSettings = settingsRef.current;
        const nextSettings = normalizeSettings(
            typeof patch === 'function' ? patch(previousSettings) : { ...previousSettings, ...patch }
        );

        const nextSignature = getSettingsSignature(nextSettings);
        if (nextSignature !== settingsSignatureRef.current) {
            settingsSignatureRef.current = nextSignature;
            settingsRef.current = nextSettings;
            setSettings(nextSettings);
        }

        const targetsRelay = Boolean(
            nextSettings.relay.enabled
            && (
                nextSettings.relay.linkedPeerId
                || nextSettings.relay.linkedPeerIp
                || nextSettings.relay.remoteToken
                || nextSettings.relay.lastKnownPeer
            )
        );

        if (!targetsRelay) {
            api.saveSettings(nextSettings);
            return;
        }

        setRelayStreamSettingsError(null);
        setIsSavingRelayStreamSettings(true);
        api.saveStreamSettings(nextSettings)
            .catch(error => {
                setRelayStreamSettingsError(
                    error instanceof Error ? error.message : String(error || 'Unable to update the linked relay.')
                );
            })
            .finally(() => {
                setIsSavingRelayStreamSettings(false);
            });
    }

    function updateBootSetting(nextValue: boolean) {
        const normalizedValue = Boolean(nextValue);
        commitBootSetting(normalizedValue);
        api.setBootSetting(normalizedValue);
    }

    return {
        settings,
        bootSetting,
        isReady,
        isSavingRelayStreamSettings,
        relayStreamSettingsError,
        saveSettings,
        saveStreamSettings,
        updateBootSetting
    };
}
