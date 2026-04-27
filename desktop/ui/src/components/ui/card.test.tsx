import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Card, CardHeader, CardTitle, CardDescription, CardContent, CardAction } from './card'

describe('Card family', () => {
    it('Card renders children', () => {
        render(<Card>card body</Card>)
        expect(screen.getByText('card body')).not.toBeNull()
    })

    it('CardHeader renders children', () => {
        render(<CardHeader>header</CardHeader>)
        expect(screen.getByText('header')).not.toBeNull()
    })

    it('CardTitle renders as an h3', () => {
        const { container } = render(<CardTitle>My Title</CardTitle>)
        expect(container.querySelector('h3')?.textContent).toBe('My Title')
    })

    it('CardDescription renders as a paragraph', () => {
        const { container } = render(<CardDescription>desc</CardDescription>)
        expect(container.querySelector('p')?.textContent).toBe('desc')
    })

    it('CardContent renders children', () => {
        render(<CardContent>content</CardContent>)
        expect(screen.getByText('content')).not.toBeNull()
    })

    it('CardAction renders children', () => {
        render(<CardAction>action</CardAction>)
        expect(screen.getByText('action')).not.toBeNull()
    })

    it('Card merges className', () => {
        const { container } = render(<Card className="extra">X</Card>)
        expect(container.querySelector('div')?.className).toContain('extra')
    })
})
