import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { DebugStreamPanel } from './DebugStreamPanel';
import { makeMockApi } from '@/test/mocks';

describe('DebugStreamPanel', () => {
    beforeEach(() => {
        window.localStorage.clear();
    });

    it('starts a stream for the submitted URL and remembers it', async () => {
        const { api } = makeMockApi();
        const { unmount } = render(<DebugStreamPanel api={api} />);

        await userEvent.type(screen.getByLabelText('Test Stream'), '  https://cdn.example.com/live.m3u8 {Enter}');

        expect(api.startDebugStream).toHaveBeenCalledWith('https://cdn.example.com/live.m3u8');
        unmount();

        render(<DebugStreamPanel api={api} />);
        expect((screen.getByLabelText('Test Stream') as HTMLInputElement).value).toBe('https://cdn.example.com/live.m3u8');
    });

    it('rejects URLs that are not http or https', async () => {
        const { api } = makeMockApi();
        render(<DebugStreamPanel api={api} />);

        await userEvent.type(screen.getByLabelText('Test Stream'), 'ftp://example.com/a.m3u8');
        await userEvent.click(screen.getByRole('button', { name: 'Play' }));

        expect(api.startDebugStream).not.toHaveBeenCalled();
        expect(screen.getByRole('alert').textContent).toBe('Enter an http:// or https:// stream URL.');
    });

    it('disables the form while the stream starts and reports a failed start', async () => {
        const { api } = makeMockApi();
        let rejectStart: (reason: unknown) => void = () => {};
        vi.mocked(api.startDebugStream).mockReturnValueOnce(new Promise((_, reject) => {
            rejectStart = reject;
        }));
        render(<DebugStreamPanel api={api} />);

        await userEvent.type(screen.getByLabelText('Test Stream'), 'https://cdn.example.com/live.m3u8');
        await userEvent.click(screen.getByRole('button', { name: 'Play' }));

        const button = screen.getByRole('button', { name: 'Starting…' }) as HTMLButtonElement;
        expect(button.disabled).toBe(true);
        expect((screen.getByLabelText('Test Stream') as HTMLInputElement).disabled).toBe(true);

        rejectStart('Pipeline timed out waiting for the GPU.');
        await waitFor(() => {
            expect(screen.getByRole('alert').textContent).toBe('Pipeline timed out waiting for the GPU.');
        });
        expect((screen.getByRole('button', { name: 'Play' }) as HTMLButtonElement).disabled).toBe(false);
    });
});
