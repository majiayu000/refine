/**
 * Grok Content Script
 *
 * 在 Grok 页面注入提取功能和侧边栏快捷入库按钮
 */

import type { PlasmoCSConfig } from 'plasmo'
import {
  initQuickSaveEngine,
  type QuickSaveTarget,
} from '../lib/content/quick-save-engine'
import {
  extractConversationBySelectors,
  getNormalizedConversationTitleFromLink,
  navigateToQuickSaveTarget,
  resolveQuickSaveTargetFromLink,
  waitForConversationExtraction,
} from '../lib/content/platform-adapter'
import { initRecommendationEngine } from '../lib/content/recommendation-engine'

const MESSAGE_POLL_TIMEOUT_MS = 20_000
const GROK_PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import_grok'
const GROK_IMPORTED_CONVERSATIONS_KEY = '__refine_imported_conversations_grok'
const INVALID_CONVERSATION_IDS = new Set(['', 'new', 'new_chat', 'newchat', 'null', 'none', 'undefined'])

export const config: PlasmoCSConfig = {
  matches: ['https://grok.com/*'],
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

function decodeId(input: string): string {
  try {
    return decodeURIComponent(input)
  } catch {
    return input
  }
}

function isValidConversationId(input: string | null | undefined): input is string {
  if (!input) return false
  const normalized = decodeId(input).trim().toLowerCase()
  return normalized.length > 0 && !INVALID_CONVERSATION_IDS.has(normalized)
}

function getConversationKey(rawUrl: string): string | null {
  try {
    const parsed = new URL(rawUrl, window.location.origin)
    const pathMatch = parsed.pathname.match(/\/(?:c|chat)\/([^/?#]+)/i)
    if (!pathMatch) return null

    const id = decodeId(pathMatch[1]).trim()
    if (!isValidConversationId(id)) return null
    return `path:${id}`
  } catch {
    return null
  }
}

function extractConversation(): string {
  return extractConversationBySelectors([
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
  ])
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  return waitForConversationExtraction(extractConversation, { timeoutMs })
}

function resolveConversationTarget(link: HTMLAnchorElement): QuickSaveTarget | null {
  return resolveQuickSaveTargetFromLink(link, getConversationKey)
}

function getConversationTitleFromLink(link: HTMLAnchorElement): string {
  return getNormalizedConversationTitleFromLink(link, document.title || 'Grok Conversation')
}

function isSameConversation(target: QuickSaveTarget): boolean {
  const currentKey = getConversationKey(window.location.href)
  return !!currentKey && currentKey === target.conversationKey
}

function isCurrentConversation(conversationKey: string): boolean {
  const currentKey = getConversationKey(window.location.href)
  return !!currentKey && currentKey === conversationKey
}

async function navigateToConversation(_link: HTMLAnchorElement, target: QuickSaveTarget): Promise<boolean> {
  return navigateToQuickSaveTarget(target)
}

initQuickSaveEngine({
  providerId: 'grok',
  source: 'grok',
  linkSelector: 'aside a[href], nav a[href], [role="navigation"] a[href], a[href^="/c/"]',
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
  buttonCopy: {
    idleText: '入库',
    savingText: '入库中',
    doneText: '已入库',
    importedText: '已入库',
    errorText: '失败',
  },
  messages: {
    navigateFallbackToast: () => '正在打开会话，加载后自动入库...',
  },
  styleCss: `
    .refine-quick-save-host {
      position: relative !important;
      padding-right: 56px !important;
    }

    .refine-quick-save-btn {
      position: absolute;
      top: 50%;
      right: 8px;
      transform: translateY(-50%);
      height: 22px;
      min-width: 38px;
      padding: 0 8px;
      border-radius: 999px;
      border: 1px solid rgba(148, 163, 184, 0.38);
      background: rgba(15, 23, 42, 0.86);
      color: #e2e8f0;
      font-size: 11px;
      line-height: 20px;
      cursor: pointer;
      opacity: 0;
      pointer-events: none;
      transition: opacity 0.18s ease, background-color 0.18s ease, border-color 0.18s ease, color 0.18s ease;
      z-index: 2;
    }

    .refine-quick-save-host:hover .refine-quick-save-btn,
    .refine-quick-save-host:focus-within .refine-quick-save-btn,
    .refine-quick-save-btn[data-state="saving"],
    .refine-quick-save-btn[data-state="done"],
    .refine-quick-save-btn[data-state="imported"],
    .refine-quick-save-btn[data-state="error"] {
      opacity: 1;
      pointer-events: auto;
    }

    .refine-quick-save-btn:hover {
      background: rgba(30, 41, 59, 0.92);
      border-color: rgba(148, 163, 184, 0.56);
    }

    .refine-quick-save-btn[data-state="saving"] {
      color: #bfdbfe;
      border-color: rgba(96, 165, 250, 0.56);
    }

    .refine-quick-save-btn[data-state="done"],
    .refine-quick-save-btn[data-state="imported"] {
      color: #86efac;
      border-color: rgba(52, 211, 153, 0.62);
    }

    .refine-quick-save-btn[data-state="error"] {
      color: #fca5a5;
      border-color: rgba(248, 113, 113, 0.62);
    }
  `,
})

export default function GrokContent() {
  return null
}
