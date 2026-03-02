/**
 * Background Service Worker
 *
 * 处理后台任务与云端同步（不依赖本地桌面端服务）
 */

import {
  checkCloudHealth,
  fetchQuotaStatus,
  fetchCloudTotalItemsWithOptions,
  fetchRecommendations,
  type QuotaStatusResponse,
  trackEvent,
  uploadConversation,
} from '../lib/api'
import {
  getCloudApiBase,
  OUTBOX_FLUSH_ALARM,
  OUTBOX_FLUSH_INTERVAL_MINUTES,
  RESET_DAILY_STATS_ALARM,
  RESET_DAILY_STATS_INTERVAL_MINUTES,
  RETRY_BASE_DELAY_MS,
  RETRY_MAX_DELAY_MS,
} from '../lib/config'
import type {
  ConversationPayload,
  ExtensionStats,
  OutboxItem,
  SyncState,
  SyncStatus,
} from '../lib/types'

const STATS_KEY = 'stats'
const OUTBOX_KEY = 'outbox'
const SYNC_STATE_KEY = 'syncState'
const SYNCING_RECOVERY_STALE_MS = 60_000
const CLOUD_STATUS_CACHE_TTL_MS = 60_000

const DEFAULT_STATS: ExtensionStats = {
  totalItems: 0,
  todayExtracted: 0,
}

const DEFAULT_SYNC_STATE: SyncState = {}

let flushPromise: Promise<void> | null = null
let cloudStatusCache: CloudStatusCache | null = null

interface StorageSnapshot {
  stats: ExtensionStats
  outbox: OutboxItem[]
  syncState: SyncState
}

interface CloudStatusCache {
  updatedAt: number
  cloudHealthy: boolean
  remoteTotalItems: number | null
  quota: QuotaStatusResponse | null
}

interface EnqueueMessage {
  action: 'enqueueExtractedConversation'
  payload: ConversationPayload
}

interface EnqueueConversationResult {
  queued: boolean
  id?: string
  message?: string
}

interface GetSyncStatusMessage {
  action: 'getSyncStatus'
}

interface ForceSyncMessage {
  action: 'forceSync'
}

interface FetchRecommendationsMessage {
  action: 'fetchRecommendations'
  query: string
  options?: {
    limit?: number
    timeoutMs?: number
  }
}

type BackgroundMessage =
  | EnqueueMessage
  | GetSyncStatusMessage
  | ForceSyncMessage
  | FetchRecommendationsMessage

function formatQuotaExceededMessage(quota: QuotaStatusResponse): string {
  if (typeof quota.limit === 'number') {
    return `额度不足（${quota.used}/${quota.limit}），请升级会员或提高服务端额度后重试。`
  }
  return '额度不足，请升级会员或提高服务端额度后重试。'
}

function backoffDelayMs(attemptCount: number): number {
  const multiplier = Math.max(0, attemptCount - 1)
  const delay = RETRY_BASE_DELAY_MS * 2 ** multiplier
  return Math.min(delay, RETRY_MAX_DELAY_MS)
}

function createOutboxItem(payload: ConversationPayload): OutboxItem {
  const now = Date.now()
  return {
    id: crypto.randomUUID(),
    idempotencyKey: crypto.randomUUID(),
    payload,
    status: 'pending',
    attemptCount: 0,
    nextAttemptAt: now,
    createdAt: now,
    updatedAt: now,
  }
}

function pruneOutbox(outbox: OutboxItem[]): OutboxItem[] {
  const sentTtlMs = 24 * 60 * 60 * 1_000
  const now = Date.now()
  return outbox.filter((item) => item.status !== 'sent' || now - item.updatedAt <= sentTtlMs)
}

function recoverStuckSyncingItems(outbox: OutboxItem[], now = Date.now()): boolean {
  let changed = false

  for (const item of outbox) {
    if (item.status !== 'syncing') continue
    if (now - item.updatedAt < SYNCING_RECOVERY_STALE_MS) continue

    item.status = 'failed'
    item.lastError = item.lastError || 'Sync interrupted, retry scheduled'
    item.nextAttemptAt = now
    item.updatedAt = now
    changed = true
  }

  return changed
}

