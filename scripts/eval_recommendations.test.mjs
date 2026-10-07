import test from 'node:test'
import assert from 'node:assert/strict'
import { evaluateHits } from './eval_recommendations.mjs'

const positive = { kind: 'gold', relevantIds: ['postgres-isolation', 'postgres-locks'] }
test('same tag and type on an unrelated document is not a relevance hit', () => {
  const verdict = evaluateHits(positive, [{ id: 'sqlite-backup', item_type: 'knowledge', tags: ['database'] }], 5)
  assert.equal(verdict.top1, false)
  assert.equal(verdict.recall, 0)
  assert.equal(verdict.reciprocalRank, 0)
})
test('recall uses unique gold IDs and MRR uses the first relevant rank', () => {
  const verdict = evaluateHits(positive, [{ id: 'wrong' }, { id: 'postgres-locks' }, { id: 'postgres-locks' }], 5)
  assert.equal(verdict.recall, 0.5)
  assert.equal(verdict.reciprocalRank, 0.5)
  assert.equal(verdict.top3, true)
})
test('negative queries require empty results', () => {
  const negative = { kind: 'gold', relevantIds: [] }
  assert.equal(evaluateHits(negative, [], 5).correct, true)
  assert.equal(evaluateHits(negative, [{ id: 'wrong' }], 5).falsePositive, true)
})
test('results beyond requested k cannot count toward recall or rank', () => {
  const verdict = evaluateHits(positive, [{ id: 'wrong' }, { id: 'postgres-locks' }], 1)
  assert.equal(verdict.recall, 0)
  assert.equal(verdict.reciprocalRank, 0)
})
test('legacy type/tag expectations are explicitly metadata proxy only', () => {
  const verdict = evaluateHits({ kind: 'metadata_proxy', expectedType: 'knowledge', expectedTags: ['database'] }, [{ id: 'wrong', item_type: 'knowledge', tags: ['database'] }], 5)
  assert.equal(verdict.kind, 'metadata_proxy')
  assert.equal(verdict.top1, true)
  assert.equal(verdict.recall, undefined)
})
