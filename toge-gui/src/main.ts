import App from './App.svelte'
import { invoke } from '@tauri-apps/api/core'
import { mount } from 'svelte'

const app = mount(App, { target: document.getElementById('app')! })

void invoke('window_ready')

export default app
