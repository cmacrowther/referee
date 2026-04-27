import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { HardwareDetectionCard } from './HardwareDetectionCard'
import { DEFAULT_STATUS } from '@/lib/types'
import type { Status } from '@/lib/types'

function makeStatus(overrides: Partial<Status> = {}): Status {
    return { ...DEFAULT_STATUS, ...overrides }
}

describe('HardwareDetectionCard — GPU name', () => {
    it('shows the gpuName when set', () => {
        render(<HardwareDetectionCard status={makeStatus({ gpuName: 'RTX 4080' })} />)
        expect(screen.getByText('RTX 4080')).not.toBeNull()
    })

    it('shows fallback text when gpuName is null', () => {
        render(<HardwareDetectionCard status={makeStatus({ gpuName: null })} />)
        expect(screen.getByText('No supported GPU detected')).not.toBeNull()
    })
})

describe('HardwareDetectionCard — vendor', () => {
    it('shows formatted vendor for nvidia', () => {
        render(<HardwareDetectionCard status={makeStatus({ gpuVendor: 'nvidia' })} />)
        expect(screen.getByText('NVIDIA®')).not.toBeNull()
    })

    it('shows formatted vendor for amd', () => {
        render(<HardwareDetectionCard status={makeStatus({ gpuVendor: 'amd' })} />)
        expect(screen.getByText('AMD®')).not.toBeNull()
    })

    it('shows "Unknown" when vendor is null', () => {
        render(<HardwareDetectionCard status={makeStatus({ gpuVendor: null })} />)
        expect(screen.getByText('Unknown')).not.toBeNull()
    })
})

describe('HardwareDetectionCard — encoder backend', () => {
    it('shows formatted encoder for nvenc', () => {
        render(<HardwareDetectionCard status={makeStatus({ encoderBackend: 'nvenc' })} />)
        expect(screen.getByText('NVENC')).not.toBeNull()
    })

    it('shows formatted encoder for amf', () => {
        render(<HardwareDetectionCard status={makeStatus({ encoderBackend: 'amf' })} />)
        expect(screen.getByText('AMF')).not.toBeNull()
    })
})

describe('HardwareDetectionCard — section title', () => {
    it('renders the "Hardware Detection" title', () => {
        render(<HardwareDetectionCard status={makeStatus()} />)
        expect(screen.getByText('Hardware Detection')).not.toBeNull()
    })
})
