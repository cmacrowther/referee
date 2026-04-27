import { vi } from 'vitest'

export const getCurrentWindow = vi.fn(() => ({
    startDragging: vi.fn(),
    minimize: vi.fn(),
    close: vi.fn(),
    setAlwaysOnTop: vi.fn(),
}))
