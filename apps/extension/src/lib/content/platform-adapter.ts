import type { QuickSaveTarget } from './quick-save-engine'
import { delay, normalizeText } from './runtime'

export interface ConversationTurnSelector {
  role: 'Human' | 'Assistant'
  selector: string
}

export interface StandardQuickSaveResolvers {
  resolveConversationTarget: (link: HTMLAnchorElement) => QuickSaveTarget | null
  getConversationTitleFromLink: (link: HTMLAnchorElement) => string
  isSameConversation: (target: QuickSaveTarget) => boolean
  isCurrentConversation: (conversationKey: string) => boolean
  navigateToConversation: (_link: HTMLAnchorElement, target: QuickSaveTarget) => Promise<boolean>
}

interface ConversationTurn {
  role: 'Human' | 'Assistant'
  el: Element
}

const DEFAULT_POLL_INTERVAL_MS = 350
const DEFAULT_POLL_TIMEOUT_MS = 20_000

export const DEFAULT_INVALID_CONVERSATION_IDS: ReadonlySet<string> = new Set([
  '',
  'none',
  'null',
  'undefined',
  'new',
  'new_chat',
  'newchat',
])

function sortTurnsByDocumentOrder(turns: ConversationTurn[]): ConversationTurn[] {
  return turns.sort((a, b) => {
    const position = a.el.compareDocumentPosition(b.el)
    if (position & Node.DOCUMENT_POSITION_FOLLOWING) return -1
    if (position & Node.DOCUMENT_POSITION_PRECEDING) return 1
    return 0
  })
}

export function extractConversationBySelectors(
  turnSelectors: ConversationTurnSelector[],
  root: ParentNode = document
): string {
  const turns: ConversationTurn[] = []

  for (const turnSelector of turnSelectors) {
    const nodes = root.querySelectorAll(turnSelector.selector)
    nodes.forEach((el) => {
      turns.push({
        role: turnSelector.role,
        el,
      })
    })
  }

  const seen = new Set<Element>()
  const orderedTurns = sortTurnsByDocumentOrder(turns).filter((turn) => {
    if (seen.has(turn.el)) return false
    seen.add(turn.el)
    return true
  })

  const messages: string[] = []
  for (const { role, el } of orderedTurns) {
    const content = normalizeText((el as HTMLElement).innerText || el.textContent || '')
    if (!content) continue
    messages.push(`${role}: ${content}`)
  }

  return messages.join('\n\n')
}

export async function waitForConversationExtraction(
  extractConversation: () => string,
  options?: {
    timeoutMs?: number
    intervalMs?: number
  }
): Promise<string | null> {
  const timeoutMs = options?.timeoutMs ?? DEFAULT_POLL_TIMEOUT_MS
  const intervalMs = options?.intervalMs ?? DEFAULT_POLL_INTERVAL_MS
  const startedAt = Date.now()

  while (Date.now() - startedAt <= timeoutMs) {
    const content = extractConversation()
    if (content) return content
    await delay(intervalMs)
  }

  return null
}

export function getNormalizedConversationTitleFromLink(
  link: HTMLAnchorElement,
  fallbackTitle: string
): string {
  const clone = link.cloneNode(true) as HTMLElement
  clone.querySelectorAll('.refine-quick-save-btn').forEach((button) => button.remove())
  const title = normalizeText(clone.textContent || '')
  return title || fallbackTitle
}

export function normalizeConversationId(
  rawId: string | null | undefined,
  invalidIds: ReadonlySet<string>
): string | null {
  if (!rawId) return null

  let decoded = rawId
  try {
    decoded = decodeURIComponent(rawId)
  } catch {
    decoded = rawId
  }

  const normalized = decoded.trim()
  if (!normalized) return null
  if (invalidIds.has(normalized.toLowerCase())) return null
  return normalized
}

export function resolveConversationPathKey(
  pathname: string,
  patterns: RegExp[],
  invalidIds: ReadonlySet<string> = DEFAULT_INVALID_CONVERSATION_IDS
): string | null {
  for (const pattern of patterns) {
    const matched = pathname.match(pattern)
    if (!matched) continue
    const normalized = normalizeConversationId(matched[1], invalidIds)
    if (normalized) return `path:${normalized}`
  }
  return null
}

export function resolveConversationQueryKey(
  searchParams: URLSearchParams,
  keys: readonly string[],
  invalidIds: ReadonlySet<string> = DEFAULT_INVALID_CONVERSATION_IDS
): string | null {
  for (const key of keys) {
    const value = searchParams.get(key)
    const normalized = normalizeConversationId(value, invalidIds)
    if (normalized) return `query:${normalized}`
  }
  return null
}

export function resolveQuickSaveTargetFromLink(
  link: HTMLAnchorElement,
  getConversationKey: (rawUrl: string) => string | null
): QuickSaveTarget | null {
  const href = link.getAttribute('href')
  if (!href) return null

  try {
    const resolved = new URL(href, window.location.origin)
    if (resolved.origin !== window.location.origin) return null

    const conversationKey = getConversationKey(resolved.toString())
    if (!conversationKey) return null

    return {
      url: resolved.toString(),
      conversationKey,
    }
  } catch {
    return null
  }
}

export function isSameConversationByKeyResolver(
  target: QuickSaveTarget,
  getConversationKey: (rawUrl: string) => string | null
): boolean {
  const currentKey = getConversationKey(window.location.href)
  return !!currentKey && currentKey === target.conversationKey
}

export function isCurrentConversationByKeyResolver(
  conversationKey: string,
  getConversationKey: (rawUrl: string) => string | null
): boolean {
  const currentKey = getConversationKey(window.location.href)
  return !!currentKey && currentKey === conversationKey
}

export async function navigateToQuickSaveTarget(target: QuickSaveTarget): Promise<boolean> {
  try {
    window.location.assign(target.url)
    return true
  } catch {
    return false
  }
}

export function createStandardQuickSaveResolvers(options: {
  getConversationKey: (rawUrl: string) => string | null
  fallbackTitle: string
}): StandardQuickSaveResolvers {
  return {
    resolveConversationTarget: (link) => resolveQuickSaveTargetFromLink(link, options.getConversationKey),
    getConversationTitleFromLink: (link) =>
      getNormalizedConversationTitleFromLink(link, document.title || options.fallbackTitle),
    isSameConversation: (target) => isSameConversationByKeyResolver(target, options.getConversationKey),
    isCurrentConversation: (conversationKey) =>
      isCurrentConversationByKeyResolver(conversationKey, options.getConversationKey),
    navigateToConversation: (_link, target) => navigateToQuickSaveTarget(target),
  }
}
