<script lang="ts">
  import { onMount } from 'svelte'
  import { statusText, isLoading, indexStatusText, fetchStatus } from '$lib/searchStore'

  let refreshTimer: ReturnType<typeof setInterval> | null = null

  onMount(() => {
    fetchStatus()
    refreshTimer = setInterval(() => {
      fetchStatus()
    }, 3000)

    return () => {
      if (refreshTimer) clearInterval(refreshTimer)
    }
  })
</script>

<div class="status-bar">
  <div class="status-group">
    <span class="status-label">Search</span>
    <span class="status-text">{$statusText}</span>
    {#if $isLoading}
      <span class="loading-indicator">⟳</span>
    {/if}
  </div>
  <div class="status-group status-group-right">
    <span class="status-label">Index</span>
    <span class="status-text">{$indexStatusText}</span>
  </div>
</div>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 9px 16px;
    border-top: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-surface) 90%, var(--bg));
    font-size: 12px;
    color: var(--text-secondary);
  }

  .status-group {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .status-group-right {
    justify-content: flex-end;
    flex: 1;
  }

  .status-label {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-placeholder);
    flex-shrink: 0;
  }

  .status-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-secondary);
  }

  .loading-indicator {
    animation: spin 1s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
