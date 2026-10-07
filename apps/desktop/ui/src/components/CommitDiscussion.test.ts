import { describe, expect, test } from 'vitest'
import { decisionQuotes } from './CommitDiscussion'
import type { CommitDiscussion, CommitProjection, Item } from '../lib/api/types'

const session: CommitDiscussion = { session_id: 's1', source: 'capture_git_evidence', status: 'verified', session_ref: 'ref', content_hash: 'sha256:a', messages: [{ id: 42, role: 'user', content: '保留本地缓存', created_at_epoch: 100 }] }
const item = { id: 'obs-1', tags: ['decision'], title: '保留缓存' } as Item
const projection: CommitProjection = { session_ref: 'ref', document_id: 'd1', source_version: 'sha256:a:interactive', items: [item], evidence: { observations: [{ item_id: 'obs-1', field: 'decisions', index: 0 }], facets: [{ field: 'decisions', index: 0, status: 'references_validated', message_ids: [42] }] }, history: [] }

describe('decision source quotations', () => {
  test('returns exact original text and sender only for referenced current snapshot', () => {
    expect(decisionQuotes(session, projection, 'obs-1')).toEqual(session.messages)
  })
  test('does not attribute edited or pending claims to original messages', () => {
    for (const tag of ['curated', 'curation_needs_review']) {
      expect(decisionQuotes(session, { ...projection, items: [{ ...item, tags: ['decision', tag] }] }, 'obs-1')).toEqual([])
    }
  })
  test('fails closed for snapshot drift, unknown evidence, and unavailable IDs', () => {
    expect(decisionQuotes(session, { ...projection, source_version: 'sha256:b' }, 'obs-1')).toEqual([])
    expect(decisionQuotes(session, { ...projection, evidence: null }, 'obs-1')).toEqual([])
    // The API removes authoritative overrides even when an edited item has no hint tag.
    expect(decisionQuotes(session, { ...projection, items: [{ ...item, title: '人工选择另一方案' }], evidence: { ...projection.evidence, observations: [] } }, 'obs-1')).toEqual([])
    expect(decisionQuotes({ ...session, messages: [] }, projection, 'obs-1')).toEqual([])
    expect(decisionQuotes(session, { ...projection, evidence: { ...projection.evidence, facets: [{ field: 'decisions', index: 0, status: 'unknown', message_ids: [42] }] } }, 'obs-1')).toEqual([])
  })
})
