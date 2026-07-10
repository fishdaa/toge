<script lang="ts">
  import { onMount } from 'svelte'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { copyDiagnosticsLog, state as searchState, fetchStatus, requestReindex, runWatcherSelfTest } from '$lib/searchStore'
  import type { WatcherSelfTestResult } from '$lib/types'

  let copyLabel = $state('Copy log')
  let reindexError = $state<string | null>(null)
  let watcherTestRunning = $state(false)
  let watcherTestResult = $state<WatcherSelfTestResult | null>(null)
  let refreshTimer: ReturnType<typeof setInterval> | null = null

  function formatTimestamp(unix: number): string {
    if (unix <= 0) return 'never'
    return new Date(unix * 1000).toLocaleString()
  }

  function statusTone(status: string | undefined): 'ready' | 'working' | 'error' {
    if (!status) return 'working'
    if (status === 'Ready') return 'ready'
    if (status === 'Error') return 'error'
    return 'working'
  }

  async function closeWindow() {
    await getCurrentWindow().close()
  }

  async function copyLog() {
    await copyDiagnosticsLog()
    copyLabel = 'Copied'
    setTimeout(() => {
      copyLabel = 'Copy log'
    }, 1200)
  }

  async function reindex() {
    reindexError = null

    try {
      await requestReindex()
    } catch (e) {
      reindexError = String(e)
    }
  }

  async function runSelfTest() {
    watcherTestRunning = true
    watcherTestResult = null

    try {
      watcherTestResult = await runWatcherSelfTest()
    } catch (e) {
      watcherTestResult = {
        passed: false,
        summary: String(e),
        events: []
      }
    } finally {
      watcherTestRunning = false
    }
  }

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

