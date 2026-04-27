import { describe, it, expect } from 'vitest'
import { decorateStreamMarkup, ACTIVE_STREAM_MARKUP } from './active-stream-markup'

// ─── Helpers ──────────────────────────────────────────────────────────────────

function makeSvgContainer(paths: Array<{ fill?: string; inlineStyle?: string }>): Element {
    const container = document.createElement('div')
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
    for (const def of paths) {
        const path = document.createElementNS('http://www.w3.org/2000/svg', 'path') as SVGPathElement
        if (def.fill) {
            path.setAttribute('fill', def.fill)
        }
        if (def.inlineStyle) {
            path.setAttribute('style', def.inlineStyle)
        }
        svg.appendChild(path)
    }
    container.appendChild(svg)
    return container
}

// ─── ACTIVE_STREAM_MARKUP constant ────────────────────────────────────────────

describe('ACTIVE_STREAM_MARKUP', () => {
    it('is a non-empty string', () => {
        expect(typeof ACTIVE_STREAM_MARKUP).toBe('string')
        expect(ACTIVE_STREAM_MARKUP.length).toBeGreaterThan(0)
    })

    it('does not contain an XML declaration', () => {
        expect(ACTIVE_STREAM_MARKUP).not.toMatch(/^<\?xml/)
    })

    it('does not contain bx:export metadata', () => {
        expect(ACTIVE_STREAM_MARKUP).not.toContain('bx:export')
    })
})

// ─── decorateStreamMarkup ─────────────────────────────────────────────────────

describe('decorateStreamMarkup', () => {
    it('does not throw when container is null', () => {
        expect(() => decorateStreamMarkup(null)).not.toThrow()
    })

    it('does not throw when container has no SVG child', () => {
        expect(() => decorateStreamMarkup(document.createElement('div'))).not.toThrow()
    })

    it('marks white (#ffffff) paths as whistle signals', () => {
        const container = makeSvgContainer([{ fill: '#ffffff' }])
        decorateStreamMarkup(container)
        const path = container.querySelector('path')!
        expect(path.getAttribute('data-whistle-signal')).toBe('true')
    })

    it('assigns incrementing --signal-index and animation-delay to signal paths', () => {
        const container = makeSvgContainer([
            { fill: '#ffffff' },
            { fill: '#ffffff' },
            { fill: '#3a3a3a' },
            { fill: '#ffffff' },
        ])
        decorateStreamMarkup(container)
        const paths = container.querySelectorAll('path')
        expect(paths[0].style.getPropertyValue('--signal-index')).toBe('0')
        expect(paths[0].style.getPropertyValue('animation-delay')).toBe('0ms')
        expect(paths[1].style.getPropertyValue('--signal-index')).toBe('1')
        expect(paths[1].style.getPropertyValue('animation-delay')).toBe('140ms')
        expect(paths[3].style.getPropertyValue('--signal-index')).toBe('2')
        expect(paths[3].style.getPropertyValue('animation-delay')).toBe('280ms')
    })

    it('marks #3a3a3a paths with data-whistle-tone="core"', () => {
        const container = makeSvgContainer([{ fill: '#3a3a3a' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-tone')).toBe('core')
    })

    it('marks #100f0f paths with data-whistle-tone="detail"', () => {
        const container = makeSvgContainer([{ fill: '#100f0f' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-tone')).toBe('detail')
    })

    it('marks all other paths with data-whistle-tone="shell"', () => {
        const container = makeSvgContainer([{ fill: '#ff6b35' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-tone')).toBe('shell')
    })

    it('marks paths with no fill attribute as shell', () => {
        const container = makeSvgContainer([{}])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-tone')).toBe('shell')
    })

    it('clears previous decoration attributes before re-decorating', () => {
        const container = makeSvgContainer([{ fill: '#ffffff' }])
        decorateStreamMarkup(container)
        // Change fill and redecorate — old attributes must be cleared
        const path = container.querySelector('path')!
        path.setAttribute('fill', '#3a3a3a')
        decorateStreamMarkup(container)
        expect(path.getAttribute('data-whistle-signal')).toBeNull()
        expect(path.style.getPropertyValue('--signal-index')).toBe('')
        expect(path.getAttribute('data-whistle-tone')).toBe('core')
    })

    it('prefers inline style fill over attribute fill', () => {
        // Inline style fill is white; attribute fill is a non-signal colour
        const container = makeSvgContainer([{ fill: '#3a3a3a', inlineStyle: 'fill: #ffffff' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-signal')).toBe('true')
    })

    it('normalizes rgb() inline style fill to hex before matching', () => {
        const container = makeSvgContainer([{ inlineStyle: 'fill: rgb(255, 255, 255)' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-signal')).toBe('true')
    })

    it('normalizes rgb() fill for a core-tone path', () => {
        // #3a3a3a = rgb(58, 58, 58)
        const container = makeSvgContainer([{ inlineStyle: 'fill: rgb(58, 58, 58)' }])
        decorateStreamMarkup(container)
        expect(container.querySelector('path')!.getAttribute('data-whistle-tone')).toBe('core')
    })
})
