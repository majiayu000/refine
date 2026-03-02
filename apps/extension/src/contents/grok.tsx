/**
 * Grok Content Script
 *
 * 在 Grok 页面注入提取功能和侧边栏快捷入库按钮
 */

import type { PlasmoCSConfig } from 'plasmo'
import { initQuickSaveEngine } from '../lib/content/quick-save-engine'
import {
  createStandardQuickSaveResolvers,
  DEFAULT_CONVERSATION_QUERY_PARAM_KEYS,
  DEFAULT_INVALID_CONVERSATION_IDS,
  extractConversationBySelectors,
  resolveConversationPathKey,
  resolveConversationQueryKey,
  waitForConversationExtraction,
} from '../lib/content/platform-adapter'
import {
  STANDARD_QUICK_SAVE_BUTTON_COPY,
  STANDARD_QUICK_SAVE_PILL_STYLE_CSS,
  standardQuickSaveNavigateFallbackToast,
} from '../lib/content/quick-save-presets'
import { initRecommendationEngine } from '../lib/content/recommendation-engine'
import { delay, normalizeText } from '../lib/content/runtime'

const MESSAGE_POLL_TIMEOUT_MS = 20_000
const HISTORY_SCROLL_MAX_ROUNDS = 24
const HISTORY_SCROLL_INTERVAL_MS = 350
const HISTORY_SCROLL_STABLE_ROUNDS = 3
const GROK_PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import_grok'
const GROK_IMPORTED_CONVERSATIONS_KEY = '__refine_imported_conversations_grok'
const QUERY_CONVERSATION_PARAM_KEYS = [
  ...DEFAULT_CONVERSATION_QUERY_PARAM_KEYS,
  'cid',
  'conversation',
  'conversationKey',
  'chatId',
  'threadId',
] as const

export const config: PlasmoCSConfig = {
  matches: [
    'https://grok.com/*',
    'https://*.grok.com/*',
    'https://x.com/i/grok*',
    'https://twitter.com/i/grok*',
    'https://x.ai/grok*',
    'https://*.x.ai/grok*',
  ],
  all_frames: false,
}

initRecommendationEngine({
  providerId: 'grok',
  source: 'grok',
  inputSelectors: [
    'textarea',
    'textarea[placeholder*="Ask"]',
    'textarea[placeholder*="消息"]',
    'input[type="text"]',
    'input[type="search"]',
    'input[placeholder*="Ask"]',
    'input[placeholder*="消息"]',
    'div[contenteditable="true"]',
    'div[role="textbox"]',
    'div[contenteditable="true"][role="textbox"]',
    'div[contenteditable="true"][data-lexical-editor="true"]',
  ],
})

