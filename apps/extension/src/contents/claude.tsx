/**
 * Claude Content Script
 *
 * 在 Claude.ai 页面注入提取功能和侧边栏快捷入库按钮
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
  path: string
  title: string
  requestedAt: number
}

type QuickSaveButtonState = 'idle' | 'saving' | 'done' | 'error'

const PENDING_SIDEBAR_IMPORT_KEY = '__refine_pending_sidebar_import'
const PENDING_SIDEBAR_IMPORT_TTL_MS = 2 * 60 * 1_000
const MESSAGE_POLL_INTERVAL_MS = 350
const MESSAGE_POLL_TIMEOUT_MS = 20_000
const QUICK_SAVE_DATA_FLAG = 'refineQuickSaveAttached'

export const config: PlasmoCSConfig = {
  matches: ['https://claude.ai/*'],
  all_frames: false,
}

function normalizeText(input: string): string {
  return input
    .replace(/\u200b/g, '')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

// 提取对话内容
function extractConversation(): string {
  const messages: string[] = []

  // Claude 的对话容器选择器
  const humanMessages = document.querySelectorAll('[data-testid="human-message"]')
  const assistantMessages = document.querySelectorAll('[data-testid="assistant-message"]')

  // 合并并排序消息
  const allMessages = [
    ...Array.from(humanMessages).map((el) => ({ role: 'Human', el })),
    ...Array.from(assistantMessages).map((el) => ({ role: 'Assistant', el })),
  ]

  // 按 DOM 位置排序
  allMessages.sort((a, b) => {
    const position = a.el.compareDocumentPosition(b.el)
    if (position & Node.DOCUMENT_POSITION_FOLLOWING) return -1
    if (position & Node.DOCUMENT_POSITION_PRECEDING) return 1
    return 0
  })

  allMessages.forEach(({ role, el }) => {
    const content = normalizeText((el as HTMLElement).innerText || el.textContent || '')
    if (content) {
      messages.push(`${role}: ${content}`)
    }
  })

  return messages.join('\n\n')
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

function persistLastExtracted(content: string, url: string): void {
  chrome.storage.local.set({
    lastExtracted: {
      content,
      url,
      timestamp: Date.now(),
      source: 'claude',
    },
  })
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

function isSameConversation(rawUrl: string): boolean {
  const targetPath = getConversationPath(rawUrl)
  const currentPath = getConversationPath(window.location.href)
  return !!targetPath && targetPath === currentPath
}

function resolveConversationUrl(link: HTMLAnchorElement): string | null {
  const href = link.getAttribute('href')
  if (!href) return null

  try {
    const resolved = new URL(href, window.location.origin)
    if (!getConversationPath(resolved.toString())) return null
    return resolved.toString()
  } catch {
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
      typeof parsed.path !== 'string' ||
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

function setQuickSaveButtonState(button: HTMLButtonElement, state: QuickSaveButtonState): void {
  button.dataset.state = state
  button.disabled = state === 'saving'

  if (state === 'saving') {
    button.textContent = '入库中'
    return
  }

  if (state === 'done') {
    button.textContent = '已入库'
    return
  }

  if (state === 'error') {
    button.textContent = '失败'
    return
  }

  button.textContent = '入库'
}

function resetQuickSaveButtonStateLater(button: HTMLButtonElement): void {
  window.setTimeout(() => {
    if (!document.contains(button)) return
    setQuickSaveButtonState(button, 'idle')
  }, 1_800)
}

function getConversationTitleFromLink(link: HTMLAnchorElement): string {
  const clone = link.cloneNode(true) as HTMLElement
  clone.querySelectorAll('.refine-quick-save-btn').forEach((button) => button.remove())
  const title = normalizeText(clone.textContent || '')
  return title || document.title || 'Claude Conversation'
}

async function extractAndEnqueueConversation(options?: {
  title?: string
  url?: string
  waitForContent?: boolean
}): Promise<ExtractResult> {
  const content = options?.waitForContent ? await waitForConversationContent() : extractConversation()

  if (!content) {
    return {
      success: false,
      message: '未找到对话内容',
    }
  }

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

async function handleSidebarQuickSaveClick(
  link: HTMLAnchorElement,
  button: HTMLButtonElement
): Promise<void> {
  const targetUrl = resolveConversationUrl(link)
  if (!targetUrl) {
    setQuickSaveButtonState(button, 'error')
    showToast('无法识别会话链接')
    resetQuickSaveButtonStateLater(button)
    return
  }

  const title = getConversationTitleFromLink(link)
  setQuickSaveButtonState(button, 'saving')

  if (isSameConversation(targetUrl)) {
    const result = await extractAndEnqueueConversation({
      title,
      url: targetUrl,
      waitForContent: true,
    })

    if (result.success) {
      setQuickSaveButtonState(button, 'done')
      showToast('已加入同步队列，稍后上传到 Refine 云端')
    } else {
      setQuickSaveButtonState(button, 'error')
      showToast(result.message || '保存失败')
    }
    resetQuickSaveButtonStateLater(button)
    return
  }

  const path = getConversationPath(targetUrl)
  if (!path) {
    setQuickSaveButtonState(button, 'error')
    showToast('会话地址无效')
    resetQuickSaveButtonStateLater(button)
    return
  }

  const written = writePendingSidebarImport({
    url: targetUrl,
    path,
    title,
    requestedAt: Date.now(),
  })
  if (!written) {
    setQuickSaveButtonState(button, 'error')
    showToast('暂存入库任务失败，请刷新页面后重试')
    resetQuickSaveButtonStateLater(button)
    return
  }

  showToast('正在打开会话，加载后自动入库...')
  window.location.assign(targetUrl)
}

let enhanceScheduled = false
let sidebarObserver: MutationObserver | null = null
let pendingImportProcessing = false

function getSidebarConversationLinks(): HTMLAnchorElement[] {
  const candidates = document.querySelectorAll<HTMLAnchorElement>('aside a[href], nav a[href]')
  return Array.from(candidates).filter((link) => !!resolveConversationUrl(link))
}

function enhanceSidebarConversationLinks(): void {
  const links = getSidebarConversationLinks()

  for (const link of links) {
    if (link.dataset[QUICK_SAVE_DATA_FLAG] === 'true') continue
    link.dataset[QUICK_SAVE_DATA_FLAG] = 'true'
    link.classList.add('refine-quick-save-host')

    const button = document.createElement('button')
    button.type = 'button'
    button.className = 'refine-quick-save-btn'
    button.setAttribute('aria-label', '入库此会话')
    setQuickSaveButtonState(button, 'idle')

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

    link.appendChild(button)
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

function startSidebarQuickSaveObserver(): void {
  if (!document.body || sidebarObserver) return

  scheduleEnhanceSidebarConversationLinks()

  sidebarObserver = new MutationObserver(() => {
    scheduleEnhanceSidebarConversationLinks()
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

    const currentPath = getConversationPath(window.location.href)
    if (!currentPath || currentPath !== pending.path) return

    const result = await extractAndEnqueueConversation({
      title: pending.title,
      url: pending.url,
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
  const title = options?.title || document.title || 'Claude Conversation'
  const capturedAt = options?.capturedAt || Date.now()

  return new Promise((resolve) => {
    chrome.runtime.sendMessage(
      {
        action: 'enqueueExtractedConversation',
        payload: {
          content,
          url,
          source: 'claude',
          title,
          capturedAt,
        },
      },
      (response: EnqueueResponse) => {
        if (chrome.runtime.lastError) {
          resolve({
            queued: false,
            message: chrome.runtime.lastError.message || 'Background message failed',
          })
          return
        }
        resolve(response || { queued: false, message: 'No response from background' })
      }
    )
  })
}

// 显示提示
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

// 添加动画样式
const style = document.createElement('style')
style.textContent = `
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

  .refine-quick-save-btn[data-state="done"] {
    color: #86efac;
    border-color: rgba(52, 211, 153, 0.62);
  }

  .refine-quick-save-btn[data-state="error"] {
    color: #fca5a5;
    border-color: rgba(248, 113, 113, 0.62);
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
  startSidebarQuickSaveObserver()
  void resumePendingSidebarImport()
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init, { once: true })
} else {
  init()
}

export default function ClaudeContent() {
  return null
}
