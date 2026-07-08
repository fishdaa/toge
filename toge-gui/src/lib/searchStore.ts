import { writable, derived, get } from 'svelte/store'
import { invoke } from '@tauri-apps/api/core'
import type { ResultRow, SearchResult, StatusResponse, SortColumn, SortDirection } from './types'

export const query = writable('')
export const results = writable<ResultRow[]>([])
export const totalCount = writable(0)
export const totalSize = writable(0)
export const isLoading = writable(false)
export const statusText = writable('Ready')
export const selectedIndex = writable(-1)
export const sortColumn = writable<SortColumn>('name')
export const sortDirection = writable<SortDirection>('asc')
export const error = writable<string | null>(null)
export const daemonStatus = writable<StatusResponse | null>(null)
export const copyFeedback = writable(false)
export const diagnosticsLog = writable<string[]>([])
export const reindexing = writable(false)

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

const SEARCH_DEBOUNCE_MS = 250

let searchTimeout: ReturnType<typeof setTimeout> | null = null
let latestSearchRequestId = 0

function appendDiagnostics(message: string) {
  diagnosticsLog.update((entries) => {
    const timestamp = new Date().toLocaleTimeString()
    const next = [`[${timestamp}] ${message}`, ...entries]
    return next.slice(0, 200)
  })
}

async function runSearch() {
  const requestId = ++latestSearchRequestId
  const q = get(query).trim()

  if (!q) {
    results.set([])
    totalCount.set(0)
    totalSize.set(0)
    statusText.set('Ready')
    isLoading.set(false)
    return
  }

  isLoading.set(true)
  error.set(null)
  appendDiagnostics(`Search started for "${q}"`)

  try {
    const col = get(sortColumn)
    const dir = get(sortDirection)
    const searchQuery = `${q} sort:${col}${dir === 'desc' ? '-desc' : ''}`
    const result = await invoke<SearchResult>('search_query', {
      query: searchQuery,
      maxResults: 10000
    })

    if (requestId !== latestSearchRequestId) return

    results.set(result.rows)
    totalCount.set(result.total_count)
    totalSize.set(result.total_size)
    statusText.set(
      result.size_indexed
        ? `${result.total_count} results | ${formatSize(result.total_size)}`
        : `${result.total_count} results | size unavailable`
    )
    selectedIndex.set(result.rows.length > 0 ? 0 : -1)
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

export function search() {
  if (searchTimeout) {
    clearTimeout(searchTimeout)
    searchTimeout = null
  }
  void runSearch()
}

export function debouncedSearch() {
  if (searchTimeout) clearTimeout(searchTimeout)
  searchTimeout = setTimeout(() => {
    searchTimeout = null
    void runSearch()
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

export async function fetchStatus() {
  try {
    const status = await invoke<StatusResponse>('get_status')
    daemonStatus.set(status)
    appendDiagnostics(`Status refreshed: ${status.status}`)
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
