/**
 * Refine 云端 API 客户端
 */

import { discoverCloudApiBase } from './config'
import type {
  CloudIngestResponse,
  CloudItemsResponse,
  CloudUploadRequest,
  CloudUploadResult,
  QuotaStatusResponse,
  RecommendationResponse,
  TrackEventRequest,
} from './cloud-contract'
import type { OutboxItem } from './types'

export type { QuotaStatusResponse, RecommendationResponse, TrackEventRequest } from './cloud-contract'
export type { RecommendationItem } from './cloud-contract'

const DEFAULT_RECOMMENDATION_TIMEOUT_MS = 1_500
const CONTRACT_VERSION_HEADER = 'X-Refine-Contract-Version'
const CLIENT_HEADER_NAME = 'X-Refine-Client'
const CLIENT_HEADER_VALUE = 'extension'
export const EXTENSION_CONTRACT_VERSION = '1.0'

function normalizeContractMajor(version: string): string {
  const raw = version.trim()
  if (!raw) return ''
  return raw.split('.')[0] || raw
}

function isServerContractCompatible(serverVersion: string | null): boolean {
  if (!serverVersion) return true
  return normalizeContractMajor(serverVersion) === normalizeContractMajor(EXTENSION_CONTRACT_VERSION)
}

function buildHeaders(extraHeaders?: Record<string, string>): Record<string, string> {
  return {
    [CLIENT_HEADER_NAME]: CLIENT_HEADER_VALUE,
    [CONTRACT_VERSION_HEADER]: EXTENSION_CONTRACT_VERSION,
    ...(extraHeaders || {}),
  }
}

function toRequestBody(item: OutboxItem): CloudUploadRequest {
  return {
    content: item.payload.content,
    url: item.payload.url,
    source: item.payload.source,
    title: item.payload.title,
    captured_at: new Date(item.payload.capturedAt).toISOString(),
    idempotency_key: item.idempotencyKey,
    ingest_only: false,
    metadata: {},
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function unwrapDataEnvelope(value: unknown): unknown {
  if (isRecord(value) && isRecord(value.data)) {
    return value.data
  }
  return value
}

function parseRecommendationResponse(value: unknown): RecommendationResponse | null {
  if (isRecord(value) && value.success === false) return null
  const payload = unwrapDataEnvelope(value)
  if (!isRecord(payload)) return null
  if (payload.success === false) return null
  if (typeof payload.triggered !== 'boolean') return null
  if (!Array.isArray(payload.items)) return null
  return payload as unknown as RecommendationResponse
}

export async function checkCloudHealth(): Promise<boolean> {
  const apiBase = await discoverCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/health`, {
      method: 'GET',
      headers: buildHeaders(),
    })
    if (!res.ok) return false
    if (!isServerContractCompatible(res.headers.get('x-refine-contract-version'))) return false
    const data = (await res.json()) as { success?: boolean }
    return data.success === true
  } catch {
    return false
  }
}

export async function uploadConversation(item: OutboxItem): Promise<CloudUploadResult> {
  const apiBase = await discoverCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/v1/conversations`, {
      method: 'POST',
      headers: buildHeaders({
        'Content-Type': 'application/json',
      }),
      body: JSON.stringify(toRequestBody(item)),
    })

    let data: CloudIngestResponse | null = null
    try {
      data = (await res.json()) as CloudIngestResponse
    } catch {
      data = null
    }

    if (!res.ok || data?.success !== true) {
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
  // Strict contract mode: `total` 字段是必需的，不再支持旧服务端回退扫描。
  const apiBase = await discoverCloudApiBase()

  try {
    const headers = buildHeaders()
    const firstRes = await fetch(`${apiBase}/v1/items?cursor=0&limit=1`, { method: 'GET', headers })
    if (!firstRes.ok) return null
    const first = (await firstRes.json()) as CloudItemsResponse
    if (first.success === false) return null
    return typeof first.total === 'number' ? first.total : null
  } catch {
    return null
  }
}

export async function fetchRecommendations(
  query: string,
  options?: { limit?: number; timeoutMs?: number }
): Promise<RecommendationResponse | null> {
  const apiBase = await discoverCloudApiBase()
  const limit = options?.limit ?? 5
  const timeoutMs = options?.timeoutMs ?? DEFAULT_RECOMMENDATION_TIMEOUT_MS
  const q = encodeURIComponent(query)
  const controller = new AbortController()
  const timeoutId = globalThis.setTimeout(() => {
    controller.abort()
  }, timeoutMs)

  try {
    const res = await fetch(`${apiBase}/v1/recommendations?q=${q}&limit=${limit}`, {
      method: 'GET',
      headers: buildHeaders(),
      signal: controller.signal,
    })

    if (!res.ok) return null
    return parseRecommendationResponse(await res.json())
  } catch {
    return null
  } finally {
    globalThis.clearTimeout(timeoutId)
  }
}

export async function fetchQuotaStatus(): Promise<QuotaStatusResponse | null> {
  const apiBase = await discoverCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/v1/quota`, {
      method: 'GET',
      headers: buildHeaders(),
    })
    if (!res.ok) return null

    const data = (await res.json()) as QuotaStatusResponse
    if (data.success !== true) return null
    if (typeof data.used !== 'number' || typeof data.exceeded !== 'boolean') return null

    return {
      success: true,
      limit: typeof data.limit === 'number' ? data.limit : null,
      used: data.used,
      remaining: typeof data.remaining === 'number' ? data.remaining : null,
      exceeded: data.exceeded,
    }
  } catch {
    return null
  }
}

export async function trackEvent(payload: TrackEventRequest): Promise<boolean> {
  const apiBase = await discoverCloudApiBase()

  try {
    const res = await fetch(`${apiBase}/v1/events`, {
      method: 'POST',
      headers: buildHeaders({
        'Content-Type': 'application/json',
      }),
      body: JSON.stringify(payload),
    })

    if (!res.ok) return false
    const data = (await res.json()) as { success?: boolean }
    return data.success === true
  } catch {
    return false
  }
}
