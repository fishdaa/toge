import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invokeMock, mountMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  mountMock: vi.fn(() => ({ mounted: true }))
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: invokeMock
}))

vi.mock('svelte', () => ({
  mount: mountMock
}))

vi.mock('@/App.svelte', () => ({
  default: {}
}))

describe('application startup', () => {
  beforeEach(() => {
    vi.resetModules()
    invokeMock.mockReset()
    mountMock.mockClear()
    document.body.innerHTML = '<div id="app"></div>'
  })

  it('shows a hidden window immediately after mounting', async () => {
    const requestAnimationFrameMock = vi
      .spyOn(window, 'requestAnimationFrame')
      .mockImplementation(() => 1)

    await import('@/main')

    expect(mountMock).toHaveBeenCalledWith({}, { target: document.getElementById('app') })
    expect(invokeMock).toHaveBeenCalledWith('window_ready')
    expect(requestAnimationFrameMock).not.toHaveBeenCalled()
  })
})
