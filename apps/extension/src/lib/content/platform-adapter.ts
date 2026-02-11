import type { QuickSaveTarget } from './quick-save-engine'
import { delay, normalizeText } from './runtime'

export interface ConversationTurnSelector {
  role: 'Human' | 'Assistant'
  selector: string
}

interface ConversationTurn {
  role: 'Human' | 'Assistant'
  el: Element
}

const DEFAULT_POLL_INTERVAL_MS = 350
const DEFAULT_POLL_TIMEOUT_MS = 20_000

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

export function resolveQuickSaveTargetFromLink(
  link: HTMLAnchorElement,
  getConversationKey: (rawUrl: string) => string | null
): QuickSaveTarget | null {
  const href = link.getAttribute('href')
  if (!href) return null

  try {
    const resolved = new URL(href, window.location.origin)
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

export async function navigateToQuickSaveTarget(target: QuickSaveTarget): Promise<boolean> {
  try {
    window.location.assign(target.url)
    return true
  } catch {
    return false
  }
}
