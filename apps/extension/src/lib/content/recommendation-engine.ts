import type { ConversationSource } from '../types'

export interface RecommendationContext {
  source: ConversationSource
  query: string
  url: string
  enabled: boolean
}

interface RecommendationEngineOptions {
  providerId: string
  source: ConversationSource
  inputSelectors: string[]
}

export const SITE_SETTINGS_STORAGE_KEY = '__refine_recommendation_enabled_by_site'

/** Page bridge only. Private recommendations are fetched and rendered by the popup. */
export function initRecommendationEngine(options: RecommendationEngineOptions): void {
  let activeInput: HTMLElement | null = null
  const siteSettingKey = `${options.providerId}:${window.location.hostname}`

  function findInput(target?: EventTarget | null): HTMLElement | null {
    for (const selector of options.inputSelectors) {
      const found = target instanceof HTMLElement
        ? target.closest(selector) : document.querySelector(selector)
      if (found instanceof HTMLElement) return found
    }
    return null
  }

  function readInput(input: HTMLElement): string {
    if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) return input.value
    return input.isContentEditable ? input.innerText || input.textContent || '' : ''
  }

  document.addEventListener('focusin', (event) => {
    const input = findInput(event.target)
    if (input) activeInput = input
  }, true)

  chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
    if (sender.id !== chrome.runtime.id || !sender.url?.startsWith(chrome.runtime.getURL(''))) return false
    if (message.action === 'getRecommendationContext') {
      const input = activeInput?.isConnected ? activeInput : findInput()
      activeInput = input
      chrome.storage.local.get([SITE_SETTINGS_STORAGE_KEY]).then((stored) => {
        sendResponse({
          source: options.source,
          query: input ? readInput(input).trim() : '',
          url: window.location.href,
          enabled: stored[SITE_SETTINGS_STORAGE_KEY]?.[siteSettingKey] !== false,
        } satisfies RecommendationContext)
      }).catch(() => sendResponse(null))
      return true
    }
    if (message.action === 'insertRecommendation') {
      const input = activeInput
      if (!input?.isConnected || window.location.href !== message.url ||
          readInput(input).trim() !== message.query || typeof message.text !== 'string') {
        sendResponse({ success: false, message: '输入或页面已变化，请刷新推荐后再插入' })
        return false
      }
      try {
        const current = readInput(input)
        const next = `${current}${current.trim() ? '\n' : ''}${message.text}`
        input.focus()
        if (input instanceof HTMLTextAreaElement || input instanceof HTMLInputElement) input.value = next
        else if (input.isContentEditable) input.textContent = next
        else throw new Error('当前输入框不支持插入')
        input.dispatchEvent(new Event('input', { bubbles: true }))
        sendResponse({ success: true })
      } catch {
        sendResponse({ success: false, message: '插入失败，请重试' })
      }
    }
    return false
  })
}
