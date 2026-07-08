import { describe, it, expect, vi, beforeEach } from 'vitest'
import { get } from 'svelte/store'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

beforeEach(() => {
  vi.resetModules()
})

describe('searchStore', () => {
  async function loadStore() {
    const mod = await import('$lib/searchStore')
    return mod
  }

  function deferred<T>() {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((res) => {
      resolve = res
    })
    return { promise, resolve }
  }

  it('initializes with default state', async () => {
    const s = await loadStore()
    expect(get(s.query)).toBe('')
    expect(get(s.results)).toEqual([])
    expect(get(s.isLoading)).toBe(false)
    expect(get(s.statusText)).toBe('Ready')
    expect(get(s.selectedIndex)).toBe(-1)
  })

  it('clears search state', async () => {
    const s = await loadStore()
    s.query.set('test')
    s.results.set([{
      path: '/test', name: 'test.txt', parent: '/', extension: 'txt',
      is_dir: false, size: '10 B', modified: '2024-01-01'
    }])
    s.selectedIndex.set(0)

    s.clearSearch()

    expect(get(s.query)).toBe('')
    expect(get(s.results)).toEqual([])
    expect(get(s.selectedIndex)).toBe(-1)
  })

  it('selects next row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size: '10 B', modified: '' },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size: '20 B', modified: '' }
    ])
    s.selectedIndex.set(0)

    s.selectNext()

    expect(get(s.selectedIndex)).toBe(1)
  })

  it('does not select past last row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size: '10 B', modified: '' }
    ])
    s.selectedIndex.set(0)

    s.selectNext()

    expect(get(s.selectedIndex)).toBe(0)
  })

  it('selects previous row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size: '10 B', modified: '' },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size: '20 B', modified: '' }
    ])
    s.selectedIndex.set(1)

    s.selectPrevious()

    expect(get(s.selectedIndex)).toBe(0)
  })

  it('does not select before first row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size: '10 B', modified: '' }
    ])
    s.selectedIndex.set(0)

    s.selectPrevious()

    expect(get(s.selectedIndex)).toBe(0)
  })

  it('formats size correctly', async () => {
    const s = await loadStore()
    expect(s.formatSize(0)).toBe('0 B')
    expect(s.formatSize(1024)).toBe('1.0 KB')
    expect(s.formatSize(1024 * 1024)).toBe('1.0 MB')
  })

  it('derives index status text from daemon status', async () => {
    const s = await loadStore()

    expect(get(s.indexStatusText)).toBe('Index unavailable')

    s.daemonStatus.set({
      status: 'Ready',
      status_message: 'Indexed 42 entries',
      indexed_count: 42,
      size_indexed: false,
      watcher_healthy: true,
      watched_dir_count: 3,
      watch_failure_count: 0,
      watch_overflow_count: 0,
      last_updated_unix: 1700000000,
      build_duration_ms: 15
    })

    expect(get(s.indexStatusText)).toBe('Ready | Indexed 42 entries | 42 indexed')
  })

  it('copies the diagnostics log as a single payload', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockResolvedValue(undefined)

    s.diagnosticsLog.set(['[10:00:00] first', '[10:00:01] second'])
    await s.copyDiagnosticsLog()

    expect(invoke).toHaveBeenCalledWith('copy_to_clipboard', {
      text: '[10:00:00] first\n[10:00:01] second'
    })
  })

  it('debounces search calls while typing', async () => {
    vi.useFakeTimers()

    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockResolvedValue({
      rows: [],
      total_count: 0,
      total_size: 0,
      size_indexed: true
    })
    vi.mocked(invoke).mockClear()

    s.query.set('f')
    s.debouncedSearch()
    s.query.set('fo')
    s.debouncedSearch()
    s.query.set('foo')
    s.debouncedSearch()

    expect(invoke).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(249)
    expect(invoke).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('search_query', {
      query: 'foo sort:name',
      maxResults: 10000
    })

    vi.useRealTimers()
  })

  it('ignores stale search responses when a newer search finishes later', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    const first = deferred<{ rows: never[]; total_count: number; total_size: number }>()
    const second = deferred<{ rows: { path: string; name: string; parent: string; extension: string; is_dir: boolean; size: string; modified: string }[]; total_count: number; total_size: number }>()

    vi.mocked(invoke)
      .mockReturnValueOnce(first.promise as ReturnType<typeof invoke>)
      .mockReturnValueOnce(second.promise as ReturnType<typeof invoke>)

    s.query.set('old')
    s.search()
    s.query.set('new')
    s.search()

    second.resolve({
      rows: [{
        path: '/new',
        name: 'new.txt',
        parent: '/',
        extension: 'txt',
        is_dir: false,
        size: '1 B',
        modified: ''
      }],
      total_count: 1,
      total_size: 1,
      size_indexed: true
    })
    await Promise.resolve()

    first.resolve({
      rows: [],
      total_count: 99,
      total_size: 999,
      size_indexed: true
    })
    await Promise.resolve()
    await Promise.resolve()

    expect(get(s.results)).toEqual([{
      path: '/new',
      name: 'new.txt',
      parent: '/',
      extension: 'txt',
      is_dir: false,
      size: '1 B',
      modified: ''
    }])
    expect(get(s.totalCount)).toBe(1)
    expect(get(s.statusText)).toBe('1 results | 1.0 B')
  })

  it('shows size unavailable when file sizes are not indexed', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke).mockResolvedValue({
      rows: [],
      total_count: 7,
      total_size: 0,
      size_indexed: false
    })

    s.query.set('needle')
    await s.search()

    expect(get(s.statusText)).toBe('7 results | size unavailable')
  })

  it('requests reindex and refreshes daemon status', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke)
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Reindexed 50 entries',
        indexed_count: 50,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        last_updated_unix: 1700000000,
        build_duration_ms: 32
      })
    vi.mocked(invoke).mockClear()

    await s.requestReindex()

    expect(vi.mocked(invoke).mock.calls[0]).toEqual(['reindex_index'])
    expect(vi.mocked(invoke).mock.calls[1]).toEqual(['get_status'])
    expect(get(s.reindexing)).toBe(false)
    expect(get(s.daemonStatus)?.status_message).toBe('Reindexed 50 entries')
  })
})
