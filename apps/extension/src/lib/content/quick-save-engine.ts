import type { ConversationSource } from '../types'
import {
  injectStyleOnce,
  persistAndEnqueueConversation,
  registerExtractActionHandler,
  showToast,
  type ExtractResult,
} from './runtime'

export type QuickSaveButtonState = 'idle' | 'saving' | 'done' | 'imported' | 'error'

export interface QuickSaveTarget {
  url: string
  conversationKey: string
}

interface PendingSidebarImport {
  url: string
  conversationKey: string
  title: string
  requestedAt: number
}

export interface QuickSaveButtonCopy {
  idleText: string
  savingText: string
  doneText: string
  errorText: string
  importedText?: string
  idleTitle?: string
  savingTitle?: string
  doneTitle?: string
  importedTitle?: string
  errorTitle?: string
}

export interface SilentExtractContentResult {
  success: boolean
  content?: string
  message?: string
}

interface QuickSaveMessages {
  successToast: string
  silentSuccessToast: string
  alreadyImportedToast: string
  invalidTargetToast: string
  pendingWriteFailedToast: string
  openFailedToast: string
  queueFailedToast: string
  notFoundContentMessage: string
  saveFailedFallback: string
  navigateFallbackToast: (silentFailureMessage?: string) => string
}

export interface QuickSaveEngineOptions {
  providerId: string
  source: ConversationSource
  linkSelector: string
  pendingStorageKey: string
  importedStorageKey: string
  resolveTarget: (link: HTMLAnchorElement) => QuickSaveTarget | null
  resolveHost?: (link: HTMLAnchorElement, target: QuickSaveTarget) => HTMLElement
  getConversationTitleFromLink: (link: HTMLAnchorElement) => string
  isSameConversation: (target: QuickSaveTarget) => boolean
  isCurrentConversation: (conversationKey: string) => boolean
  navigateToTarget: (link: HTMLAnchorElement, target: QuickSaveTarget) => Promise<boolean>
  extractConversation: () => string
  waitForConversationContent?: () => Promise<string | null>
  tryExtractContentWithoutNavigation?: (
    target: QuickSaveTarget,
    title: string
  ) => Promise<SilentExtractContentResult>
  getConversationKeyFromUrl?: (url: string) => string | null
  buttonCopy: QuickSaveButtonCopy
  buttonAriaLabel?: string
  buttonClassName?: string
  hostClassName?: string
  quickSaveDataFlag?: string
  styleId?: string
  styleCss?: string
  pendingTtlMs?: number
  importedLimit?: number
  messages?: Partial<QuickSaveMessages>
}

const DEFAULT_PENDING_TTL_MS = 2 * 60 * 1_000
const DEFAULT_IMPORTED_LIMIT = 1_500

