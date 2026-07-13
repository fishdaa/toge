<script lang="ts">
  import { state as searchState, hasResults } from '$lib/searchStore'
  import { setSort, selectRow, openSelected, revealSelected, copySelectedPath, trashSelected, deleteSelected, formatSize, formatTimestamp, setTableColumnWidths } from '$lib/searchStore'
  import { setKeyboardFocusScope } from '$lib/keyboardStore.svelte'
  import ContextMenu from './ContextMenu.svelte'
  import type { SortColumn } from '$lib/types'
  import { onDestroy, onMount } from 'svelte'

  const columns: { key: SortColumn; label: string; width: number; min: number }[] = [
    { key: 'name', label: 'NAME', width: 220, min: 140 },
    { key: 'path', label: 'PATH', width: 320, min: 180 },
    { key: 'size', label: 'SIZE', width: 88, min: 72 },
    { key: 'modified', label: 'MODIFIED', width: 140, min: 110 }
  ]
  const ROW_HEIGHT = 27
  const OVERSCAN_ROWS = 8

  let contextMenu = $state({ visible: false, x: 0, y: 0 })
  let scrollEl: HTMLDivElement | undefined = $state(undefined)
  let scrollTop = $state(0)
  let scrollLeft = $state(0)
  let viewportHeight = $state(0)
  let programmaticScroll = false

  let removeResizeListeners: (() => void) | null = null
  let removeViewportListeners: (() => void) | null = null

  function currentColumnWidths() {
    return searchState.tableColumnWidths.length === columns.length
      ? searchState.tableColumnWidths
      : columns.map((col) => col.width)
  }

  function columnTemplate() {
    return currentColumnWidths().map((width) => `${width}px`).join(' ')
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
    if (index >= columns.length - 1) return

    event.preventDefault()
    event.stopPropagation()

    const startX = event.clientX
    const widths = [...currentColumnWidths()]
    const initialCurrent = widths[index]
    const initialNext = widths[index + 1]
    const currentMin = columns[index].min
    const nextMin = columns[index + 1].min

    const onMove = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX
      const grownCurrent = Math.max(currentMin, initialCurrent + delta)
      const consumed = grownCurrent - initialCurrent
      const nextWidth = Math.max(nextMin, initialNext - consumed)
      const actualCurrent = initialCurrent + (initialNext - nextWidth)

      widths[index] = actualCurrent
      widths[index + 1] = nextWidth
      setTableColumnWidths(widths)
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
    removeViewportListeners?.()
  })

  onMount(() => {
    const updateViewportHeight = () => {
      viewportHeight = scrollEl?.clientHeight ?? 0
      scrollTop = scrollEl?.scrollTop ?? 0
    }

    updateViewportHeight()
    window.addEventListener('resize', updateViewportHeight)
    removeViewportListeners = () => {
      window.removeEventListener('resize', updateViewportHeight)
      removeViewportListeners = null
    }

    return () => {
      removeViewportListeners?.()
    }
  })

  const totalRows = $derived(searchState.results.length)
  const visibleRowCount = $derived(Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN_ROWS * 2)
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN_ROWS))
  const endIndex = $derived(Math.min(totalRows, startIndex + visibleRowCount))
  const topSpacerHeight = $derived(startIndex * ROW_HEIGHT)
  const bottomSpacerHeight = $derived(Math.max(0, (totalRows - endIndex) * ROW_HEIGHT))
  const visibleRows = $derived.by(() => {
    const rows = searchState.results.slice(startIndex, endIndex)
    const sizesEnabled = searchState.sizeIndexed

    return rows.map((row) => ({
      ...row,
      sizeLabel: sizesEnabled ? formatSize(row.size_bytes) : '—',
      modifiedLabel: formatTimestamp(row.modified_unix)
    }))
  })

  const selectedPath = $derived(
    searchState.selectedIndex >= 0 && searchState.selectedIndex < searchState.results.length
      ? searchState.results[searchState.selectedIndex].path
      : null
  )

  $effect(() => {
    const idx = searchState.selectedIndex
    if (idx < 0 || !scrollEl) return

    const rowTop = idx * ROW_HEIGHT
    const rowBottom = rowTop + ROW_HEIGHT
    const currentTop = scrollEl.scrollTop
    const currentBottom = currentTop + viewportHeight

    if (rowTop < currentTop) {
      programmaticScroll = true
      scrollEl.scrollTop = rowTop
      scrollTop = rowTop
    } else if (rowBottom > currentBottom) {
      programmaticScroll = true
      scrollEl.scrollTop = rowBottom - viewportHeight
      scrollTop = rowBottom - viewportHeight
    }
  })
