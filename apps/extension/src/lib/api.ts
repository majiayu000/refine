/**
 * Refine 云端 API 客户端
 */

import { discoverCloudApiBase, readApiToken } from './config'
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
import { withRequestDeadline } from './request-deadline'

export type { QuotaStatusResponse, RecommendationResponse, TrackEventRequest } from './cloud-contract'
export type { RecommendationItem } from './cloud-contract'

const DEFAULT_RECOMMENDATION_TIMEOUT_MS = 1_500
const DEFAULT_REQUEST_TIMEOUT_MS = 10_000
const DEFAULT_UPLOAD_TIMEOUT_MS = 25_000
const CONTRACT_VERSION_HEADER = 'X-Refine-Contract-Version'
const CLIENT_HEADER_NAME = 'X-Refine-Client'
const CLIENT_HEADER_VALUE = 'extension'
export const EXTENSION_CONTRACT_VERSION = '1.0'

interface RequestOptions {
  timeoutMs?: number
  signal?: AbortSignal
  public?: boolean
}

async function requestApi<T>(
  path: string,
  init: { method?: string; headers?: Record<string, string>; body?: string } = {},
  options: RequestOptions = {},
): Promise<{ res: Response; data: T | null }> {
  return withRequestDeadline(async (signal) => {
    const apiBase = await discoverCloudApiBase()
    const headers = options.public
      ? buildPublicHeaders(init.headers)
      : await buildProtectedHeaders(init.headers)
    if (signal.aborted) throw signal.reason
    const res = await fetch(`${apiBase}${path}`, { ...init, headers, signal })
    let data: T | null = null
    try {
      data = (await res.json()) as T
    } catch (error) {
      if (signal.aborted) throw error
    }
    return { res, data }
  }, options.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS, options.signal)
}

function normalizeContractMajor(version: string): string {
  const raw = version.trim()
  if (!raw) return ''
  return raw.split('.')[0] || raw
}

function isServerContractCompatible(serverVersion: string | null): boolean {
  if (!serverVersion) return true
  return normalizeContractMajor(serverVersion) === normalizeContractMajor(EXTENSION_CONTRACT_VERSION)
}

function buildPublicHeaders(extraHeaders?: Record<string, string>): Record<string, string> {
  return {
    [CLIENT_HEADER_NAME]: CLIENT_HEADER_VALUE,
    [CONTRACT_VERSION_HEADER]: EXTENSION_CONTRACT_VERSION,
    ...(extraHeaders || {}),
  }
}

async function buildProtectedHeaders(
  extraHeaders?: Record<string, string>
): Promise<Record<string, string>> {
  const headers = buildPublicHeaders(extraHeaders)
  const token = await readApiToken()
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  return headers
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
  try {
    const { res, data } = await requestApi<{ success?: boolean }>('/health', {}, { public: true })
    if (!res.ok) return false
    if (!isServerContractCompatible(res.headers.get('x-refine-contract-version'))) return false
    return data?.success === true
  } catch {
    return false
  }
}

export async function uploadConversation(item: OutboxItem, options?: RequestOptions): Promise<CloudUploadResult> {
  try {
    const { res, data } = await requestApi<CloudIngestResponse>('/v1/conversations', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(toRequestBody(item)),
    }, { timeoutMs: DEFAULT_UPLOAD_TIMEOUT_MS, ...options })

    if (!res.ok || data?.success !== true) {
      return {
        success: false,
        message: data?.message || `服务暂未接收会话（HTTP ${res.status}），本地任务已保留。`,
      }
    }

    if (typeof data.conversation_id !== 'string' || !data.conversation_id.trim()) {
      return { success: false, message: '服务未返回有效的会话回执，本地任务已保留。' }
    }

    return {
      success: true,
      conversationId: data.conversation_id,
      jobId: typeof data.job_id === 'string' ? data.job_id : undefined,
      status: data.status,
    }
  } catch (error) {
    return {
      success: false,
      message: error instanceof Error ? error.message : '无法连接到服务，本地任务已保留。',
    }
  }
}

export async function fetchCloudTotalItems(): Promise<number | null> {
  // Strict contract mode: `total` 字段是必需的，不再支持旧服务端回退扫描。
  try {
    const { res, data: first } = await requestApi<CloudItemsResponse>('/v1/items?cursor=0&limit=1')
    if (!res.ok || !first || first.success === false) return null
    return typeof first.total === 'number' ? first.total : null
  } catch {
    return null
  }
}

export async function fetchRecommendations(
  query: string,
  options?: { limit?: number; timeoutMs?: number }
): Promise<RecommendationResponse | null> {
  const limit = options?.limit ?? 5
  const timeoutMs = options?.timeoutMs ?? DEFAULT_RECOMMENDATION_TIMEOUT_MS
  const q = encodeURIComponent(query)
  try {
    const { res, data } = await requestApi<unknown>(`/v1/recommendations?q=${q}&limit=${limit}`, {}, { timeoutMs })

    if (!res.ok) return null
    return parseRecommendationResponse(data)
  } catch {
    return null
  }
}

export async function fetchQuotaStatus(options?: RequestOptions): Promise<QuotaStatusResponse | null> {
  try {
    const { res, data } = await requestApi<QuotaStatusResponse>('/v1/quota', {}, options)
    if (!res.ok) return null
    if (data?.success !== true) return null
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
  try {
    const { res, data } = await requestApi<{ success?: boolean }>('/v1/events', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    })

    if (!res.ok) return false
    return data?.success === true
  } catch {
    return false
  }
}
