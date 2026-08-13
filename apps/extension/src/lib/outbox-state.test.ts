import { describe, expect, test } from 'bun:test'

import { findLeasedItem, type LeasedOutboxItem } from './outbox-state'

function item(overrides: Partial<LeasedOutboxItem> = {}): LeasedOutboxItem {
  return {
    id: 'item-1',
    idempotencyKey: 'key-1',
    status: 'syncing',
    syncLeaseId: 'lease-new',
    ...overrides,
  }
}

describe('findLeasedItem', () => {
  test('allows only the current upload lease to commit', () => {
    const persisted = item()
    expect(findLeasedItem([persisted], item())).toBe(persisted)
    expect(findLeasedItem([persisted], item({ syncLeaseId: 'lease-old' }))).toBeUndefined()
  })

  test('rejects replaced identities and terminal items', () => {
    const claim = item()
    expect(findLeasedItem([item({ idempotencyKey: 'key-replaced' })], claim)).toBeUndefined()
    expect(findLeasedItem([item({ status: 'sent' })], claim)).toBeUndefined()
  })
})
