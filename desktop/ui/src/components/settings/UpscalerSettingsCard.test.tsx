import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { UpscalerSettingsCard } from './UpscalerSettingsCard';
import { DEFAULT_SETTINGS, DEFAULT_STATUS } from '@/lib/types';

describe('UpscalerSettingsCard', () => {
    it('shows editable remote relay state when REFEREE Relay is enabled', () => {
        const onOpenRelaySettings = vi.fn();
        const onChange = vi.fn();

        render(
            <UpscalerSettingsCard
                settings={DEFAULT_SETTINGS}
                locked={false}
                relayRemote
                relayUpdating
                relayPeerName="media-box"
                status={DEFAULT_STATUS}
                engineSettings={false}
                onOpenRelaySettings={onOpenRelaySettings}
                onChange={onChange}
            />
        );

        expect(screen.getByText('Remote Relay Settings')).not.toBeNull();
        expect(screen.getByText('Updating media-box')).not.toBeNull();
        expect(
            screen.getByText('Changes here are saved to media-box for new relay streams.')
        ).not.toBeNull();
        expect(screen.getByText('Updating remote relay...')).not.toBeNull();

        fireEvent.change(screen.getByLabelText('Upscaling Quality'), { target: { value: '3' } });
        expect(onChange).toHaveBeenCalledWith({ quality: 3 });

        const relaySettingsButton = screen.getByRole('button', { name: 'Open Relay Settings' });
        expect(relaySettingsButton.querySelector('svg')).toBeNull();

        fireEvent.click(relaySettingsButton);

        expect(onOpenRelaySettings).toHaveBeenCalledTimes(1);
    });

    it('shows a remote relay update error', () => {
        render(
            <UpscalerSettingsCard
                settings={DEFAULT_SETTINGS}
                locked={false}
                relayRemote
                relayUpdateError="Could not reach media-box"
                relayPeerName="media-box"
                status={DEFAULT_STATUS}
                engineSettings={false}
                onChange={vi.fn()}
            />
        );

        expect(screen.getByText('Could not reach media-box')).not.toBeNull();
    });

    it('allows Motion Boost to be toggled for an NVIDIA relay peer', () => {
        const onChange = vi.fn();
        const { container } = render(
            <UpscalerSettingsCard
                settings={{ ...DEFAULT_SETTINGS, framegen: true }}
                locked={false}
                relayRemote
                relayPeerName="media-box"
                relayPeer={{
                    instanceId: 'relay-1',
                    hostname: 'media-box',
                    ip: '192.168.1.50',
                    version: '1.0.0',
                    platform: 'linux',
                    gpuReady: true,
                    gpuVendor: 'nvidia',
                    gpuName: 'RTX',
                }}
                status={{ ...DEFAULT_STATUS, encoderHasFramegen: false }}
                engineSettings={false}
                onChange={onChange}
            />
        );

        const motionBoostToggle = container.querySelectorAll<HTMLInputElement>('.toggle-checkbox')[0];

        expect(motionBoostToggle.disabled).toBe(false);

        fireEvent.click(motionBoostToggle);

        expect(onChange).toHaveBeenCalledWith({ framegen: false });
    });

    it('locks remote relay settings when the linked peer is offline', () => {
        const onChange = vi.fn();

        const { container } = render(
            <UpscalerSettingsCard
                settings={DEFAULT_SETTINGS}
                locked={false}
                relayRemote
                relayRemoteOffline
                relayPeerName="media-box"
                status={DEFAULT_STATUS}
                engineSettings={false}
                onChange={onChange}
            />
        );

        expect(screen.getByText('Remote Relay Settings')).not.toBeNull();
        expect(screen.getByText('media-box is offline')).not.toBeNull();
        expect(screen.getByText('Remote Relay settings are locked until media-box comes online.')).not.toBeNull();
        expect(screen.getByText('Remote relay offline')).not.toBeNull();
        expect(screen.queryByText('Remote relay ready')).toBeNull();
        expect((container.querySelector('#upscaler-controls') as HTMLFieldSetElement | null)?.disabled).toBe(true);
    });

    it('shows a relay-required lock message when local hardware is unsupported', () => {
        const onOpenRelaySettings = vi.fn();

        render(
            <UpscalerSettingsCard
                settings={DEFAULT_SETTINGS}
                locked={false}
                relayLocked
                relayRequired
                status={DEFAULT_STATUS}
                engineSettings={false}
                onOpenRelaySettings={onOpenRelaySettings}
                onChange={vi.fn()}
            />
        );

        expect(screen.getByText('Stream Settings Locked')).not.toBeNull();
        expect(screen.getByText('This device requires REFEREE Relay')).not.toBeNull();
        expect(
            screen.getByText('No supported graphics hardware detected. Link a REFEREE Relay peer to route streams.')
        ).not.toBeNull();

        fireEvent.click(screen.getByRole('button', { name: 'Open Relay Settings' }));

        expect(onOpenRelaySettings).toHaveBeenCalledTimes(1);
    });
});
