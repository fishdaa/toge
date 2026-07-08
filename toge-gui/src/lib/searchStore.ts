import { writable, derived, get } from 'svelte/store'
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

export const query = writable('')
export const results = writable<ResultRow[]>([])
export const totalCount = writable(0)
export const totalSize = writable(0)
export const isLoading = writable(false)
export const statusText = writable('Ready')
export const selectedIndex = writable(-1)
export const sortColumn = writable<SortColumn>(loadSortColumn())
export const sortDirection = writable<SortDirection>(loadSortDirection())
export const tableColumnWidths = writable<number[]>(loadColumnWidths())
export const error = writable<string | null>(null)
export const daemonStatus = writable<StatusResponse | null>(null)
export const copyFeedback = writable(false)
export const diagnosticsLog = writable<string[]>([])
export const reindexing = writable(false)
export const sizeIndexed = writable(false)

sortColumn.subscribe((value) => {
  writeStorage(STORAGE_KEYS.sortColumn, value)
})

sortDirection.subscribe((value) => {
  writeStorage(STORAGE_KEYS.sortDirection, value)
})

tableColumnWidths.subscribe((value) => {
  writeStorage(STORAGE_KEYS.columnWidths, JSON.stringify(value))
})

export const hasResults = derived(results, ($results) => $results.length > 0)
export const indexStatusText = derived(daemonStatus, ($daemonStatus) => {
  if (!$daemonStatus) return 'Index unavailable'

  const count = `${$daemonStatus.indexed_count.toLocaleString()} indexed`
  const status = $daemonStatus.status
  const message = $daemonStatus.status_message?.trim()

  if (!message || message === status) {
    return `${status} | ${count}`
  }

  return `${status} | ${message} | ${count}`
})

const SEARCH_DEBOUNCE_MS = 300

let searchTimeout: ReturnType<typeof setTimeout> | null = null
let latestSearchRequestId = 0

function isJsdomRuntime(): boolean {
  return typeof navigator !== 'undefined' && /jsdom/i.test(navigator.userAgent)
}

async function yieldForPaint() {
  await new Promise<void>((resolve) => {
    if (typeof window.requestAnimationFrame === 'function') {
      window.requestAnimationFrame(() => resolve())
      return
    }
    window.setTimeout(resolve, 0)
  })
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
  diagnosticsLog.update((entries) => {
    const timestamp = new Date().toLocaleTimeString()
    const next = [`[${timestamp}] ${message}`, ...entries]
    return next.slice(0, 200)
  })
}

async function runSearch(nextQuery?: string) {
  const requestId = ++latestSearchRequestId
  const q = (nextQuery ?? get(query)).trim()

  if (!q) {
    query.set('')
    results.set([])
    totalCount.set(0)
    totalSize.set(0)
    sizeIndexed.set(false)
    statusText.set('Ready')
    isLoading.set(false)
    return
  }

  query.set(q)

  const col = get(sortColumn)
  const dir = get(sortDirection)
  const searchQuery = `${q} sort:${col}${dir === 'desc' ? '-desc' : ''}`

  isLoading.set(true)
  error.set(null)
  appendDiagnostics(`Search started for "${q}"`)

  try {
    if (typeof window !== 'undefined' && !isJsdomRuntime()) {
      await yieldForPaint()
    }

    if (requestId !== latestSearchRequestId) return

    const result = await invoke<SearchResult>('search_query', {
      query: searchQuery
    })

    if (requestId !== latestSearchRequestId) return

    const prevIdx = get(selectedIndex)
    const prevRows = get(results)
    const prevPath = prevIdx >= 0 && prevIdx < prevRows.length ? prevRows[prevIdx]?.path : null

    results.set(result.rows)

    const newIdx = prevPath ? result.rows.findIndex((r) => r.path === prevPath) : -1
    selectedIndex.set(newIdx >= 0 ? newIdx : result.rows.length > 0 ? 0 : -1)
    sizeIndexed.set(result.size_indexed)
    totalCount.set(result.total_count)
    totalSize.set(result.total_size)
    statusText.set(
      result.size_indexed
        ? `${result.total_count} results | ${formatSize(result.total_size)}`
        : `${result.total_count} results | size unavailable`
    )
    appendDiagnostics(`Search returned ${result.total_count} results`)
  } catch (e) {
    if (requestId !== latestSearchRequestId) return

    error.set(String(e))
    statusText.set(`Error: ${e}`)
    appendDiagnostics(`Search failed: ${String(e)}`)
  } finally {
    if (requestId === latestSearchRequestId) {
      isLoading.set(false)
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
  query.set('')
  results.set([])
  sizeIndexed.set(false)
  totalCount.set(0)
  totalSize.set(0)
  statusText.set('Ready')
  selectedIndex.set(-1)
  error.set(null)
  isLoading.set(false)
}

export function setSort(column: SortColumn) {
  const currentCol = get(sortColumn)
  if (currentCol === column) {
    sortDirection.update(d => d === 'asc' ? 'desc' : 'asc')
  } else {
    sortColumn.set(column)
    sortDirection.set('asc')
  }
  search()
}

export function setTableColumnWidths(widths: number[]) {
  tableColumnWidths.set([...widths])
}

export function selectRow(index: number) {
  selectedIndex.set(index)
}

export function selectNext() {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx < rows.length - 1) {
    selectedIndex.set(idx + 1)
  }
}

export function selectPrevious() {
  const idx = get(selectedIndex)
  if (idx > 0) {
    selectedIndex.set(idx - 1)
  }
}

export async function openSelected() {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx >= 0) {
    appendDiagnostics(`Opened ${rows[idx].path}`)
    await invoke('open_path', { path: rows[idx].path })
  }
}

