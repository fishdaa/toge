export interface ResultRow {
  path: string
  name: string
  parent: string
  extension: string
  is_dir: boolean
  size_bytes: number
  modified_unix: number
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
  watcher_log: string[]
  last_updated_unix: number
  build_duration_ms: number
}

export interface DiagnosticEntry {
  time: string
  message: string
}

export interface WatcherSelfTestResult {
  passed: boolean
  summary: string
  events: string[]
}

export type SortColumn = 'name' | 'path' | 'size' | 'modified'
export type SortDirection = 'asc' | 'desc'

export type KeyboardScope = 'global' | 'search_edit' | 'result_list'

export interface KeyboardShortcut {
  command_id: string
  scope: KeyboardScope
  accelerator: string
}

export interface KeyboardSettings {
  new_window_hotkey: string
  show_window_hotkey: string
  toggle_window_hotkey: string
  command_shortcuts: KeyboardShortcut[]
}

export interface KeyboardCommand {
  id: string
  group: string
  label: string
  scopes: KeyboardScope[]
}
