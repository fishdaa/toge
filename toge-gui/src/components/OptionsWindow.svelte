<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import {
    applyShortcutEdit,
    fetchDefaultKeyboardSettings,
    filterKeyboardCommands,
    formatKeyboardEvent,
    keyboardCommands,
    loadKeyboardSettings,
    setKeyboardSettings,
    removeShortcut,
    saveKeyboardSettings
  } from '$lib/keyboardStore.svelte'
  import type { KeyboardScope, KeyboardSettings, KeyboardShortcut } from '$lib/types'

  const navItems = [
    'General',
    'UI',
    'Home',
    'Search',
    'Results',
    'View',
    'Context Menu',
    'Fonts and Colors',
    'Keyboard',
    'History',
    'Indexes',
    'ETP/FTP Server',
    'HTTP Server'
  ]

  let isLoading = $state(true)
  let error = $state<string | null>(null)
  let savedSettings = $state<KeyboardSettings | null>(null)
  let draftSettings = $state<KeyboardSettings | null>(null)
  let commandFilter = $state('')
  let selectedCommandId = $state('search.execute')
  let selectedShortcutKey = $state('')
  let isEditorOpen = $state(false)
  let editorScope = $state<KeyboardScope>('global')
  let editorAccelerator = $state('')
  let editorOriginal = $state<KeyboardShortcut | null>(null)

  onMount(async () => {
    try {
      const settings = await loadKeyboardSettings()
      savedSettings = cloneSettings(settings)
      draftSettings = cloneSettings(settings)
    } catch (e) {
      error = String(e)
    } finally {
      isLoading = false
    }
  })

  const filteredCommands = $derived(filterKeyboardCommands(commandFilter))
  const selectedCommand = $derived(
    filteredCommands.find((command) => command.id === selectedCommandId) ??
      keyboardCommands.find((command) => command.id === selectedCommandId) ??
      filteredCommands[0] ??
      keyboardCommands[0]
  )

  $effect(() => {
    if (selectedCommand && selectedCommand.id !== selectedCommandId) {
      selectedCommandId = selectedCommand.id
    }
  })

  const commandShortcuts = $derived.by(() => {
    if (!draftSettings || !selectedCommand) return []
    return draftSettings.command_shortcuts.filter((shortcut) => shortcut.command_id === selectedCommand.id)
  })

  const selectedShortcut = $derived(
    commandShortcuts.find((shortcut) => shortcutKey(shortcut) === selectedShortcutKey) ?? null
  )

  const hasUnsavedChanges = $derived(
    !!draftSettings && !!savedSettings && JSON.stringify(draftSettings) !== JSON.stringify(savedSettings)
  )

  const replacementTargets = $derived.by(() => {
    if (!draftSettings || !editorAccelerator) return []
    const nextShortcut: KeyboardShortcut = {
      command_id: selectedCommand?.id ?? '',
      scope: editorScope,
      accelerator: editorAccelerator
    }

    return draftSettings.command_shortcuts.filter(
      (shortcut) =>
        shortcut.accelerator === nextShortcut.accelerator &&
        scopesConflict(shortcut.scope, nextShortcut.scope) &&
        shortcutKey(shortcut) !== shortcutKey(editorOriginal)
    )
  })

  async function applyChanges(closeAfter = false) {
    if (!draftSettings) return

    try {
      const normalized = await saveKeyboardSettings(cloneSettings(draftSettings))
      setKeyboardSettings(normalized)
      savedSettings = cloneSettings(normalized)
      draftSettings = cloneSettings(normalized)
      error = null
      if (closeAfter) {
        await invoke('close_options_window')
      }
    } catch (e) {
      error = String(e)
    }
  }

  async function closeWindow() {
    await invoke('close_options_window')
  }

  async function restoreDefaults() {
    const defaults = await fetchDefaultKeyboardSettings()
    draftSettings = cloneSettings(defaults)
    if (!keyboardCommands.some((command) => command.id === selectedCommandId)) {
      selectedCommandId = keyboardCommands[0]?.id ?? ''
    }
    selectedShortcutKey = ''
  }

  function beginEdit(shortcut?: KeyboardShortcut) {
    if (!selectedCommand) return
    isEditorOpen = true
    editorOriginal = shortcut ?? null
    editorScope = shortcut?.scope ?? selectedCommand.scopes[0] ?? 'global'
    editorAccelerator = shortcut?.accelerator ?? ''
  }

  function commitShortcutEdit() {
    if (!draftSettings || !selectedCommand || !editorAccelerator) return

    draftSettings = {
      ...draftSettings,
      command_shortcuts: applyShortcutEdit(draftSettings.command_shortcuts, {
        command_id: selectedCommand.id,
        scope: editorScope,
        accelerator: editorAccelerator
      }, editorOriginal)
    }
    selectedShortcutKey = shortcutKey({
      command_id: selectedCommand.id,
      scope: editorScope,
      accelerator: editorAccelerator
    })
    isEditorOpen = false
    editorOriginal = null
  }

  function deleteSelectedShortcut() {
    if (!draftSettings || !selectedShortcut) return
    draftSettings = {
      ...draftSettings,
      command_shortcuts: removeShortcut(draftSettings.command_shortcuts, selectedShortcut)
    }
    selectedShortcutKey = ''
  }

  function shortcutKey(shortcut: KeyboardShortcut | null | undefined): string {
    return shortcut ? `${shortcut.command_id}|${shortcut.scope}|${shortcut.accelerator}` : ''
  }

  function cloneSettings(settings: KeyboardSettings): KeyboardSettings {
    return {
      ...settings,
      command_shortcuts: settings.command_shortcuts.map((shortcut) => ({ ...shortcut }))
    }
  }

  function scopesConflict(left: KeyboardScope, right: KeyboardScope) {
    return left === 'global' || right === 'global' || left === right
  }

  function scopeLabel(scope: KeyboardScope) {
    switch (scope) {
      case 'global':
        return 'Global'
      case 'search_edit':
        return 'Search Edit'
      case 'result_list':
        return 'Result List'
    }
  }

  function handleHotkeyCapture(
    event: KeyboardEvent,
    setter: (value: string) => void
  ) {
    event.preventDefault()
    if ((event.key === 'Backspace' || event.key === 'Delete') && !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey) {
      setter('')
      return
    }
    const next = formatKeyboardEvent(event)
    if (next) setter(next)
  }
