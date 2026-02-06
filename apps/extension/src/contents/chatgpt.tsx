/**
 * ChatGPT Content Script
 *
 * 在 ChatGPT 页面注入提取功能
 */

import type { PlasmoCSConfig } from 'plasmo'

interface EnqueueResponse {
  queued: boolean
  message?: string
}

export const config: PlasmoCSConfig = {
  matches: ['https://chat.openai.com/*', 'https://chatgpt.com/*'],
  all_frames: false,
}

// 提取对话内容
function extractConversation(): string {
  const messages: string[] = []

  // ChatGPT 的对话容器选择器
  const messageElements = document.querySelectorAll('[data-message-author-role]')

  messageElements.forEach((el) => {
    const role = el.getAttribute('data-message-author-role')
    const content = el.textContent?.trim() || ''

    if (role === 'user') {
      messages.push(`Human: ${content}`)
    } else if (role === 'assistant') {
      messages.push(`Assistant: ${content}`)
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
          source: 'chatgpt',
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
          source: 'chatgpt',
          title: document.title || 'ChatGPT Conversation',
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

export default function ChatGPTContent() {
  return null
}
