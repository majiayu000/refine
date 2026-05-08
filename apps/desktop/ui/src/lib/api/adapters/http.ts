import type { RefineApiClient } from '../client'
import type {
  ApiCapabilities,
  Conversation,
  ConversationListResult,
  CreateExtractionJobParams,
  CreateExtractionJobResult,
  CreateItemParams,
  Document,
  DocumentDetail,
  DocumentListResult,
  EventSummaryResult,
  FunnelCounts,
  Item,
  ItemListResult,
  ListConversationsParams,
  ListDocumentsParams,
  ListItemsParams,
  QuotaResult,
  SearchResult,
  UpdateItemParams,
} from '../types'

type ApiEnvelope<T extends object> = T & {
  success?: boolean
  message?: string
}

const DEFAULT_API_BASE = 'http://127.0.0.1:21567'
const TOKEN_STORAGE_KEY = 'refine_api_token'

const capabilities: ApiCapabilities = {
  runtime: 'http',
  items: {
    list: true,
    get: true,
    search: true,
    create: false,
    update: false,
    delete: true,
  },
  ops: {
    conversations: true,
    documents: true,
    funnel: true,
    extractionJobs: true,
    quota: true,
  },
  auth: {
    supportsBearerToken: true,
  },
}

function cloneCapabilities(): ApiCapabilities {
  return {
    runtime: capabilities.runtime,
    items: { ...capabilities.items },
    ops: { ...capabilities.ops },
    auth: { ...capabilities.auth },
  }
}

function getApiBase(): string {
  const envBase = (import.meta as any)?.env?.VITE_REFINE_API_BASE
  if (typeof envBase === 'string' && envBase.trim()) {
    return envBase
  }
  return DEFAULT_API_BASE
}

function readStoredToken(): string {
  if (typeof window === 'undefined') {
    return ''
  }
  try {
    return window.localStorage.getItem(TOKEN_STORAGE_KEY) || ''
  } catch {
    return ''
  }
}

function persistToken(token: string): void {
  if (typeof window === 'undefined') {
    return
  }
  try {
    if (token) {
      window.localStorage.setItem(TOKEN_STORAGE_KEY, token)
    } else {
      window.localStorage.removeItem(TOKEN_STORAGE_KEY)
    }
  } catch {
    // ignore storage failures
  }
}

function toNumber(value: unknown, fallback = 0): number {
  return typeof value === 'number' ? value : fallback
}

function toOptionalNumber(value: unknown): number | null {
  return typeof value === 'number' ? value : null
}

function toItems(value: unknown): Item[] {
  return Array.isArray(value) ? (value as Item[]) : []
}

function toConversations(value: unknown): Conversation[] {
  return Array.isArray(value) ? (value as Conversation[]) : []
}

