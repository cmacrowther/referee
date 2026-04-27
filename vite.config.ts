/// <reference types="vitest" />
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

const desktopUiRoot = resolve(__dirname, 'desktop/ui')
const desktopUiSrc = resolve(__dirname, 'desktop/ui/src')

const rendererDebugModulePath = process.env.NODE_ENV === 'production'
    ? resolve(desktopUiSrc, 'dev/renderer-debug.stub.tsx')
    : resolve(desktopUiSrc, 'dev/renderer-debug.tsx')

export default defineConfig({
    root: desktopUiRoot,
    resolve: {
        alias: {
            '@/dev/renderer-debug': rendererDebugModulePath,
            '@': desktopUiSrc
        }
    },
    build: {
        outDir: resolve(desktopUiRoot, 'dist'),
        emptyOutDir: true,
        rollupOptions: {
            input: {
                main: resolve(desktopUiRoot, 'index.html'),
                player: resolve(desktopUiRoot, 'player.html'),
            }
        }
    },
    plugins: [react(), tailwindcss()],
    server: {
        port: 5173,
        strictPort: true
    },
    clearScreen: false,
    test: {
        environment: 'jsdom',
        globals: true,
        setupFiles: ['./src/test/setup.ts'],
        include: ['src/**/*.test.{ts,tsx}'],
        coverage: {
            provider: 'v8',
            include: ['src/**/*.{ts,tsx}'],
            exclude: [
                'src/test/**',
                'src/__mocks__/**',
                'src/main.tsx',
                'src/vite-env.d.ts',
            ],
        },
    },
})
