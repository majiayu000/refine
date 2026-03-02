import { createHttpAdapter } from './adapters/http'
import { createTauriAdapter } from './adapters/tauri'
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
} from './types'

export interface RefineApiClient {
  readonly capabilities: ApiCapabilities
  getCapabilities: () => ApiCapabilities
  getAuthToken: () => string
  setAuthToken: (token: string) => void
  getItems: (params?: ListItemsParams) => Promise<ItemListResult>
  getItem: (id: string) => Promise<Item | null>
  searchItems: (query: string, limit?: number) => Promise<SearchResult>
  createItem: (params: CreateItemParams) => Promise<Item>
  updateItem: (params: UpdateItemParams) => Promise<Item>
  deleteItem: (id: string) => Promise<boolean>
  listConversations: (params?: ListConversationsParams) => Promise<ConversationListResult>
  getEventSummary: (days?: number) => Promise<EventSummaryResult>
  createExtractionJob: (params: CreateExtractionJobParams) => Promise<CreateExtractionJobResult>
  getQuota: () => Promise<QuotaResult>
}

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && typeof (window as any).__TAURI_INTERNALS__ !== 'undefined'
}

function buildApiClient(): RefineApiClient {
  return isTauriRuntime() ? createTauriAdapter() : createHttpAdapter()
}

let cachedClient: RefineApiClient | null = null

export function getApiClient(): RefineApiClient {
  if (cachedClient == null) {
    cachedClient = buildApiClient()
  }
  return cachedClient
}

export function resetApiClientForTests(): void {
  cachedClient = null
}
