import { invoke } from '@tauri-apps/api/core'
import type {
  ResultRow,
  SearchResult,
  StatusResponse,
  SortColumn,
  SortDirection,
  WatcherSelfTestResult
} from './types'

const STORAGE_KEYS = {
  sortColumn: 'toge:table:sort-column',
  sortDirection: 'toge:table:sort-direction',
  columnWidths: 'toge:table:column-widths'
} as const

const DEFAULT_COLUMN_WIDTHS = [220, 320, 88, 140]

function readStorage(key: string): string | null {
  if (typeof localStorage === 'undefined') return null

  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeStorage(key: string, value: string) {
  if (typeof localStorage === 'undefined') return

  try {
    localStorage.setItem(key, value)
  } catch {
    // Ignore storage failures; persistence is a convenience.
  }
}

function loadSortColumn(): SortColumn {
  const value = readStorage(STORAGE_KEYS.sortColumn)
  return value === 'name' || value === 'path' || value === 'size' || value === 'modified'
    ? value
    : 'name'
}

function loadSortDirection(): SortDirection {
  const value = readStorage(STORAGE_KEYS.sortDirection)
  return value === 'asc' || value === 'desc' ? value : 'asc'
}

function loadColumnWidths(): number[] {
  const value = readStorage(STORAGE_KEYS.columnWidths)
  if (!value) return [...DEFAULT_COLUMN_WIDTHS]

  try {
    const parsed = JSON.parse(value)
    if (
      Array.isArray(parsed) &&
      parsed.length === DEFAULT_COLUMN_WIDTHS.length &&
      parsed.every((entry) => typeof entry === 'number' && Number.isFinite(entry) && entry > 0)
    ) {
      return parsed
    }
  } catch {
    // fall through to defaults
  }

  return [...DEFAULT_COLUMN_WIDTHS]
}

export const state = $state({
  query: '',
  results: [] as ResultRow[],
  totalCount: 0,
  totalSize: 0,
  isLoading: false,
  hasCompletedSearch: false,
  statusText: 'Ready',
  selectedIndex: -1,
  sortColumn: loadSortColumn() as SortColumn,
  sortDirection: loadSortDirection() as SortDirection,
  tableColumnWidths: loadColumnWidths() as number[],
  error: null as string | null,
  daemonStatus: null as StatusResponse | null,
  copyFeedback: false,
  diagnosticsLog: [] as string[],
  reindexing: false,
  sizeIndexed: false
})

export function hasResults() {
  return state.results.length > 0
}

export function indexStatusText() {
  if (!state.daemonStatus) return 'Index unavailable'

  const count = `${state.daemonStatus.indexed_count.toLocaleString()} indexed`
  const status = state.daemonStatus.status
  const message = state.daemonStatus.status_message?.trim()

  if (!message || message === status) {
    return `${status} | ${count}`
  }

  return `${status} | ${message} | ${count}`
}

export function setQuery(value: string) {
  state.query = value
}

export function setResults(value: ResultRow[]) {
  state.results = value
}

export function setSelectedIndex(value: number) {
  state.selectedIndex = value
}

export function setTotalCount(value: number) {
  state.totalCount = value
}

export function setSizeIndexed(value: boolean) {
  state.sizeIndexed = value
}

export function setDaemonStatus(value: StatusResponse | null) {
  state.daemonStatus = value
}

export function setDiagnosticsLog(value: string[]) {
  state.diagnosticsLog = value
}

export function setCopyFeedback(value: boolean) {
  state.copyFeedback = value
}

export function setReindexing(value: boolean) {
  state.reindexing = value
}

export function setSortColumn(value: SortColumn) {
  state.sortColumn = value
  writeStorage(STORAGE_KEYS.sortColumn, value)
}

export function setSortDirection(value: SortDirection) {
  state.sortDirection = value
  writeStorage(STORAGE_KEYS.sortDirection, value)
}

const SEARCH_DEBOUNCE_MS = 120

let searchTimeout: ReturnType<typeof setTimeout> | null = null
let latestSearchRequestId = 0
// The daemon serves queries serially, so keep one IPC request active and
// replace any queued request with the newest input.
let pendingSearch: PendingSearch | null = null
let searchDrain: Promise<void> | null = null

interface PendingSearch {
  requestId: number
  query: string
  searchQuery: string
}

function shouldRefreshActiveSearch(
  previous: StatusResponse | null,
  next: StatusResponse
): boolean {
  if (!previous) return false

  return (
    previous.last_updated_unix !== next.last_updated_unix ||
    previous.indexed_count !== next.indexed_count
  )
}

function appendDiagnostics(message: string) {
  const timestamp = new Date().toLocaleTimeString()
  const next = [`[${timestamp}] ${message}`, ...state.diagnosticsLog]
  state.diagnosticsLog = next.slice(0, 200)
}

function runSearch(nextQuery?: string): Promise<void> {
  const requestId = ++latestSearchRequestId
  const q = (nextQuery ?? state.query).trim()

  if (!q) {
    pendingSearch = null
    state.query = ''
    state.results = []
    state.totalCount = 0
    state.totalSize = 0
    state.sizeIndexed = false
    state.statusText = 'Ready'
    state.isLoading = false
    state.hasCompletedSearch = false
    return Promise.resolve()
  }

  state.query = q
  state.hasCompletedSearch = false

  const col = state.sortColumn
  const dir = state.sortDirection
  const searchQuery = `${q} sort:${col}${dir === 'desc' ? '-desc' : ''}`

  state.isLoading = true
  state.error = null
  pendingSearch = { requestId, query: q, searchQuery }

  if (!searchDrain) {
    searchDrain = drainSearches()
  }
  return searchDrain
}

async function drainSearches() {
  try {
    while (pendingSearch) {
      const request = pendingSearch
      pendingSearch = null
      await executeSearch(request)
    }
  } finally {
    searchDrain = null
  }
}

async function executeSearch(request: PendingSearch) {
  appendDiagnostics(`Search started for "${request.query}"`)

  try {
    const result = await invoke<SearchResult>('search_query', {
      query: request.searchQuery
    })

    if (request.requestId !== latestSearchRequestId) return

    const prevIdx = state.selectedIndex
    const prevRows = state.results
    const prevPath = prevIdx >= 0 && prevIdx < prevRows.length ? prevRows[prevIdx]?.path : null

    state.results = result.rows
    state.hasCompletedSearch = true

    const newIdx = prevPath ? result.rows.findIndex((r) => r.path === prevPath) : -1
    state.selectedIndex = newIdx >= 0 ? newIdx : result.rows.length > 0 ? 0 : -1
    state.sizeIndexed = result.size_indexed
    state.totalCount = result.total_count
    state.totalSize = result.total_size
    state.statusText = result.size_indexed
      ? `${result.total_count} results | ${formatSize(result.total_size)}`
      : `${result.total_count} results | size unavailable`
    appendDiagnostics(`Search returned ${result.total_count} results`)
  } catch (e) {
    if (request.requestId !== latestSearchRequestId) return

    state.error = String(e)
    state.hasCompletedSearch = false
    state.statusText = `Error: ${e}`
    appendDiagnostics(`Search failed: ${String(e)}`)
  } finally {
    if (request.requestId === latestSearchRequestId) {
      state.isLoading = false
    }
  }
}

export function search(nextQuery?: string) {
  if (searchTimeout) {
    clearTimeout(searchTimeout)
    searchTimeout = null
  }
  return runSearch(nextQuery)
}

export function debouncedSearch(nextQuery?: string) {
  latestSearchRequestId += 1
  pendingSearch = null
  if (searchTimeout) clearTimeout(searchTimeout)
  searchTimeout = setTimeout(() => {
    searchTimeout = null
    void runSearch(nextQuery)
  }, SEARCH_DEBOUNCE_MS)
}

export function clearSearch() {
  if (searchTimeout) {
    clearTimeout(searchTimeout)
    searchTimeout = null
  }
  latestSearchRequestId += 1
  pendingSearch = null
  state.query = ''
  state.results = []
  state.sizeIndexed = false
  state.totalCount = 0
  state.totalSize = 0
  state.statusText = 'Ready'
  state.selectedIndex = -1
  state.error = null
  state.isLoading = false
  state.hasCompletedSearch = false
}

export function setSort(column: SortColumn) {
  if (state.sortColumn === column) {
    setSortDirection(state.sortDirection === 'asc' ? 'desc' : 'asc')
  } else {
    setSortColumn(column)
    setSortDirection('asc')
  }
  search()
}

export function setTableColumnWidths(widths: number[]) {
  state.tableColumnWidths = [...widths]
  writeStorage(STORAGE_KEYS.columnWidths, JSON.stringify(state.tableColumnWidths))
}

export function selectRow(index: number) {
  state.selectedIndex = index
}

export function selectNext() {
  if (state.selectedIndex < state.results.length - 1) {
    state.selectedIndex += 1
  }
}

export function selectPrevious() {
  if (state.selectedIndex > 0) {
    state.selectedIndex -= 1
  }
}

export async function openSelected() {
  if (state.selectedIndex >= 0) {
    appendDiagnostics(`Opened ${state.results[state.selectedIndex].path}`)
    await invoke('open_path', { path: state.results[state.selectedIndex].path })
  }
}

export async function revealSelected() {
  if (state.selectedIndex >= 0) {
    appendDiagnostics(`Revealed ${state.results[state.selectedIndex].path}`)
    await invoke('reveal_in_folder', { path: state.results[state.selectedIndex].path })
  }
}

export async function copySelectedPath() {
  if (state.selectedIndex >= 0) {
    await invoke('copy_to_clipboard', { text: state.results[state.selectedIndex].path })
    appendDiagnostics(`Copied path ${state.results[state.selectedIndex].path}`)
    state.copyFeedback = true
    setTimeout(() => {
      state.copyFeedback = false
    }, 1500)
  }
}

export async function trashSelected(): Promise<boolean> {
  const idx = state.selectedIndex
  if (idx < 0) return false

  const row = state.results[idx]
  try {
    await invoke('trash_path', { path: row.path })
    appendDiagnostics(`Trashed ${row.path}`)
    state.results = state.results.filter((_, i) => i !== idx)
    state.totalCount = Math.max(0, state.totalCount - 1)
    state.selectedIndex = state.results.length > 0 ? Math.min(idx, state.results.length - 1) : -1
    return true
  } catch (e) {
    appendDiagnostics(`Trash failed: ${String(e)}`)
    return false
  }
}

export async function deleteSelected(): Promise<boolean> {
  const idx = state.selectedIndex
  if (idx < 0) return false

  const row = state.results[idx]
  try {
    await invoke('delete_path', { path: row.path })
    appendDiagnostics(`Deleted ${row.path}`)
    state.results = state.results.filter((_, i) => i !== idx)
    state.totalCount = Math.max(0, state.totalCount - 1)
    state.selectedIndex = state.results.length > 0 ? Math.min(idx, state.results.length - 1) : -1
    return true
  } catch (e) {
    appendDiagnostics(`Delete failed: ${String(e)}`)
    return false
  }
}

export async function fetchStatus() {
  if (state.isLoading) return

  try {
    const previousStatus = state.daemonStatus
    const status = await invoke<StatusResponse>('get_status')
    state.daemonStatus = status
    if (shouldRefreshActiveSearch(previousStatus, status) || !previousStatus) {
      appendDiagnostics(`Status refreshed: ${status.status}`)
    }

    if (state.query.trim() && shouldRefreshActiveSearch(previousStatus, status)) {
      search()
    }
  } catch (e) {
    state.daemonStatus = null
    appendDiagnostics(`Status refresh failed: ${String(e)}`)
  }
}

export async function openDiagnosticsWindow() {
  appendDiagnostics('Opening debug window')
  await invoke('open_debug_window')
}

export async function requestReindex() {
  state.reindexing = true
  appendDiagnostics('Reindex requested')

  try {
    await invoke('reindex_index')
    appendDiagnostics('Reindex completed')
    await fetchStatus()
  } catch (e) {
    appendDiagnostics(`Reindex failed: ${String(e)}`)
    throw e
  } finally {
    state.reindexing = false
  }
}

export async function copyDiagnosticsLog() {
  const text = state.diagnosticsLog.join('\n')
  await invoke('copy_to_clipboard', { text })
  appendDiagnostics('Copied diagnostics log')
}

export async function runWatcherSelfTest() {
  appendDiagnostics('Watcher self-test started')
  const result = await invoke<WatcherSelfTestResult>('run_watcher_self_test')
  appendDiagnostics(result.summary)
  for (const entry of result.events) {
    appendDiagnostics(`watcher-self-test: ${entry}`)
  }
  return result
}

export function formatSize(bytes: number): string {
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

export function formatTimestamp(unix: number): string {
  if (unix <= 0) return ''

  const date = new Date(unix * 1000)
  const pad = (value: number) => String(value).padStart(2, '0')

  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}
