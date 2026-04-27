import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { AppShell } from './AppShell'

describe('AppShell', () => {
    it('renders the titleBar slot', () => {
        render(
            <AppShell view="status" titleBar={<div>title</div>}>
                <div />
            </AppShell>
        )
        expect(screen.getByText('title')).not.toBeNull()
    })

    it('renders children', () => {
        render(
            <AppShell view="status" titleBar={<div />}>
                <div>body content</div>
            </AppShell>
        )
        expect(screen.getByText('body content')).not.toBeNull()
    })

    it('renders the overlay slot when provided', () => {
        render(
            <AppShell view="status" titleBar={<div />} overlay={<div>overlay</div>}>
                <div />
            </AppShell>
        )
        expect(screen.getByText('overlay')).not.toBeNull()
    })

    it('sets data-view attribute to the current view', () => {
        const { container } = render(
            <AppShell view="settings" titleBar={<div />}>
                <div />
            </AppShell>
        )
        expect(container.querySelector('#app-shell')?.getAttribute('data-view')).toBe('settings')
    })

    it('sets data-debug-access to "false" by default', () => {
        const { container } = render(
            <AppShell view="status" titleBar={<div />}>
                <div />
            </AppShell>
        )
        expect(container.querySelector('#app-shell')?.getAttribute('data-debug-access')).toBe('false')
    })

    it('sets data-debug-access to "true" when debugAccessEnabled', () => {
        const { container } = render(
            <AppShell view="status" titleBar={<div />} debugAccessEnabled>
                <div />
            </AppShell>
        )
        expect(container.querySelector('#app-shell')?.getAttribute('data-debug-access')).toBe('true')
    })
})
