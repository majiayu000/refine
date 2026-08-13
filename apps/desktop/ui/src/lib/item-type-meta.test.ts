import { describe, expect, it } from 'vitest'
import type { Item } from './api/types'
import { getItemTypeMeta } from './item-type-meta'

describe('getItemTypeMeta', () => {
  it('renders a production observation item', () => {
    const observation: Item = {
      id: 'obs-1',
      item_type: 'observation',
      title: 'Decision recorded during a session',
      summary: 'The team chose the atomic migration path.',
      content: 'Use one transaction for the full import.',
      tags: ['project:refine'],
      created_at: '2026-08-13T00:00:00Z',
    }

    const meta = getItemTypeMeta(observation.item_type)
    expect(meta.label).toBe('观察')
    expect(meta.icon).toBeDefined()
  })

  it('returns a safe fallback for a future runtime item type', () => {
    for (const itemType of [
      'future-type-from-newer-server',
      '__proto__',
      'constructor',
      'toString',
    ]) {
      const meta = getItemTypeMeta(itemType)
      expect(meta.label).toBe('未知类型')
      expect(meta.icon).toBeDefined()
      expect(meta.chipClass).toContain('slate')
    }
  })
})
