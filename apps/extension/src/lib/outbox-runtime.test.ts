import { describe, expect, test } from 'bun:test'

import { OutboxRuntime, type OutboxSnapshot, type OutboxStorage } from './outbox-runtime'
import type { ConversationPayload, OutboxItem } from './types'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function memoryStorage(initial?: Partial<OutboxSnapshot>): OutboxStorage & {
  current(): OutboxSnapshot
  readonly loads: number
} {
  let snapshot: OutboxSnapshot = {
    stats: { totalItems: 0, todayExtracted: 0 },
    outbox: [],
    syncState: {},
    ...initial,
  }
  let loads = 0
  return {
    async load() {
      loads += 1
      return structuredClone(snapshot)
    },
    async save(next) {
      snapshot = structuredClone(next)
    },
    current() {
      return structuredClone(snapshot)
    },
    get loads() {
      return loads
    },
  }
}

function payload(content: string): ConversationPayload {
  return {
    content,
    url: `https://example.test/${content}`,
    source: 'chatgpt',
    capturedAt: 1,
  }
}

function runtime(
  storage: OutboxStorage,
  upload: (item: OutboxItem) => Promise<{ success: boolean; conversationId?: string }>,
  counters?: { uploadCalls: number },
) {
  let sequence = 0
  return new OutboxRuntime({
    storage,
    async upload(item) {
      if (counters) counters.uploadCalls += 1
      return upload(item)
    },
    now: () => 10_000,
    randomUUID: () => `uuid-${++sequence}`,
    retryBaseDelayMs: 1,
    retryMaxDelayMs: 10,
    syncingRecoveryStaleMs: 60_000,
  })
}

describe('OutboxRuntime', () => {
  test('preserves an enqueue while an older upload is paused and counts success once', async () => {
    const storage = memoryStorage()
    const uploadStarted = deferred<void>()
    const releaseUpload = deferred<void>()
    const service = runtime(storage, async () => {
      uploadStarted.resolve()
      await releaseUpload.promise
      return { success: true, conversationId: 'remote-1' }
    })

    const first = await service.enqueue(payload('first'))
    const flushing = service.requestFlush(false)
    await uploadStarted.promise
    const second = await service.enqueue(payload('second'))

    let snapshot = storage.current()
    expect(snapshot.outbox.map((item) => item.id)).toEqual([first.id, second.id])
    expect(snapshot.outbox[0]?.status).toBe('syncing')
    expect(snapshot.outbox[1]?.status).toBe('pending')

    releaseUpload.resolve()
    await flushing
    snapshot = storage.current()
    expect(snapshot.outbox.map((item) => item.id)).toEqual([first.id, second.id])
    expect(snapshot.outbox.map((item) => item.status)).toEqual(['sent', 'pending'])
    expect(snapshot.stats).toEqual({ totalItems: 1, todayExtracted: 2 })
  })

  test('stale lease result cannot change status or double-count success', async () => {
    const storage = memoryStorage()
    const oldUploadStarted = deferred<void>()
    const releaseOldUpload = deferred<void>()
    const oldWorker = runtime(storage, async () => {
      oldUploadStarted.resolve()
      await releaseOldUpload.promise
      return { success: true, conversationId: 'remote-old' }
    })
    await oldWorker.enqueue(payload('first'))
    const oldFlush = oldWorker.requestFlush(false)
    await oldUploadStarted.promise

    const persisted = storage.current()
    persisted.outbox[0]!.updatedAt = -60_001
    await storage.save(persisted)
    const newWorker = runtime(storage, async () => ({ success: true, conversationId: 'remote-new' }))
    await newWorker.initialize()
    await newWorker.requestFlush(false)

    releaseOldUpload.resolve()
    await oldFlush
    const snapshot = storage.current()
    expect(snapshot.outbox[0]?.remoteConversationId).toBe('remote-new')
    expect(snapshot.stats.totalItems).toBe(1)
  })

  test('coalesces many normal flush requests into one trailing pass', async () => {
    const storage = memoryStorage()
    const firstUploadStarted = deferred<void>()
    const releaseFirstUpload = deferred<void>()
    const counters = { uploadCalls: 0 }
    const service = runtime(
      storage,
      async (item) => {
        if (item.payload.content === 'first') {
          firstUploadStarted.resolve()
          await releaseFirstUpload.promise
        }
        return { success: true }
      },
      counters,
    )

    await service.enqueue(payload('first'))
    const firstFlush = service.requestFlush(false)
    await firstUploadStarted.promise
    await service.enqueue(payload('second'))
    const requests = Array.from({ length: 20 }, () => service.requestFlush(false))
    releaseFirstUpload.resolve()
    await Promise.all([firstFlush, ...requests])

    expect(counters.uploadCalls).toBe(2)
    expect(storage.loads).toBe(8)
    expect(storage.current().outbox.map((item) => item.status)).toEqual(['sent', 'sent'])
  })

  test('keeps force flush ordered behind an active normal flush without duplicate upload', async () => {
    const storage = memoryStorage()
    const uploadStarted = deferred<void>()
    const releaseUpload = deferred<void>()
    const counters = { uploadCalls: 0 }
    const service = runtime(
      storage,
      async () => {
        uploadStarted.resolve()
        await releaseUpload.promise
        return { success: true }
      },
      counters,
    )

    await service.enqueue(payload('first'))
    const normal = service.requestFlush(false)
    await uploadStarted.promise
    const forced = service.requestFlush(true)
    releaseUpload.resolve()
    await Promise.all([normal, forced])

    expect(counters.uploadCalls).toBe(1)
    expect(storage.current().outbox[0]?.status).toBe('sent')
  })
})
