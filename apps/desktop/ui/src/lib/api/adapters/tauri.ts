import { invoke } from '@tauri-apps/api/core'
import type { RefineApiClient } from '../client'
import type {
  ApiCapabilities,
  ConversationListResult,
  CreateExtractionJobParams,
  CreateExtractionJobResult,
  CreateItemParams,
  EventSummaryResult,
  Item,
  ItemListResult,
  ListConversationsParams,
  ListItemsParams,
  QuotaResult,
  SearchResult,
  UpdateItemParams,
} from '../types'

interface RawItemListResult {
  items?: Item[]
  total?: number
  next_cursor?: number | null
}

function normalizeItemListResult(data: RawItemListResult): ItemListResult {
  const items = Array.isArray(data.items) ? data.items : []
  return {
    items,
    total: typeof data.total === 'number' ? data.total : items.length,
    nextCursor: typeof data.next_cursor === 'number' ? data.next_cursor : null,
  }
}

function unsupported(feature: string): Error {
  return new Error(`当前 Tauri 本地模式暂不支持 ${feature}`)
}

const capabilities: ApiCapabilities = {
  runtime: 'tauri',
  items: {
    list: true,
    get: true,
    search: true,
    create: true,
    update: true,
    delete: true,
  },
  ops: {
    conversations: false,
    funnel: false,
    extractionJobs: false,
    quota: false,
  },
  auth: {
    supportsBearerToken: false,
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

export function createTauriAdapter(): RefineApiClient {
  return {
    capabilities,
    getCapabilities: cloneCapabilities,
    getAuthToken: () => '',
    setAuthToken: (_token: string) => {
      // local invoke mode does not use bearer token
    },

    getItems: async (params?: ListItemsParams): Promise<ItemListResult> => {
      const data = await invoke<RawItemListResult>('get_items', {
        item_type: params?.item_type,
        cursor: params?.cursor ?? 0,
        limit: params?.limit ?? 50,
      })
      return normalizeItemListResult(data)
    },

    getItem: (id: string): Promise<Item | null> => invoke('get_item', { id }),

    searchItems: (query: string, limit?: number): Promise<SearchResult> =>
      invoke('search_items', { query, limit }),

    createItem: (params: CreateItemParams): Promise<Item> =>
      invoke('create_item', {
        title: params.title,
        summary: params.summary,
        content: params.content,
        item_type: params.item_type,
        tags: params.tags,
      }),

    updateItem: (params: UpdateItemParams): Promise<Item> =>
      invoke('update_item', {
        id: params.id,
        title: params.title,
        summary: params.summary,
        content: params.content,
      }),

    deleteItem: (id: string): Promise<boolean> => invoke('delete_item', { id }),

    listConversations: async (_params?: ListConversationsParams): Promise<ConversationListResult> => {
      throw unsupported('会话管理（/v1/conversations）')
    },

    getEventSummary: async (_days?: number): Promise<EventSummaryResult> => {
      throw unsupported('漏斗统计（/v1/events/summary）')
    },

    createExtractionJob: async (
      _params: CreateExtractionJobParams
    ): Promise<CreateExtractionJobResult> => {
      throw unsupported('手动总结任务（/v1/extraction-jobs）')
    },

    getQuota: async (): Promise<QuotaResult> => {
      throw unsupported('配额接口（/v1/quota）')
    },
  }
}
