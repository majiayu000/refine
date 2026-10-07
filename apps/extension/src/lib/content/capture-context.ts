/** A capture belongs to one navigation, even if the page later returns to the same URL. */
export interface CaptureValidation {
  isCurrent: () => boolean
  previousContentFingerprint?: string
}

export class ConversationChangedError extends Error {
  constructor() {
    super('会话已切换，本次采集未入队。请在目标会话重新保存。')
    this.name = 'ConversationChangedError'
  }
}

export function assertCurrentCapture(validation?: CaptureValidation): void {
  if (validation && !validation.isCurrent()) throw new ConversationChangedError()
}

// This is a freshness marker, not a security digest. Only the marker is put in
// page-owned sessionStorage; the previous conversation's text stays out of it.
// A hash collision can make capture fail closed, never bypass the freshness check.
export function contentFingerprint(content: string): string {
  let hash = 2166136261
  for (let i = 0; i < content.length; i += 1) {
    hash = Math.imul(hash ^ content.charCodeAt(i), 16777619)
  }
  return `${content.length}:${hash >>> 0}`
}

export function createCaptureGuard(options: {
  currentUrl: () => string
  conversationKey: (url: string) => string | null
}) {
  let observedUrl = options.currentUrl()
  let generation = 0

  function observeNavigation(): void {
    const url = options.currentUrl()
    if (url !== observedUrl) {
      observedUrl = url
      generation += 1
    }
  }

  return {
    observeNavigation,
    invalidate(): void {
      generation += 1
      observedUrl = options.currentUrl()
    },
    begin(url: string, previousContentFingerprint?: string): CaptureValidation {
      observeNavigation()
      const startedGeneration = generation
      const expectedKey = options.conversationKey(url)
      const validation: CaptureValidation = {
        previousContentFingerprint,
        isCurrent() {
          observeNavigation()
          if (generation !== startedGeneration) return false
          return expectedKey
            ? options.conversationKey(observedUrl) === expectedKey
            : observedUrl === url
        },
      }
      assertCurrentCapture(validation)
      return validation
    },
  }
}

export async function captureAndSave<T>(
  validation: CaptureValidation,
  read: () => string | null | Promise<string | null>,
  save: (content: string) => Promise<T>,
): Promise<T | null> {
  assertCurrentCapture(validation)
  const content = await read()
  // The provider can spend time expanding history or waiting for DOM updates.
  // Never combine that result with the original URL without rechecking ownership.
  assertCurrentCapture(validation)
  if (!content) return null
  if (validation.previousContentFingerprint === contentFingerprint(content)) return null
  return save(content)
}
