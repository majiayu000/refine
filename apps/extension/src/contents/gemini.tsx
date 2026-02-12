/**
 * Gemini Content Script
 *
 * 在 Gemini 页面注入提取功能和侧边栏快捷入库按钮
 */

import type { PlasmoCSConfig } from 'plasmo'
import {
  initQuickSaveEngine,
  type QuickSaveTarget,
  type SilentExtractContentResult,
} from '../lib/content/quick-save-engine'
import {
  DEFAULT_INVALID_CONVERSATION_IDS,
  extractConversationBySelectors,
  getNormalizedConversationTitleFromLink,
  isCurrentConversationByKeyResolver,
  isSameConversationByKeyResolver,
  normalizeConversationId,
  resolveConversationPathKey,
  resolveConversationQueryKey,
  waitForConversationExtraction,
} from '../lib/content/platform-adapter'
import { delay, toErrorMessage } from '../lib/content/runtime'

interface DebugLogEntry {
  ts: string
  level: 'info' | 'warn' | 'error'
  event: string
  message: string
  context?: Record<string, string | number | boolean | null>
}

const GEMINI_PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import_gemini'
const GEMINI_IMPORTED_CONVERSATIONS_KEY = '__refine_imported_conversations_gemini'
const MESSAGE_POLL_INTERVAL_MS = 350
const MESSAGE_POLL_TIMEOUT_MS = 20_000
const HIDDEN_IFRAME_EXTRACT_TIMEOUT_MS = 18_000
const DEBUG_LOG_KEY = '__refine_gemini_quicksave_logs'
const DEBUG_LOG_LIMIT = 100
const GEMINI_TURN_SELECTORS = [
  { role: 'Human' as const, selector: 'main user-query' },
  { role: 'Human' as const, selector: 'main [data-turn-role="user"]' },
  { role: 'Human' as const, selector: 'main [data-source="user"]' },
  { role: 'Assistant' as const, selector: 'main model-response' },
  { role: 'Assistant' as const, selector: 'main [data-turn-role="model"]' },
  { role: 'Assistant' as const, selector: 'main [data-turn-role="assistant"]' },
  { role: 'Assistant' as const, selector: 'main [data-source="model"]' },
]

// Gemini 当前对 iframe 提取是策略级封禁（X-Frame-Options: deny），默认直接禁用该路径。
let hiddenIframeCapability: 'unknown' | 'supported' | 'blocked' = 'blocked'

export const config: PlasmoCSConfig = {
  matches: ['https://gemini.google.com/*'],
  all_frames: false,
}

