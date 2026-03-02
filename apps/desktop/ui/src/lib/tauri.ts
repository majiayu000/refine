import { getApiClient } from './api/client'
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
} from './api/types'

const client = getApiClient()

export type {
  ApiCapabilities,
  Conversation,
  ConversationListResult,
  CreateExtractionJobParams,
  CreateExtractionJobResult,
  CreateItemParams,
  EventSummaryResult,
  ExtractionMode,
  FunnelCounts,
  Item,
  ItemListResult,
  ListConversationsParams,
  ListItemsParams,
  QuotaResult,
  SearchResult,
  UpdateItemParams,
} from './api/types'

export function getApiCapabilities(): ApiCapabilities {
  return client.getCapabilities()
}

export function getAuthToken(): string {
  return client.getAuthToken()
}

export function setAuthToken(token: string): void {
  client.setAuthToken(token)
}

export function getItems(params?: ListItemsParams): Promise<ItemListResult> {
  return client.getItems(params)
}

export function getItem(id: string): Promise<Item | null> {
  return client.getItem(id)
}

export function searchItems(query: string, limit?: number): Promise<SearchResult> {
  return client.searchItems(query, limit)
}

export function createItem(params: CreateItemParams): Promise<Item> {
  return client.createItem(params)
}

export function updateItem(params: UpdateItemParams): Promise<Item> {
  return client.updateItem(params)
}

export function deleteItem(id: string): Promise<boolean> {
  return client.deleteItem(id)
}

export function listConversations(
  params?: ListConversationsParams
): Promise<ConversationListResult> {
  return client.listConversations(params)
}

export function getEventSummary(days?: number): Promise<EventSummaryResult> {
  return client.getEventSummary(days)
}

export function createExtractionJob(
  params: CreateExtractionJobParams
): Promise<CreateExtractionJobResult> {
  return client.createExtractionJob(params)
}

export function getQuota(): Promise<QuotaResult> {
  return client.getQuota()
}
