import { beforeEach, describe, expect, test, vi } from 'vitest'

import { createHttpAdapter } from './http'

let storedToken = ''
let getItemMock: ReturnType<typeof vi.fn>
let setItemMock: ReturnType<typeof vi.fn>
let fetchMock: ReturnType<typeof vi.fn>

beforeEach(() => {
  storedToken = ''
  setItemMock = vi.fn((_key: string, value: string) => {
    storedToken = value
  })
  getItemMock = vi.fn(() => storedToken || null)
  const localStorage = {
    getItem: getItemMock,
    setItem: setItemMock,
    removeItem: vi.fn(() => {
      storedToken = ''
    }),
  }
  Object.defineProperty(globalThis, 'window', {
    value: { localStorage },
    configurable: true,
    writable: true,
  })
  fetchMock = vi.fn(async () =>
    new Response(JSON.stringify({ success: true, items: [], total: 0, next_cursor: null }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    })
  )
  globalThis.fetch = fetchMock as typeof fetch
  createHttpAdapter().setAuthToken('')
})

describe('HTTP adapter bearer token', () => {
  test('attaches a configured token and removes it after clear', async () => {
    const adapter = createHttpAdapter()
    adapter.setAuthToken('  desktop-token  ')
    await adapter.getItems()

    const firstInit = fetchMock.mock.calls[0]?.[1] as RequestInit | undefined
    expect(new Headers(firstInit?.headers).get('Authorization')).toBe('Bearer desktop-token')
    expect(storedToken).toBe('desktop-token')

    adapter.setAuthToken('')
    await adapter.getItems()
    const secondInit = fetchMock.mock.calls[1]?.[1] as RequestInit | undefined
    expect(new Headers(secondInit?.headers).has('Authorization')).toBe(false)
    expect(storedToken).toBe('')
  })

  test('surfaces persistence failures and keeps the prior in-memory token', () => {
    const adapter = createHttpAdapter()
    adapter.setAuthToken('working-token')
    setItemMock.mockImplementationOnce(() => {
      throw new Error('storage denied')
    })

    expect(() => adapter.setAuthToken('lost-token')).toThrow('storage denied')
    expect(adapter.getAuthToken()).toBe('working-token')
  })

  test('surfaces restoration failures without crashing adapter creation', () => {
    getItemMock.mockImplementationOnce(() => {
      throw new Error('storage read denied')
    })

    const adapter = createHttpAdapter()
    expect(adapter.getAuthToken()).toBe('')
    expect(adapter.getAuthTokenError()).toContain('storage read denied')
  })

  test('rejects tokens that Web Headers cannot encode', () => {
    const adapter = createHttpAdapter()
    expect(() => adapter.setAuthToken('中文-token')).toThrow('ASCII')
  })
})
