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
  navigateToQuickSaveTarget,
  resolveQuickSaveTargetFromLink,
  waitForConversationExtraction,
} from '../lib/content/platform-adapter'
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

export default function ClaudeContent() {
  return null
}
