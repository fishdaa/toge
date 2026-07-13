import { invoke } from '@tauri-apps/api/core'
import type { KeyboardCommand, KeyboardScope, KeyboardSettings, KeyboardShortcut } from './types'
import {
  clearSearch,
  copySelectedPath,
  deleteSelected,
  openDiagnosticsWindow,
  openSelected,
  search,
  selectNext,
  selectPrevious,
  trashSelected
} from './searchStore'

export const keyboardCommands: KeyboardCommand[] = [
  { id: 'search.execute', group: 'Search', label: 'Execute Search', scopes: ['search_edit'] },
  { id: 'search.clear', group: 'Search', label: 'Clear Search', scopes: ['search_edit'] },
  { id: 'results.select_next', group: 'Results', label: 'Select Next Result', scopes: ['global'] },
  { id: 'results.select_previous', group: 'Results', label: 'Select Previous Result', scopes: ['global'] },
  { id: 'results.open', group: 'Results', label: 'Open Selected Result', scopes: ['result_list'] },
  { id: 'results.copy_path', group: 'Results', label: 'Copy Full Path', scopes: ['global'] },
  { id: 'results.trash', group: 'Results', label: 'Move Selected Result To Trash', scopes: ['global'] },
  { id: 'results.delete_permanently', group: 'Results', label: 'Delete Selected Result Permanently', scopes: ['global'] },
  { id: 'window.open_diagnostics', group: 'Window', label: 'Open Diagnostics Window', scopes: ['global'] },
  { id: 'window.open_options', group: 'Window', label: 'Open Options Window', scopes: ['global'] }
]

export const keyboardState = $state({
  settings: defaultKeyboardSettings(),
  loaded: false,
  focusScope: 'global' as KeyboardScope,
  pendingSearchQuery: ''
})

export function defaultKeyboardSettings(): KeyboardSettings {
  return {
    new_window_hotkey: 'Ctrl+N',
    show_window_hotkey: '',
    toggle_window_hotkey: '',
    command_shortcuts: [
      { command_id: 'search.execute', scope: 'search_edit', accelerator: 'Enter' },
      { command_id: 'search.clear', scope: 'search_edit', accelerator: 'Escape' },
      { command_id: 'results.select_next', scope: 'global', accelerator: 'ArrowDown' },
      { command_id: 'results.select_previous', scope: 'global', accelerator: 'ArrowUp' },
      { command_id: 'results.open', scope: 'result_list', accelerator: 'Enter' },
      { command_id: 'results.copy_path', scope: 'global', accelerator: 'Ctrl+C' },
      { command_id: 'results.trash', scope: 'global', accelerator: 'Delete' },
      { command_id: 'results.delete_permanently', scope: 'global', accelerator: 'Shift+Delete' },
      { command_id: 'window.open_diagnostics', scope: 'global', accelerator: 'Ctrl+Period' },
      { command_id: 'window.open_options', scope: 'global', accelerator: 'Ctrl+Comma' }
    ]
  }
}

export async function loadKeyboardSettings(): Promise<KeyboardSettings> {
  const settings = await invoke<KeyboardSettings>('get_keyboard_settings')
  setKeyboardSettings(settings)
  return settings
}

export async function saveKeyboardSettings(settings: KeyboardSettings): Promise<KeyboardSettings> {
  const normalized = await invoke<KeyboardSettings>('save_keyboard_settings', { settings })
  setKeyboardSettings(normalized)
  return normalized
}

export async function fetchDefaultKeyboardSettings(): Promise<KeyboardSettings> {
  return invoke<KeyboardSettings>('restore_default_keyboard_settings')
}

export function setKeyboardFocusScope(scope: KeyboardScope) {
  keyboardState.focusScope = scope
}

export function setKeyboardSettings(settings: KeyboardSettings) {
  keyboardState.settings = settings
  keyboardState.loaded = true
}

