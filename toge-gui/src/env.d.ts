/// <reference types="vite/client" />

declare module '*.svelte' {
  import type { ComponentType } from 'svelte'
  const component: ComponentType
  export default component
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      metadata?: {
        currentWindow?: {
          label?: string
        }
      }
    }
  }
}