export function initQuickSaveEngine(options: QuickSaveEngineOptions): void {
  const buttonClassName = options.buttonClassName || 'refine-quick-save-btn'
  const hostClassName = options.hostClassName || 'refine-quick-save-host'
  const quickSaveDataFlag = options.quickSaveDataFlag || 'refineQuickSaveAttached'
  const pendingTtlMs = options.pendingTtlMs ?? DEFAULT_PENDING_TTL_MS
  const importedLimit = options.importedLimit ?? DEFAULT_IMPORTED_LIMIT
  const buttonAriaLabel = options.buttonAriaLabel || '入库此会话'
  const styleId = options.styleId || `__refine_quick_save_style_${options.providerId}`

  const messages: QuickSaveMessages = {
    successToast: '已加入同步队列，稍后上传到 Refine 云端',
    silentSuccessToast: '已后台入库，无需跳转',
    alreadyImportedToast: '该会话已入库',
    invalidTargetToast: '无法识别会话链接',
    pendingWriteFailedToast: '暂存入库任务失败，请刷新页面后重试',
    openFailedToast: '链接打开失败，请手动进入该会话后再点入库',
    queueFailedToast: '对话已提取，但入队失败，请稍后重试',
    notFoundContentMessage: '未找到对话内容',
    saveFailedFallback: '保存失败',
    navigateFallbackToast: () => '正在打开会话，加载后自动入库...',
    ...options.messages,
  }

  let importedConversations = new Map<string, number>()
  let importedConversationsLoaded = false
  let importedConversationsLoadPromise: Promise<void> | null = null
  let enhanceScheduled = false
  let pendingImportScheduled = false
  let sidebarObserver: MutationObserver | null = null
  let pendingImportProcessing = false

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
    if (map.size <= importedLimit) return map

    return new Map(
      Array.from(map.entries())
        .sort((a, b) => b[1] - a[1])
        .slice(0, importedLimit)
    )
  }

  async function ensureImportedConversationsLoaded(): Promise<void> {
    if (importedConversationsLoaded) return
    if (importedConversationsLoadPromise) return importedConversationsLoadPromise

    importedConversationsLoadPromise = chrome.storage.local
      .get([options.importedStorageKey])
      .then((stored) => {
        importedConversations = trimImportedConversations(
          buildImportedConversationMap(stored[options.importedStorageKey])
        )
        importedConversationsLoaded = true
      })
      .catch(() => {
        importedConversations = new Map()
        importedConversationsLoaded = true
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
      [options.importedStorageKey]: payload,
    })
  }

  async function markConversationImported(conversationKey: string): Promise<void> {
    await ensureImportedConversationsLoaded()
    if (isConversationImported(conversationKey)) return

    importedConversations.set(conversationKey, Date.now())
    importedConversations = trimImportedConversations(importedConversations)

    try {
      await persistImportedConversations()
    } catch {
      // ignore write errors to avoid blocking UX
    }
  }

  function readPendingSidebarImport(): PendingSidebarImport | null {
    let raw: string | null = null
    try {
      raw = window.sessionStorage.getItem(options.pendingStorageKey)
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
      window.sessionStorage.setItem(options.pendingStorageKey, JSON.stringify(pending))
      return true
    } catch {
      return false
    }
  }

  function clearPendingSidebarImport(): void {
    try {
      window.sessionStorage.removeItem(options.pendingStorageKey)
    } catch {
      // ignore unavailable sessionStorage
    }
  }

  function setQuickSaveButtonState(button: HTMLButtonElement, state: QuickSaveButtonState): void {
    button.dataset.state = state
    button.disabled = state === 'saving' || state === 'imported'

    const doneText = options.buttonCopy.doneText
    const doneTitle = options.buttonCopy.doneTitle
    const importedText = options.buttonCopy.importedText ?? doneText
    const importedTitle = options.buttonCopy.importedTitle ?? doneTitle

    if (state === 'saving') {
      button.textContent = options.buttonCopy.savingText
      button.title = options.buttonCopy.savingTitle || ''
      return
    }

    if (state === 'done') {
      button.textContent = doneText
      button.title = doneTitle || ''
      return
    }

    if (state === 'imported') {
      button.textContent = importedText
      button.title = importedTitle || ''
      return
    }

    if (state === 'error') {
      button.textContent = options.buttonCopy.errorText
      button.title = options.buttonCopy.errorTitle || ''
      return
    }

    button.textContent = options.buttonCopy.idleText
    button.title = options.buttonCopy.idleTitle || ''
  }

  function setConversationButtonsState(conversationKey: string, state: QuickSaveButtonState): void {
    const selector = `.${buttonClassName}[data-provider="${options.providerId}"]`
    const buttons = document.querySelectorAll<HTMLButtonElement>(selector)
    buttons.forEach((candidate) => {
      if (candidate.dataset.conversationKey !== conversationKey) return
      setQuickSaveButtonState(candidate, state)
    })
  }

  function resetQuickSaveButtonStateLater(button: HTMLButtonElement): void {
    window.setTimeout(() => {
      if (!document.contains(button)) return
      if (button.dataset.state === 'imported') return
      setQuickSaveButtonState(button, 'idle')
    }, 1_800)
  }

  function saveFailedMessage(result: { message?: string }): string {
    return result.message || messages.saveFailedFallback
  }

  function setButtonErrorState(button: HTMLButtonElement, message: string): void {
    setQuickSaveButtonState(button, 'error')
    showToast(message)
    resetQuickSaveButtonStateLater(button)
  }

  async function enqueueExtractedContent(
    content: string,
    enqueueOptions?: {
      title?: string
      url?: string
      conversationKey?: string
    }
  ): Promise<ExtractResult> {
    const url = enqueueOptions?.url || window.location.href
    const conversationKey =
      enqueueOptions?.conversationKey ||
      options.getConversationKeyFromUrl?.(url) ||
      options.getConversationKeyFromUrl?.(window.location.href) ||
      null

    return persistAndEnqueueConversation({
      source: options.source,
      content,
      title: enqueueOptions?.title,
      url,
      saveFailedFallback: messages.saveFailedFallback,
      onQueued: async () => {
        if (!conversationKey) return
        await markConversationImported(conversationKey)
        setConversationButtonsState(conversationKey, 'imported')
      },
    })
  }

  async function extractAndEnqueueConversation(params?: {
    title?: string
    url?: string
    conversationKey?: string
    waitForContent?: boolean
  }): Promise<ExtractResult> {
    const content =
      params?.waitForContent && options.waitForConversationContent
        ? await options.waitForConversationContent()
        : options.extractConversation()

    if (!content) {
      return {
        success: false,
        message: messages.notFoundContentMessage,
      }
    }

    return enqueueExtractedContent(content, {
      title: params?.title,
      url: params?.url,
      conversationKey: params?.conversationKey,
    })
  }

  async function handleSidebarQuickSaveClick(
    link: HTMLAnchorElement,
    button: HTMLButtonElement
  ): Promise<void> {
    const target = options.resolveTarget(link)
    if (!target) {
      setButtonErrorState(button, messages.invalidTargetToast)
      return
    }

    await ensureImportedConversationsLoaded()
    if (isConversationImported(target.conversationKey)) {
      setConversationButtonsState(target.conversationKey, 'imported')
      showToast(messages.alreadyImportedToast)
      return
    }

    const title = options.getConversationTitleFromLink(link)
    setQuickSaveButtonState(button, 'saving')

    if (options.isSameConversation(target)) {
      const result = await extractAndEnqueueConversation({
        title,
        url: target.url,
        conversationKey: target.conversationKey,
        waitForContent: true,
      })

      if (result.success) {
        showToast(messages.successToast)
      } else {
        setButtonErrorState(button, saveFailedMessage(result))
      }
      return
    }

    let silentFailureMessage: string | undefined
    if (options.tryExtractContentWithoutNavigation) {
      const silentResult = await options.tryExtractContentWithoutNavigation(target, title)
      if (silentResult.success && typeof silentResult.content === 'string' && silentResult.content.length > 0) {
        const enqueueResult = await enqueueExtractedContent(silentResult.content, {
          title,
          url: target.url,
          conversationKey: target.conversationKey,
        })
        if (enqueueResult.success) {
          showToast(messages.silentSuccessToast)
        } else {
          setButtonErrorState(button, saveFailedMessage(enqueueResult))
        }
        return
      }
      silentFailureMessage = silentResult.message
    }

    const written = writePendingSidebarImport({
      url: target.url,
      conversationKey: target.conversationKey,
      title,
      requestedAt: Date.now(),
    })
    if (!written) {
      setButtonErrorState(button, messages.pendingWriteFailedToast)
      return
    }

    showToast(messages.navigateFallbackToast(silentFailureMessage))
    const opened = await options.navigateToTarget(link, target)
    if (!opened) {
      setButtonErrorState(button, messages.openFailedToast)
    }
  }

  function getSidebarConversationLinks(): HTMLAnchorElement[] {
    const candidates = document.querySelectorAll<HTMLAnchorElement>(options.linkSelector)
    return Array.from(candidates).filter((link) => !!options.resolveTarget(link))
  }

  function enhanceSidebarConversationLinks(): void {
    const links = getSidebarConversationLinks()

    for (const link of links) {
      const target = options.resolveTarget(link)
      if (!target) continue

      const host = options.resolveHost?.(link, target) || link
      if (host.dataset[quickSaveDataFlag] === 'true') continue

      host.dataset[quickSaveDataFlag] = 'true'
      host.classList.add(hostClassName)

      const button = document.createElement('button')
      button.type = 'button'
      button.className = buttonClassName
      button.dataset.provider = options.providerId
      button.dataset.conversationKey = target.conversationKey
      button.setAttribute('aria-label', buttonAriaLabel)
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

      if (Date.now() - pending.requestedAt > pendingTtlMs) {
        clearPendingSidebarImport()
        return
      }

      if (!options.isCurrentConversation(pending.conversationKey)) return

      const result = await extractAndEnqueueConversation({
        title: pending.title,
        url: pending.url,
        conversationKey: pending.conversationKey,
        waitForContent: true,
      })
      clearPendingSidebarImport()

      if (result.success) {
        showToast(messages.successToast)
      } else {
        showToast(saveFailedMessage(result))
      }
    } finally {
      pendingImportProcessing = false
    }
  }

  function registerExtractMessageHandler(): void {
    registerExtractActionHandler(async () => {
      try {
        const result = await extractAndEnqueueConversation()
        if (result.success) {
          showToast(messages.successToast)
          return result
        }

        const message = saveFailedMessage(result)
        showToast(message)
        return {
          success: false,
          message,
        }
      } catch {
        const message = messages.queueFailedToast
        showToast(message)
        return {
          success: false,
          message,
        }
      }
    })
  }

  function initialize(): void {
    if (options.styleCss) {
      injectStyleOnce(styleId, options.styleCss)
    }

    registerExtractMessageHandler()
    void ensureImportedConversationsLoaded().finally(() => {
      startSidebarQuickSaveObserver()
      void resumePendingSidebarImport()
    })
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initialize, { once: true })
  } else {
    initialize()
  }
}