function readDebugLogs(): DebugLogEntry[] {
  try {
    const raw = window.sessionStorage.getItem(DEBUG_LOG_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as DebugLogEntry[]
    if (!Array.isArray(parsed)) return []
    return parsed
  } catch {
    return []
  }
}

function writeDebugLogs(logs: DebugLogEntry[]): void {
  try {
    window.sessionStorage.setItem(DEBUG_LOG_KEY, JSON.stringify(logs.slice(-DEBUG_LOG_LIMIT)))
  } catch {
    // ignore storage errors
  }
}

function debugLog(
  level: 'info' | 'warn' | 'error',
  event: string,
  message: string,
  context?: Record<string, string | number | boolean | null>
): void {
  const entry: DebugLogEntry = {
    ts: new Date().toISOString(),
    level,
    event,
    message,
    context,
  }

  const logs = readDebugLogs()
  logs.push(entry)
  writeDebugLogs(logs)

  const printer =
    level === 'error' ? console.error : level === 'warn' ? console.warn : console.info
  printer(`[Refine Gemini] ${event}: ${message}`, context || '')
}

function extractConversationFromRoot(root: ParentNode = document): string {
  return extractConversationBySelectors(GEMINI_TURN_SELECTORS, root)
}

function extractConversation(): string {
  return extractConversationFromRoot(document)
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  return waitForConversationExtraction(extractConversation, {
    timeoutMs,
    intervalMs: MESSAGE_POLL_INTERVAL_MS,
  })
}

async function waitForConversationInDocument(
  doc: Document,
  timeoutMs = HIDDEN_IFRAME_EXTRACT_TIMEOUT_MS
): Promise<string | null> {
  return waitForConversationExtraction(() => extractConversationFromRoot(doc), {
    timeoutMs,
    intervalMs: MESSAGE_POLL_INTERVAL_MS,
  })
}

function extractAccountPrefix(pathname: string): string | null {
  const matched = pathname.match(/^\/u\/\d+/i)
  return matched ? matched[0] : null
}

function withCurrentAccountPrefix(url: URL): URL {
  const currentPrefix = extractAccountPrefix(window.location.pathname)
  if (!currentPrefix) return url

  const targetPrefix = extractAccountPrefix(url.pathname)
  if (targetPrefix && targetPrefix !== currentPrefix) {
    const withoutTargetPrefix = url.pathname.replace(/^\/u\/\d+/i, '')
    url.pathname = `${currentPrefix}${withoutTargetPrefix}`
    return url
  }

  if (!targetPrefix && url.pathname.startsWith('/app')) {
    url.pathname = `${currentPrefix}${url.pathname}`
  }

  return url
}

function getConversationKey(rawUrl: string): string | null {
  try {
    const parsed = withCurrentAccountPrefix(new URL(rawUrl, window.location.origin))
    const pathKey = resolveConversationPathKey(
      parsed.pathname,
      [/\/app\/([^/?#]+)/i, /\/chat\/([^/?#]+)/i],
      DEFAULT_INVALID_CONVERSATION_IDS
    )
    if (pathKey) return pathKey

    if (/\/(?:u\/\d+\/)?app\/?$/i.test(parsed.pathname)) {
      return resolveConversationQueryKey(
        parsed.searchParams,
        ['pageId', 'conversationId', 'conversation_id', 'id'],
        DEFAULT_INVALID_CONVERSATION_IDS
      )
    }

    return null
  } catch {
    return null
  }
}

function sanitizeConversationUrl(url: URL): URL {
  const normalized = new URL(url.toString())
  const maybeInvalidParamKeys = ['pageId', 'conversationId', 'conversation_id', 'id']

  for (const key of maybeInvalidParamKeys) {
    const value = normalized.searchParams.get(key)
    if (value && !normalizeConversationId(value, DEFAULT_INVALID_CONVERSATION_IDS)) {
      normalized.searchParams.delete(key)
    }
  }

  return normalized
}

function resolveConversationTarget(link: HTMLAnchorElement): QuickSaveTarget | null {
  const href = link.getAttribute('href')
  if (!href) {
    debugLog('warn', 'resolve_target', 'link has no href')
    return null
  }

  try {
    const resolved = withCurrentAccountPrefix(new URL(href, window.location.href))
    if (resolved.origin !== window.location.origin) {
      debugLog('warn', 'resolve_target', 'origin mismatch', {
        href: resolved.toString(),
      })
      return null
    }

    const sanitized = sanitizeConversationUrl(resolved)
    const conversationKey = getConversationKey(sanitized.toString())
    if (!conversationKey) {
      debugLog('warn', 'resolve_target', 'conversation key invalid', {
        href: sanitized.toString(),
      })
      return null
    }

    return {
      url: sanitized.toString(),
      conversationKey,
    }
  } catch (error) {
    debugLog('error', 'resolve_target', toErrorMessage(error), {
      href,
    })
    return null
  }
}

function isSameConversation(target: QuickSaveTarget): boolean {
  return isSameConversationByKeyResolver(target, getConversationKey)
}

function isCurrentConversation(conversationKey: string): boolean {
  return isCurrentConversationByKeyResolver(conversationKey, getConversationKey)
}

function getConversationTitleFromLink(link: HTMLAnchorElement): string {
  return getNormalizedConversationTitleFromLink(link, document.title || 'Gemini Conversation')
}

function isHiddenIframeBlocked(): boolean {
  return hiddenIframeCapability === 'blocked'
}

function waitForIframeLoad(iframe: HTMLIFrameElement): Promise<void> {
  return new Promise((resolve, reject) => {
    let settled = false

    const timeoutId = window.setTimeout(() => {
      if (settled) return
      settled = true
      cleanup()
      debugLog('warn', 'iframe_load_timeout', 'iframe load timed out')
      reject(new Error('iframe load timeout'))
    }, HIDDEN_IFRAME_EXTRACT_TIMEOUT_MS)

    const cleanup = () => {
      window.clearTimeout(timeoutId)
      iframe.removeEventListener('load', onLoad)
      iframe.removeEventListener('error', onError)
    }

    const onLoad = () => {
      if (settled) return
      settled = true
      cleanup()
      debugLog('info', 'iframe_load', 'iframe loaded')
      resolve()
    }

    const onError = () => {
      if (settled) return
      settled = true
      cleanup()
      debugLog('warn', 'iframe_load_error', 'iframe load error event fired')
      reject(new Error('iframe load error'))
    }

    iframe.addEventListener('load', onLoad, { once: true })
    iframe.addEventListener('error', onError, { once: true })
  })
}

async function extractConversationViaHiddenIframe(targetUrl: string): Promise<string | null> {
  if (!document.body) return null

  if (hiddenIframeCapability === 'blocked') {
    debugLog('warn', 'hidden_iframe_skip', 'hidden iframe mode already blocked by policy', {
      targetUrl,
    })
    return null
  }

  debugLog('info', 'hidden_iframe_start', 'start hidden iframe extraction', {
    targetUrl,
  })

  const iframe = document.createElement('iframe')
  iframe.className = 'refine-hidden-extract-frame'
  iframe.src = targetUrl
  iframe.setAttribute('aria-hidden', 'true')
  iframe.tabIndex = -1
  iframe.style.cssText = `
    position: fixed;
    left: -10000px;
    top: -10000px;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
    border: 0;
  `
  document.body.appendChild(iframe)

  try {
    await waitForIframeLoad(iframe)
    const frameDoc = iframe.contentDocument
    if (!frameDoc) {
      hiddenIframeCapability = 'blocked'
      debugLog('warn', 'hidden_iframe_doc', 'iframe contentDocument is null', {
        targetUrl,
        capability: hiddenIframeCapability,
      })
      return null
    }

    const extracted = await waitForConversationInDocument(frameDoc)
    if (!extracted) {
      hiddenIframeCapability = 'blocked'
      debugLog('warn', 'hidden_iframe_empty', 'conversation extraction returned empty', {
        targetUrl,
        capability: hiddenIframeCapability,
      })
      return null
    }

    hiddenIframeCapability = 'supported'
    debugLog('info', 'hidden_iframe_success', 'hidden iframe extraction succeeded', {
      targetUrl,
      length: extracted.length,
      capability: hiddenIframeCapability,
    })
    return extracted
  } catch (error) {
    hiddenIframeCapability = 'blocked'
    debugLog('warn', 'hidden_iframe_error', toErrorMessage(error), {
      targetUrl,
      capability: hiddenIframeCapability,
    })
    return null
  } finally {
    iframe.remove()
  }
}

async function tryExtractContentWithoutNavigation(
  target: QuickSaveTarget,
  _title: string
): Promise<SilentExtractContentResult> {
  if (isHiddenIframeBlocked()) {
    return {
      success: false,
      message: 'Gemini 安全策略禁止后台读取',
    }
  }

  const iframeContent = await extractConversationViaHiddenIframe(target.url)
  if (!iframeContent) {
    debugLog('warn', 'silent_import_fail', 'background extraction failed', {
      targetUrl: target.url,
      conversationKey: target.conversationKey,
      capability: hiddenIframeCapability,
    })

    return {
      success: false,
      message:
        isHiddenIframeBlocked()
          ? 'Gemini 安全策略禁止后台读取'
          : '后台读取失败，需要打开会话页再入库',
    }
  }

  return {
    success: true,
    content: iframeContent,
  }
}

async function waitForNavigationSignal(targetConversationKey: string, initialUrl: string): Promise<boolean> {
  const timeoutMs = 2_400
  const intervalMs = 120
  const startedAt = Date.now()

  while (Date.now() - startedAt <= timeoutMs) {
    const currentKey = getConversationKey(window.location.href)
    if (currentKey === targetConversationKey) return true
    if (window.location.href !== initialUrl) return true
    await delay(intervalMs)
  }

  return false
}

async function navigateToConversation(link: HTMLAnchorElement, target: QuickSaveTarget): Promise<boolean> {
  const initialUrl = window.location.href
  debugLog('info', 'nav_start', 'attempt navigation to target conversation', {
    from: initialUrl,
    to: target.url,
    conversationKey: target.conversationKey,
  })

  try {
    link.click()
  } catch (error) {
    debugLog('warn', 'nav_click_error', toErrorMessage(error), {
      to: target.url,
    })
  }

  const clickedNavigated = await waitForNavigationSignal(target.conversationKey, initialUrl)
  if (clickedNavigated) {
    debugLog('info', 'nav_click_success', 'navigation detected after link click', {
      to: target.url,
    })
    return true
  }

  try {
    window.location.assign(target.url)
    debugLog('info', 'nav_assign_fallback', 'fallback location.assign executed', {
      to: target.url,
    })
    return true
  } catch (error) {
    debugLog('error', 'nav_assign_error', toErrorMessage(error), {
      to: target.url,
    })
    return false
  }
}

function attachDebugHelpers(): void {
  const debugWindow = window as Window & {
    __refineGeminiQuickSaveLogs?: () => DebugLogEntry[]
    __refineGeminiClearQuickSaveLogs?: () => void
  }

  debugWindow.__refineGeminiQuickSaveLogs = () => readDebugLogs()
  debugWindow.__refineGeminiClearQuickSaveLogs = () => {
    writeDebugLogs([])
  }
}

attachDebugHelpers()
debugLog('info', 'init', 'gemini quick-save initialized', {
  url: window.location.href,
  hiddenIframeCapability,
})

initQuickSaveEngine({
  providerId: 'gemini',
  source: 'gemini',
  linkSelector: 'aside a[href], nav a[href], [role="navigation"] a[href]',
  pendingStorageKey: GEMINI_PENDING_SIDEBAR_IMPORT_KEY,
  importedStorageKey: GEMINI_IMPORTED_CONVERSATIONS_KEY,
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
  tryExtractContentWithoutNavigation,
  getConversationKeyFromUrl: getConversationKey,
  buttonCopy: {
    idleText: '☆',
    savingText: '…',
    doneText: '✓',
    importedText: '✓',
    errorText: '!',
    idleTitle: '入库此会话',
    savingTitle: '入库中',
    doneTitle: '已入库',
    importedTitle: '已入库',
    errorTitle: '入库失败',
  },
  messages: {
    navigateFallbackToast: (silentFailureMessage) =>
      silentFailureMessage === 'Gemini 安全策略禁止后台读取'
        ? 'Gemini 限制后台读取，正在打开会话后自动入库...'
        : '后台读取失败，正在打开会话后自动入库...',
  },
  styleCss: `
    .refine-quick-save-host {
      position: relative !important;
      overflow: visible !important;
      padding-right: 106px !important;
    }

    .refine-quick-save-btn {
      position: absolute;
      top: 50%;
      right: 70px;
      transform: translateY(-50%);
      width: 18px;
      height: 18px;
      padding: 0;
      border-radius: 999px;
      border: 1px solid rgba(148, 163, 184, 0.38);
      background: rgba(15, 23, 42, 0.62);
      color: #bfdbfe;
      font-size: 11px;
      line-height: 16px;
      font-weight: 700;
      cursor: pointer;
      opacity: 1;
      pointer-events: auto;
      box-shadow: 0 1px 4px rgba(2, 6, 23, 0.25);
      transition: transform 0.18s ease, box-shadow 0.18s ease, background-color 0.18s ease, border-color 0.18s ease, color 0.18s ease;
      z-index: 2;
    }

    .refine-quick-save-host:hover .refine-quick-save-btn,
    .refine-quick-save-host:focus-within .refine-quick-save-btn {
      transform: translateY(-50%) scale(1.05);
      box-shadow: 0 3px 9px rgba(2, 6, 23, 0.32);
    }

    .refine-quick-save-btn:hover {
      transform: translateY(-50%) scale(1.05);
      border-color: rgba(147, 197, 253, 0.5);
      background: rgba(30, 41, 59, 0.76);
    }

    .refine-quick-save-btn[data-state="saving"] {
      color: #bfdbfe;
      border-color: rgba(147, 197, 253, 0.6);
      background: rgba(30, 64, 175, 0.84);
    }

    .refine-quick-save-btn[data-state="done"],
    .refine-quick-save-btn[data-state="imported"] {
      color: #dcfce7;
      border-color: rgba(74, 222, 128, 0.62);
      background: rgba(22, 163, 74, 0.84);
    }

    .refine-quick-save-btn[data-state="error"] {
      color: #ffe4e6;
      border-color: rgba(251, 113, 133, 0.62);
      background: rgba(225, 29, 72, 0.86);
    }
  `,
})

export default function GeminiContent() {
  return null
}
