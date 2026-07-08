import { describe, it, expect } from 'vitest'

describe('formatSize', () => {
  function formatSize(bytes: number): string {
    if (bytes === 0) return '0 B'
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
    let value = bytes
    let unitIdx = 0
    while (value >= 1024 && unitIdx + 1 < units.length) {
      value /= 1024
      unitIdx++
    }
    return `${value.toFixed(1)} ${units[unitIdx]}`
  }

  it('formats 0 bytes', () => {
    expect(formatSize(0)).toBe('0 B')
  })

  it('formats bytes', () => {
    expect(formatSize(512)).toBe('512.0 B')
  })

  it('formats kilobytes', () => {
    expect(formatSize(1024)).toBe('1.0 KB')
  })

  it('formats megabytes', () => {
    expect(formatSize(1024 * 1024)).toBe('1.0 MB')
  })

  it('formats gigabytes', () => {
    expect(formatSize(1024 * 1024 * 1024)).toBe('1.0 GB')
  })

  it('formats terabytes', () => {
    expect(formatSize(1024 * 1024 * 1024 * 1024)).toBe('1.0 TB')
  })
})
