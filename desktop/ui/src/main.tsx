import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from '@/App';
import '@/globals.css';

const rootElement = document.getElementById('root');

if (!rootElement) {
    throw new Error('Failed to find root element for React renderer.');
}

createRoot(rootElement).render(
    <StrictMode>
        <App />
    </StrictMode>
);
