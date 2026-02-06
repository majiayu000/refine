/**
 * Background Service Worker
 *
 * 处理后台任务与云端同步（不依赖本地桌面端服务）
 */

import { checkCloudHealth, uploadConversation } from '../lib/api'
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

const DEFAULT_STATS: ExtensionStats = {
  totalItems: 0,
  todayExtracted: 0,
}

const DEFAULT_SYNC_STATE: SyncState = {}

let flushPromise: Promise<void> | null = null

interface StorageSnapshot {
  stats: ExtensionStats
  outbox: OutboxItem[]
  syncState: SyncState
}

interface EnqueueMessage {
  action: 'enqueueExtractedConversation'
  payload: ConversationPayload
}

interface GetSyncStatusMessage {
  action: 'getSyncStatus'
}

interface ForceSyncMessage {
  action: 'forceSync'
}

type BackgroundMessage = EnqueueMessage | GetSyncStatusMessage | ForceSyncMessage

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
  snapshot.outbox = pruneOutbox(snapshot.outbox)
  await saveSnapshot(snapshot)
}

async function resetDailyStats(): Promise<void> {
  const snapshot = await loadSnapshot()
  snapshot.stats.todayExtracted = 0
  await saveSnapshot(snapshot)
}

async function enqueueConversation(payload: ConversationPayload): Promise<{ queued: boolean; id: string }> {
  const snapshot = await loadSnapshot()
  const item = createOutboxItem(payload)
  snapshot.outbox.push(item)
  snapshot.stats.todayExtracted += 1
  await saveSnapshot(snapshot)

  void flushOutbox()

  return {
    queued: true,
    id: item.id,
  }
}

async function flushOutboxNow(): Promise<void> {
  const snapshot = await loadSnapshot()
  const now = Date.now()
  let changed = false

  for (const item of snapshot.outbox) {
    if (item.status === 'sent') continue
    if (item.status !== 'pending' && item.status !== 'failed') continue
    if (item.nextAttemptAt > now) continue

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
  if (flushPromise) {
    return flushPromise
  }

  flushPromise = flushOutboxNow().finally(() => {
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
        const cloudHealthy = await checkCloudHealth()
        sendResponse({
          cloudHealthy,
          status: buildSyncStatus(snapshot),
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
    flushOutbox()
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
