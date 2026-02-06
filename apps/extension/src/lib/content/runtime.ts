import type { ConversationSource } from '../types'

export interface EnqueueResponse {
  queued: boolean
  message?: string
}

export interface ExtractResult {
  success: boolean
  length?: number
  message?: string
}

interface EnqueueConversationOptions {
  source: ConversationSource
  title?: string
  url?: string
  capturedAt?: number
}

const TOAST_STYLE_ID = '__refine_content_toast_style'

export function normalizeText(input: string): string {
  return input
    .replace(/\u200b/g, '')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}

export function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms)
  })
}

export function toErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  try {
    return JSON.stringify(error)
  } catch {
    return String(error)
  }
}

export function injectStyleOnce(styleId: string, cssText: string): void {
  if (!document.head) return
  if (document.getElementById(styleId)) return

  const style = document.createElement('style')
  style.id = styleId
  style.textContent = cssText
  document.head.appendChild(style)
}

export function persistLastExtracted(content: string, url: string, source: ConversationSource): void {
  chrome.storage.local.set({
    lastExtracted: {
      content,
      url,
      timestamp: Date.now(),
      source,
    },
  })
}

export function enqueueConversation(
  content: string,
  options: EnqueueConversationOptions
): Promise<EnqueueResponse> {
  const url = options.url || window.location.href
  const title = options.title || document.title || 'AI Conversation'
  const capturedAt = options.capturedAt || Date.now()

  return new Promise((resolve) => {
    chrome.runtime.sendMessage(
      {
        action: 'enqueueExtractedConversation',
        payload: {
          content,
          url,
          source: options.source,
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

function ensureToastAnimationStyle(): void {
  injectStyleOnce(
    TOAST_STYLE_ID,
    `
      @keyframes refineToastSlideIn {
        from { transform: translateX(100%); opacity: 0; }
        to { transform: translateX(0); opacity: 1; }
      }
      @keyframes refineToastSlideOut {
        from { transform: translateX(0); opacity: 1; }
        to { transform: translateX(100%); opacity: 0; }
      }
    `
  )
}

export function showToast(message: string): void {
  ensureToastAnimationStyle()

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
    animation: refineToastSlideIn 0.3s ease;
  `

  document.body.appendChild(toast)

  window.setTimeout(() => {
    toast.style.animation = 'refineToastSlideOut 0.3s ease'
    window.setTimeout(() => toast.remove(), 300)
  }, 3000)
}

export function registerExtractActionHandler(handler: () => Promise<ExtractResult>): void {
  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message?.action !== 'extract') return false

    handler()
      .then((result) => {
        sendResponse(result)
      })
      .catch(() => {
        sendResponse({
          success: false,
          message: '入队失败，请稍后重试',
        })
      })

    return true
  })
}
