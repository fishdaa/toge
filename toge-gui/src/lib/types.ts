export interface ResultRow {
  path: string
  name: string
  parent: string
  extension: string
  is_dir: boolean
  size: string
  modified: string
}

export interface SearchResult {
  rows: ResultRow[]
  total_count: number
  total_size: number
  size_indexed: boolean
}

export interface StatusResponse {
  status: string
  status_message: string
  indexed_count: number
  size_indexed: boolean
  watcher_healthy: boolean
  watched_dir_count: number
  watch_failure_count: number
  watch_overflow_count: number
  last_updated_unix: number
  build_duration_ms: number
}

export interface DiagnosticEntry {
  time: string
  message: string
}

export type SortColumn = 'name' | 'path' | 'size' | 'modified'
export type SortDirection = 'asc' | 'desc'
