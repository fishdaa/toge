<script lang="ts">
  import { copyFeedback } from '$lib/searchStore'

  let { x, y, onopen, onreveal, oncopypath, onclose }: {
    x: number
    y: number
    onopen: () => void
    onreveal: () => void
    oncopypath: () => void
    onclose: () => void
  } = $props()

  function handleOverlayClick() {
    onclose()
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="context-menu-overlay" onclick={handleOverlayClick}>
  <div
    class="context-menu"
    style:left="{x}px"
    style:top="{y}px"
    onclick={(e) => e.stopPropagation()}
  >
    <button class="menu-item" onclick={onopen}>Open</button>
    <button class="menu-item" onclick={onreveal}>Reveal in Folder</button>
    <button class="menu-item" onclick={oncopypath}>
      {$copyFeedback ? 'Copied!' : 'Copy Path'}
    </button>
  </div>
</div>

<style>
  .context-menu-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
  }

  .context-menu {
    position: fixed;
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    padding: 4px;
    min-width: 160px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  }

  .menu-item {
    display: block;
    width: 100%;
    padding: 8px 12px;
    text-align: left;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
    border-radius: var(--radius-sm);
  }

  .menu-item:hover {
    background: var(--bg-hover);
  }
</style>
