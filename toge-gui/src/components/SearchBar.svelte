<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import {
    state as searchState,
    clearSearch,
    debouncedSearch,
    openDiagnosticsWindow,
    setSelectedIndex
  } from '$lib/searchStore'
  import {
    handleScopedKeydown,
    keyboardState,
    openOptionsWindow,
    setKeyboardFocusScope,
    setPendingSearchQuery
  } from '$lib/keyboardStore.svelte'

  const menuItems = ['File', 'Edit', 'View', 'Search', 'Bookmarks', 'Tools', 'Help']

  let inputEl: HTMLInputElement | undefined = $state(undefined)
  let openMenu = $state<string | null>(null)

  function focusSearchInput() {
    inputEl?.focus({ preventScroll: true })
    setKeyboardFocusScope('search_edit')
  }

  function activateSearchInputAfterShow() {
    setSelectedIndex(-1)
    focusSearchInput()
  }

  onMount(() => {
    setPendingSearchQuery(searchState.query)
    focusSearchInput()
    window.addEventListener('focus', activateSearchInputAfterShow)

    let disposed = false
    let unlisten: (() => void) | undefined
    void getCurrentWindow().onFocusChanged((event) => {
      if (event.payload) activateSearchInputAfterShow()
    }).then((cleanup) => {
      if (disposed) {
        cleanup()
      } else {
        unlisten = cleanup
      }
    })

    return () => {
      disposed = true
      window.removeEventListener('focus', activateSearchInputAfterShow)
      unlisten?.()
    }
  })

  function handleInput() {
    debouncedSearch(keyboardState.pendingSearchQuery)
  }
</script>

<div class="search-chrome">
  <div class="menu-bar" role="menubar" aria-label="Application menu">
    {#each menuItems as item}
      <div class="menu-entry">
        <button
          class="menu-item"
          type="button"
          aria-haspopup={item === 'Tools' ? 'menu' : undefined}
          aria-expanded={item === 'Tools' ? openMenu === item : undefined}
          onclick={() => {
            openMenu = item === 'Tools' ? (openMenu === item ? null : item) : null
          }}
        >
          {item}
        </button>

        {#if item === 'Tools' && openMenu === item}
          <div class="menu-popup" role="menu" aria-label="Tools">
            <button
              class="menu-popup-item"
              type="button"
              role="menuitem"
              onclick={async () => {
                openMenu = null
                await openOptionsWindow()
              }}
            >
              Options...
            </button>
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <div class="search-bar">
    <div class="search-icon" aria-hidden="true">⌕</div>
    <input
      bind:this={inputEl}
      type="text"
      placeholder="Search files..."
      class="search-input"
      bind:value={keyboardState.pendingSearchQuery}
      oninput={handleInput}
      onkeydown={(event) => {
        const handled = handleScopedKeydown(event, 'search_edit')
        if (handled) {
          event.stopPropagation()
          return
        }
        event.stopPropagation()
      }}
      onfocus={() => setKeyboardFocusScope('search_edit')}
    />
    <button
      class="clear-btn"
      class:visible={keyboardState.pendingSearchQuery.length > 0}
      type="button"
      aria-label="Clear search"
      aria-hidden={keyboardState.pendingSearchQuery.length === 0}
      tabindex={keyboardState.pendingSearchQuery.length > 0 ? 0 : -1}
      disabled={keyboardState.pendingSearchQuery.length === 0}
      onclick={() => {
        setPendingSearchQuery('')
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

  .menu-entry {
    position: relative;
  }

  .menu-popup {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    min-width: 144px;
    border: 1px solid var(--border);
    background: var(--bg);
    box-shadow: 0 8px 20px rgb(0 0 0 / 14%);
    z-index: 20;
    padding: 4px;
  }

  .menu-popup-item {
    width: 100%;
    border: none;
    background: transparent;
    color: var(--text-primary);
    text-align: left;
    font-size: 12px;
    padding: 6px 8px;
  }

  .menu-popup-item:hover {
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
