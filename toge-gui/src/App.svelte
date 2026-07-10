<script lang="ts">
  import SearchBar from './components/SearchBar.svelte'
  import ResultTable from './components/ResultTable.svelte'
  import StatusBar from './components/StatusBar.svelte'
  import DebugWindow from './components/DiagnosticsPanel.svelte'
  import ErrorToast from './components/ErrorToast.svelte'
  import OptionsWindow from './components/OptionsWindow.svelte'
  import { listen } from '@tauri-apps/api/event'
  import {
    handleMainWindowKeydown,
    loadKeyboardSettings,
    setKeyboardFocusScope,
    setKeyboardSettings
  } from '$lib/keyboardStore.svelte'
  import type { KeyboardSettings } from '$lib/types'

  let isDark = $state(false)
  const windowLabel = window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? 'main'

  $effect(() => {
    isDark = window.matchMedia('(prefers-color-scheme: dark)').matches
  })

  $effect(() => {
    if (windowLabel !== 'main' && !windowLabel.startsWith('main-')) return

    void loadKeyboardSettings()
    setKeyboardFocusScope('search_edit')

    const handleKeydown = (event: KeyboardEvent) => {
      handleMainWindowKeydown(event)
    }

    let unlisten: (() => void) | undefined
    void listen<KeyboardSettings>('keyboard-settings-updated', (event) => {
      setKeyboardSettings(event.payload)
    }).then((cleanup) => {
      unlisten = cleanup
    })

    window.addEventListener('keydown', handleKeydown)
    return () => {
      window.removeEventListener('keydown', handleKeydown)
      unlisten?.()
    }
  })
</script>

<div class="app" class:dark={isDark}>
  {#if windowLabel === 'debug'}
    <DebugWindow />
  {:else if windowLabel === 'options'}
    <OptionsWindow />
  {:else}
    <SearchBar />
    <ResultTable />
    <StatusBar />
    <ErrorToast />
  {/if}
</div>

<style>
  :root {
    --bg: #ffffff;
    --bg-surface: #f5f5f5;
    --bg-hover: #e8e8e8;
    --bg-active: #d0d0d0;
    --border: #e0e0e0;
    --border-subtle: #f0f0f0;
    --border-focus: #3b82f6;
    --text-primary: #1a1a1a;
    --text-secondary: #666666;
    --text-placeholder: #999999;
    --accent: #3b82f6;
    --accent-hover: #2563eb;
    --selected-bg: #dbeafe;
    --selected-text: #1e40af;
    --row-even: #ffffff;
    --row-odd: #fafafa;
    --radius-sm: 4px;
    --radius-md: 8px;
  }

  :global(.dark) {
    --bg: #1a1b26;
    --bg-surface: #24283b;
    --bg-hover: #292e42;
    --bg-active: #3b4261;
    --border: #3b4261;
    --border-subtle: #292e42;
    --border-focus: #7aa2f7;
    --text-primary: #c0caf5;
    --text-secondary: #a9b1d6;
    --text-placeholder: #565f89;
    --accent: #7aa2f7;
    --accent-hover: #89b4fa;
    --selected-bg: #283457;
    --selected-text: #c0caf5;
    --row-even: #1a1b26;
    --row-odd: #1f2335;
  }

  :global(*) {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg);
    color: var(--text-primary);
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
</style>