function buildSyncStatus(snapshot: StorageSnapshot): SyncStatus {
  const counts = {
    pending: 0,
    syncing: 0,
    failed: 0,
    sent: 0,
  }

  for (const item of snapshot.outbox) {
    counts[item.status] += 1
  }

  return {
    ...counts,
    lastError: snapshot.syncState.lastError,
    lastSyncedAt: snapshot.syncState.lastSyncedAt,
    apiBase: getCloudApiBase(),
  }
}

async function loadSnapshot(): Promise<StorageSnapshot> {
  const result = await chrome.storage.local.get([STATS_KEY, OUTBOX_KEY, SYNC_STATE_KEY])
  return {
    stats: (result[STATS_KEY] as ExtensionStats | undefined) || { ...DEFAULT_STATS },
    outbox: (result[OUTBOX_KEY] as OutboxItem[] | undefined) || [],
    syncState: (result[SYNC_STATE_KEY] as SyncState | undefined) || { ...DEFAULT_SYNC_STATE },
  }
}

async function saveSnapshot(snapshot: StorageSnapshot): Promise<void> {
  await chrome.storage.local.set({
    [STATS_KEY]: snapshot.stats,
    [OUTBOX_KEY]: snapshot.outbox,
    [SYNC_STATE_KEY]: snapshot.syncState,
  })
}

async function initializeStorage(): Promise<void> {
  const snapshot = await loadSnapshot()
  const now = Date.now()
  recoverStuckSyncingItems(snapshot.outbox, now)
  snapshot.outbox = pruneOutbox(snapshot.outbox)
  await saveSnapshot(snapshot)
}

async function getCloudStatusWithCache(): Promise<{
  cloudHealthy: boolean
  remoteTotalItems: number | null
  quota: QuotaStatusResponse | null
}> {
  const now = Date.now()
  if (cloudStatusCache && now - cloudStatusCache.updatedAt < CLOUD_STATUS_CACHE_TTL_MS) {
    return {
      cloudHealthy: cloudStatusCache.cloudHealthy,
      remoteTotalItems: cloudStatusCache.remoteTotalItems,
      quota: cloudStatusCache.quota,
    }
  }

  const cloudHealthy = await checkCloudHealth()
  let remoteTotalItems: number | null = null
  let quota: QuotaStatusResponse | null = null
  if (cloudHealthy) {
    ;[remoteTotalItems, quota] = await Promise.all([
      fetchCloudTotalItemsWithOptions({ allowLegacyScan: false }),
      fetchQuotaStatus(),
    ])
  }

  cloudStatusCache = {
    updatedAt: now,
    cloudHealthy,
    remoteTotalItems,
    quota,
  }

  return {
    cloudHealthy,
    remoteTotalItems,
    quota,
  }
}

async function resetDailyStats(): Promise<void> {
  const snapshot = await loadSnapshot()
  snapshot.stats.todayExtracted = 0
  await saveSnapshot(snapshot)
}

async function enqueueConversation(payload: ConversationPayload): Promise<EnqueueConversationResult> {
  const quota = await fetchQuotaStatus()
  if (quota?.success && quota.exceeded) {
    return {
      queued: false,
      message: formatQuotaExceededMessage(quota),
    }
  }

  const snapshot = await loadSnapshot()
  const item = createOutboxItem(payload)
  snapshot.outbox.push(item)
  snapshot.stats.todayExtracted += 1
  await saveSnapshot(snapshot)

  void trackEvent({
    event_name: 'conversation_extracted',
    source: payload.source,
    properties: {
      provider: payload.source,
      has_title: typeof payload.title === 'string' && payload.title.length > 0,
      content_length: payload.content.length,
    },
    occurred_at: new Date().toISOString(),
  })

  void flushOutbox()

  return {
    queued: true,
    id: item.id,
  }
}

async function flushOutboxNow(): Promise<void> {
  await flushOutboxWithOptions({ forceRetry: false })
}

