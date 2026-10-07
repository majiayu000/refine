import { describe, expect, test } from 'bun:test'
import { captureAndSave, contentFingerprint, createCaptureGuard } from './capture-context'
import { waitForConversationExtraction } from './platform-adapter'

function fixture() {
  let url = 'https://example.test/chat/A'
  const guard = createCaptureGuard({
    currentUrl: () => url,
    conversationKey: (value) => new URL(value).pathname,
  })
  return { guard, navigate(next: string) { url = `https://example.test/chat/${next}` } }
}

describe('conversation-bound capture', () => {
  test('does not save B text with A metadata after an awaited provider read', async () => {
    const { guard, navigate } = fixture()
    let finish!: (content: string) => void
    const saved: string[] = []
    const read = new Promise<string>((resolve) => { finish = resolve })
    const capture = captureAndSave(guard.begin('https://example.test/chat/A'), () => read, async (content) => {
      saved.push(content)
    })
    navigate('B')
    finish('Human: conversation B')
    await expect(capture).rejects.toThrow('会话已切换')
    expect(saved).toEqual([])
  })

  test('returning to A does not revive a capture invalidated by navigation', async () => {
    const { guard, navigate } = fixture()
    const capture = guard.begin('https://example.test/chat/A')
    navigate('B')
    guard.invalidate()
    navigate('A')
    guard.invalidate()
    expect(capture.isCurrent()).toBe(false)
    expect(guard.begin('https://example.test/chat/A').isCurrent()).toBe(true)
  })

  test('cannot start a capture for a different current conversation', () => {
    const { guard } = fixture()
    expect(() => guard.begin('https://example.test/chat/B')).toThrow('会话已切换')
  })

  test('matching stable content is saved while a prior DOM fingerprint fails closed', async () => {
    const { guard } = fixture()
    const saved: string[] = []
    const save = async (content: string) => { saved.push(content); return 'queued' }
    await expect(captureAndSave(guard.begin('https://example.test/chat/A'), () => 'A text', save)).resolves.toBe('queued')
    await expect(captureAndSave(
      guard.begin('https://example.test/chat/A', contentFingerprint('old DOM')),
      () => 'old DOM', save,
    )).resolves.toBeNull()
    expect(saved).toEqual(['A text'])
  })
})

describe('DOM transition readiness', () => {
  // The polling helper only needs the browser timer, not a live page or account.
  const browserTimers = () => {
    Object.defineProperty(globalThis, 'window', { value: { setTimeout }, configurable: true, writable: true })
  }

  test('does not accept stable old DOM merely because the URL already changed', async () => {
    browserTimers()
    let polls = 0
    const result = await waitForConversationExtraction(() => {
      polls += 1
      return polls < 6 ? 'old A' : 'new B'
    }, {
      intervalMs: 1, stableForMs: 2, timeoutMs: 100,
      validation: { isCurrent: () => true, previousContentFingerprint: contentFingerprint('old A') },
    })
    expect(result).toBe('new B')
    expect(polls).toBeGreaterThanOrEqual(6)
  })

  test('does not return stale or unstable partial text when the deadline expires', async () => {
    browserTimers()
    let poll = 0
    await expect(waitForConversationExtraction(() => `streaming ${++poll}`, {
      intervalMs: 1, stableForMs: 50, timeoutMs: 8,
      validation: { isCurrent: () => true },
    })).resolves.toBeNull()
    await expect(waitForConversationExtraction(() => 'old A', {
      intervalMs: 1, stableForMs: 2, timeoutMs: 8,
      validation: { isCurrent: () => true, previousContentFingerprint: contentFingerprint('old A') },
    })).resolves.toBeNull()
  })

  test('stops polling when the bound navigation changes', async () => {
    browserTimers()
    let polls = 0
    await expect(waitForConversationExtraction(() => { polls += 1; return 'A' }, {
      intervalMs: 1, stableForMs: 20, timeoutMs: 100,
      validation: { isCurrent: () => polls < 2 },
    })).rejects.toThrow('会话已切换')
    expect(polls).toBe(2)
  })
})
