import { describe, expect, test } from 'bun:test'

import { createInvalidationGuard } from './invalidation-guard'

describe('invalidation guard', () => {
  test('rejects an in-flight snapshot after credentials change', () => {
    const guard = createInvalidationGuard()
    const inFlight = guard.snapshot()
    expect(guard.isCurrent(inFlight)).toBe(true)

    guard.invalidate()
    expect(guard.isCurrent(inFlight)).toBe(false)
    expect(guard.isCurrent(guard.snapshot())).toBe(true)
  })
})
