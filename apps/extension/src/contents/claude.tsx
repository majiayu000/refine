/**
 * Claude Content Script
 *
 * 在 Claude.ai 页面注入提取功能和侧边栏快捷入库按钮
 */

import type { PlasmoCSConfig } from 'plasmo'
import {
  initQuickSaveEngine,
  type QuickSaveTarget,
} from '../lib/content/quick-save-engine'
import {
  extractConversationBySelectors,
  getNormalizedConversationTitleFromLink,
  isCurrentConversationByKeyResolver,
  isSameConversationByKeyResolver,
  navigateToQuickSaveTarget,
  resolveQuickSaveTargetFromLink,
  waitForConversationExtraction,
} from '../lib/content/platform-adapter'
import {
  STANDARD_QUICK_SAVE_BUTTON_COPY,
  STANDARD_QUICK_SAVE_PILL_STYLE_CSS,
  standardQuickSaveNavigateFallbackToast,
} from '../lib/content/quick-save-presets'
import { initRecommendationEngine } from '../lib/content/recommendation-engine'

const MESSAGE_POLL_TIMEOUT_MS = 20_000

const CLAUDE_PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import'
const CLAUDE_IMPORTED_CONVERSATIONS_KEY = '__refine_imported_conversations_claude'

export const config: PlasmoCSConfig = {
  matches: ['https://claude.ai/*'],
  all_frames: false,
}

initRecommendationEngine({
  providerId: 'claude',
  source: 'claude',
  inputSelectors: [
    'div[contenteditable="true"][role="textbox"]',
    'div[contenteditable="true"][data-lexical-editor="true"]',
    'textarea[placeholder*="Talk"]',
    'textarea[placeholder*="消息"]',
  ],
})

function extractConversation(): string {
  return extractConversationBySelectors([
    { role: 'Human', selector: '[data-testid="human-message"]' },
    { role: 'Assistant', selector: '[data-testid="assistant-message"]' },
  ])
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  return waitForConversationExtraction(extractConversation, { timeoutMs })
}

function getConversationPath(rawUrl: string): string | null {
  try {
    const parsed = new URL(rawUrl, window.location.origin)
    const matched = parsed.pathname.match(/\/chat\/[^/?#]+/i)
    return matched ? matched[0] : null
  } catch {
    return null
  }
}

function getConversationKey(rawUrl: string): string | null {
  return getConversationPath(rawUrl)
}

function resolveConversationTarget(link: HTMLAnchorElement): QuickSaveTarget | null {
  return resolveQuickSaveTargetFromLink(link, getConversationKey)
}

function getConversationTitleFromLink(link: HTMLAnchorElement): string {
  return getNormalizedConversationTitleFromLink(link, document.title || 'Claude Conversation')
}

function isSameConversation(target: QuickSaveTarget): boolean {
  return isSameConversationByKeyResolver(target, getConversationKey)
}

function isCurrentConversation(conversationKey: string): boolean {
  return isCurrentConversationByKeyResolver(conversationKey, getConversationKey)
}

async function navigateToConversation(_link: HTMLAnchorElement, target: QuickSaveTarget): Promise<boolean> {
  return navigateToQuickSaveTarget(target)
}

initQuickSaveEngine({
  providerId: 'claude',
  source: 'claude',
  linkSelector: 'aside a[href], nav a[href]',
  pendingStorageKey: CLAUDE_PENDING_SIDEBAR_IMPORT_KEY,
  importedStorageKey: CLAUDE_IMPORTED_CONVERSATIONS_KEY,
  resolveTarget: resolveConversationTarget,
  resolveHost: (link) => link,
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

export default function ClaudeContent() {
  return null
}