</script>

<div
  class="result-table"
  style={`--table-columns: ${columnTemplate()};`}
  role="grid"
  aria-label="Search results"
>
  <div class="table-header-viewport">
    <div class="table-header" style={`transform: translateX(${-scrollLeft}px);`}>
      {#each columns as col, index}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="header-cell"
          class:active={searchState.sortColumn === col.key}
          onclick={() => setSort(col.key)}
        >
          <span class="header-label">
            {col.label}
            {#if searchState.sortColumn === col.key}
              <span class="sort-indicator">
                {searchState.sortDirection === 'asc' ? '↑' : '↓'}
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
  </div>

  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="table-scroll"
    bind:this={scrollEl}
    tabindex="0"
    onclick={() => {
      scrollEl?.focus()
      setKeyboardFocusScope('result_list')
    }}
    onfocus={() => setKeyboardFocusScope('result_list')}
    onkeydown={() => setKeyboardFocusScope('result_list')}
    onscroll={() => {
      scrollLeft = scrollEl?.scrollLeft ?? 0
      if (programmaticScroll) {
        programmaticScroll = false
        return
      }
      scrollTop = scrollEl?.scrollTop ?? 0
    }}
  >
    <div class="table-body">
      {#if topSpacerHeight > 0}
        <div class="row-spacer" style={`height: ${topSpacerHeight}px;`}></div>
      {/if}

      {#each visibleRows as row, offset (row.path)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="table-row"
          class:selected={row.path === selectedPath}
          class:even={(startIndex + offset) % 2 === 0}
          onclick={() => selectRow(startIndex + offset)}
          ondblclick={() => openSelected()}
          oncontextmenu={(e) => showContextMenu(e, startIndex + offset)}
        >
          <div class="cell cell-name">
            <span class="icon" aria-hidden="true">{row.is_dir ? '▸' : '•'}</span>
            {row.name}
          </div>
          <div class="cell cell-path">
            {row.parent}
          </div>
          <div class="cell cell-meta">{row.sizeLabel}</div>
          <div class="cell cell-meta">{row.modifiedLabel}</div>
        </div>
      {/each}

      {#if bottomSpacerHeight > 0}
        <div class="row-spacer" style={`height: ${bottomSpacerHeight}px;`}></div>
      {/if}

      {#if searchState.hasCompletedSearch && !hasResults() && !searchState.isLoading}
        <div class="empty-searchState">
          No results found
        </div>
      {:else if !searchState.hasCompletedSearch && !hasResults() && !searchState.isLoading}
        <div class="empty-searchState">
          Start typing to search
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
      ontrash={() => { trashSelected(); closeContextMenu() }}
      ondelete={() => { deleteSelected(); closeContextMenu() }}
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
    scrollbar-gutter: stable;
    outline: none;
  }

  .table-scroll:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .table-header-viewport {
    flex: none;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-surface) 82%, var(--bg));
    box-shadow: inset 0 -1px 0 var(--border-subtle);
  }

  .table-header {
    display: grid;
    grid-template-columns: var(--table-columns);
    gap: 16px;
    padding: 10px 16px;
    min-width: max-content;
    will-change: transform;
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
    padding: 4px 16px;
    min-height: 27px;
    cursor: pointer;
    border-bottom: 1px solid var(--border-subtle);
    align-items: center;
    color: var(--text-primary);
    transition: background 120ms ease, color 120ms ease;
  }

  .row-spacer {
    width: 100%;
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

  .empty-searchState {
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
