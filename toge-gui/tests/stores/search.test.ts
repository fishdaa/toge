import { describe, it, expect, vi, beforeEach } from 'vitest'
import { get } from 'svelte/store'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

beforeEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  localStorage.clear()

  const s = await import('$lib/searchStore')
  s.clearSearch()
  s.daemonStatus.set(null)
  s.diagnosticsLog.set([])
  s.copyFeedback.set(false)
  s.reindexing.set(false)
  s.sortColumn.set('name')
  s.sortDirection.set('asc')
  s.setTableColumnWidths([220, 320, 88, 140])
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
    expect(get(s.sortColumn)).toBe('name')
    expect(get(s.sortDirection)).toBe('asc')
    expect(get(s.tableColumnWidths)).toEqual([220, 320, 88, 140])
  })

  it('persists table UI state without persisting data', async () => {
    const s = await loadStore()
    s.sortColumn.set('size')
    s.sortDirection.set('desc')
    s.setTableColumnWidths([240, 280, 100, 160])
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
    ])

    vi.resetModules()
    const reloaded = await import('$lib/searchStore')

    expect(get(reloaded.sortColumn)).toBe('size')
    expect(get(reloaded.sortDirection)).toBe('desc')
    expect(get(reloaded.tableColumnWidths)).toEqual([240, 280, 100, 160])
    expect(get(reloaded.results)).toEqual([])
  })

  it('clears search state', async () => {
    const s = await loadStore()
    s.query.set('test')
    s.results.set([{
      path: '/test', name: 'test.txt', parent: '/', extension: 'txt',
      is_dir: false, size_bytes: 10, modified_unix: 1704067200
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
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 20, modified_unix: 0 }
    ])
    s.selectedIndex.set(0)

    s.selectNext()

    expect(get(s.selectedIndex)).toBe(1)
  })

  it('does not select past last row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
    ])
    s.selectedIndex.set(0)

    s.selectNext()

    expect(get(s.selectedIndex)).toBe(0)
  })

  it('selects previous row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 20, modified_unix: 0 }
    ])
    s.selectedIndex.set(1)

    s.selectPrevious()

    expect(get(s.selectedIndex)).toBe(0)
  })

  it('does not select before first row', async () => {
    const s = await loadStore()
    s.results.set([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
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
      watcher_log: [],
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

  it('runs watcher self-test and appends returned watcher events to diagnostics log', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke).mockResolvedValue({
      passed: true,
      summary: 'Watcher self-test passed',
      events: [
        'create /tmp/toge-watcher-self-test/watcher-self-test.mkv',
        'delete /tmp/toge-watcher-self-test/watcher-self-test.mkv'
      ]
    })

    const result = await s.runWatcherSelfTest()
    const log = get(s.diagnosticsLog)

    expect(invoke).toHaveBeenCalledWith('run_watcher_self_test')
    expect(result).toEqual({
      passed: true,
      summary: 'Watcher self-test passed',
      events: [
        'create /tmp/toge-watcher-self-test/watcher-self-test.mkv',
        'delete /tmp/toge-watcher-self-test/watcher-self-test.mkv'
      ]
    })
    expect(log.some((entry) => entry.endsWith('Watcher self-test started'))).toBe(true)
    expect(log.some((entry) => entry.endsWith('Watcher self-test passed'))).toBe(true)
    expect(
      log.some((entry) =>
        entry.endsWith('watcher-self-test: create /tmp/toge-watcher-self-test/watcher-self-test.mkv'))
    ).toBe(true)
    expect(
      log.some((entry) =>
        entry.endsWith('watcher-self-test: delete /tmp/toge-watcher-self-test/watcher-self-test.mkv'))
    ).toBe(true)
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

    await vi.advanceTimersByTimeAsync(299)
    expect(invoke).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(1)
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledWith('search_query', {
      query: 'foo sort:name'
    })

    vi.useRealTimers()
  })

  it('ignores stale search responses when a newer search finishes later', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    const first = deferred<{ rows: never[]; total_count: number; total_size: number; size_indexed: boolean }>()
    const second = deferred<{ rows: { path: string; name: string; parent: string; extension: string; is_dir: boolean; size_bytes: number; modified_unix: number }[]; total_count: number; total_size: number; size_indexed: boolean }>()

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
        size_bytes: 1,
        modified_unix: 0
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
      size_bytes: 1,
      modified_unix: 0
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

  it('clears the committed query when a debounced search becomes empty', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke).mockResolvedValueOnce({
      rows: [
        { path: '/movie.mkv', name: 'movie.mkv', parent: '/', extension: 'mkv', is_dir: false, size_bytes: 1, modified_unix: 0 }
      ],
      total_count: 1,
      total_size: 1,
      size_indexed: true
    })

    await s.search('.mkv')
    expect(get(s.query)).toBe('.mkv')
    expect(get(s.results)).toHaveLength(1)

    await s.search('')
    expect(get(s.query)).toBe('')
    expect(get(s.results)).toEqual([])
    expect(get(s.statusText)).toBe('Ready')
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
        watcher_log: [],
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

  it('refreshes an active extension-filtered search when daemon status reports index changes', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke)
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 1 entries',
        indexed_count: 1,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: ['[1700000000] create /downloads/movie.mkv'],
        last_updated_unix: 1700000000,
        build_duration_ms: 10
      })
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 2 entries',
        indexed_count: 2,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: ['[1700000001] move /downloads/movie.part -> /downloads/movie.mkv'],
        last_updated_unix: 1700000001,
        build_duration_ms: 12
      })
      .mockResolvedValueOnce({
        rows: [{
          path: '/downloads/movie.mkv',
          name: 'movie.mkv',
          parent: '/downloads',
          extension: 'mkv',
          is_dir: false,
          size_bytes: 1024 * 1024 * 1024,
          modified_unix: 0
        }],
        total_count: 1,
        total_size: 1024,
        size_indexed: true
      })

    s.query.set('.mkv')

    await s.fetchStatus()
    await s.fetchStatus()
    await Promise.resolve()

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['get_status'],
      ['get_status'],
      ['search_query', {
        query: '.mkv sort:name',
        
      }]
    ])
    expect(get(s.results)).toEqual([{
      path: '/downloads/movie.mkv',
      name: 'movie.mkv',
      parent: '/downloads',
      extension: 'mkv',
      is_dir: false,
      size_bytes: 1024 * 1024 * 1024,
      modified_unix: 0
    }])
  })

  it.each([
    {
      query: '.mp4',
      row: {
        path: '/downloads/clip.mp4',
        name: 'clip.mp4',
        parent: '/downloads',
        extension: 'mp4',
        is_dir: false,
        size_bytes: 512 * 1024 * 1024,
        modified_unix: 0
      }
    },
    {
      query: '.txt',
      row: {
        path: '/downloads/notes.txt',
        name: 'notes.txt',
        parent: '/downloads',
        extension: 'txt',
        is_dir: false,
        size_bytes: 4 * 1024,
        modified_unix: 0
      }
    },
    {
      query: '.zip',
      row: {
        path: '/downloads/archive.zip',
        name: 'archive.zip',
        parent: '/downloads',
        extension: 'zip',
        is_dir: false,
        size_bytes: 128 * 1024 * 1024,
        modified_unix: 0
      }
    }
  ])('refreshes active search for $query when daemon status changes', async ({ query, row }) => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke)
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 10 entries',
        indexed_count: 10,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: [],
        last_updated_unix: 1700000100,
        build_duration_ms: 10
      })
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 11 entries',
        indexed_count: 11,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: [],
        last_updated_unix: 1700000101,
        build_duration_ms: 12
      })
      .mockResolvedValueOnce({
        rows: [row],
        total_count: 1,
        total_size: 1024,
        size_indexed: true
      })

    s.query.set(query)

    await s.fetchStatus()
    await s.fetchStatus()
    await Promise.resolve()

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['get_status'],
      ['get_status'],
      ['search_query', {
        query: `${query} sort:name`,
        
      }]
    ])
    expect(get(s.results)).toEqual([row])
  })

  it('does not refresh search when daemon status is unchanged', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')

    vi.mocked(invoke)
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 1 entries',
        indexed_count: 1,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: [],
        last_updated_unix: 1700000000,
        build_duration_ms: 10
      })
      .mockResolvedValueOnce({
        status: 'Ready',
        status_message: 'Indexed 1 entries',
        indexed_count: 1,
        size_indexed: true,
        watcher_healthy: true,
        watched_dir_count: 5,
        watch_failure_count: 0,
        watch_overflow_count: 0,
        watcher_log: [],
        last_updated_unix: 1700000000,
        build_duration_ms: 10
      })

    s.query.set('.mkv')

    await s.fetchStatus()
    await s.fetchStatus()

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['get_status'],
      ['get_status']
    ])
  })
})
