/**
 * ChatGPT Content Script
 *
 * 在 ChatGPT 页面注入提取功能
 */

import type { PlasmoCSConfig } from 'plasmo'
import {
  enqueueConversation,
  normalizeText,
  persistLastExtracted,
  registerExtractActionHandler,
  showToast,
  type ExtractResult,
} from '../lib/content/runtime'

export const config: PlasmoCSConfig = {
  matches: ['https://chat.openai.com/*', 'https://chatgpt.com/*'],
  all_frames: false,
}

function extractConversation(): string {
  const messages: string[] = []

  const messageElements = document.querySelectorAll('[data-message-author-role]')

  messageElements.forEach((el) => {
    const role = el.getAttribute('data-message-author-role')
    const content = normalizeText((el as HTMLElement).innerText || el.textContent || '')

    if (!content) return
    if (role === 'user') {
      messages.push(`Human: ${content}`)
      return
    }
    if (role === 'assistant') {
      messages.push(`Assistant: ${content}`)
    }
  })

  return messages.join('\n\n')
}

async function extractAndEnqueueConversation(): Promise<ExtractResult> {
  const conversation = extractConversation()
  if (!conversation) {
    showToast('未找到对话内容')
    return {
      success: false,
      message: '未找到对话内容',
    }
  }

  const url = window.location.href
  persistLastExtracted(conversation, url, 'chatgpt')

  const enqueueResult = await enqueueConversation(conversation, {
    source: 'chatgpt',
    url,
    title: document.title || 'ChatGPT Conversation',
  })

  if (!enqueueResult.queued) {
    const message = enqueueResult.message || '保存失败'
    showToast(message)
    return {
      success: false,
      message,
    }
  }

  showToast('已加入同步队列，稍后上传到 Refine 云端')
  return {
    success: true,
    length: conversation.length,
  }
}

registerExtractActionHandler(async () => {
  try {
    return await extractAndEnqueueConversation()
  } catch {
    showToast('对话已提取，但入队失败，请稍后重试')
    return {
      success: false,
      message: '入队失败，请稍后重试',
    }
  }
})

export default function ChatGPTContent() {
  return null
}
