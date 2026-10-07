import { describe, expect, test } from 'bun:test'
import { CONTENT_EVENT_ACTION, trackContentEvent, validateContentEvent } from './content-events'

describe('background recommendation events', () => {
  test('content scripts send a fixed runtime message without calling fetch', async () => {
    const previousChrome = globalThis.chrome
    const previousFetch = globalThis.fetch
    const messages: unknown[] = []
    globalThis.chrome = { runtime: { sendMessage(message: unknown, reply: (result: { ok: boolean }) => void) {
      messages.push(message)
      reply({ ok: true })
    } } } as unknown as typeof chrome
    globalThis.fetch = (() => { throw new Error('content script must not fetch events') }) as unknown as typeof fetch
    const event = { event_name: 'recommendation_exposed', source: 'claude' }
    try {
      expect(await trackContentEvent(event)).toBe(true)
      expect(messages).toEqual([{ action: CONTENT_EVENT_ACTION, event }])
    } finally {
      globalThis.chrome = previousChrome
      globalThis.fetch = previousFetch
    }
  })

  test('validates sender/source and drops raw text or arbitrary network fields', () => {
    const event = { event_name: 'knowledge_reused', source: 'claude', properties: {
      action: 'copy', item_id: 'item-1', query: 'private text', url: 'https://other.test', provider: 'spoofed',
    } }
    const accepted = validateContentEvent(event, 'https://claude.ai/chat/example')
    expect(accepted?.properties).toEqual({ provider: 'claude', item_id: 'item-1', action: 'copy' })
    expect(validateContentEvent(event, 'https://chatgpt.com/c/example')).toBeNull()
    expect(validateContentEvent(event, 'https://claude.ai.evil.test/chat/example')).toBeNull()
    expect(validateContentEvent({ ...event, event_name: 'arbitrary' }, 'https://claude.ai/chat/example')).toBeNull()
  })

  test('preserves only finite nonnegative counters', () => {
    const accepted = validateContentEvent({ event_name: 'recommendation_exposed', source: 'chatgpt', properties: {
      query_length: 12, item_count: Infinity,
    } }, 'https://chatgpt.com/c/test')
    expect(accepted?.properties).toEqual({ provider: 'chatgpt', query_length: 12 })
  })
})
