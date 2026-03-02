export type ItemType = 'knowledge' | 'skill' | 'snippet'

export interface Item {
  id: string
  item_type: ItemType
  title: string
  summary: string
  content: string
  tags: string[]
  created_at: string
}

export interface ItemListResult {
  items: Item[]
  total: number
  nextCursor: number | null
}

export interface SearchResult {
  items: Item[]
  total: number
}

export interface CreateItemParams {
  title: string
  summary: string
  content: string
  item_type?: ItemType
  tags?: string[]
}

export interface UpdateItemParams {
  id: string
  title?: string
  summary?: string
  content?: string
}

export interface ListItemsParams {
  item_type?: string
  cursor?: number
  limit?: number
}

export type ConversationStatus = 'captured' | 'queued' | 'processing' | 'processed' | 'failed'
export type ExtractionMode = 'auto' | 'knowledge' | 'skill' | 'snippet'

export interface Conversation {
  id: string
  source: string
  url: string
  title: string
  status: ConversationStatus
  captured_at: string
  created_at: string
  preview: string
}

export interface ListConversationsParams {
  status?: string
  cursor?: number
  limit?: number
}

export interface ConversationListResult {
  conversations: Conversation[]
  total: number
  nextCursor: number | null
}

export interface FunnelCounts {
  conversation_extracted: number
  conversation_synced: number
  recommendation_exposed: number
  recommendation_clicked: number
  knowledge_reused: number
}

export interface EventSummaryResult {
  days: number
  since: string
  counts: FunnelCounts
}

export interface CreateExtractionJobParams {
  conversationId: string
  mode?: ExtractionMode
}

export interface CreateExtractionJobResult {
  jobId: string
  status: string
}

export interface QuotaResult {
  limit: number | null
  used: number
  remaining: number | null
  exceeded: boolean
}

export interface ApiCapabilities {
  runtime: 'tauri' | 'http'
  items: {
    list: boolean
    get: boolean
    search: boolean
    create: boolean
    update: boolean
    delete: boolean
  }
  ops: {
    conversations: boolean
    funnel: boolean
    extractionJobs: boolean
    quota: boolean
  }
  auth: {
    supportsBearerToken: boolean
  }
}
