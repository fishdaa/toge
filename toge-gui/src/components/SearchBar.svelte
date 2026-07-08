<script lang="ts">
  import { onMount } from 'svelte'
  import { query } from '$lib/searchStore'
  import { clearSearch, search, debouncedSearch, selectNext, selectPrevious, openDiagnosticsWindow } from '$lib/searchStore'

  let inputEl: HTMLInputElement | undefined = $state(undefined)

  onMount(() => {
    inputEl?.focus()
  })

  function handleInput() {
    debouncedSearch()
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      search()
    } else if (e.key === 'Escape') {
      clearSearch()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectNext()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectPrevious()
    }
  }
</script>

<div class="search-bar">
  <div class="search-icon" aria-hidden="true">⌕</div>
  <input
    bind:this={inputEl}
    type="text"
    placeholder="Search files..."
    class="search-input"
    bind:value={$query}
    oninput={handleInput}
    onkeydown={handleKeydown}
  />
  {#if $query}
    <button class="clear-btn" onclick={() => clearSearch()}>
      ✕
    </button>
  {/if}
  <button class="diagnostics-btn" onclick={() => openDiagnosticsWindow()}>
    ⋯
  </button>
</div>

<style>
  .search-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, color-mix(in srgb, var(--bg-surface) 88%, var(--bg)) 0%, var(--bg-surface) 100%);
  }

  .search-icon {
    color: var(--accent);
    font-size: 15px;
    opacity: 0.95;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-size: 15px;
    color: var(--text-primary);
  }

  .search-input::placeholder {
    color: var(--text-placeholder);
  }

  .search-input:focus {
    color: var(--text-primary);
  }

  .clear-btn,
  .diagnostics-btn {
    background: transparent;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .clear-btn:hover,
  .diagnostics-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }
</style>
