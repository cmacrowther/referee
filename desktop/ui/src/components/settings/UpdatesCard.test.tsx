import { act, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { checkedAgo, UpdatesCard } from './UpdatesCard';
import { DEFAULT_SETTINGS, type UpdateStatus } from '@/lib/types';
import { makeMockApi, UP_TO_DATE } from '@/test/mocks';

function updateStatus(overrides: Partial<UpdateStatus>): UpdateStatus {
    return { ...UP_TO_DATE, currentVersion: '0.7.0', ...overrides };
}

const AVAILABLE = updateStatus({
    latestVersion: '0.8.0',
    releaseUrl: 'https://github.com/cmacrowther/referee/releases/tag/v0.8.0',
});

function renderCard(status: UpdateStatus, { locked = false, autoUpdate = true } = {}) {
    const { api, emit } = makeMockApi();
    vi.mocked(api.getUpdateStatus).mockResolvedValue(status);
    vi.mocked(api.checkForUpdate).mockResolvedValue(status);
    const onChange = vi.fn();
    render(
        <UpdatesCard
            api={api}
            settings={{ ...DEFAULT_SETTINGS, autoUpdate }}
            locked={locked}
            onChange={onChange}
        />
    );
    return { api, emit, onChange };
}

describe('UpdatesCard', () => {
    it('shows a recent check without checking again', async () => {
        const { api } = renderCard(updateStatus({}));

        await screen.findByText('Up to date');
        expect(screen.getByText('v0.7.0 · checked 1 min ago')).not.toBeNull();
        expect(api.checkForUpdate).not.toHaveBeenCalled();
    });

    it('checks GitHub when opened without a recent check', async () => {
        const { api } = makeMockApi();
        vi.mocked(api.getUpdateStatus).mockResolvedValue(updateStatus({ lastChecked: null }));
        vi.mocked(api.checkForUpdate).mockResolvedValue(AVAILABLE);

        render(<UpdatesCard api={api} settings={DEFAULT_SETTINGS} locked={false} onChange={vi.fn()} />);

        await screen.findByText('v0.8.0 available');
        expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
        expect(screen.getByText('You have v0.7.0')).not.toBeNull();
    });

    it('checks on request', async () => {
        const { api } = renderCard(updateStatus({}));

        await userEvent.click(await screen.findByRole('button', { name: 'Check' }));
        expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
    });

    it('installs an available update and links its release notes', async () => {
        const { api } = renderCard(AVAILABLE);

        await userEvent.click(await screen.findByRole('button', { name: 'Update' }));
        expect(api.installUpdate).toHaveBeenCalledTimes(1);

        await userEvent.click(screen.getByRole('button', { name: "What's new in v0.8.0" }));
        expect(api.openExternal).toHaveBeenCalledWith(AVAILABLE.releaseUrl);
    });

    it('waits for streams to stop before installing', async () => {
        renderCard(AVAILABLE, { locked: true });

        const button = (await screen.findByRole('button', { name: 'Update' })) as HTMLButtonElement;
        expect(button.disabled).toBe(true);
        expect(screen.getByText('Stop active streams to update.')).not.toBeNull();
    });

    it('shows why an install could not start', async () => {
        const { api } = renderCard(AVAILABLE);
        vi.mocked(api.installUpdate).mockRejectedValue('The update was cancelled at the administrator prompt.');

        await userEvent.click(await screen.findByRole('button', { name: 'Update' }));
        expect((await screen.findByRole('alert')).textContent).toBe(
            'The update was cancelled at the administrator prompt.'
        );
    });

    it('follows download progress from status events', async () => {
        const { emit } = renderCard(AVAILABLE);
        await screen.findByText('v0.8.0 available');

        act(() => emit.updateStatus({ ...AVAILABLE, phase: 'downloading', progress: 42 }));
        expect(screen.getByText('Downloading v0.8.0')).not.toBeNull();
        const bar = screen.getByRole('progressbar', { name: 'Update download' });
        expect(bar.getAttribute('aria-valuenow')).toBe('42');
        expect((screen.getByRole('button', { name: 'Update' }) as HTMLButtonElement).disabled).toBe(true);

        act(() => emit.updateStatus({ ...AVAILABLE, phase: 'ready' }));
        expect(screen.getByText('v0.8.0 ready')).not.toBeNull();
        expect(screen.getByText('Installs when no stream is running')).not.toBeNull();
        expect(screen.getByRole('button', { name: 'Install' })).not.toBeNull();
    });

    it('turns automatic updates off', async () => {
        const { onChange } = renderCard(updateStatus({}));
        await screen.findByText('Up to date');

        await userEvent.click(screen.getByRole('switch', { name: 'Automatic Updates' }));
        expect(onChange).toHaveBeenCalledWith({ autoUpdate: false });
    });

    it('links the download when this installation cannot update itself', async () => {
        const reason = 'Only installed builds of REFEREE can update themselves.';
        const { api } = renderCard({ ...AVAILABLE, unsupportedReason: reason });

        await userEvent.click(await screen.findByRole('button', { name: 'Download' }));
        expect(api.openExternal).toHaveBeenCalledWith(AVAILABLE.releaseUrl);
        expect(screen.queryByRole('button', { name: 'Update' })).toBeNull();
        expect(screen.getByText(reason)).not.toBeNull();
        await waitFor(() => {
            expect((screen.getByRole('switch', { name: 'Automatic Updates' }) as HTMLInputElement).disabled).toBe(true);
        });
    });
});

describe('checkedAgo', () => {
    const now = Date.UTC(2026, 9, 15, 12, 0);

    it('describes how long ago the last check ran', () => {
        expect(checkedAgo(now - 20_000, now)).toBe('checked just now');
        expect(checkedAgo(now - 5 * 60_000, now)).toBe('checked 5 min ago');
        expect(checkedAgo(now - 3 * 60 * 60_000, now)).toBe('checked 3 h ago');
        expect(checkedAgo(now - 3 * 24 * 60 * 60_000, now)).toMatch(/^checked \S+/);
    });
});