async function flushOutboxWithOptions(options: { forceRetry: boolean }): Promise<void> {
  const snapshot = await loadSnapshot()
  const now = Date.now()
  const forceRetry = options.forceRetry
  let changed = recoverStuckSyncingItems(snapshot.outbox, now)

  for (const item of snapshot.outbox) {
    if (item.status === 'sent') continue
    if (item.status !== 'pending' && item.status !== 'failed') continue
    if (!forceRetry && item.nextAttemptAt > now) continue
    if (forceRetry && item.status === 'failed') {
      item.nextAttemptAt = now
    }

    item.status = 'syncing'
    item.updatedAt = Date.now()
    changed = true
    await saveSnapshot(snapshot)

    const result = await uploadConversation(item)

    if (result.success) {
      item.status = 'sent'
      item.lastError = undefined
      item.remoteConversationId = result.conversationId
      item.updatedAt = Date.now()
      snapshot.stats.totalItems += 1
      snapshot.syncState.lastSyncedAt = Date.now()
      snapshot.syncState.lastError = undefined
      changed = true

      void trackEvent({
        event_name: 'conversation_synced',
        source: item.payload.source,
        properties: {
          provider: item.payload.source,
          outbox_item_id: item.id,
          remote_conversation_id: result.conversationId || null,
          attempt_count: item.attemptCount,
        },
        occurred_at: new Date().toISOString(),
      })
      continue
    }

    item.status = 'failed'
    item.attemptCount += 1
    item.lastError = result.message || 'Unknown cloud sync error'
    item.nextAttemptAt = Date.now() + backoffDelayMs(item.attemptCount)
    item.updatedAt = Date.now()
    snapshot.syncState.lastError = item.lastError
    changed = true
  }

  if (changed) {
    snapshot.outbox = pruneOutbox(snapshot.outbox)
    await saveSnapshot(snapshot)
  }
}

async function flushOutbox(): Promise<void> {
  return flushOutboxWith({ forceRetry: false })
}

async function flushOutboxWith(options: { forceRetry: boolean }): Promise<void> {
  if (flushPromise) {
    await flushPromise
    if (!options.forceRetry) return
  }

  flushPromise = flushOutboxWithOptions(options).finally(() => {
    flushPromise = null
  })

  return flushPromise
}

async function ensureAlarms(): Promise<void> {
  await chrome.alarms.create(OUTBOX_FLUSH_ALARM, {
    periodInMinutes: OUTBOX_FLUSH_INTERVAL_MINUTES,
  })
  await chrome.alarms.create(RESET_DAILY_STATS_ALARM, {
    periodInMinutes: RESET_DAILY_STATS_INTERVAL_MINUTES,
  })
}

async function bootstrap(): Promise<void> {
  await initializeStorage()
  await ensureAlarms()
  void flushOutbox()
}

chrome.runtime.onInstalled.addListener(() => {
  console.log('Refine extension installed')
  void bootstrap()
})

chrome.runtime.onStartup?.addListener(() => {
  void bootstrap()
})

void bootstrap()

chrome.runtime.onMessage.addListener((message: BackgroundMessage, _sender, sendResponse) => {
  if (message.action === 'enqueueExtractedConversation') {
    enqueueConversation(message.payload)
      .then((result) => {
        sendResponse(result)
      })
      .catch((error: unknown) => {
        sendResponse({
          queued: false,
          message: error instanceof Error ? error.message : String(error),
        })
      })
    return true
  }

  if (message.action === 'getSyncStatus') {
    loadSnapshot()
      .then(async (snapshot) => {
        const cloud = await getCloudStatusWithCache()
        sendResponse({
          cloudHealthy: cloud.cloudHealthy,
          status: buildSyncStatus(snapshot),
          remoteTotalItems: cloud.remoteTotalItems ?? undefined,
          quota: cloud.quota ?? undefined,
        })
      })
      .catch((error: unknown) => {
        sendResponse({
          cloudHealthy: false,
          status: {
            pending: 0,
            syncing: 0,
            failed: 0,
            sent: 0,
            apiBase: getCloudApiBase(),
            lastError: error instanceof Error ? error.message : String(error),
          },
        })
      })
    return true
  }

  if (message.action === 'forceSync') {
    flushOutboxWith({ forceRetry: true })
      .then(async () => {
        const snapshot = await loadSnapshot()
        sendResponse({
          ok: true,
          status: buildSyncStatus(snapshot),
        })
      })
      .catch((error: unknown) => {
        sendResponse({
          ok: false,
          message: error instanceof Error ? error.message : String(error),
        })
      })
    return true
  }

  if (message.action === 'fetchRecommendations') {
    fetchRecommendations(message.query, message.options)
      .then((result) => {
        sendResponse(result)
      })
      .catch(() => {
        sendResponse(null)
      })
    return true
  }

  return false
})

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === RESET_DAILY_STATS_ALARM) {
    void resetDailyStats()
    return
  }

  if (alarm.name === OUTBOX_FLUSH_ALARM) {
    void flushOutbox()
  }
})

export {}