export function setPendingSearchQuery(value: string) {
  keyboardState.pendingSearchQuery = value
}

export async function openOptionsWindow() {
  await invoke('open_options_window')
}

export function filterKeyboardCommands(query: string): KeyboardCommand[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return keyboardCommands
  return keyboardCommands.filter(
    (command) =>
      command.label.toLowerCase().includes(needle) ||
      command.group.toLowerCase().includes(needle) ||
      command.id.toLowerCase().includes(needle)
  )
}

export function applyShortcutEdit(
  shortcuts: KeyboardShortcut[],
  nextShortcut: KeyboardShortcut,
  previousShortcut?: KeyboardShortcut | null
): KeyboardShortcut[] {
  return shortcuts
    .filter((shortcut) => !isSameShortcut(shortcut, previousShortcut))
    .filter((shortcut) => !shortcutConflicts(shortcut, nextShortcut))
    .concat([nextShortcut])
}

export function removeShortcut(
  shortcuts: KeyboardShortcut[],
  target: KeyboardShortcut | null
): KeyboardShortcut[] {
  return shortcuts.filter((shortcut) => !isSameShortcut(shortcut, target))
}

export function handleMainWindowKeydown(event: KeyboardEvent): boolean {
  if (getEditableTarget(event)) {
    return false
  }

  const accelerator = formatKeyboardEvent(event)
  if (!accelerator) return false

  if (consumeNativeWindowHotkey(event, accelerator)) return true

  const shortcut = matchingShortcut(accelerator, keyboardState.focusScope, keyboardState.settings.command_shortcuts)
  if (!shortcut) return false

  event.preventDefault()
  void executeCommand(shortcut.command_id)
  return true
}

export function handleScopedKeydown(event: KeyboardEvent, focusScope: KeyboardScope): boolean {
  const accelerator = formatKeyboardEvent(event)
  if (!accelerator) return false

  if (consumeNativeWindowHotkey(event, accelerator)) return true

  const shortcut = matchingShortcut(accelerator, focusScope, keyboardState.settings.command_shortcuts)
  if (!shortcut) return false

  event.preventDefault()
  void executeCommand(shortcut.command_id)
  return true
}

function consumeNativeWindowHotkey(event: KeyboardEvent, accelerator: string): boolean {
  const settings = keyboardState.settings
  if (
    accelerator !== settings.new_window_hotkey &&
    accelerator !== settings.show_window_hotkey &&
    accelerator !== settings.toggle_window_hotkey
  ) {
    return false
  }

  // The native handler owns execution even while this window is focused.
  // Consume a renderer event if the platform also delivers one.
  event.preventDefault()
  return true
}

function getEditableTarget(event: KeyboardEvent): HTMLElement | null {
  const activeElement = document.activeElement
  if (isEditableTarget(activeElement)) {
    return activeElement
  }

  const path = typeof event.composedPath === 'function' ? event.composedPath() : []
  for (const item of path) {
    if (isEditableTarget(item)) {
      return item
    }
  }

  return isEditableTarget(event.target) ? (event.target as HTMLElement) : null
}

function isEditableTarget(target: EventTarget | null): target is HTMLElement {
  if (!(target instanceof HTMLElement)) {
    return false
  }

  if (target.isContentEditable) {
    return true
  }

  if (target instanceof HTMLInputElement) {
    return !target.readOnly && !target.disabled
  }

  return target instanceof HTMLTextAreaElement && !target.readOnly && !target.disabled
}
export function formatKeyboardEvent(event: KeyboardEvent): string {
  const key = normalizeKey(event.key)
  if (!key) return ''

  const parts: string[] = []
  if (event.ctrlKey && key !== 'Ctrl') parts.push('Ctrl')
  if (event.altKey && key !== 'Alt') parts.push('Alt')
  if (event.shiftKey && key !== 'Shift') parts.push('Shift')
  if (event.metaKey && key !== 'Super') parts.push('Meta')
  parts.push(key)
  return parts.join('+')
}