<div class="debug-shell">
  <div class="panel">
    <div class="panel-header">
      <div class="header-copy">
        <h2>Diagnostics</h2>
        <p class="subtitle">Live daemon status, index state, and recent GUI activity.</p>
      </div>
      <div class="actions">
        <button class="action-btn action-btn-primary" onclick={reindex} disabled={searchState.reindexing}>
          {searchState.reindexing ? 'Reindexing…' : 'Reindex'}
        </button>
        <button class="action-btn" onclick={runSelfTest} disabled={watcherTestRunning}>
          {watcherTestRunning ? 'Testing…' : 'Watcher Self-Test'}
        </button>
        <button class="action-btn" onclick={() => fetchStatus()}>Refresh</button>
        <button class="action-btn" onclick={copyLog}>{copyLabel}</button>
        <button class="close-btn" onclick={closeWindow}>Close</button>
      </div>
    </div>

    <div class="panel-body">
      {#if searchState.daemonStatus}
        {@const s = searchState.daemonStatus}
        <section class="hero">
          <div class="hero-main">
            <div class="section-title">Index Status</div>
            <div class="hero-status">
              <span class={`status-pill ${statusTone(s.status)}`}>{s.status}</span>
              <span class="hero-message">{s.status_message || 'No daemon message'}</span>
            </div>
            <div class="hero-meta">
              <span>{searchState.reindexing ? 'Reindex running from debug window' : `${s.indexed_count.toLocaleString()} files indexed`}</span>
              <span>Updated {formatTimestamp(s.last_updated_unix)}</span>
            </div>
          </div>
          <div class="hero-stats">
            <div class="hero-stat">
              <span class="stat-label">Size Index</span>
              <span class="stat-value">{s.size_indexed ? 'Enabled' : 'Disabled'}</span>
            </div>
            <div class="hero-stat">
              <span class="stat-label">Build</span>
              <span class="stat-value">{s.build_duration_ms}ms</span>
            </div>
          </div>
        </section>
      {/if}

      {#if reindexError}
        <div class="error-banner">{reindexError}</div>
      {/if}

      {#if watcherTestResult}
        <div class="error-banner" class:success-banner={watcherTestResult.passed}>
          {watcherTestResult.summary}
        </div>
      {/if}

      <section class="section">
        <div class="section-title">Daemon</div>
        {#if searchState.daemonStatus}
          {@const s = searchState.daemonStatus}
          <div class="status-grid">
            <div class="status-item">
              <span class="label">Status</span>
              <span class="value">{s.status}</span>
            </div>
            <div class="status-item">
              <span class="label">Message</span>
              <span class="value">{s.status_message}</span>
            </div>
            <div class="status-item">
              <span class="label">Indexed Files</span>
              <span class="value">{s.indexed_count.toLocaleString()}</span>
            </div>
            <div class="status-item">
              <span class="label">Size Index</span>
              <span class="value">{s.size_indexed ? 'Enabled' : 'Disabled'}</span>
            </div>
            <div class="status-item">
              <span class="label">Build Duration</span>
              <span class="value">{s.build_duration_ms}ms</span>
            </div>
            <div class="status-item">
              <span class="label">Watcher Healthy</span>
              <span class="value" class:healthy={s.watcher_healthy} class:unhealthy={!s.watcher_healthy}>
                {s.watcher_healthy ? 'Yes' : 'No'}
              </span>
            </div>
            <div class="status-item">
              <span class="label">Watched Dirs</span>
              <span class="value">{s.watched_dir_count}</span>
            </div>
            <div class="status-item">
              <span class="label">Watch Failures</span>
              <span class="value" class:unhealthy={s.watch_failure_count > 0}>
                {s.watch_failure_count}
              </span>
            </div>
            <div class="status-item">
              <span class="label">Watch Overflows</span>
              <span class="value" class:unhealthy={s.watch_overflow_count > 0}>
                {s.watch_overflow_count}
              </span>
            </div>
            <div class="status-item">
              <span class="label">Last Updated</span>
              <span class="value">{formatTimestamp(s.last_updated_unix)}</span>
            </div>
          </div>
        {:else}
          <div class="loading">Loading daemon status...</div>
        {/if}
      </section>

      <section class="section">
        <div class="section-title">Watcher Log</div>
        <div class="log-panel">
          {#if searchState.daemonStatus?.watcher_log?.length}
            {#each searchState.daemonStatus.watcher_log as entry}
              <div class="log-entry">{entry}</div>
            {/each}
          {:else}
            <div class="loading">No watcher events yet.</div>
          {/if}
        </div>
      </section>

      <section class="section">
        <div class="section-title">Recent Log</div>
        <div class="log-panel">
          {#if searchState.diagnosticsLog.length > 0}
            {#each searchState.diagnosticsLog as entry}
              <div class="log-entry">{entry}</div>
            {/each}
          {:else}
            <div class="loading">No log entries yet.</div>
          {/if}
        </div>
      </section>
    </div>
  </div>
</div>

<style>
  .debug-shell {
    height: 100vh;
    padding: 18px;
    overflow: hidden;
    background:
      radial-gradient(circle at top left, rgba(59, 130, 246, 0.12), transparent 28%),
      linear-gradient(180deg, var(--bg-surface), var(--bg));
  }

  .panel {
    height: calc(100vh - 36px);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
    gap: 12px;
    flex-shrink: 0;
  }

  .header-copy {
    min-width: 0;
  }

  .subtitle {
    margin-top: 4px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .panel-header h2 {
    font-size: 14px;
    font-weight: 600;
  }

  .action-btn,
  .close-btn {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    color: var(--text-primary);
    cursor: pointer;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
  }

  .action-btn-primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }

  .action-btn:hover,
  .close-btn:hover {
    background: var(--bg-hover);
  }

  .action-btn-primary:hover {
    background: var(--accent-hover);
  }

  .action-btn:disabled {
    opacity: 0.6;
    cursor: progress;
  }

  .panel-body {
    padding: 16px 20px;
    display: grid;
    gap: 18px;
    overflow: auto;
    min-height: 0;
  }

  .hero {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(220px, 0.8fr);
    gap: 12px;
  }

  .hero-main,
  .hero-stats {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: linear-gradient(180deg, var(--bg-surface), var(--bg));
    padding: 16px;
  }

  .hero-stats {
    display: grid;
    gap: 12px;
    align-content: start;
  }

  .hero-status {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
    flex-wrap: wrap;
  }

  .status-pill {
    display: inline-flex;
    align-items: center;
    padding: 6px 10px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .status-pill.ready {
    background: rgba(34, 197, 94, 0.14);
    color: #4ade80;
  }

  .status-pill.working {
    background: rgba(250, 204, 21, 0.14);
    color: #facc15;
  }

  .status-pill.error {
    background: rgba(239, 68, 68, 0.16);
    color: #f87171;
  }

  .hero-message {
    color: var(--text-primary);
    font-size: 13px;
  }

  .hero-meta {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    margin-top: 12px;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .hero-stat {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .stat-label {
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .stat-value {
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
  }

  .error-banner {
    border: 1px solid rgba(239, 68, 68, 0.35);
    background: rgba(239, 68, 68, 0.08);
    color: #fca5a5;
    border-radius: var(--radius-md);
    padding: 12px 14px;
    font-size: 12px;
  }

  .success-banner {
    border-color: rgba(74, 222, 128, 0.28);
    background: rgba(74, 222, 128, 0.12);
    color: #86efac;
  }

  .section {
    display: grid;
    gap: 10px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-secondary);
  }

  .status-grid {
    display: grid;
    gap: 12px;
  }

  .status-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-surface);
    border-radius: var(--radius-sm);
    gap: 12px;
  }

  .label {
    font-size: 12px;
    color: var(--text-secondary);
    text-transform: uppercase;
    font-weight: 600;
  }

  .value {
    font-size: 13px;
    color: var(--text-primary);
    text-align: right;
  }

  .healthy {
    color: #22c55e;
  }

  .unhealthy {
    color: #ef4444;
  }

  .loading {
    text-align: center;
    color: var(--text-secondary);
    padding: 24px;
  }

  .log-panel {
    display: grid;
    gap: 8px;
    max-height: 320px;
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--bg-surface);
  }

  .log-entry {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
    line-height: 1.5;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg);
    border: 1px solid var(--border-subtle);
    color: var(--text-primary);
    word-break: break-word;
  }

  @media (max-width: 720px) {
    .panel-header {
      flex-direction: column;
      align-items: stretch;
    }

    .hero {
      grid-template-columns: 1fr;
    }
  }
</style>
