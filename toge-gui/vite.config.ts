import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { resolve } from 'path'

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
      '$lib': resolve(__dirname, 'src/lib')
    },
    extensions: ['.svelte.ts', '.svelte.js', '.ts', '.js', '.svelte', '.json']
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true
  }
})
