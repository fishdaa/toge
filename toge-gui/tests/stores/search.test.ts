import { describe, it, expect, vi, beforeEach } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

beforeEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  localStorage.clear()

  const s = await import('$lib/searchStore')
  s.clearSearch()
  s.setDaemonStatus(null)
  s.setDiagnosticsLog([])
  s.setCopyFeedback(false)
  s.setReindexing(false)
  s.setSortColumn('name')
  s.setSortDirection('asc')
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
    expect(s.state.query).toBe('')
    expect(s.state.results).toEqual([])
    expect(s.state.isLoading).toBe(false)
    expect(s.state.hasCompletedSearch).toBe(false)
    expect(s.state.statusText).toBe('Ready')
    expect(s.state.selectedIndex).toBe(-1)
    expect(s.state.sortColumn).toBe('name')
    expect(s.state.sortDirection).toBe('asc')
    expect(s.state.tableColumnWidths).toEqual([220, 320, 88, 140])
  })

  it('persists table UI state without persisting data', async () => {
    const s = await loadStore()
    s.setSortColumn('size')
    s.setSortDirection('desc')
    s.setTableColumnWidths([240, 280, 100, 160])
    s.setResults([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
    ])

    vi.resetModules()
    const reloaded = await import('$lib/searchStore')

    expect(reloaded.state.sortColumn).toBe('size')
    expect(reloaded.state.sortDirection).toBe('desc')
    expect(reloaded.state.tableColumnWidths).toEqual([240, 280, 100, 160])
    expect(reloaded.state.results).toEqual([])
  })

  it('clears search state', async () => {
    const s = await loadStore()
    s.setQuery('test')
    s.setResults([{
      path: '/test', name: 'test.txt', parent: '/', extension: 'txt',
      is_dir: false, size_bytes: 10, modified_unix: 1704067200
    }])
    s.setSelectedIndex(0)

    s.clearSearch()

    expect(s.state.query).toBe('')
    expect(s.state.results).toEqual([])
    expect(s.state.selectedIndex).toBe(-1)
    expect(s.state.hasCompletedSearch).toBe(false)
  })

  it('selects next row', async () => {
    const s = await loadStore()
    s.setResults([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 20, modified_unix: 0 }
    ])
    s.setSelectedIndex(0)

    s.selectNext()

    expect(s.state.selectedIndex).toBe(1)
  })

  it('does not select past last row', async () => {
    const s = await loadStore()
    s.setResults([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
    ])
    s.setSelectedIndex(0)

    s.selectNext()

    expect(s.state.selectedIndex).toBe(0)
  })

  it('selects previous row', async () => {
    const s = await loadStore()
    s.setResults([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 },
      { path: '/b', name: 'b.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 20, modified_unix: 0 }
    ])
    s.setSelectedIndex(1)

    s.selectPrevious()

    expect(s.state.selectedIndex).toBe(0)
  })

  it('does not select before first row', async () => {
    const s = await loadStore()
    s.setResults([
      { path: '/a', name: 'a.txt', parent: '/', extension: 'txt', is_dir: false, size_bytes: 10, modified_unix: 0 }
    ])
    s.setSelectedIndex(0)

    s.selectPrevious()

    expect(s.state.selectedIndex).toBe(0)
  })

  it('formats size correctly', async () => {
    const s = await loadStore()
    expect(s.formatSize(0)).toBe('0 B')
    expect(s.formatSize(1024)).toBe('1.0 KB')
    expect(s.formatSize(1024 * 1024)).toBe('1.0 MB')
  })

  it('derives index status text from daemon status', async () => {
    const s = await loadStore()

    expect(s.indexStatusText()).toBe('Index unavailable')

    s.setDaemonStatus({
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

    expect(s.indexStatusText()).toBe('Ready | Indexed 42 entries | 42 indexed')
  })

  it('copies the diagnostics log as a single payload', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockResolvedValue(undefined)

    s.setDiagnosticsLog(['[10:00:00] first', '[10:00:01] second'])
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
    const log = s.state.diagnosticsLog

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

    s.setQuery('f')
    s.debouncedSearch()
    s.setQuery('fo')
    s.debouncedSearch()
    s.setQuery('foo')
    s.debouncedSearch()

    expect(invoke).not.toHaveBeenCalled()

    await vi.advanceTimersByTimeAsync(119)
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

    void s.search('old')
    const newSearch = s.search('new')

    expect(invoke).toHaveBeenCalledTimes(1)

    first.resolve({
      rows: [],
      total_count: 99,
      total_size: 999,
      size_indexed: true
    })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))
    expect(invoke).toHaveBeenLastCalledWith('search_query', {
      query: 'new sort:name'
    })

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
    await newSearch

    expect(s.state.results).toEqual([{
      path: '/new',
      name: 'new.txt',
      parent: '/',
      extension: 'txt',
      is_dir: false,
      size_bytes: 1,
      modified_unix: 0
    }])
    expect(s.state.totalCount).toBe(1)
    expect(s.state.statusText).toBe('1 results | 1.0 B')
  })

  it('keeps only the latest search queued behind an active request', async () => {
    const s = await loadStore()
    const { invoke } = await import('@tauri-apps/api/core')
    const active = deferred<{ rows: never[]; total_count: number; total_size: number; size_indexed: boolean }>()
    const latest = deferred<{ rows: never[]; total_count: number; total_size: number; size_indexed: boolean }>()

    vi.mocked(invoke)
      .mockReturnValueOnce(active.promise as ReturnType<typeof invoke>)
      .mockReturnValueOnce(latest.promise as ReturnType<typeof invoke>)

    void s.search('n')
    void s.search('ne')
    const latestSearch = s.search('needle')

    expect(invoke).toHaveBeenCalledTimes(1)

    active.resolve({ rows: [], total_count: 0, total_size: 0, size_indexed: true })
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2))
    expect(invoke).toHaveBeenLastCalledWith('search_query', {
      query: 'needle sort:name'
    })

    latest.resolve({ rows: [], total_count: 0, total_size: 0, size_indexed: true })
    await latestSearch
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

    s.setQuery('needle')
    await s.search()

    expect(s.state.statusText).toBe('7 results | size unavailable')
    expect(s.state.hasCompletedSearch).toBe(true)
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
    expect(s.state.query).toBe('.mkv')
    expect(s.state.results).toHaveLength(1)

    await s.search('')
    expect(s.state.query).toBe('')
    expect(s.state.results).toEqual([])
    expect(s.state.statusText).toBe('Ready')
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
    expect(s.state.reindexing).toBe(false)
    expect(s.state.daemonStatus?.status_message).toBe('Reindexed 50 entries')
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

    s.setQuery('.mkv')

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
    expect(s.state.results).toEqual([{
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

    s.setQuery(query)

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
    expect(s.state.results).toEqual([row])
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

    s.setQuery('.mkv')

    await s.fetchStatus()
    await s.fetchStatus()

    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['get_status'],
      ['get_status']
    ])
  })
})
