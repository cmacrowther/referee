import { useCallback, useEffect, useState } from 'react';

import { type AppView, type RendererApi } from '@/lib/types';

function normalizeView(value?: string | null): AppView {
    if (value === 'settings' || value === 'setup') {
        return value;
    }

    return 'status';
}

function getHashView(): AppView {
    if (typeof window === 'undefined') {
        return 'status';
    }

    return normalizeView(window.location.hash.replace('#', ''));
}

export function useViewRouter(api: RendererApi) {
    const [currentView, setCurrentView] = useState<AppView>(getHashView);

    useEffect(() => {
        const handleHashChange = () => {
            const nextView = getHashView();
            if (nextView !== 'setup') {
                setCurrentView(nextView);
            }
        };

        window.addEventListener('hashchange', handleHashChange);
        return () => {
            window.removeEventListener('hashchange', handleHashChange);
        };
    }, []);

    useEffect(() => {
        return api.onNavigateView(view => {
            setCurrentView(normalizeView(view));
        });
    }, [api]);

    useEffect(() => {
        if (typeof window === 'undefined' || currentView === 'setup') {
            return;
        }

        const targetHash = currentView === 'settings' ? '#settings' : '';
        if (window.location.hash !== targetHash) {
            const targetUrl = `${window.location.pathname}${window.location.search}${targetHash}`;
            window.history.replaceState(null, '', targetUrl);
        }
    }, [currentView]);

    const setView = useCallback((view: AppView) => {
        setCurrentView(normalizeView(view));
    }, []);

    return {
        currentView,
        setView
    };
}
