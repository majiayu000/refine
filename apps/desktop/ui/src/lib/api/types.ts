export type ItemType = 'knowledge' | 'skill' | 'snippet' | 'observation'
export type CreatableItemType = Exclude<ItemType, 'observation'>

export interface Item {
  id: string
  item_type: ItemType
  title: string
  summary: string
  content: string
  tags: string[]
  document_id?: string
  excerpt?: string
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

export interface Document {
  id: string
  title: string | null
  source: string
  url: string
  item_count: number
  captured_at: string
  created_at: string
}

export interface DocumentDetail {
  id: string
  title: string | null
  raw_content: string
  source: string
  url: string
  captured_at: string
  created_at: string
  items: Item[]
}

export interface DocumentListResult {
  documents: Document[]
  total: number
  nextCursor: number | null
}

export interface ListDocumentsParams {
  cursor?: number
  limit?: number
}

export interface CreateItemParams {
  title: string
  summary: string
  content: string
  item_type?: CreatableItemType
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
  title: string | null
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
    documents: boolean
    funnel: boolean
    extractionJobs: boolean
    quota: boolean
  }
  auth: {
    supportsBearerToken: boolean
  }
}

export interface CommitMessage { id: number; role: string; content: string; created_at_epoch: number }
export interface CommitDiscussion { session_id: string; source: string; status: string; session_ref: string | null; content_hash: string | null; messages: CommitMessage[] }
export interface CommitContext { sha: string; project: string; message: string | null; sessions: CommitDiscussion[] }
export interface ProjectionEvidence {
  facets?: { field: string; index: number; status: string; message_ids: number[] }[]
  observations?: { item_id: string; field?: string; index?: number }[]
}
export interface CommitProjection {
  session_ref: string; document_id: string; source_version: string | null
  items: Item[]; evidence: ProjectionEvidence | null
  history: { revision_id: string; source_version: string | null; archived_at: string; items: Item[] }[]
}
export interface CommitResult { commits: CommitContext[]; projections: CommitProjection[] }
