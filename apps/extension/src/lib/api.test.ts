import { beforeEach, describe, expect, test } from 'bun:test'

import {
  checkCloudHealth,
  fetchCloudTotalItems,
  fetchQuotaStatus,
  fetchRecommendations,
  trackEvent,
  uploadConversation,
} from './api'
import { API_TOKEN_STORAGE_KEY } from './config'
import type { OutboxItem } from './types'

interface FetchCall {
  url: string
  authorization: string | null
}

let token = ''
let storageGate: Promise<void> | null = null
let calls: FetchCall[] = []

function json(data: unknown): Response {
  return new Response(JSON.stringify(data), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

beforeEach(() => {
  token = ''
  storageGate = null
  calls = []
  process.env.PLASMO_PUBLIC_REFINE_API_BASE = 'http://refine.test'
  ;(globalThis as typeof globalThis & { chrome: typeof chrome }).chrome = {
    storage: {
      local: {
        get: async (key: string) => {
          if (storageGate) await storageGate
          return { [key]: key === API_TOKEN_STORAGE_KEY ? token : undefined }
        },
      },
    },
  } as unknown as typeof chrome

  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    calls.push({
      url,
      authorization: new Headers(init?.headers).get('Authorization'),
    })
    if (url.endsWith('/health')) return json({ success: true })
    if (url.includes('/v1/conversations')) return json({ success: true, conversation_id: 'c1' })
    if (url.includes('/v1/items')) return json({ success: true, total: 4 })
    if (url.includes('/v1/recommendations')) {
      return json({ success: true, triggered: true, query: 'q', items: [] })
    }
    if (url.includes('/v1/quota')) {
      return json({ success: true, limit: null, used: 1, remaining: null, exceeded: false })
    }
    if (url.includes('/v1/events')) return json({ success: true })
    return json({ success: false })
  }) as typeof fetch
})

const outboxItem: OutboxItem = {
  id: 'outbox-1',
  idempotencyKey: 'key-1',
  payload: {
    content: 'conversation',
    url: 'https://example.test/chat',
    source: 'chatgpt',
    capturedAt: 1,
  },
  status: 'pending',
  attemptCount: 0,
  nextAttemptAt: 0,
  createdAt: 1,
  updatedAt: 1,
}

describe('extension protected request headers', () => {
  test('health is public while every protected endpoint receives the current token', async () => {
    token = 'first-token'
    await checkCloudHealth()
    await uploadConversation(outboxItem)
    await fetchCloudTotalItems()
    await fetchRecommendations('query')
    await fetchQuotaStatus()
    await trackEvent({ event_name: 'test' })

    expect(calls[0]).toEqual({ url: 'http://refine.test/health', authorization: null })
    expect(calls.slice(1)).toHaveLength(5)
    expect(calls.slice(1).every((call) => call.authorization === 'Bearer first-token')).toBe(true)

    token = 'rotated-token'
    await fetchCloudTotalItems()
    expect(calls.at(-1)?.authorization).toBe('Bearer rotated-token')

    token = '  '
    await fetchCloudTotalItems()
    expect(calls.at(-1)?.authorization).toBeNull()
  })

  test('awaits extension-local storage before starting a protected fetch', async () => {
    token = 'delayed-token'
    let release!: () => void
    storageGate = new Promise<void>((resolve) => {
      release = resolve
    })

    const pending = fetchCloudTotalItems()
    await Promise.resolve()
    expect(calls).toHaveLength(0)
    release()
    await pending
    expect(calls[0]?.authorization).toBe('Bearer delayed-token')
  })
})
