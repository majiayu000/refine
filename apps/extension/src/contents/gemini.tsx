/**
 * Gemini Content Script
 *
 * 在 Gemini 页面注入提取功能
 */

import type { PlasmoCSConfig } from 'plasmo'

interface EnqueueResponse {
  queued: boolean
  message?: string
}

interface Turn {
  role: 'Human' | 'Assistant'
  el: Element
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

function collectTurns(): Turn[] {
  const turns: Turn[] = []

  const userNodes = document.querySelectorAll('main user-query, main [data-turn-role="user"], main [data-source="user"]')
  userNodes.forEach((el) => {
    turns.push({ role: 'Human', el })
  })

  const assistantNodes = document.querySelectorAll(
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

function extractConversation(): string {
  const turns = collectTurns()
  const messages: string[] = []

  for (const { role, el } of turns) {
    const text = normalizeText((el as HTMLElement).innerText || el.textContent || '')
    if (!text) continue
    messages.push(`${role}: ${text}`)
  }

  return messages.join('\n\n')
}

// 监听来自 popup 的消息
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.action === 'extract') {
    const conversation = extractConversation()

    if (conversation) {
      chrome.storage.local.set({
        lastExtracted: {
          content: conversation,
          url: window.location.href,
          timestamp: Date.now(),
          source: 'gemini',
        },
      })

      enqueueConversation(conversation)
        .then((res) => {
          if (res.queued) {
            showToast('已加入同步队列，稍后上传到 Refine 云端')
          } else {
            showToast(res.message || '保存失败')
          }
        })
        .catch(() => {
          showToast('对话已提取，但入队失败，请稍后重试')
        })

      sendResponse({ success: true, length: conversation.length })
    } else {
      showToast('未找到对话内容')
      sendResponse({ success: false, message: '未找到对话内容' })
    }
  }
  return true
})

function enqueueConversation(content: string): Promise<EnqueueResponse> {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage(
      {
        action: 'enqueueExtractedConversation',
        payload: {
          content,
          url: window.location.href,
          source: 'gemini',
          title: document.title || 'Gemini Conversation',
          capturedAt: Date.now(),
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

export default function GeminiContent() {
  return null
}