export async function revealSelected() {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx >= 0) {
    appendDiagnostics(`Revealed ${rows[idx].path}`)
    await invoke('reveal_in_folder', { path: rows[idx].path })
  }
}

export async function copySelectedPath() {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx >= 0) {
    await invoke('copy_to_clipboard', { text: rows[idx].path })
    appendDiagnostics(`Copied path ${rows[idx].path}`)
    copyFeedback.set(true)
    setTimeout(() => copyFeedback.set(false), 1500)
  }
}

export async function trashSelected(): Promise<boolean> {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx < 0) return false

  const row = rows[idx]
  try {
    await invoke('trash_path', { path: row.path })
    appendDiagnostics(`Trashed ${row.path}`)
    results.update((r) => r.filter((_, i) => i !== idx))
    totalCount.update((c) => Math.max(0, c - 1))
    const newRows = get(results)
    selectedIndex.set(newRows.length > 0 ? Math.min(idx, newRows.length - 1) : -1)
    return true
  } catch (e) {
    appendDiagnostics(`Trash failed: ${String(e)}`)
    return false
  }
}

export async function deleteSelected(): Promise<boolean> {
  const idx = get(selectedIndex)
  const rows = get(results)
  if (idx < 0) return false

  const row = rows[idx]
  try {
    await invoke('delete_path', { path: row.path })
    appendDiagnostics(`Deleted ${row.path}`)
    results.update((r) => r.filter((_, i) => i !== idx))
    totalCount.update((c) => Math.max(0, c - 1))
    const newRows = get(results)
    selectedIndex.set(newRows.length > 0 ? Math.min(idx, newRows.length - 1) : -1)
    return true
  } catch (e) {
    appendDiagnostics(`Delete failed: ${String(e)}`)
    return false
  }
}

export async function fetchStatus() {
  if (get(isLoading)) return

  try {
    const previousStatus = get(daemonStatus)
    const status = await invoke<StatusResponse>('get_status')
    daemonStatus.set(status)
    if (shouldRefreshActiveSearch(previousStatus, status) || !previousStatus) {
      appendDiagnostics(`Status refreshed: ${status.status}`)
    }

    if (get(query).trim() && shouldRefreshActiveSearch(previousStatus, status)) {
      search()
    }
  } catch (e) {
    daemonStatus.set(null)
    appendDiagnostics(`Status refresh failed: ${String(e)}`)
  }
}

export async function openDiagnosticsWindow() {
  appendDiagnostics('Opening debug window')
  await invoke('open_debug_window')
}

export async function requestReindex() {
  reindexing.set(true)
  appendDiagnostics('Reindex requested')

  try {
    await invoke('reindex_index')
    appendDiagnostics('Reindex completed')
    await fetchStatus()
  } catch (e) {
    appendDiagnostics(`Reindex failed: ${String(e)}`)
    throw e
  } finally {
    reindexing.set(false)
  }
}

export async function copyDiagnosticsLog() {
  const text = get(diagnosticsLog).join('\n')
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
