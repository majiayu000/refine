export interface LeasedOutboxItem {
  id: string
  idempotencyKey: string
  status: 'pending' | 'syncing' | 'failed' | 'sent'
  syncLeaseId?: string
}

export function findLeasedItem<T extends LeasedOutboxItem>(
  outbox: T[],
  claimedItem: LeasedOutboxItem,
): T | undefined {
  return outbox.find(
    (candidate) =>
      candidate.id === claimedItem.id &&
      candidate.idempotencyKey === claimedItem.idempotencyKey &&
      candidate.status === 'syncing' &&
      candidate.syncLeaseId === claimedItem.syncLeaseId,
  )
}
