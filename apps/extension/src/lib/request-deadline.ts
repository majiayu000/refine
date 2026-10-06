export class RequestDeadlineError extends Error {
  constructor() {
    super('服务请求超时；本地保存的会话将保留并重试。')
    this.name = 'RequestDeadlineError'
  }
}

/** Bounds discovery, credentials, headers AND response-body reads, not just fetch headers. */
export async function withRequestDeadline<T>(
  operation: (signal: AbortSignal) => Promise<T>,
  timeoutMs: number,
  parentSignal?: AbortSignal,
): Promise<T> {
  const controller = new AbortController()
  let timeout: ReturnType<typeof setTimeout> | undefined
  let onParentAbort: (() => void) | undefined
  const canceled = new Promise<never>((_resolve, reject) => {
    const cancel = (error: unknown) => {
      controller.abort(error)
      reject(error)
    }
    timeout = setTimeout(() => cancel(new RequestDeadlineError()), Math.max(1, timeoutMs))
    if (parentSignal) {
      onParentAbort = () => cancel(parentSignal.reason ?? new Error('服务请求已取消'))
      if (parentSignal.aborted) onParentAbort()
      else parentSignal.addEventListener('abort', onParentAbort, { once: true })
    }
  })
  try {
    return await Promise.race([
      canceled,
      Promise.resolve().then(() => {
        if (controller.signal.aborted) throw controller.signal.reason
        return operation(controller.signal)
      }),
    ])
  } finally {
    clearTimeout(timeout)
    if (onParentAbort) parentSignal?.removeEventListener('abort', onParentAbort)
  }
}
