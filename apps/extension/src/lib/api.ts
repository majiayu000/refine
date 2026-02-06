/**
 * Refine 云端 API 客户端
 */

import { getCloudApiBase } from './config'
import type { CloudUploadRequest, CloudUploadResult, OutboxItem } from './types'

interface CloudIngestResponse {
  success: boolean
  message?: string
  conversation_id?: string
  status?: string
}

function toRequestBody(item: OutboxItem): CloudUploadRequest {
  return {
    content: item.payload.content,
    url: item.payload.url,
    source: item.payload.source,
    title: item.payload.title,
    captured_at: new Date(item.payload.capturedAt).toISOString(),
    idempotency_key: item.idempotencyKey,
  }
}

export async function checkCloudHealth(): Promise<boolean> {
  const apiBase = getCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/health`, {
      method: 'GET',
      headers: {
        'X-Refine-Client': 'extension',
      },
    })
    if (!res.ok) return false
    const data = (await res.json()) as { success?: boolean }
    return data.success === true
  } catch {
    return false
  }
}

export async function uploadConversation(item: OutboxItem): Promise<CloudUploadResult> {
  const apiBase = getCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/v1/conversations`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Refine-Client': 'extension',
      },
      body: JSON.stringify(toRequestBody(item)),
    })

    let data: CloudIngestResponse | null = null
    try {
      data = (await res.json()) as CloudIngestResponse
    } catch {
      data = null
    }

    if (!res.ok || !data?.success) {
      return {
        success: false,
        message: data?.message || `Cloud API error (${res.status})`,
      }
    }

    return {
      success: true,
      conversationId: data.conversation_id,
      status: data.status || 'queued',
    }
  } catch {
    return {
      success: false,
      message: '无法连接到云端服务，请检查网络或服务地址',
    }
  }
}