</script>

{#if isLoading}
  <div class="loading">Loading keyboard settings...</div>
{:else if !draftSettings || !savedSettings}
  <div class="loading">Unable to load keyboard settings.</div>
{:else}
  <div class="options-window">
    <div class="title-bar">Everything Options</div>
    <div class="content">
      <aside class="nav">
        {#each navItems as item}
          <button
            class="nav-item"
            class:active={item === 'Keyboard'}
            type="button"
            disabled={item !== 'Keyboard'}
          >
            {item}
          </button>
        {/each}
      </aside>

      <section class="panel">
        <div class="panel-card">
          <label class="field-row">
            <span>New window Hotkey:</span>
            <input
              type="text"
              readonly
              value={draftSettings.new_window_hotkey}
              onkeydown={(event) =>
                handleHotkeyCapture(event, (value) => (draftSettings = { ...draftSettings, new_window_hotkey: value }))}
            />
          </label>
          <label class="field-row">
            <span>Show window Hotkey:</span>
            <input
              type="text"
              readonly
              value={draftSettings.show_window_hotkey}
              onkeydown={(event) =>
                handleHotkeyCapture(event, (value) => (draftSettings = { ...draftSettings, show_window_hotkey: value }))}
            />
          </label>
          <label class="field-row">
            <span>Toggle window Hotkey:</span>
            <input
              type="text"
              readonly
              value={draftSettings.toggle_window_hotkey}
              onkeydown={(event) =>
                handleHotkeyCapture(event, (value) => (draftSettings = { ...draftSettings, toggle_window_hotkey: value }))}
            />
          </label>
          <label class="field-row">
            <span>Show commands containing:</span>
            <input type="text" bind:value={commandFilter} />
          </label>

          <div class="lists">
            <div class="command-list">
              {#each filteredCommands as command}
                <button
                  class="list-item"
                  class:selected={selectedCommand?.id === command.id}
                  type="button"
                  onclick={() => {
                    selectedCommandId = command.id
                    selectedShortcutKey = ''
                  }}
                >
                  <span>{command.group}</span>
                  <span>{command.label}</span>
                </button>
              {/each}
            </div>

            <div class="shortcut-section">
              <div class="shortcut-header">
                Shortcuts for {selectedCommand?.group} | {selectedCommand?.label}
              </div>
              <div class="shortcut-body">
                <div class="shortcut-list">
                  {#each commandShortcuts as shortcut}
                    <button
                      class="list-item"
                      class:selected={selectedShortcutKey === shortcutKey(shortcut)}
                      type="button"
                      onclick={() => (selectedShortcutKey = shortcutKey(shortcut))}
                    >
                      {shortcut.accelerator} ({scopeLabel(shortcut.scope)})
                    </button>
                  {/each}
                </div>

                <div class="shortcut-actions">
                  <button type="button" onclick={() => beginEdit()}>Add...</button>
                  <button type="button" disabled={!selectedShortcut} onclick={() => beginEdit(selectedShortcut ?? undefined)}>Edit...</button>
                  <button type="button" disabled={!selectedShortcut} onclick={deleteSelectedShortcut}>Remove</button>
                </div>
              </div>
            </div>
          </div>

          {#if error}
            <p class="error-text">{error}</p>
          {/if}
        </div>

        <div class="footer">
          <button type="button" onclick={restoreDefaults}>Restore Defaults</button>
          <div class="footer-actions">
            <button type="button" onclick={() => applyChanges(true)}>OK</button>
            <button type="button" onclick={closeWindow}>Cancel</button>
            <button type="button" disabled={!hasUnsavedChanges} onclick={() => applyChanges(false)}>Apply</button>
          </div>
        </div>
      </section>
    </div>
  </div>
{/if}

{#if isEditorOpen}
  <div class="editor-overlay">
    <div class="editor-dialog">
      <h2>{editorOriginal ? 'Edit Shortcut' : 'Add Shortcut'}</h2>
      <label>
        <span>Location</span>
        <select bind:value={editorScope}>
          {#each selectedCommand?.scopes ?? [] as scope}
            <option value={scope}>{scopeLabel(scope)}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Shortcut key</span>
        <input
          type="text"
          readonly
          value={editorAccelerator}
          onkeydown={(event) => {
            event.preventDefault()
            const next = formatKeyboardEvent(event)
            if (next) {
              editorAccelerator = next
            }
          }}
        />
      </label>
      {#if replacementTargets.length > 0}
        <p class="replacement-note">
          Applying this shortcut will replace {replacementTargets.length} existing shortcut{replacementTargets.length === 1 ? '' : 's'}.
        </p>
      {/if}
      <div class="editor-actions">
        <button type="button" disabled={!editorAccelerator} onclick={commitShortcutEdit}>OK</button>
        <button
          type="button"
          onclick={() => {
            isEditorOpen = false
            editorOriginal = null
          }}
        >
          Cancel
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .loading {
    display: grid;
    place-items: center;
    min-height: 100vh;
    font: 13px 'Segoe UI', sans-serif;
    color: #222;
    background: #f2f2f2;
  }

  .options-window {
    min-height: 100vh;
    background: #ececec;
    color: #111;
    font: 13px 'Segoe UI', sans-serif;
  }

  .title-bar {
    padding: 14px 18px 10px;
    font-size: 20px;
    color: #1b1b1b;
  }

  .content {
    display: grid;
    grid-template-columns: 136px 1fr;
    gap: 10px;
    padding: 0 12px 12px;
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 4px;
    border: 1px solid #b9b9b9;
    background: #f5f5f5;
  }

  .nav-item {
    border: none;
    background: transparent;
    text-align: left;
    padding: 2px 8px;
    color: #111;
    font-size: 13px;
  }

  .nav-item.active {
    background: #2a83d8;
    color: #fff;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .panel-card {
    border: 1px solid #b9b9b9;
    background: #f6f6f6;
    padding: 10px 12px 12px;
  }

  .field-row {
    display: grid;
    grid-template-columns: 150px 1fr;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .field-row input,
  .editor-dialog input,
  .editor-dialog select {
    min-height: 26px;
    border: 1px solid #a8a8a8;
    background: #fff;
    padding: 3px 8px;
  }

  .lists {
    display: grid;
    grid-template-columns: 1fr;
    gap: 12px;
    margin-top: 10px;
  }

  .command-list,
  .shortcut-list {
    border: 1px solid #a8a8a8;
    background: #fff;
    min-height: 180px;
    max-height: 220px;
    overflow: auto;
  }

  .list-item {
    display: flex;
    gap: 8px;
    width: 100%;
    border: none;
    background: transparent;
    padding: 4px 8px;
    text-align: left;
    font-size: 13px;
  }

  .list-item.selected {
    background: #2a83d8;
    color: #fff;
  }

  .shortcut-section {
    border: 1px solid #b9b9b9;
    padding: 8px;
    background: #f8f8f8;
  }

  .shortcut-header {
    margin-bottom: 8px;
  }

  .shortcut-body {
    display: grid;
    grid-template-columns: 1fr 110px;
    gap: 10px;
  }

  .shortcut-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .shortcut-actions button,
  .footer button,
  .editor-actions button {
    min-height: 28px;
    border: 1px solid #999;
    background: #ececec;
    padding: 0 12px;
  }

  .footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  .footer-actions {
    display: flex;
    gap: 8px;
  }

  .error-text {
    margin-top: 10px;
    color: #b42318;
  }

  .editor-overlay {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 20%);
  }

  .editor-dialog {
    width: min(360px, calc(100vw - 24px));
    border: 1px solid #9c9c9c;
    background: #f6f6f6;
    padding: 16px;
  }

  .editor-dialog h2 {
    margin-bottom: 12px;
    font-size: 16px;
    font-weight: 600;
  }

  .editor-dialog label {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
  }

  .replacement-note {
    color: #7a4d00;
    margin-bottom: 12px;
  }

  .editor-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  @media (max-width: 760px) {
    .content {
      grid-template-columns: 1fr;
    }

    .nav {
      display: none;
    }

    .field-row {
      grid-template-columns: 1fr;
    }

    .shortcut-body {
      grid-template-columns: 1fr;
    }

    .footer {
      flex-direction: column;
      align-items: stretch;
    }

    .footer-actions {
      justify-content: stretch;
    }

    .footer-actions button,
    .footer > button {
      flex: 1;
    }
  }
</style>
