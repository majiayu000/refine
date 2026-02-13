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
import { normalizeText } from '../lib/content/runtime'

const MESSAGE_POLL_TIMEOUT_MS = 20_000
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

function extractConversationByKnownSelectors(): string {
  return extractConversationBySelectors(
    [
      { role: 'Human', selector: 'div.message-bubble[data-role="user"]' },
      { role: 'Assistant', selector: 'div.message-bubble[data-role="assistant"]' },
      { role: 'Human', selector: 'main div.message-bubble.user' },
      { role: 'Assistant', selector: 'main div.message-bubble.assistant' },
      { role: 'Human', selector: 'main [data-testid="conversation-turn-user"]' },
      { role: 'Human', selector: 'main [data-testid*="user-message"]' },
      { role: 'Human', selector: 'main [data-message-author-role="user"]' },
      { role: 'Human', selector: 'main [data-author="user"]' },
      { role: 'Human', selector: 'main [data-role="user"]' },
      { role: 'Human', selector: 'main [class*="user-message"]' },
      { role: 'Assistant', selector: 'main [data-testid="conversation-turn-assistant"]' },
      { role: 'Assistant', selector: 'main [data-testid*="assistant-message"]' },
      { role: 'Assistant', selector: 'main [data-message-author-role="assistant"]' },
      { role: 'Assistant', selector: 'main [data-author="assistant"]' },
      { role: 'Assistant', selector: 'main [data-role="assistant"]' },
      { role: 'Assistant', selector: 'main [class*="assistant-message"]' },
    ],
    document
  )
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

function extractConversation(): string {
  const fromKnownSelectors = extractConversationByKnownSelectors()
  if (fromKnownSelectors) return fromKnownSelectors

  return extractConversationByBubbleFallback()
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  return waitForConversationExtraction(extractConversation, { timeoutMs })
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
