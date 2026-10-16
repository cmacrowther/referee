import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { EasyProxyCard } from './EasyProxyCard';
import { DEFAULT_SETTINGS, type ManagedEasyProxyStatus, type Settings } from '@/lib/types';
import { makeMockApi } from '@/test/mocks';

const MANAGED_SETTINGS: Settings = {
    ...DEFAULT_SETTINGS,
    easyProxy: { enabled: true, mode: 'managed', url: null, apiPassword: null },
};

const EXTERNAL_SETTINGS: Settings = {
    ...DEFAULT_SETTINGS,
    easyProxy: { enabled: true, mode: 'external', url: null, apiPassword: null },
};

function managedStatus(overrides: Partial<ManagedEasyProxyStatus>): ManagedEasyProxyStatus {
    return {
        installState: 'missing',
        installing: false,
        progress: null,
        error: null,
        running: false,
        starting: false,
        version: '2.13.35',
        commit: '1d4ad76',
        ...overrides,
    };
}

describe('EasyProxyCard', () => {
    it('is off by default and turns on in managed mode', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(<EasyProxyCard api={api} settings={DEFAULT_SETTINGS} locked={false} onChange={onChange} />);

        const toggle = screen.getByRole('switch', { name: 'EasyProxy' }) as HTMLInputElement;
        expect(toggle.checked).toBe(false);
        expect(screen.queryByRole('radiogroup')).toBeNull();

        await userEvent.click(toggle);
        expect(onChange).toHaveBeenCalledWith({
            easyProxy: { enabled: true, mode: 'managed', url: null, apiPassword: null },
        });
    });

    it('offers to install the managed server when it is missing', async () => {
        const { api } = makeMockApi();

        render(<EasyProxyCard api={api} settings={MANAGED_SETTINGS} locked={false} onChange={vi.fn()} />);

        await screen.findByText('Downloads Python and EasyProxy, about 230 MB installed.');
        await userEvent.click(screen.getByRole('button', { name: 'Install' }));
        expect(api.installEasyProxy).toHaveBeenCalled();
    });

    it('shows install progress', async () => {
        const { api } = makeMockApi();
        vi.mocked(api.getEasyProxyStatus).mockResolvedValue(managedStatus({
            installing: true,
            progress: { phase: 'easyproxy', percent: 58, detail: 'Installing Python packages (12/40)...' },
        }));

        render(<EasyProxyCard api={api} settings={MANAGED_SETTINGS} locked={false} onChange={vi.fn()} />);

        const bar = await screen.findByRole('progressbar', { name: 'EasyProxy install' });
        expect(bar.getAttribute('aria-valuenow')).toBe('58');
        expect(screen.getByText('Installing Python packages (12/40)...')).not.toBeNull();
    });

    it('reports a failed install with a retry', async () => {
        const { api } = makeMockApi();
        vi.mocked(api.getEasyProxyStatus).mockResolvedValue(managedStatus({ error: 'Python download failed' }));

        render(<EasyProxyCard api={api} settings={MANAGED_SETTINGS} locked={false} onChange={vi.fn()} />);

        expect((await screen.findByRole('alert')).textContent).toBe('Python download failed');
        await userEvent.click(screen.getByRole('button', { name: 'Retry' }));
        expect(api.installEasyProxy).toHaveBeenCalled();
    });

    it('tests and removes an installed managed server', async () => {
        const { api } = makeMockApi();
        vi.mocked(api.getEasyProxyStatus).mockResolvedValue(managedStatus({ installState: 'current' }));
        vi.mocked(api.checkEasyProxy).mockResolvedValueOnce({ version: '2.13.35' });

        render(<EasyProxyCard api={api} settings={MANAGED_SETTINGS} locked={false} onChange={vi.fn()} />);

        await screen.findByText('EasyProxy 2.13.35');
        expect(screen.getByText(/Stopped until a stream needs it/)).not.toBeNull();

        await userEvent.click(screen.getByRole('button', { name: 'Test' }));
        expect(api.checkEasyProxy).toHaveBeenCalledWith('managed', null, null);
        await waitFor(() => {
            expect(screen.getByRole('status').textContent).toBe('Connected to EasyProxy 2.13.35');
        });

        await userEvent.click(screen.getByRole('button', { name: 'Remove' }));
        expect(api.removeEasyProxy).toHaveBeenCalled();
    });

    it('switches to an external server', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(<EasyProxyCard api={api} settings={MANAGED_SETTINGS} locked={false} onChange={onChange} />);
        await userEvent.click(screen.getByRole('radio', { name: /External server/ }));

        expect(onChange).toHaveBeenCalledWith({
            easyProxy: { enabled: true, mode: 'external', url: null, apiPassword: null },
        });
    });

    it('saves external server fields on blur, not per keystroke', async () => {
        const { api } = makeMockApi();
        const onChange = vi.fn();

        render(<EasyProxyCard api={api} settings={EXTERNAL_SETTINGS} locked={false} onChange={onChange} />);
        expect(api.getEasyProxyStatus).not.toHaveBeenCalled();

        const urlInput = screen.getByLabelText('Server URL');
        expect(urlInput.getAttribute('placeholder')).toBe('http://127.0.0.1:7860');
        await userEvent.type(urlInput, '  http://media-box:7860 ');
        expect(onChange).not.toHaveBeenCalled();

        await userEvent.click(screen.getByLabelText('API Password'));
        expect(onChange).toHaveBeenLastCalledWith({
            easyProxy: { enabled: true, mode: 'external', url: 'http://media-box:7860', apiPassword: null },
        });

        await userEvent.type(screen.getByLabelText('API Password'), 'ep{Enter}');
        expect(onChange).toHaveBeenLastCalledWith({
            easyProxy: { enabled: true, mode: 'external', url: null, apiPassword: 'ep' },
        });
    });

    it('tests the external values being edited', async () => {
        const { api } = makeMockApi();
        vi.mocked(api.checkEasyProxy).mockRejectedValueOnce('EasyProxy rejected the API password.');

        render(<EasyProxyCard api={api} settings={EXTERNAL_SETTINGS} locked={false} onChange={vi.fn()} />);

        await userEvent.type(screen.getByLabelText('API Password'), 'ep');
        await userEvent.click(screen.getByRole('button', { name: 'Test' }));

        expect(api.checkEasyProxy).toHaveBeenCalledWith('external', null, 'ep');
        await waitFor(() => {
            expect(screen.getByRole('status').textContent).toBe('EasyProxy rejected the API password.');
        });
    });

    it('notes that a relay peer applies its own setting', () => {
        const { api } = makeMockApi();

        render(
            <EasyProxyCard
                api={api}
                settings={EXTERNAL_SETTINGS}
                locked={false}
                relayPeerName="media-box"
                onChange={vi.fn()}
            />
        );

        expect(screen.getByText('Streams relay to media-box, which uses its own EasyProxy setting.')).not.toBeNull();
    });

    it('opens the EasyProxy project page', async () => {
        const { api } = makeMockApi();

        render(<EasyProxyCard api={api} settings={DEFAULT_SETTINGS} locked={false} onChange={vi.fn()} />);
        await userEvent.click(screen.getByRole('button', { name: 'About EasyProxy' }));

        expect(api.openExternal).toHaveBeenCalledWith('https://github.com/realbestia1/EasyProxy');
    });
});
