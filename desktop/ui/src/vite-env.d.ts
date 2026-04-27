/// <reference types="vite/client" />

import type { RendererApi } from '@/lib/types';

declare global {
    interface Window {
        referee?: RendererApi;
    }
}

export {};
