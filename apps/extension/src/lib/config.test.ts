import { beforeEach, describe, expect, test } from 'bun:test'

import {
  API_TOKEN_STORAGE_KEY,
  discoverCloudApiBase,
  readApiToken,
  setApiToken,
} from './config'

let storage: Record<string, unknown>

beforeEach(() => {
  storage = {}
  ;(globalThis as typeof globalThis & { chrome: typeof chrome }).chrome = {
    storage: {
      local: {
        get: async (key: string) => ({ [key]: storage[key] }),
        set: async (values: Record<string, unknown>) => {
          Object.assign(storage, values)
        },
        remove: async (key: string) => {
          delete storage[key]
        },
      },
    },
  } as unknown as typeof chrome
})

describe('extension API token storage', () => {
  test('stores trimmed tokens and removes blank values', async () => {
    await expect(setApiToken('  secret-token  ')).resolves.toBe(true)
    expect(storage[API_TOKEN_STORAGE_KEY]).toBe('secret-token')
    await expect(readApiToken()).resolves.toBe('secret-token')

    await expect(setApiToken('   ')).resolves.toBe(false)
    expect(storage[API_TOKEN_STORAGE_KEY]).toBeUndefined()
    await expect(readApiToken()).resolves.toBe('')
  })

  test('rejects tokens that Web Headers cannot encode', async () => {
    await expect(setApiToken('中文-token')).rejects.toThrow('visible ASCII')
    expect(storage[API_TOKEN_STORAGE_KEY]).toBeUndefined()
  })

  test('discovery health probes never attach Authorization', async () => {
    const calls: Array<RequestInit | undefined> = []
    globalThis.fetch = (async (_input: RequestInfo | URL, init?: RequestInit) => {
      calls.push(init)
      return new Response('Refine cloud API', { status: 200 })
    }) as typeof fetch

    await expect(discoverCloudApiBase()).resolves.toBe('http://localhost:21567')
    expect(calls).toHaveLength(1)
    expect(new Headers(calls[0]?.headers).has('Authorization')).toBe(false)
  })
})
