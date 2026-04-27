import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { StreamPlayerWindow } from '@/components/player/StreamPlayerWindow';
import '@/globals.css';

const rootElement = document.getElementById('root');

if (!rootElement) {
    throw new Error('Failed to find root element for the stream player renderer.');
}

createRoot(rootElement).render(
    <StrictMode>
        <StreamPlayerWindow />
    </StrictMode>
);
