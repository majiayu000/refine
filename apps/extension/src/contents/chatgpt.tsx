/**
 * ChatGPT Content Script
 *
 * 在 ChatGPT 页面注入提取功能
 */

import type { PlasmoCSConfig } from 'plasmo'
import { extractConversationBySelectors } from '../lib/content/platform-adapter'
import { initRecommendationEngine } from '../lib/content/recommendation-engine'
import {
  persistAndEnqueueConversation,
  registerExtractActionHandler,
  showToast,
  type ExtractResult,
} from '../lib/content/runtime'

export const config: PlasmoCSConfig = {
  matches: ['https://chat.openai.com/*', 'https://chatgpt.com/*'],
  all_frames: false,
}

initRecommendationEngine({
  providerId: 'chatgpt',
  source: 'chatgpt',
  inputSelectors: [
    '#prompt-textarea',
    'textarea[data-id="root"]',
    'textarea[placeholder*="Message"]',
    'textarea[placeholder*="消息"]',
  ],
})

function extractConversation(): string {
  return extractConversationBySelectors([
    { role: 'Human', selector: '[data-message-author-role="user"]' },
    { role: 'Assistant', selector: '[data-message-author-role="assistant"]' },
  ])
}

async function extractAndEnqueueConversation(): Promise<ExtractResult> {
  const conversation = extractConversation()
  if (!conversation) {
    return {
      success: false,
      message: '未找到对话内容',
    }
  }

  return persistAndEnqueueConversation({
    source: 'chatgpt',
    content: conversation,
    url: window.location.href,
    title: document.title || 'ChatGPT Conversation',
    saveFailedFallback: '保存失败',
  })
}

registerExtractActionHandler(async () => {
  try {
    const result = await extractAndEnqueueConversation()
    if (result.success) {
      showToast('已加入同步队列，稍后上传到 Refine 云端')
      return result
    }

    showToast(result.message || '保存失败')
    return {
      success: false,
      message: result.message || '保存失败',
    }
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
