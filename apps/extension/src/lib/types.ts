export type ConversationSource = 'chatgpt' | 'claude' | 'gemini'

export interface ConversationPayload {
  content: string
  url: string
  source: ConversationSource
  title?: string
  capturedAt: number
}

export interface OutboxItem {
  id: string
  idempotencyKey: string
  payload: ConversationPayload
  status: 'pending' | 'syncing' | 'failed' | 'sent'
  attemptCount: number
  nextAttemptAt: number
  lastError?: string
  remoteConversationId?: string
  createdAt: number
  updatedAt: number
}

export interface ExtensionStats {
  totalItems: number
  todayExtracted: number
}

export interface SyncState {
  lastError?: string
  lastSyncedAt?: number
}

export interface SyncStatus {
  pending: number
  syncing: number
  failed: number
  sent: number
  lastError?: string
  lastSyncedAt?: number
  apiBase: string
}

export interface CloudUploadRequest {
  content: string
  url: string
  source: string
  title?: string
  captured_at: string
  idempotency_key: string
}

export interface CloudUploadResult {
  success: boolean
  conversationId?: string
  status?: string
  message?: string
}
