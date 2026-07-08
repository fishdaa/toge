import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/svelte'
import StatusBar from '@/components/StatusBar.svelte'
import { invoke } from '@tauri-apps/api/core'
import { setResults, setSelectedIndex, setTotalCount, setSizeIndexed } from '$lib/searchStore'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

describe('StatusBar', () => {
  beforeEach(() => {
    vi.resetModules()
    setResults([])
    setSelectedIndex(-1)
    setTotalCount(0)
    setSizeIndexed(false)
    vi.mocked(invoke).mockResolvedValue({
      status: 'Ready',
      status_message: 'Indexed 123 entries',
      indexed_count: 123,
      size_indexed: false,
      watcher_healthy: true,
      watched_dir_count: 4,
      watch_failure_count: 0,
      watch_overflow_count: 0,
      watcher_log: [],
      last_updated_unix: 1700000000,
      build_duration_ms: 12
    })
  })

  it('displays default status text', async () => {
    render(StatusBar)
    expect(screen.getByText('Ready')).toBeTruthy()
  })

  it('shows the index status text', async () => {
    render(StatusBar)
    expect(await screen.findByText(/Ready \| Indexed 123 entries \| 123 indexed/)).toBeTruthy()
  })

  it('shows selected row details instead of generic status text', async () => {
    setSizeIndexed(true)
    setTotalCount(24)
    setSelectedIndex(3)
    setResults([
      { path: '/tmp/a.txt', name: 'a.txt', parent: '/tmp', extension: 'txt', is_dir: false, size_bytes: 1, modified_unix: 0 },
      { path: '/tmp/b.txt', name: 'b.txt', parent: '/tmp', extension: 'txt', is_dir: false, size_bytes: 2, modified_unix: 0 },
      { path: '/tmp/c.txt', name: 'c.txt', parent: '/tmp', extension: 'txt', is_dir: false, size_bytes: 3, modified_unix: 0 },
      { path: '/tmp/notepad.exe', name: 'notepad.exe', parent: '/tmp', extension: 'exe', is_dir: false, size_bytes: 210 * 1024, modified_unix: 1444510800 }
    ])

    render(StatusBar)

    expect(await screen.findByText(/Size: 210.0 KB, Date Modified:/)).toBeTruthy()
    expect(screen.getByText('4 of 24')).toBeTruthy()
  })
})
