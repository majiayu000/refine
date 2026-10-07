import type { TrackEventRequest } from './cloud-contract'
import { withRequestDeadline } from './request-deadline'

export const CONTENT_EVENT_ACTION = 'trackRecommendationEvent'
const EVENT_NAMES = new Set(['recommendation_exposed', 'recommendation_clicked', 'knowledge_reused'])

function matchesSource(source: string, rawUrl: string): boolean {
  try {
    const { protocol, hostname, pathname } = new URL(rawUrl)
    if (protocol !== 'https:') return false
    if (source === 'chatgpt') return hostname === 'chatgpt.com' || hostname === 'chat.openai.com'
    if (source === 'claude') return hostname === 'claude.ai'
    if (source === 'gemini') return hostname === 'gemini.google.com'
    if (source !== 'grok') return false
    return hostname === 'grok.com' || hostname.endsWith('.grok.com') ||
      ((hostname === 'x.com' || hostname === 'twitter.com') && pathname.startsWith('/i/grok')) ||
      ((hostname === 'x.ai' || hostname.endsWith('.x.ai')) && pathname.startsWith('/grok'))
  } catch {
    return false
  }
}

/** Fixed event contract: no caller-controlled URL, raw query or conversation text. */
export function validateContentEvent(value: unknown, senderUrl: string): TrackEventRequest | null {
  if (!value || typeof value !== 'object') return null
  const event = value as Record<string, unknown>
  if (typeof event.event_name !== 'string' || !EVENT_NAMES.has(event.event_name)) return null
  if (typeof event.source !== 'string' || !matchesSource(event.source, senderUrl)) return null
  const raw = event.properties && typeof event.properties === 'object'
    ? event.properties as Record<string, unknown> : {}
  const properties: Record<string, unknown> = { provider: event.source }
  for (const key of ['query_length', 'item_count']) {
    const value = raw[key]
    if (typeof value === 'number' && Number.isSafeInteger(value) && value >= 0) properties[key] = value
  }
  for (const key of ['item_id', 'item_type']) {
    const value = raw[key]
    if (typeof value === 'string' && value.length <= 200) properties[key] = value
  }
  if (raw.action === 'copy' || raw.action === 'insert') properties.action = raw.action
  return { event_name: event.event_name, source: event.source, properties, occurred_at: new Date().toISOString() }
}

export async function trackContentEvent(event: TrackEventRequest, tabId?: number): Promise<boolean> {
  try {
    // Content-script fetch runs with the host page's origin. Only the service
    // worker performs the authenticated request, using its existing host grant.
    return await withRequestDeadline(() => new Promise<boolean>((resolve) => {
      chrome.runtime.sendMessage({ action: CONTENT_EVENT_ACTION, event, tabId }, (response?: { ok?: boolean }) => {
        resolve(!chrome.runtime.lastError && response?.ok === true)
      })
    }), 12_000)
  } catch {
    return false
  }
}
