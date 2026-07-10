import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/svelte'
import SearchBar from '@/components/SearchBar.svelte'
import { invoke } from '@tauri-apps/api/core'
import { setQuery, clearSearch, setResults, setSelectedIndex, state } from '$lib/searchStore'

const { onFocusChangedMock, windowHandlers } = vi.hoisted(() => ({
  onFocusChangedMock: vi.fn(),
  windowHandlers: { focusChanged: undefined as ((event: { payload: boolean }) => void) | undefined }
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onFocusChanged: onFocusChangedMock })
}))

describe('SearchBar', () => {
  beforeEach(() => {
    vi.resetModules()
    windowHandlers.focusChanged = undefined
    onFocusChangedMock.mockReset()
    onFocusChangedMock.mockImplementation(async (handler: (event: { payload: boolean }) => void) => {
      windowHandlers.focusChanged = handler
      return vi.fn()
    })
    clearSearch()
    setQuery('')
  })

  it('renders input element', async () => {
    render(SearchBar)
    expect(screen.getByPlaceholderText('Search files...')).toBeTruthy()
  })

  it('renders the menu bar items', async () => {
    render(SearchBar)
    expect(screen.getByRole('menubar', { name: 'Application menu' })).toBeTruthy()
    expect(screen.getByText('File')).toBeTruthy()
    expect(screen.getByText('Search')).toBeTruthy()
    expect(screen.getByText('Help')).toBeTruthy()
  })

  it('shows clear button when query is present', async () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...')
    await fireEvent.input(input, { target: { value: 'test' } })
    expect(screen.getByLabelText('Clear search')).toBeTruthy()
  })

  it('opens the debug window from the diagnostics button', async () => {
    render(SearchBar)
    const button = screen.getByText('⋯')
    await fireEvent.click(button)
    expect(invoke).toHaveBeenCalledWith('open_debug_window')
  })

  it('opens the options window from the tools menu', async () => {
    render(SearchBar)
    await fireEvent.click(screen.getByText('Tools'))
    await fireEvent.click(screen.getByText('Options...'))
    expect(invoke).toHaveBeenCalledWith('open_options_window')
  })

  it('does not commit the query store on plain typing before debounce fires', async () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...')
    await fireEvent.input(input, { target: { value: 'needle' } })

    expect((input as HTMLInputElement).value).toBe('needle')
  })

  it('focuses search and clears only the table selection when the window is shown', async () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...') as HTMLInputElement
    const diagnostics = screen.getByLabelText('Open diagnostics') as HTMLButtonElement
    const results = [{
      path: '/tmp/demo.mkv',
      name: 'demo.mkv',
      parent: '/tmp',
      extension: 'mkv',
      is_dir: false,
      size_bytes: 1,
      modified_unix: 0
    }]

    setResults(results)
    setSelectedIndex(0)
    diagnostics.focus()
    expect(document.activeElement).toBe(diagnostics)

    windowHandlers.focusChanged?.({ payload: true })
    expect(document.activeElement).toBe(input)
    expect(state.selectedIndex).toBe(-1)
    expect(state.results).toEqual(results)
  })

  it('uses the webview focus event as a Linux fallback', () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...') as HTMLInputElement
    const diagnostics = screen.getByLabelText('Open diagnostics') as HTMLButtonElement

    setSelectedIndex(0)
    diagnostics.focus()
    window.dispatchEvent(new FocusEvent('focus'))

    expect(document.activeElement).toBe(input)
    expect(state.selectedIndex).toBe(-1)
  })
})
