<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { error } from '$lib/searchStore'

  let visible = $state(false)
  let message = $state('')

  let unsub: (() => void) | undefined

  onMount(() => {
    unsub = error.subscribe((val) => {
      if (val) {
        message = val
        visible = true
        setTimeout(() => { visible = false }, 4000)
      }
    })
  })

  onDestroy(() => {
    unsub?.()
  })
</script>

{#if visible && message}
  <div class="toast">
    <span class="toast-icon">⚠</span>
    <span class="toast-message">{message}</span>
    <button class="toast-close" onclick={() => { visible = false }}>✕</button>
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    bottom: 48px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    background: #dc2626;
    color: white;
    border-radius: var(--radius-md);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    font-size: 13px;
    z-index: 2000;
    animation: slideUp 0.2s ease-out;
  }

  .toast-icon {
    font-size: 14px;
  }

  .toast-message {
    flex: 1;
    max-width: 400px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .toast-close {
    background: transparent;
    border: none;
    color: white;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    opacity: 0.7;
  }

  .toast-close:hover {
    opacity: 1;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%) translateY(0);
    }
  }
</style>
