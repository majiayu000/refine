import type { CloudUploadResult } from './cloud-contract'
import { createAsyncTaskQueue } from './async-queue'
import { findLeasedItem } from './outbox-state'
import type {
  ConversationPayload,
  ExtensionStats,
  OutboxItem,
  SyncState,
} from './types'

export interface OutboxSnapshot {
  stats: ExtensionStats
  outbox: OutboxItem[]
  syncState: SyncState
}

export interface OutboxStorage {
  load(): Promise<OutboxSnapshot>
  save(snapshot: OutboxSnapshot): Promise<void>
}

export interface OutboxRuntimeOptions {
  storage: OutboxStorage
  upload(item: OutboxItem): Promise<CloudUploadResult>
  onSynced?(item: OutboxItem, result: CloudUploadResult): void
  now?: () => number
  randomUUID?: () => string
  retryBaseDelayMs: number
  retryMaxDelayMs: number
  syncingRecoveryStaleMs: number
  sentTtlMs?: number
}

export class OutboxRuntime {
  private readonly mutationQueue = createAsyncTaskQueue()
  private readonly flushQueue = createAsyncTaskQueue()
  private normalFlushDirty = false
  private normalFlushPromise: Promise<void> | null = null

  constructor(private readonly options: OutboxRuntimeOptions) {}

  readSnapshot(): Promise<OutboxSnapshot> {
    return this.options.storage.load()
  }

  initialize(): Promise<void> {
    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      this.recoverStuckItems(snapshot.outbox)
      snapshot.outbox = this.prune(snapshot.outbox)
      await this.options.storage.save(snapshot)
    })
  }

  resetDailyStats(): Promise<void> {
    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      snapshot.stats.todayExtracted = 0
      await this.options.storage.save(snapshot)
    })
  }

  enqueue(payload: ConversationPayload): Promise<OutboxItem> {
    const now = this.now()
    const item: OutboxItem = {
      id: this.uuid(),
      idempotencyKey: this.uuid(),
      payload,
      status: 'pending',
      attemptCount: 0,
      nextAttemptAt: now,
      createdAt: now,
      updatedAt: now,
    }

    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      snapshot.outbox.push(item)
      snapshot.stats.todayExtracted += 1
      await this.options.storage.save(snapshot)
      return item
    })
  }

  requestFlush(forceRetry: boolean): Promise<void> {
    if (forceRetry) {
      return this.flushQueue.run(() => this.flushPass(true))
    }

    this.normalFlushDirty = true
    if (this.normalFlushPromise) return this.normalFlushPromise

    const run = this.flushQueue.run(async () => {
      while (this.normalFlushDirty) {
        this.normalFlushDirty = false
        await this.flushPass(false)
      }
    })
    this.normalFlushPromise = run.finally(() => {
      this.normalFlushPromise = null
      if (this.normalFlushDirty) void this.requestFlush(false)
    })
    return this.normalFlushPromise
  }

  private async flushPass(forceRetry: boolean): Promise<void> {
    const candidateIds = await this.listCandidateIds(forceRetry)
    for (const candidateId of candidateIds) {
      const item = await this.claim(candidateId, forceRetry)
      if (!item) continue

      const result = await this.options.upload(item)
      const committed = await this.finish(item, result)
      if (committed && result.success) this.options.onSynced?.(item, result)
    }
  }

  private listCandidateIds(forceRetry: boolean): Promise<string[]> {
    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      const recovered = this.recoverStuckItems(snapshot.outbox)
      if (recovered) await this.options.storage.save(snapshot)
      const now = this.now()
      return snapshot.outbox
        .filter((item) => {
          if (item.status !== 'pending' && item.status !== 'failed') return false
          return forceRetry || item.nextAttemptAt <= now
        })
        .map((item) => item.id)
    })
  }

  private claim(itemId: string, forceRetry: boolean): Promise<OutboxItem | null> {
    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      const now = this.now()
      const item = snapshot.outbox.find((candidate) => candidate.id === itemId)
      if (!item || (item.status !== 'pending' && item.status !== 'failed')) return null
      if (!forceRetry && item.nextAttemptAt > now) return null

      item.status = 'syncing'
      item.syncLeaseId = this.uuid()
      item.updatedAt = now
      await this.options.storage.save(snapshot)
      return structuredClone(item)
    })
  }

  private finish(claimedItem: OutboxItem, result: CloudUploadResult): Promise<boolean> {
    return this.mutationQueue.run(async () => {
      const snapshot = await this.options.storage.load()
      const item = findLeasedItem(snapshot.outbox, claimedItem)
      if (!item) return false

      item.syncLeaseId = undefined
      item.updatedAt = this.now()
      if (result.success) {
        item.status = 'sent'
        item.lastError = undefined
        item.remoteConversationId = result.conversationId
        snapshot.stats.totalItems += 1
        snapshot.syncState.lastSyncedAt = item.updatedAt
        snapshot.syncState.lastError = undefined
      } else {
        item.status = 'failed'
        item.attemptCount += 1
        item.lastError = result.message || 'Unknown cloud sync error'
        item.nextAttemptAt = item.updatedAt + this.backoffDelayMs(item.attemptCount)
        snapshot.syncState.lastError = item.lastError
      }

      snapshot.outbox = this.prune(snapshot.outbox)
      await this.options.storage.save(snapshot)
      return true
    })
  }

  private recoverStuckItems(outbox: OutboxItem[]): boolean {
    const now = this.now()
    let changed = false
    for (const item of outbox) {
      if (item.status !== 'syncing') continue
      if (now - item.updatedAt < this.options.syncingRecoveryStaleMs) continue
      item.status = 'failed'
      item.syncLeaseId = undefined
      item.lastError = item.lastError || 'Sync interrupted, retry scheduled'
      item.nextAttemptAt = now
      item.updatedAt = now
      changed = true
    }
    return changed
  }

  private prune(outbox: OutboxItem[]): OutboxItem[] {
    const ttl = this.options.sentTtlMs ?? 24 * 60 * 60 * 1_000
    const now = this.now()
    return outbox.filter((item) => item.status !== 'sent' || now - item.updatedAt <= ttl)
  }

  private backoffDelayMs(attemptCount: number): number {
    const multiplier = Math.max(0, attemptCount - 1)
    return Math.min(
      this.options.retryBaseDelayMs * 2 ** multiplier,
      this.options.retryMaxDelayMs,
    )
  }

  private now(): number {
    return (this.options.now ?? Date.now)()
  }

  private uuid(): string {
    return (this.options.randomUUID ?? (() => crypto.randomUUID()))()
  }
}
