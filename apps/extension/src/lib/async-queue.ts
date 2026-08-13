export interface AsyncTaskQueue {
  run<T>(task: () => Promise<T>): Promise<T>
}

export function createAsyncTaskQueue(): AsyncTaskQueue {
  let tail: Promise<void> = Promise.resolve()

  return {
    run<T>(task: () => Promise<T>): Promise<T> {
      const result = tail.then(task)
      tail = result.then(
        () => undefined,
        () => undefined,
      )
      return result
    },
  }
}
