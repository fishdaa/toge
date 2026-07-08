import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/svelte'
import { get } from 'svelte/store'
import ResultTable from '@/components/ResultTable.svelte'
import { results, selectedIndex } from '$lib/searchStore'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

describe('ResultTable', () => {
  beforeEach(() => {
    vi.resetModules()
  })

  it('renders empty state when no results', async () => {
    render(ResultTable)
    expect(screen.getByText('No results found')).toBeTruthy()
  })

  it('renders resize handles for adjustable columns', async () => {
    render(ResultTable)
    expect(screen.getByLabelText('Resize NAME column')).toBeTruthy()
    expect(screen.getByLabelText('Resize PATH column')).toBeTruthy()
    expect(screen.getByLabelText('Resize SIZE column')).toBeTruthy()
  })

  it('prevents the native context menu when opening the custom row menu', async () => {
    results.set([
      {
        path: '/tmp/demo.mkv',
        name: 'demo.mkv',
        parent: '/tmp',
        extension: 'mkv',
        is_dir: false,
        size: '0 B',
        modified: '2026-07-07'
      }
    ])

    render(ResultTable)

    const row = screen.getByText('demo.mkv').closest('.table-row')
    expect(row).toBeTruthy()

    const event = new MouseEvent('contextmenu', {
      bubbles: true,
      cancelable: true,
      clientX: 80,
      clientY: 120
    })

    const prevented = !row!.dispatchEvent(event)

    expect(prevented).toBe(true)
    expect(event.defaultPrevented).toBe(true)
    expect(get(selectedIndex)).toBe(0)
  })
})
