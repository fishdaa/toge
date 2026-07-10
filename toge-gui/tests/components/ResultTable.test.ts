import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/svelte'
import ResultTable from '@/components/ResultTable.svelte'
import { setResults, setTableColumnWidths, state, setSelectedIndex } from '$lib/searchStore'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

describe('ResultTable', () => {
  beforeEach(() => {
    vi.resetModules()
    setResults([])
    setSelectedIndex(-1)
    setTableColumnWidths([220, 320, 88, 140])
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

  it('starts the scroll viewport below the table header', () => {
    const { container } = render(ResultTable)
    const headerViewport = container.querySelector('.table-header-viewport')
    const header = container.querySelector('.table-header')
    const scrollViewport = container.querySelector('.table-scroll')

    expect(headerViewport).toBeTruthy()
    expect(scrollViewport).toBeTruthy()
    expect(scrollViewport?.contains(header)).toBe(false)
    expect(headerViewport?.nextElementSibling).toBe(scrollViewport)
  })

  it('prevents the native context menu when opening the custom row menu', async () => {
    setResults([
      {
        path: '/tmp/demo.mkv',
        name: 'demo.mkv',
        parent: '/tmp',
        extension: 'mkv',
        is_dir: false,
        size_bytes: 0,
        modified_unix: 1751846400
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
    expect(state.selectedIndex).toBe(0)
  })
})
