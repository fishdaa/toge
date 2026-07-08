import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/svelte'
import StatusBar from '@/components/StatusBar.svelte'
import { invoke } from '@tauri-apps/api/core'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

describe('StatusBar', () => {
  beforeEach(() => {
    vi.resetModules()
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
})
