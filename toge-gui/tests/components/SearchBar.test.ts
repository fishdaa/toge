import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/svelte'
import SearchBar from '@/components/SearchBar.svelte'
import { invoke } from '@tauri-apps/api/core'
import { get } from 'svelte/store'
import { query } from '$lib/searchStore'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn()
}))

describe('SearchBar', () => {
  beforeEach(() => {
    vi.resetModules()
    query.set('')
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

  it('submits search immediately on Enter', async () => {
    vi.mocked(invoke).mockResolvedValue({
      rows: [],
      total_count: 0,
      total_size: 0,
      size_indexed: true
    })

    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...')
    await fireEvent.input(input, { target: { value: 'needle' } })
    await fireEvent.keyDown(input, { key: 'Enter' })

    expect(get(query)).toBe('needle')
    expect(invoke).toHaveBeenCalledWith('search_query', {
      query: 'needle sort:name'
    })
  })

  it('does not commit the query store on plain typing before debounce fires', async () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...')
    await fireEvent.input(input, { target: { value: 'needle' } })

    expect((input as HTMLInputElement).value).toBe('needle')
    expect(get(query)).toBe('')
  })

  it('clears the current query on Escape', async () => {
    render(SearchBar)
    const input = screen.getByPlaceholderText('Search files...')
    await fireEvent.input(input, { target: { value: 'needle' } })

    await fireEvent.keyDown(input, { key: 'Escape' })

    expect((input as HTMLInputElement).value).toBe('')
    expect(get(query)).toBe('')
  })
})