function normalizeKey(key: string): string {
  const lower = key.trim().toLowerCase()
  switch (lower) {
    case '':
      return ''
    case 'control':
      return 'Ctrl'
    case 'shift':
      return 'Shift'
    case 'alt':
      return 'Alt'
    case 'meta':
      return 'Super'
    case 'enter':
      return 'Enter'
    case 'escape':
    case 'esc':
      return 'Escape'
    case 'arrowup':
      return 'ArrowUp'
    case 'arrowdown':
      return 'ArrowDown'
    case 'arrowleft':
      return 'ArrowLeft'
    case 'arrowright':
      return 'ArrowRight'
    case 'delete':
      return 'Delete'
    case 'backspace':
      return 'Backspace'
    case ' ':
    case 'space':
      return 'Space'
    case '.':
      return 'Period'
    case ',':
      return 'Comma'
    case 'tab':
      return 'Tab'
    case 'mediatrackprevious':
      return 'MediaTrackPrevious'
    case 'mediatracknext':
      return 'MediaTrackNext'
    case 'mediaplay':
      return 'MediaPlay'
    case 'mediapause':
      return 'MediaPause'
    case 'mediaplaypause':
      return navigator.userAgent.toLowerCase().includes('linux') ? 'MediaPlay' : 'MediaPlayPause'
    case 'mediastop':
      return 'MediaStop'
    case 'audiovolumedown':
      return 'AudioVolumeDown'
    case 'audiovolumeup':
      return 'AudioVolumeUp'
    case 'audiovolumemute':
      return 'AudioVolumeMute'
    default:
      if (/^f\d{1,2}$/i.test(key)) {
        return key.toUpperCase()
      }
      if (key.length === 1) {
        return key.toUpperCase()
      }
      return key
  }
}

function matchingShortcut(
  accelerator: string,
  focusScope: KeyboardScope,
  shortcuts: KeyboardShortcut[]
): KeyboardShortcut | null {
  const orderedScopes: KeyboardScope[] =
    focusScope === 'search_edit'
      ? ['search_edit', 'global']
      : focusScope === 'result_list'
        ? ['result_list', 'global']
        : ['global']

  for (const scope of orderedScopes) {
    const shortcut = shortcuts.find(
      (entry) => entry.scope === scope && entry.accelerator === accelerator
    )
    if (shortcut) return shortcut
  }

  return null
}

async function executeCommand(commandId: string) {
  switch (commandId) {
    case 'search.execute':
      await search(keyboardState.pendingSearchQuery)
      return
    case 'search.clear':
      setPendingSearchQuery('')
      clearSearch()
      return
    case 'results.select_next':
      selectNext()
      return
    case 'results.select_previous':
      selectPrevious()
      return
    case 'results.open':
      await openSelected()
      return
    case 'results.copy_path':
      if (!window.getSelection()?.toString()) {
        await copySelectedPath()
      }
      return
    case 'results.trash':
      await trashSelected()
      return
    case 'results.delete_permanently':
      await deleteSelected()
      return
    case 'window.open_diagnostics':
      await openDiagnosticsWindow()
      return
    case 'window.open_options':
      await openOptionsWindow()
      return
    default:
      return
  }
}

function shortcutConflicts(left: KeyboardShortcut, right: KeyboardShortcut): boolean {
  return left.accelerator === right.accelerator && scopesConflict(left.scope, right.scope)
}

function scopesConflict(left: KeyboardScope, right: KeyboardScope): boolean {
  if (left === 'global' || right === 'global') return true
  return left === right
}

function isSameShortcut(
  left: KeyboardShortcut,
  right: KeyboardShortcut | null | undefined
): boolean {
  return !!right &&
    left.command_id === right.command_id &&
    left.scope === right.scope &&
    left.accelerator === right.accelerator
}
