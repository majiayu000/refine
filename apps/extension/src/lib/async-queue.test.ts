import { describe, expect, test } from 'bun:test'

import { createAsyncTaskQueue } from './async-queue'

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void
  const promise = new Promise<void>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

describe('createAsyncTaskQueue', () => {
  test('serializes a read-await-write interleaving without losing either update', async () => {
    const queue = createAsyncTaskQueue()
    const firstRead = deferred()
    const releaseFirstWrite = deferred()
    let persisted: string[] = []
    const reads: string[][] = []

    const mutate = (value: string, pause = false) =>
      queue.run(async () => {
        const snapshot = [...persisted]
        reads.push([...snapshot])
        if (pause) {
          firstRead.resolve()
          await releaseFirstWrite.promise
        }
        snapshot.push(value)
        persisted = snapshot
      })

    const first = mutate('first', true)
    await firstRead.promise
    const second = mutate('second')
    await Promise.resolve()

    expect(reads).toEqual([[]])
    releaseFirstWrite.resolve()
    await Promise.all([first, second])

    expect(reads).toEqual([[], ['first']])
    expect(persisted).toEqual(['first', 'second'])
  })

  test('continues after a rejected task', async () => {
    const queue = createAsyncTaskQueue()
    const failed = queue.run(async () => {
      throw new Error('storage write failed')
    })
    const recovered = queue.run(async () => 'next task completed')

    await expect(failed).rejects.toThrow('storage write failed')
    await expect(recovered).resolves.toBe('next task completed')
  })
})
