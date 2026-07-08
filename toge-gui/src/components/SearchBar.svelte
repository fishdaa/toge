<script lang="ts">
  import { onMount } from 'svelte'
  import { query } from '$lib/searchStore'
  import { clearSearch, search, debouncedSearch, selectNext, selectPrevious, openDiagnosticsWindow } from '$lib/searchStore'
  import { openSelected, copySelectedPath, trashSelected, deleteSelected } from '$lib/searchStore'

  const menuItems = ['File', 'Edit', 'View', 'Search', 'Bookmarks', 'Tools', 'Help']

  let inputEl: HTMLInputElement | undefined = $state(undefined)
  let queryText = $state('')

  onMount(() => {
    queryText = $query
    inputEl?.focus()
  })

  function handleInput() {
    debouncedSearch(queryText)
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      search(queryText)
    } else if (e.key === 'Escape') {
      queryText = ''
      clearSearch()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectNext()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectPrevious()
    } else if (e.key === 'Delete' && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      e.preventDefault()
      trashSelected()
    } else if (e.key === 'Delete' && e.shiftKey) {
      e.preventDefault()
      deleteSelected()
    } else if (e.key === 'c' && (e.ctrlKey || e.metaKey) && !window.getSelection()?.toString()) {
      e.preventDefault()
      copySelectedPath()
    } else if (e.key === 'o' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault()
      openSelected()
    }
  }
</script>

<div class="search-chrome">
  <div class="menu-bar" role="menubar" aria-label="Application menu">
    {#each menuItems as item}
      <button class="menu-item" type="button">{item}</button>
    {/each}
  </div>

  <div class="search-bar">
    <div class="search-icon" aria-hidden="true">⌕</div>
    <input
      bind:this={inputEl}
      type="text"
      placeholder="Search files..."
      class="search-input"
      bind:value={queryText}
      oninput={handleInput}
      onkeydown={handleKeydown}
    />
    <button
      class="clear-btn"
      class:visible={queryText.length > 0}
      type="button"
      aria-label="Clear search"
      aria-hidden={queryText.length === 0}
      tabindex={queryText.length > 0 ? 0 : -1}
      disabled={queryText.length === 0}
      onclick={() => {
        queryText = ''
        clearSearch()
      }}
    >
      ✕
    </button>
    <button class="diagnostics-btn" type="button" aria-label="Open diagnostics" onclick={() => openDiagnosticsWindow()}>
      ⋯
    </button>
  </div>
</div>

<style>
  .search-chrome {
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-surface) 94%, var(--bg));
  }

  .menu-bar {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    min-height: 28px;
  }

  .menu-item {
    border: none;
    background: transparent;
    color: var(--text-primary);
    font-size: 12px;
    padding: 3px 8px;
    border-radius: 2px;
    cursor: default;
  }

  .menu-item:hover {
    background: var(--bg-hover);
  }

  .search-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-surface) 88%, var(--bg)) 0%, var(--bg-surface) 100%);
  }

  .search-icon {
    color: var(--accent);
    font-size: 13px;
    opacity: 0.95;
  }

  .search-input {
    flex: 1;
    background: var(--bg);
    border: 1px solid var(--border);
    outline: none;
    font-size: 13px;
    color: var(--text-primary);
    min-height: 26px;
    padding: 0 8px;
    box-shadow: inset 0 1px 0 color-mix(in srgb, var(--bg) 84%, #fff);
  }

  .search-input::placeholder {
    color: var(--text-placeholder);
  }

  .search-input:focus {
    color: var(--text-primary);
    border-color: var(--border-focus);
  }

  .clear-btn,
  .diagnostics-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    width: 24px;
    height: 24px;
    border-radius: 2px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 24px;
  }

  .clear-btn:hover,
  .diagnostics-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .clear-btn {
    visibility: hidden;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms ease;
  }

  .clear-btn.visible {
    visibility: visible;
    opacity: 1;
    pointer-events: auto;
  }
</style>
