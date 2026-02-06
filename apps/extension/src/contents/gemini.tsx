/**
 * Gemini Content Script
 *
 * 在 Gemini 页面注入提取功能和侧边栏快捷入库按钮
 */

import type { PlasmoCSConfig } from 'plasmo'

interface EnqueueResponse {
  queued: boolean
  message?: string
}

interface ExtractResult {
  success: boolean
  length?: number
  message?: string
}

interface PendingSidebarImport {
  url: string
  conversationKey: string
  title: string
  requestedAt: number
}

interface ConversationTarget {
  url: string
  conversationKey: string
}

type QuickSaveButtonState = 'idle' | 'saving' | 'done' | 'imported' | 'error'
type DebugLevel = 'info' | 'warn' | 'error'

interface DebugLogEntry {
  ts: string
  level: DebugLevel
  event: string
  message: string
  context?: Record<string, string | number | boolean | null>
}

interface Turn {
  role: 'Human' | 'Assistant'
  el: Element
}

const PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import_gemini'
const PENDING_SIDEBAR_IMPORT_TTL_MS = 2 * 60 * 1_000
const MESSAGE_POLL_INTERVAL_MS = 350
const MESSAGE_POLL_TIMEOUT_MS = 20_000
const HIDDEN_IFRAME_EXTRACT_TIMEOUT_MS = 18_000
const QUICK_SAVE_DATA_FLAG = 'refineQuickSaveAttached'
const IMPORTED_CONVERSATIONS_KEY = '__refine_imported_conversations_gemini'
const IMPORTED_CONVERSATIONS_LIMIT = 1_500
const INVALID_CONVERSATION_IDS = new Set(['', 'none', 'null', 'undefined', 'new', 'new_chat', 'newchat'])
const DEBUG_LOG_KEY = '__refine_gemini_quicksave_logs'
const DEBUG_LOG_LIMIT = 100

// Gemini 当前对 iframe 提取是策略级封禁（X-Frame-Options: deny），默认直接禁用该路径。
let hiddenIframeCapability: 'unknown' | 'supported' | 'blocked' = 'blocked'
let importedConversations = new Map<string, number>()
let importedConversationsLoaded = false
let importedConversationsLoadPromise: Promise<void> | null = null

function isHiddenIframeBlocked(): boolean {
  return hiddenIframeCapability === 'blocked'
}

export const config: PlasmoCSConfig = {
  matches: ['https://gemini.google.com/*'],
  all_frames: false,
}

