import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { PlayerSettingsCard } from './PlayerSettingsCard';
import { DEFAULT_SETTINGS } from '@/lib/types';
import { makeMockApi } from '@/test/mocks';

const ENABLED_PLAYER_SETTINGS = {
    ...DEFAULT_SETTINGS,
    player: {
        ...DEFAULT_SETTINGS.player,
        enabled: true,
    },
};

describe('PlayerSettingsCard', () => {
    it('renders the built-in player option', async () => {
        const { api } = makeMockApi();

        render(
            <PlayerSettingsCard
                api={api}
                settings={ENABLED_PLAYER_SETTINGS}
                locked={false}
                onChange={vi.fn()}
            />
        );

        await waitFor(() => {
            expect(screen.getByText('REFEREE Built-in Player')).not.toBeNull();
        });
    });

    it('selects the built-in player without showing the custom path input', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(
            <PlayerSettingsCard
                api={api}
                settings={ENABLED_PLAYER_SETTINGS}
                locked={false}
                onChange={onChange}
            />
        );

        await waitFor(() => {
            expect(screen.getByText('REFEREE Built-in Player')).not.toBeNull();
        });

        await userEvent.click(screen.getByText('REFEREE Built-in Player'));

        expect(onChange).toHaveBeenCalledWith({
            player: {
                ...ENABLED_PLAYER_SETTINGS.player,
                selectedPlayer: 'builtin',
            },
        });
        expect(screen.queryByLabelText('Executable Path')).toBeNull();
    });
});
