import type { ReactNode } from 'react';

import type { AppView } from '@/lib/types';

interface AppShellProps {
    view: AppView;
    titleBar: ReactNode;
    children: ReactNode;
    overlay?: ReactNode;
    debugAccessEnabled?: boolean;
}

export function AppShell({ view, titleBar, children, overlay, debugAccessEnabled = false }: AppShellProps) {
    return (
        <div
            id="app-shell"
            data-view={view}
            data-debug-access={debugAccessEnabled ? 'true' : 'false'}
            className="relative flex h-[500px] w-[300px] flex-col overflow-hidden rounded-[8px] border border-[#282828] bg-referee-black font-sans text-white shadow-2xl select-none"
        >
            {titleBar}
            <div className="relative flex-1 overflow-hidden">
                {children}
                {overlay}
            </div>
        </div>
    );
}
