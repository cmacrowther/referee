import { describe, it, expect } from 'vitest'
import { cn } from './utils'

describe('cn', () => {
    it('returns a single class unchanged', () => {
        expect(cn('foo')).toBe('foo')
    })

    it('merges multiple classes', () => {
        expect(cn('foo', 'bar')).toBe('foo bar')
    })

    it('deduplicates conflicting Tailwind classes, keeping the last', () => {
        expect(cn('p-2', 'p-4')).toBe('p-4')
    })

    it('ignores falsy values', () => {
        expect(cn('foo', false, undefined, null, 'bar')).toBe('foo bar')
    })

    it('handles conditional objects', () => {
        expect(cn({ 'text-red-500': true, 'text-blue-500': false })).toBe('text-red-500')
    })

    it('returns empty string when given no arguments', () => {
        expect(cn()).toBe('')
    })
})
