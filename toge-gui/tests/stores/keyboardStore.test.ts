import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invokeMock } = vi.hoisted(() => ({
  invokeMock: vi.fn()
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock
}))

import {
  applyShortcutEdit,
  defaultKeyboardSettings,
  filterKeyboardCommands,
  handleMainWindowKeydown,
  formatKeyboardEvent,
  removeShortcut,
  setKeyboardSettings
} from '$lib/keyboardStore.svelte'

describe('keyboardStore', () => {
  beforeEach(() => {
    invokeMock.mockReset()
  })

  it('filters commands by label and group', () => {
    expect(filterKeyboardCommands('diagnostics').map((command) => command.id)).toContain(
      'window.open_diagnostics'
    )
    expect(filterKeyboardCommands('results').length).toBeGreaterThan(1)
  })

  it('formats keyboard events into canonical accelerators', () => {
    const event = new KeyboardEvent('keydown', { key: 'n', ctrlKey: true, shiftKey: true })
    expect(formatKeyboardEvent(event)).toBe('Ctrl+Shift+N')
  })

  it('formats a modifier key as the primary key', () => {
    const event = new KeyboardEvent('keydown', {
      key: 'Meta',
      ctrlKey: true,
      shiftKey: true,
      metaKey: true
    })
    expect(formatKeyboardEvent(event)).toBe('Ctrl+Shift+Super')
  })

  it('formats media keys in the renderer', () => {
    const event = new KeyboardEvent('keydown', { key: 'MediaTrackPrevious' })
    expect(formatKeyboardEvent(event)).toBe('MediaTrackPrevious')
  })

  it('does not intercept plain typing in editable inputs', () => {
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    const event = new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true })
    Object.defineProperty(event, 'target', { value: input })

    expect(handleMainWindowKeydown(event)).toBe(false)

    input.remove()
  })

  it('does not intercept non-allowlisted navigation keys in editable inputs', () => {
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    const event = new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true })
    Object.defineProperty(event, 'target', { value: input })

    expect(handleMainWindowKeydown(event)).toBe(false)

    input.remove()
  })

  it('leaves window hotkeys to the native global shortcut handler', () => {
    const event = new KeyboardEvent('keydown', {
      key: 'k',
      ctrlKey: true,
      altKey: true,
      bubbles: true,
      cancelable: true
    })
    Object.defineProperty(event, 'target', { value: document.body })

    const settings = defaultKeyboardSettings()
    settings.toggle_window_hotkey = 'Ctrl+Alt+K'
    setKeyboardSettings(settings)

    expect(handleMainWindowKeydown(event)).toBe(true)
    expect(event.defaultPrevented).toBe(true)
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('replaces conflicting shortcuts when applying edits', () => {
    const settings = defaultKeyboardSettings()
    const previous = settings.command_shortcuts.find(
      (shortcut) => shortcut.command_id === 'window.open_options'
    )!
    const updated = applyShortcutEdit(settings.command_shortcuts, {
      command_id: 'window.open_options',
      scope: 'global',
      accelerator: 'Ctrl+Period'
    }, previous)

    expect(
      updated.find((shortcut) => shortcut.command_id === 'window.open_diagnostics')
    ).toBeUndefined()
    expect(
      updated.find((shortcut) => shortcut.command_id === 'window.open_options')?.accelerator
    ).toBe('Ctrl+Period')
  })

  it('does not treat search-edit and result-list Enter shortcuts as conflicting', () => {
    const settings = defaultKeyboardSettings()
    const updated = applyShortcutEdit(settings.command_shortcuts, {
      command_id: 'results.open',
      scope: 'result_list',
      accelerator: 'Enter'
    })

    expect(
      updated.find((shortcut) => shortcut.command_id === 'search.execute')?.accelerator
    ).toBe('Enter')
    expect(
      updated.find((shortcut) => shortcut.command_id === 'results.open')?.accelerator
    ).toBe('Enter')
  })

  it('removes the selected shortcut', () => {
    const settings = defaultKeyboardSettings()
    const target = settings.command_shortcuts[0]
    const updated = removeShortcut(settings.command_shortcuts, target)
    expect(updated).toHaveLength(settings.command_shortcuts.length - 1)
  })
})
