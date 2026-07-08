<script lang="ts">
  import { results, sortColumn, sortDirection, selectedIndex, hasResults, isLoading } from '$lib/searchStore'
  import { setSort, selectRow, openSelected, revealSelected, copySelectedPath } from '$lib/searchStore'
  import ContextMenu from './ContextMenu.svelte'
  import type { SortColumn } from '$lib/types'
  import { onDestroy } from 'svelte'

  const columns: { key: SortColumn; label: string; width: number; min: number }[] = [
    { key: 'name', label: 'NAME', width: 220, min: 140 },
    { key: 'path', label: 'PATH', width: 320, min: 180 },
    { key: 'size', label: 'SIZE', width: 88, min: 72 },
    { key: 'modified', label: 'MODIFIED', width: 140, min: 110 }
  ]

  let contextMenu = $state({ visible: false, x: 0, y: 0 })
  let columnWidths = $state(columns.map((col) => col.width))

  let removeResizeListeners: (() => void) | null = null

  function columnTemplate() {
    return columnWidths.map((width) => `${width}px`).join(' ')
  }

  function showContextMenu(event: MouseEvent, index: number) {
    event.preventDefault()
    event.stopPropagation()
    selectRow(index)
    contextMenu = { visible: true, x: event.clientX, y: event.clientY }
  }

  function closeContextMenu() {
    contextMenu.visible = false
  }

  function startResize(index: number, event: PointerEvent) {
    if (index >= columnWidths.length - 1) return

    event.preventDefault()
    event.stopPropagation()

    const startX = event.clientX
    const initialCurrent = columnWidths[index]
    const initialNext = columnWidths[index + 1]
    const currentMin = columns[index].min
    const nextMin = columns[index + 1].min

    const onMove = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX
      const grownCurrent = Math.max(currentMin, initialCurrent + delta)
      const consumed = grownCurrent - initialCurrent
      const nextWidth = Math.max(nextMin, initialNext - consumed)
      const actualCurrent = initialCurrent + (initialNext - nextWidth)

      columnWidths[index] = actualCurrent
      columnWidths[index + 1] = nextWidth
      columnWidths = [...columnWidths]
    }

    const onUp = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      removeResizeListeners = null
    }

    removeResizeListeners?.()
    removeResizeListeners = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      removeResizeListeners = null
    }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  onDestroy(() => {
    removeResizeListeners?.()
  })
</script>

<div class="result-table" style={`--table-columns: ${columnTemplate()};`}>
  <div class="table-scroll">
    <div class="table-header">
      {#each columns as col, index}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="header-cell"
          class:active={$sortColumn === col.key}
          onclick={() => setSort(col.key)}
        >
          <span class="header-label">
            {col.label}
            {#if $sortColumn === col.key}
              <span class="sort-indicator">
                {$sortDirection === 'asc' ? '↑' : '↓'}
              </span>
            {/if}
          </span>
          {#if index < columns.length - 1}
            <button
              class="resize-handle"
              type="button"
              aria-label={`Resize ${col.label} column`}
              onpointerdown={(event) => startResize(index, event)}
            ></button>
          {/if}
        </div>
      {/each}
    </div>

    <div class="table-body">
      {#each $results as row, index}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="table-row"
          class:selected={index === $selectedIndex}
          class:even={index % 2 === 0}
          onclick={() => selectRow(index)}
          ondblclick={() => openSelected()}
          oncontextmenu={(e) => showContextMenu(e, index)}
        >
          <div class="cell cell-name">
            <span class="icon" aria-hidden="true">{row.is_dir ? '▸' : '•'}</span>
            {row.name}
          </div>
          <div class="cell cell-path">
            {row.parent}
          </div>
          <div class="cell cell-meta">
            {row.size}
          </div>
          <div class="cell cell-meta">
            {row.modified}
          </div>
        </div>
      {/each}

      {#if !$hasResults && !$isLoading}
        <div class="empty-state">
          No results found
        </div>
      {/if}
    </div>
  </div>

  {#if contextMenu.visible}
    <ContextMenu
      x={contextMenu.x}
      y={contextMenu.y}
      onopen={() => { openSelected(); closeContextMenu() }}
      onreveal={() => { revealSelected(); closeContextMenu() }}
      oncopypath={() => { copySelectedPath(); closeContextMenu() }}
      onclose={closeContextMenu}
    />
  {/if}
</div>

<style>
  .result-table {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: linear-gradient(180deg, var(--bg-surface) 0%, var(--bg) 12%);
  }

  .table-scroll {
    flex: 1;
    overflow: auto;
  }

  .table-header {
    display: grid;
    grid-template-columns: var(--table-columns);
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-surface) 82%, var(--bg));
    min-width: max-content;
    position: sticky;
    top: 0;
    z-index: 2;
    box-shadow: inset 0 -1px 0 var(--border-subtle);
  }

  .header-cell {
    position: relative;
    font-size: 11px;
    font-weight: 700;
    color: var(--text-secondary);
    text-transform: uppercase;
    cursor: pointer;
    user-select: none;
    min-width: 0;
    letter-spacing: 0.08em;
  }

  .header-cell:hover {
    color: var(--text-primary);
  }

  .header-cell.active {
    color: var(--accent);
  }

  .header-label {
    display: inline-flex;
    align-items: center;
    min-width: 0;
  }

  .sort-indicator {
    margin-left: 4px;
  }

  .resize-handle {
    position: absolute;
    top: -8px;
    right: -10px;
    width: 20px;
    height: calc(100% + 16px);
    border: none;
    background: transparent;
    cursor: col-resize;
    z-index: 3;
  }

  .resize-handle::after {
    content: '';
    position: absolute;
    top: 12px;
    bottom: 12px;
    left: 50%;
    width: 1px;
    transform: translateX(-50%);
    background: var(--border);
    opacity: 0;
    transition: opacity 120ms ease, background 120ms ease;
  }

  .resize-handle:hover::after,
  .resize-handle:focus-visible::after {
    opacity: 1;
    background: var(--accent);
  }

  .table-body {
    background: var(--row-even);
    min-width: max-content;
  }

  .table-row {
    display: grid;
    grid-template-columns: var(--table-columns);
    gap: 16px;
    padding: 9px 16px;
    cursor: pointer;
    border-bottom: 1px solid var(--border-subtle);
    align-items: center;
    color: var(--text-primary);
    transition: background 120ms ease, color 120ms ease;
  }

  .table-row.even {
    background: var(--row-even);
  }

  .table-row:not(.even) {
    background: var(--row-odd);
  }

  .table-row:hover {
    background: var(--bg-hover);
  }

  .table-row.selected {
    background: var(--selected-bg);
    color: var(--selected-text);
  }

  .cell {
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    color: inherit;
  }

  .cell-name {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 500;
  }

  .icon {
    color: var(--accent);
    font-size: 11px;
    flex-shrink: 0;
  }

  .cell-path {
    color: var(--text-secondary);
  }

  .cell-meta {
    color: var(--text-secondary);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .table-row.selected .cell-path,
  .table-row.selected .cell-meta,
  .table-row.selected .icon {
    color: inherit;
  }

  .empty-state {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: calc(100vh - 220px);
    padding: 24px;
    color: var(--text-secondary);
    text-align: center;
    background:
      radial-gradient(circle at center, rgba(122, 162, 247, 0.08) 0%, transparent 36%);
  }
</style>