function normalizeText(input: string): string {
  return input
    .replace(/\u200b/g, '')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

function toErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  try {
    return JSON.stringify(error)
  } catch {
    return String(error)
  }
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
  level: DebugLevel,
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

function collectTurns(root: ParentNode = document): Turn[] {
  const turns: Turn[] = []

  const userNodes = root.querySelectorAll('main user-query, main [data-turn-role="user"], main [data-source="user"]')
  userNodes.forEach((el) => {
    turns.push({ role: 'Human', el })
  })

  const assistantNodes = root.querySelectorAll(
    'main model-response, main [data-turn-role="model"], main [data-turn-role="assistant"], main [data-source="model"]'
  )
  assistantNodes.forEach((el) => {
    turns.push({ role: 'Assistant', el })
  })

  // 去重并按 DOM 顺序排序
  const deduped: Turn[] = []
  const seen = new Set<Element>()
  for (const turn of turns) {
    if (seen.has(turn.el)) continue
    seen.add(turn.el)
    deduped.push(turn)
  }

  deduped.sort((a, b) => {
    const position = a.el.compareDocumentPosition(b.el)
    if (position & Node.DOCUMENT_POSITION_FOLLOWING) return -1
    if (position & Node.DOCUMENT_POSITION_PRECEDING) return 1
    return 0
  })

  return deduped
}

function extractConversationFromRoot(root: ParentNode = document): string {
  const turns = collectTurns(root)
  const messages: string[] = []

  for (const { role, el } of turns) {
    const text = normalizeText((el as HTMLElement).innerText || el.textContent || '')
    if (!text) continue
    messages.push(`${role}: ${text}`)
  }

  return messages.join('\n\n')
}

function extractConversation(): string {
  return extractConversationFromRoot(document)
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

async function waitForConversationContent(timeoutMs = MESSAGE_POLL_TIMEOUT_MS): Promise<string | null> {
  const startedAt = Date.now()

  while (Date.now() - startedAt <= timeoutMs) {
    const content = extractConversation()
    if (content) return content
    await delay(MESSAGE_POLL_INTERVAL_MS)
  }

  return null
}

async function waitForConversationInDocument(doc: Document, timeoutMs = HIDDEN_IFRAME_EXTRACT_TIMEOUT_MS): Promise<string | null> {
  const startedAt = Date.now()

  while (Date.now() - startedAt <= timeoutMs) {
    const content = extractConversationFromRoot(doc)
    if (content) return content
    await delay(MESSAGE_POLL_INTERVAL_MS)
  }

  return null
}

function persistLastExtracted(content: string, url: string): void {
  chrome.storage.local.set({
    lastExtracted: {
      content,
      url,
      timestamp: Date.now(),
      source: 'gemini',
    },
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
    const parsed = withCurrentAccountPrefix(new URL(rawUrl, window.location.origin))

    const pathIdMatch = parsed.pathname.match(/\/app\/([^/?#]+)/i) || parsed.pathname.match(/\/chat\/([^/?#]+)/i)
    if (pathIdMatch) {
      const id = decodeId(pathIdMatch[1]).trim()
      if (!isValidConversationId(id)) return null
      return `path:${id}`
    }

    if (/\/(?:u\/\d+\/)?app\/?$/i.test(parsed.pathname)) {
      const queryId =
        parsed.searchParams.get('pageId') ||
        parsed.searchParams.get('conversationId') ||
        parsed.searchParams.get('conversation_id') ||
        parsed.searchParams.get('id')

      if (isValidConversationId(queryId)) {
        return `query:${decodeId(queryId).trim()}`
      }
    }

    return null
  } catch {
    return null
  }
}

function isSameConversation(conversationKey: string): boolean {
  const currentKey = getConversationKey(window.location.href)
  return !!currentKey && currentKey === conversationKey
}

function sanitizeConversationUrl(url: URL): URL {
  const normalized = new URL(url.toString())
  const maybeInvalidParamKeys = ['pageId', 'conversationId', 'conversation_id', 'id']

  for (const key of maybeInvalidParamKeys) {
    const value = normalized.searchParams.get(key)
    if (value && !isValidConversationId(value)) {
      normalized.searchParams.delete(key)
    }
  }

  return normalized
}

function resolveConversationTarget(
  link: HTMLAnchorElement,
  options?: {
    logInvalid?: boolean
  }
): ConversationTarget | null {
  const logInvalid = options?.logInvalid === true
  const href = link.getAttribute('href')
  if (!href) {
    if (logInvalid) {
      debugLog('warn', 'resolve_target', 'link has no href')
    }
    return null
  }

  try {
    const resolved = withCurrentAccountPrefix(new URL(href, window.location.href))
    if (resolved.origin !== window.location.origin) {
      if (logInvalid) {
        debugLog('warn', 'resolve_target', 'origin mismatch', {
          href: resolved.toString(),
        })
      }
      return null
    }

    const sanitized = sanitizeConversationUrl(resolved)
    const conversationKey = getConversationKey(sanitized.toString())
    if (!conversationKey) {
      if (logInvalid) {
        debugLog('warn', 'resolve_target', 'conversation key invalid', {
          href: sanitized.toString(),
        })
      }
      return null
    }

    return {
      url: sanitized.toString(),
      conversationKey,
    }
  } catch (error) {
    if (logInvalid) {
      debugLog('error', 'resolve_target', toErrorMessage(error), {
        href,
      })
    }
    return null
  }
}

function readPendingSidebarImport(): PendingSidebarImport | null {
  let raw: string | null = null
  try {
    raw = window.sessionStorage.getItem(PENDING_SIDEBAR_IMPORT_KEY)
  } catch {
    return null
  }
  if (!raw) return null

  try {
    const parsed = JSON.parse(raw) as PendingSidebarImport
    if (
      typeof parsed.url !== 'string' ||
      typeof parsed.conversationKey !== 'string' ||
      typeof parsed.title !== 'string' ||
      typeof parsed.requestedAt !== 'number'
    ) {
      clearPendingSidebarImport()
      return null
    }
    return parsed
  } catch {
    clearPendingSidebarImport()
    return null
  }
}

function writePendingSidebarImport(pending: PendingSidebarImport): boolean {
  try {
    window.sessionStorage.setItem(PENDING_SIDEBAR_IMPORT_KEY, JSON.stringify(pending))
    return true
  } catch {
    return false
  }
}

function clearPendingSidebarImport(): void {
  try {
    window.sessionStorage.removeItem(PENDING_SIDEBAR_IMPORT_KEY)
  } catch {
    // 忽略 sessionStorage 不可用场景
  }
}

function buildImportedConversationMap(raw: unknown): Map<string, number> {
  const imported = new Map<string, number>()
  if (!raw || typeof raw !== 'object') return imported

  for (const [conversationKey, importedAt] of Object.entries(raw as Record<string, unknown>)) {
    if (typeof conversationKey !== 'string' || conversationKey.length === 0) continue
    if (typeof importedAt !== 'number' || !Number.isFinite(importedAt)) continue
    imported.set(conversationKey, importedAt)
  }

  return imported
}

function trimImportedConversations(map: Map<string, number>): Map<string, number> {
  if (map.size <= IMPORTED_CONVERSATIONS_LIMIT) return map

  const sorted = Array.from(map.entries())
    .sort((a, b) => b[1] - a[1])
    .slice(0, IMPORTED_CONVERSATIONS_LIMIT)

  return new Map(sorted)
}

async function ensureImportedConversationsLoaded(): Promise<void> {
  if (importedConversationsLoaded) return
  if (importedConversationsLoadPromise) return importedConversationsLoadPromise

  importedConversationsLoadPromise = chrome.storage.local
    .get([IMPORTED_CONVERSATIONS_KEY])
    .then((stored) => {
      importedConversations = trimImportedConversations(
        buildImportedConversationMap(stored[IMPORTED_CONVERSATIONS_KEY])
      )
      importedConversationsLoaded = true
    })
    .catch((error) => {
      importedConversations = new Map()
      importedConversationsLoaded = true
      debugLog('warn', 'imported_state_read_fail', toErrorMessage(error))
    })
    .finally(() => {
      importedConversationsLoadPromise = null
    })

  return importedConversationsLoadPromise
}

function isConversationImported(conversationKey: string): boolean {
  return importedConversations.has(conversationKey)
}

async function persistImportedConversations(): Promise<void> {
  const payload: Record<string, number> = {}
  for (const [conversationKey, importedAt] of importedConversations.entries()) {
    payload[conversationKey] = importedAt
  }
  await chrome.storage.local.set({
    [IMPORTED_CONVERSATIONS_KEY]: payload,
  })
}

async function markConversationImported(conversationKey: string): Promise<void> {
  await ensureImportedConversationsLoaded()
  if (isConversationImported(conversationKey)) return

  importedConversations.set(conversationKey, Date.now())
  importedConversations = trimImportedConversations(importedConversations)

  try {
    await persistImportedConversations()
  } catch (error) {
    debugLog('warn', 'imported_state_write_fail', toErrorMessage(error), {
      conversationKey,
    })
  }
}

function setConversationButtonsState(conversationKey: string, state: QuickSaveButtonState): void {
  const buttons = document.querySelectorAll<HTMLButtonElement>('.refine-quick-save-btn')
  buttons.forEach((candidate) => {
    if (candidate.dataset.conversationKey !== conversationKey) return
    setQuickSaveButtonState(candidate, state)
  })
}

function setQuickSaveButtonState(button: HTMLButtonElement, state: QuickSaveButtonState): void {
  button.dataset.state = state
  button.disabled = state === 'saving' || state === 'imported'

  if (state === 'saving') {
    button.textContent = '…'
    button.title = '入库中'
    return
  }

  if (state === 'done') {
    button.textContent = '✓'
    button.title = '已入库'
    return
  }

  if (state === 'imported') {
    button.textContent = '✓'
    button.title = '已入库'
    return
  }

  if (state === 'error') {
    button.textContent = '!'
    button.title = '入库失败'
    return
  }

  button.textContent = '☆'
  button.title = '入库此会话'
}

function resetQuickSaveButtonStateLater(button: HTMLButtonElement): void {
  window.setTimeout(() => {
    if (!document.contains(button)) return
    if (button.dataset.state === 'imported') return
    setQuickSaveButtonState(button, 'idle')
  }, 1_800)
}

function getConversationTitleFromLink(link: HTMLAnchorElement): string {
  const clone = link.cloneNode(true) as HTMLElement
  clone.querySelectorAll('.refine-quick-save-btn').forEach((button) => button.remove())
  const title = normalizeText(clone.textContent || '')
  return title || document.title || 'Gemini Conversation'
}

async function enqueueExtractedContent(
  content: string,
  options?: {
    title?: string
    url?: string
  }
): Promise<ExtractResult> {
  const url = options?.url || window.location.href
  persistLastExtracted(content, url)

  const enqueueResult = await enqueueConversation(content, {
    title: options?.title,
    url,
  })

  if (!enqueueResult.queued) {
    return {
      success: false,
      message: enqueueResult.message || '保存失败',
    }
  }

  return {
    success: true,
    length: content.length,
  }
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

async function extractAndEnqueueWithoutNavigation(target: ConversationTarget, title: string): Promise<ExtractResult> {
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

  const result = await enqueueExtractedContent(iframeContent, {
    title,
    url: target.url,
  })
  if (result.success) {
    await markConversationImported(target.conversationKey)
    setConversationButtonsState(target.conversationKey, 'imported')
  }
  return result
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

async function navigateToConversation(link: HTMLAnchorElement, target: ConversationTarget): Promise<boolean> {
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
    // 忽略 click 失败，继续回退 assign
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

async function extractAndEnqueueConversation(options?: {
  title?: string
  url?: string
  conversationKey?: string
  waitForContent?: boolean
}): Promise<ExtractResult> {
  const content = options?.waitForContent ? await waitForConversationContent() : extractConversation()

  if (!content) {
    return {
      success: false,
      message: '未找到对话内容',
    }
  }

  const result = await enqueueExtractedContent(content, {
    title: options?.title,
    url: options?.url,
  })
  if (!result.success) return result

  const conversationKey = options?.conversationKey || getConversationKey(options?.url || window.location.href)
  if (!conversationKey) return result

  await markConversationImported(conversationKey)
  setConversationButtonsState(conversationKey, 'imported')
  return result
}

async function handleSidebarQuickSaveClick(
  link: HTMLAnchorElement,
  button: HTMLButtonElement
): Promise<void> {
  debugLog('info', 'click_save', 'sidebar quick-save clicked', {
    href: link.getAttribute('href') || null,
  })

  const target = resolveConversationTarget(link, { logInvalid: true })
  if (!target) {
    setQuickSaveButtonState(button, 'error')
    showToast('无法识别会话链接')
    resetQuickSaveButtonStateLater(button)
    return
  }

  await ensureImportedConversationsLoaded()
  if (isConversationImported(target.conversationKey)) {
    setConversationButtonsState(target.conversationKey, 'imported')
    showToast('该会话已入库')
    return
  }

  const title = getConversationTitleFromLink(link)
  setQuickSaveButtonState(button, 'saving')

  if (isSameConversation(target.conversationKey)) {
    debugLog('info', 'same_conversation', 'saving current conversation directly', {
      conversationKey: target.conversationKey,
    })
    const result = await extractAndEnqueueConversation({
      title,
      url: target.url,
      conversationKey: target.conversationKey,
      waitForContent: true,
    })

    if (result.success) {
      setConversationButtonsState(target.conversationKey, 'imported')
      showToast('已加入同步队列，稍后上传到 Refine 云端')
    } else {
      setQuickSaveButtonState(button, 'error')
      showToast(result.message || '保存失败')
      resetQuickSaveButtonStateLater(button)
    }
    return
  }

  const silentResult = await extractAndEnqueueWithoutNavigation(target, title)
  if (silentResult.success) {
    setConversationButtonsState(target.conversationKey, 'imported')
    showToast('已后台入库，无需跳转')
    return
  }

  const written = writePendingSidebarImport({
    url: target.url,
    conversationKey: target.conversationKey,
    title,
    requestedAt: Date.now(),
  })
  if (!written) {
    debugLog('error', 'pending_write_failed', 'failed to persist pending import', {
      targetUrl: target.url,
      conversationKey: target.conversationKey,
    })
    setQuickSaveButtonState(button, 'error')
    showToast('暂存入库任务失败，请刷新页面后重试')
    resetQuickSaveButtonStateLater(button)
    return
  }

  showToast(
    silentResult.message === 'Gemini 安全策略禁止后台读取'
      ? 'Gemini 限制后台读取，正在打开会话后自动入库...'
      : '后台读取失败，正在打开会话后自动入库...'
  )
  const opened = await navigateToConversation(link, target)
  if (!opened) {
    setQuickSaveButtonState(button, 'error')
    showToast('链接打开失败，请手动进入该会话后再点入库')
    resetQuickSaveButtonStateLater(button)
  }
}

let enhanceScheduled = false
let pendingImportScheduled = false
let sidebarObserver: MutationObserver | null = null
let pendingImportProcessing = false

function getSidebarConversationLinks(): HTMLAnchorElement[] {
  const candidates = document.querySelectorAll<HTMLAnchorElement>(
    'aside a[href], nav a[href], [role="navigation"] a[href]'
  )
  return Array.from(candidates).filter((link) => !!resolveConversationTarget(link))
}

function enhanceSidebarConversationLinks(): void {
  const links = getSidebarConversationLinks()

  for (const link of links) {
    const target = resolveConversationTarget(link)
    if (!target) continue

    const host =
      link.closest<HTMLElement>('li, [role="listitem"], [data-testid*="conversation"], [data-test-id*="conversation"]') ||
      link

    if (host.dataset[QUICK_SAVE_DATA_FLAG] === 'true') continue
    host.dataset[QUICK_SAVE_DATA_FLAG] = 'true'
    host.classList.add('refine-quick-save-host')

    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'refine-quick-save-btn'
    button.setAttribute('aria-label', '入库此会话')
    button.dataset.conversationKey = target.conversationKey
    setQuickSaveButtonState(button, isConversationImported(target.conversationKey) ? 'imported' : 'idle')

    const stopPropagation = (event: Event) => {
      event.preventDefault()
      event.stopPropagation()
    }

    button.addEventListener('mousedown', stopPropagation)
    button.addEventListener('pointerdown', stopPropagation)
    button.addEventListener('click', (event) => {
      stopPropagation(event)
      void handleSidebarQuickSaveClick(link, button)
    })

    host.appendChild(button)
  }
}

function scheduleEnhanceSidebarConversationLinks(): void {
  if (enhanceScheduled) return
  enhanceScheduled = true

  window.requestAnimationFrame(() => {
    enhanceScheduled = false
    enhanceSidebarConversationLinks()
  })
}

function scheduleResumePendingSidebarImport(): void {
  if (pendingImportScheduled) return
  pendingImportScheduled = true

  window.requestAnimationFrame(() => {
    pendingImportScheduled = false
    void resumePendingSidebarImport()
  })
}

function startSidebarQuickSaveObserver(): void {
  if (!document.body || sidebarObserver) return

  scheduleEnhanceSidebarConversationLinks()
  scheduleResumePendingSidebarImport()

  sidebarObserver = new MutationObserver(() => {
    scheduleEnhanceSidebarConversationLinks()
    scheduleResumePendingSidebarImport()
  })
  sidebarObserver.observe(document.body, {
    childList: true,
    subtree: true,
  })
}

async function resumePendingSidebarImport(): Promise<void> {
  if (pendingImportProcessing) return
  pendingImportProcessing = true

  try {
    const pending = readPendingSidebarImport()
    if (!pending) return

    if (Date.now() - pending.requestedAt > PENDING_SIDEBAR_IMPORT_TTL_MS) {
      clearPendingSidebarImport()
      return
    }

    const currentKey = getConversationKey(window.location.href)
    if (!currentKey || currentKey !== pending.conversationKey) return

    debugLog('info', 'resume_pending', 'resuming pending import after navigation', {
      conversationKey: pending.conversationKey,
    })

    const result = await extractAndEnqueueConversation({
      title: pending.title,
      url: pending.url,
      conversationKey: pending.conversationKey,
      waitForContent: true,
    })
    clearPendingSidebarImport()

    if (result.success) {
      showToast('已加入同步队列，稍后上传到 Refine 云端')
    } else {
      showToast(result.message || '保存失败')
    }
  } finally {
    pendingImportProcessing = false
  }
}

// 监听来自 popup 的消息
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.action === 'extract') {
    extractAndEnqueueConversation()
      .then((result) => {
        if (result.success) {
          showToast('已加入同步队列，稍后上传到 Refine 云端')
          sendResponse({ success: true, length: result.length })
          return
        }

        showToast(result.message || '保存失败')
        sendResponse({
          success: false,
          message: result.message || '保存失败',
        })
      })
      .catch(() => {
        showToast('对话已提取，但入队失败，请稍后重试')
        sendResponse({
          success: false,
          message: '入队失败，请稍后重试',
        })
      })
  }
  return true
})

function enqueueConversation(
  content: string,
  options?: {
    title?: string
    url?: string
    capturedAt?: number
  }
): Promise<EnqueueResponse> {
  const url = options?.url || window.location.href
  const title = options?.title || document.title || 'Gemini Conversation'
  const capturedAt = options?.capturedAt || Date.now()

  return new Promise((resolve) => {
    chrome.runtime.sendMessage(
      {
        action: 'enqueueExtractedConversation',
        payload: {
          content,
          url,
          source: 'gemini',
          title,
          capturedAt,
        },
      },
      (response: EnqueueResponse) => {
        if (chrome.runtime.lastError) {
          debugLog('error', 'enqueue_runtime_error', chrome.runtime.lastError.message || 'Background message failed', {
            url,
          })
          resolve({
            queued: false,
            message: chrome.runtime.lastError.message || 'Background message failed',
          })
          return
        }
        if (!response?.queued) {
          debugLog('warn', 'enqueue_failed', response?.message || 'No response from background', {
            url,
          })
        } else {
          debugLog('info', 'enqueue_success', 'conversation queued', {
            url,
            length: content.length,
          })
        }
        resolve(response || { queued: false, message: 'No response from background' })
      }
    )
  })
}

function showToast(message: string) {
  const toast = document.createElement('div')
  toast.textContent = message
  toast.style.cssText = `
    position: fixed;
    bottom: 20px;
    right: 20px;
    padding: 12px 20px;
    background: #6366f1;
    color: white;
    border-radius: 8px;
    font-size: 14px;
    z-index: 10000;
    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
    animation: slideIn 0.3s ease;
  `

  document.body.appendChild(toast)

  setTimeout(() => {
    toast.style.animation = 'slideOut 0.3s ease'
    setTimeout(() => toast.remove(), 300)
  }, 3000)
}

const style = document.createElement('style')
style.textContent = `
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

  @keyframes slideIn {
    from { transform: translateX(100%); opacity: 0; }
    to { transform: translateX(0); opacity: 1; }
  }
  @keyframes slideOut {
    from { transform: translateX(0); opacity: 1; }
    to { transform: translateX(100%); opacity: 0; }
  }
`
document.head.appendChild(style)

function init(): void {
  const debugWindow = window as Window & {
    __refineGeminiQuickSaveLogs?: () => DebugLogEntry[]
    __refineGeminiClearQuickSaveLogs?: () => void
  }

  debugWindow.__refineGeminiQuickSaveLogs = () => readDebugLogs()
  debugWindow.__refineGeminiClearQuickSaveLogs = () => {
    writeDebugLogs([])
  }

  debugLog('info', 'init', 'gemini quick-save initialized', {
    url: window.location.href,
    hiddenIframeCapability,
  })

  void ensureImportedConversationsLoaded().finally(() => {
    startSidebarQuickSaveObserver()
    void resumePendingSidebarImport()
  })
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init, { once: true })
} else {
  init()
}

export default function GeminiContent() {
  return null
}