async function requestJson<T extends object>(path: string, init?: RequestInit): Promise<ApiEnvelope<T>> {
  const headers = new Headers(init?.headers)
  if (init?.body != null && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  if (authToken) {
    headers.set('Authorization', `Bearer ${authToken}`)
  }

  const res = await fetch(`${getApiBase()}${path}`, {
    ...init,
    headers,
  })

  let data: ApiEnvelope<T> = {} as ApiEnvelope<T>
  try {
    data = (await res.json()) as ApiEnvelope<T>
  } catch {
    // keep default empty object
  }

  if (!res.ok || data.success === false) {
    throw new Error(data.message || `HTTP ${res.status}`)
  }

  return data
}

let authToken = readStoredToken().trim()

export function createHttpAdapter(): RefineApiClient {
  return {
    capabilities,
    getCapabilities: cloneCapabilities,
    getAuthToken: () => authToken,
    setAuthToken: (token: string) => {
      authToken = token.trim()
      persistToken(authToken)
    },

    getItems: async (params?: ListItemsParams): Promise<ItemListResult> => {
      const cursor = params?.cursor ?? 0
      const limit = params?.limit ?? 50

      const query = new URLSearchParams({
        cursor: String(cursor),
        limit: String(limit),
      })
      if (params?.item_type) {
        query.set('item_type', params.item_type)
      }

      const data = await requestJson<{
        items?: Item[]
        total?: number
        next_cursor?: number | null
      }>(`/v1/items?${query.toString()}`)

      const items = toItems(data.items)
      return {
        items,
        total: toNumber(data.total, items.length),
        nextCursor: toOptionalNumber(data.next_cursor),
      }
    },

    getItem: async (id: string): Promise<Item | null> => {
      const data = await requestJson<{ items?: Item[] }>('/v1/items?cursor=0&limit=200')
      return toItems(data.items).find((item) => item.id === id) ?? null
    },

    searchItems: async (query: string, limit?: number): Promise<SearchResult> => {
      const searchParams = new URLSearchParams({ q: query })
      if (typeof limit === 'number') {
        searchParams.set('limit', String(limit))
      }
      const data = await requestJson<{ items?: Item[]; total?: number }>(
        `/v1/search?${searchParams.toString()}`
      )
      const items = toItems(data.items)
      return {
        items,
        total: toNumber(data.total, items.length),
      }
    },

    createItem: async (_params: CreateItemParams): Promise<Item> => {
      throw new Error('HTTP 模式暂不支持 create_item')
    },

    updateItem: async (_params: UpdateItemParams): Promise<Item> => {
      throw new Error('HTTP 模式暂不支持 update_item')
    },

    deleteItem: async (id: string): Promise<boolean> => {
      const data = await requestJson<{ deleted?: boolean }>(`/v1/items/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      })
      return Boolean(data.deleted)
    },

    getDocuments: async (params?: ListDocumentsParams): Promise<DocumentListResult> => {
      const cursor = params?.cursor ?? 0
      const limit = params?.limit ?? 20
      const query = new URLSearchParams({
        cursor: String(cursor),
        limit: String(limit),
      })
      const data = await requestJson<{
        documents?: Document[]
        total?: number
        next_cursor?: number | null
      }>(`/v1/documents?${query.toString()}`)
      const documents = Array.isArray(data.documents) ? data.documents : []
      return {
        documents,
        total: toNumber(data.total, documents.length),
        nextCursor: toOptionalNumber(data.next_cursor),
      }
    },

    getDocument: async (id: string): Promise<DocumentDetail | null> => {
      try {
        const data = await requestJson<DocumentDetail>(`/v1/documents/${encodeURIComponent(id)}`)
        return data
      } catch {
        return null
      }
    },

    listConversations: async (
      params?: ListConversationsParams
    ): Promise<ConversationListResult> => {
      const cursor = params?.cursor ?? 0
      const limit = params?.limit ?? 20
      const searchParams = new URLSearchParams({
        cursor: String(cursor),
        limit: String(limit),
      })
      if (params?.status) {
        searchParams.set('status', params.status)
      }

      const data = await requestJson<{
        conversations?: Conversation[]
        total?: number
        next_cursor?: number | null
      }>(`/v1/conversations?${searchParams.toString()}`)
      const conversations = toConversations(data.conversations)
      return {
        conversations,
        total: toNumber(data.total, conversations.length),
        nextCursor: toOptionalNumber(data.next_cursor),
      }
    },

    getEventSummary: async (days?: number): Promise<EventSummaryResult> => {
      const dayCount = typeof days === 'number' ? Math.max(1, Math.min(days, 90)) : 7
      const data = await requestJson<{
        days?: number
        since?: string
        counts?: Partial<FunnelCounts>
      }>(`/v1/events/summary?days=${dayCount}`)

      const counts = data.counts ?? {}
      return {
        days: toNumber(data.days, dayCount),
        since: typeof data.since === 'string' ? data.since : '',
        counts: {
          conversation_extracted: toNumber(counts.conversation_extracted),
          conversation_synced: toNumber(counts.conversation_synced),
          recommendation_exposed: toNumber(counts.recommendation_exposed),
          recommendation_clicked: toNumber(counts.recommendation_clicked),
          knowledge_reused: toNumber(counts.knowledge_reused),
        },
      }
    },

    createExtractionJob: async (
      params: CreateExtractionJobParams
    ): Promise<CreateExtractionJobResult> => {
      const payload = {
        conversation_id: params.conversationId,
        mode: params.mode,
      }
      const data = await requestJson<{
        job_id?: string
        status?: string
      }>('/v1/extraction-jobs', {
        method: 'POST',
        body: JSON.stringify(payload),
      })

      return {
        jobId: typeof data.job_id === 'string' ? data.job_id : '',
        status: typeof data.status === 'string' ? data.status : 'pending',
      }
    },

    getQuota: async (): Promise<QuotaResult> => {
      const data = await requestJson<{
        limit?: number | null
        used?: number
        remaining?: number | null
        exceeded?: boolean
      }>('/v1/quota')

      return {
        limit: toOptionalNumber(data.limit),
        used: toNumber(data.used),
        remaining: toOptionalNumber(data.remaining),
        exceeded: Boolean(data.exceeded),
      }
    },
  }
}
