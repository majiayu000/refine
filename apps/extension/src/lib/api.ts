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

interface CloudItemsResponse {
  success?: boolean
  total?: number
  items?: unknown[]
  next_cursor?: number | null
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

export async function fetchCloudTotalItems(): Promise<number | null> {
  return fetchCloudTotalItemsWithOptions({ allowLegacyScan: true })
}

export async function fetchCloudTotalItemsWithOptions(options?: {
  allowLegacyScan?: boolean
}): Promise<number | null> {
  const apiBase = getCloudApiBase()
  const allowLegacyScan = options?.allowLegacyScan ?? true

  try {
    const headers = {
      'X-Refine-Client': 'extension',
    }
    const firstRes = await fetch(`${apiBase}/v1/items?cursor=0&limit=1`, { method: 'GET', headers })
    if (!firstRes.ok) return null
    const first = (await firstRes.json()) as CloudItemsResponse
    if (first.success === false) return null
    if (typeof first.total === 'number') return first.total

    if (!allowLegacyScan) return null

    // Backward-compatible fallback for older servers without `total` support.
    let legacyCount = 0
    let cursor = 0
    let rounds = 0
    const maxRounds = 100

    while (rounds < maxRounds) {
      rounds += 1
      const res = await fetch(`${apiBase}/v1/items?cursor=${cursor}&limit=100`, { method: 'GET', headers })
      if (!res.ok) return null
      const data = (await res.json()) as CloudItemsResponse
      if (data.success === false) return null
      if (!Array.isArray(data.items)) return null

      legacyCount += data.items.length
      if (typeof data.next_cursor !== 'number') {
        return legacyCount
      }
      cursor = data.next_cursor
    }

    return legacyCount
  } catch {
    return null
  }
}
