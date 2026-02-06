/**
 * Claude Content Script
 *
 * 在 Claude.ai 页面注入提取功能
 */

import type { PlasmoCSConfig } from 'plasmo'

interface EnqueueResponse {
  queued: boolean
  message?: string
}

export const config: PlasmoCSConfig = {
  matches: ['https://claude.ai/*'],
  all_frames: false,
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
    const content = el.textContent?.trim() || ''
    if (content) {
      messages.push(`${role}: ${content}`)
    }
  })

  return messages.join('\n\n')
}

// 监听来自 popup 的消息
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.action === 'extract') {
    const conversation = extractConversation()

    if (conversation) {
      // 保存到 storage（备份）
      chrome.storage.local.set({
        lastExtracted: {
          content: conversation,
          url: window.location.href,
          timestamp: Date.now(),
          source: 'claude',
        },
      })

      // 发送到 background 入队并异步同步到云端
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
      sendResponse({ success: false })
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
          source: 'claude',
          title: document.title || 'Claude Conversation',
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

export default function ClaudeContent() {
  return null
}
