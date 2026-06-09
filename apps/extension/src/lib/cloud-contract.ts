export interface CloudUploadRequest {
  content: string
  url: string
  source: string
  title?: string
  captured_at: string
  idempotency_key: string
  ingest_only: boolean
  metadata: Record<string, unknown>
}

export interface CloudUploadResult {
  success: boolean
  conversationId?: string
  status?: string
  message?: string
}

export type CloudIngestResponse = CloudIngestSuccess | CloudIngestError

export interface CloudIngestSuccess {
  success: true
  message?: string
  conversation_id?: string
  status?: string
}

export interface CloudIngestError {
  success: false
  message?: string
}

export interface CloudItemsResponse {
  success?: boolean
  total?: number
  items?: unknown[]
  next_cursor?: number | null
}

export interface QuotaStatusResponse {
  success: boolean
  limit: number | null
  used: number
  remaining: number | null
  exceeded: boolean
}

export interface RecommendationItem {
  id: string
  item_type: 'knowledge' | 'skill' | 'snippet' | string
  title: string
  summary: string
  content: string
  tags: string[]
  score: number
  match_strategy: string
}

export interface RecommendationResponse {
  success?: boolean
  triggered: boolean
  reason?: string
  min_chars?: number
  query: string
  total?: number
  items: RecommendationItem[]
  meta?: {
    latency_ms?: number
    strategy?: string
  }
}

export interface TrackEventRequest {
  event_name: string
  source?: string
  properties?: Record<string, unknown>
  occurred_at?: string
}