function getConversationKey(rawUrl: string): string | null {
  try {
    const parsed = new URL(rawUrl, window.location.origin)
    return (
      resolveConversationPathKey(
        parsed.pathname,
        [
          /\/(?:c|chat|conversation|conversations)\/([^/?#]+)/i,
          /\/i\/grok\/(?:c|chat|conversation|conversations|s)\/([^/?#]+)/i,
        ],
        DEFAULT_INVALID_CONVERSATION_IDS
      ) ||
      resolveConversationQueryKey(
        parsed.searchParams,
        QUERY_CONVERSATION_PARAM_KEYS,
        DEFAULT_INVALID_CONVERSATION_IDS
      )
    )
  } catch {
    return null
  }
}

const {
  resolveConversationTarget,
  getConversationTitleFromLink,
  isSameConversation,
  isCurrentConversation,
  navigateToConversation,
} = createStandardQuickSaveResolvers({
  getConversationKey,
  fallbackTitle: 'Grok Conversation',
})

const GROK_TURN_SELECTORS = [
  { role: 'Human' as const, selector: 'div.message-bubble[data-role="user"]' },
  { role: 'Assistant' as const, selector: 'div.message-bubble[data-role="assistant"]' },
  { role: 'Human' as const, selector: 'main div.message-bubble.user' },
  { role: 'Assistant' as const, selector: 'main div.message-bubble.assistant' },
  { role: 'Human' as const, selector: 'main [data-testid="conversation-turn-user"]' },
  { role: 'Human' as const, selector: 'main [data-testid*="user-message"]' },
  { role: 'Human' as const, selector: 'main [data-message-author-role="user"]' },
  { role: 'Human' as const, selector: 'main [data-author="user"]' },
  { role: 'Human' as const, selector: 'main [data-role="user"]' },
  { role: 'Human' as const, selector: 'main [class*="user-message"]' },
  { role: 'Assistant' as const, selector: 'main [data-testid="conversation-turn-assistant"]' },
  { role: 'Assistant' as const, selector: 'main [data-testid*="assistant-message"]' },
  { role: 'Assistant' as const, selector: 'main [data-message-author-role="assistant"]' },
  { role: 'Assistant' as const, selector: 'main [data-author="assistant"]' },
  { role: 'Assistant' as const, selector: 'main [data-role="assistant"]' },
  { role: 'Assistant' as const, selector: 'main [class*="assistant-message"]' },
]

function extractConversationByKnownSelectors(): string {
  return extractConversationBySelectors(GROK_TURN_SELECTORS, document)
}

function inferBubbleRole(bubble: Element, index: number): 'Human' | 'Assistant' {
  const text = [
    bubble.getAttribute('data-role') || '',
    bubble.getAttribute('data-author') || '',
    bubble.getAttribute('data-testid') || '',
    bubble.className || '',
  ]
    .join(' ')
    .toLowerCase()

  if (text.includes('user') || text.includes('human')) return 'Human'
  if (text.includes('assistant') || text.includes('grok') || text.includes('model') || text.includes('bot')) {
    return 'Assistant'
  }

  return index % 2 === 0 ? 'Human' : 'Assistant'
}

function extractConversationByBubbleFallback(): string {
  const container = document.querySelector('div#last-reply-container')?.parentElement
  if (!container) return ''

  const bubbles = Array.from(container.querySelectorAll('div.message-bubble'))
  if (bubbles.length === 0) return ''

  const messages: string[] = []
  bubbles.forEach((bubble, index) => {
    const content = (bubble as HTMLElement).innerText || bubble.textContent || ''
    const normalized = normalizeText(content)
    if (!normalized) return

    const role = inferBubbleRole(bubble, index)
    messages.push(`${role}: ${normalized}`)
  })

  return messages.join('\n\n')
}

function pickRicherContent(primary: string, secondary: string): string {
  if (!primary) return secondary
  if (!secondary) return primary
  return secondary.length > primary.length ? secondary : primary
}

function countRenderedTurns(): number {
  const selectorSet = new Set<string>(GROK_TURN_SELECTORS.map((entry) => entry.selector))
  selectorSet.add('div.message-bubble')

  const seen = new Set<Element>()
  for (const selector of selectorSet) {
    document.querySelectorAll(selector).forEach((el) => seen.add(el))
  }
  return seen.size
}

function isScrollableElement(el: HTMLElement): boolean {
  if (el.scrollHeight <= el.clientHeight + 8) return false
  const style = window.getComputedStyle(el)
  const overflowY = style.overflowY || style.overflow
  return overflowY === 'auto' || overflowY === 'scroll'
}

function findConversationScrollContainer(): HTMLElement | null {
  const candidateTurns = Array.from(document.querySelectorAll('div.message-bubble, [data-message-author-role], [data-testid*="conversation-turn"]'))
  const scored = new Map<HTMLElement, number>()

  for (const turn of candidateTurns) {
    let cursor = turn.parentElement
    while (cursor) {
      if (isScrollableElement(cursor)) {
        scored.set(cursor, (scored.get(cursor) ?? 0) + 1)
      }
      cursor = cursor.parentElement
    }
  }

  let best: HTMLElement | null = null
  let bestScore = -1
  for (const [container, score] of scored.entries()) {
    if (score > bestScore) {
      best = container
      bestScore = score
    }
  }

  if (best) return best

  const scrollingEl = document.scrollingElement
  return scrollingEl instanceof HTMLElement ? scrollingEl : null
}

async function loadConversationHistoryForCurrentUrl(): Promise<void> {
  const scroller = findConversationScrollContainer()
  if (!scroller) return

  let stableRounds = 0
  let lastSignature = ''

  for (let round = 0; round < HISTORY_SCROLL_MAX_ROUNDS; round += 1) {
    if (scroller === document.scrollingElement) {
      window.scrollTo(0, 0)
    } else {
      scroller.scrollTop = 0
    }

    await delay(HISTORY_SCROLL_INTERVAL_MS)

    const turns = countRenderedTurns()
    const signature = `${turns}:${scroller.scrollHeight}:${scroller.scrollTop}`
    if (signature === lastSignature) {
      stableRounds += 1
    } else {
      stableRounds = 0
      lastSignature = signature
    }

    if (stableRounds >= HISTORY_SCROLL_STABLE_ROUNDS) {
      return
    }
  }
}

function extractConversation(): string {
  const fromKnownSelectors = extractConversationByKnownSelectors()
  const fromFallback = extractConversationByBubbleFallback()
  return pickRicherContent(fromKnownSelectors, fromFallback)
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  await loadConversationHistoryForCurrentUrl()
  return waitForConversationExtraction(extractConversation, {
    timeoutMs,
    stableForMs: 1_800,
  })
}

initQuickSaveEngine({
  providerId: 'grok',
  source: 'grok',
  linkSelector:
    'aside a[href], nav a[href], [role="navigation"] a[href], a[href*="/c/"], a[href*="/chat/"], a[href*="/conversation/"], a[href*="/i/grok/"]',
  pendingStorageKey: GROK_PENDING_SIDEBAR_IMPORT_KEY,
  importedStorageKey: GROK_IMPORTED_CONVERSATIONS_KEY,
  resolveTarget: resolveConversationTarget,
  resolveHost: (link) =>
    link.closest<HTMLElement>('li, [role="listitem"], [data-testid*="conversation"], [data-test-id*="conversation"]') ||
    link,
  getConversationTitleFromLink,
  isSameConversation,
  isCurrentConversation,
  navigateToTarget: navigateToConversation,
  extractConversation,
  waitForConversationContent,
  getConversationKeyFromUrl: getConversationKey,
  buttonCopy: STANDARD_QUICK_SAVE_BUTTON_COPY,
  messages: {
    navigateFallbackToast: standardQuickSaveNavigateFallbackToast,
  },
  styleCss: STANDARD_QUICK_SAVE_PILL_STYLE_CSS,
})

export default function GrokContent() {
  return null
}
