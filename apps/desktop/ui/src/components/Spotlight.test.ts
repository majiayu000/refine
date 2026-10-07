import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import type { Item, SearchResult } from '../lib/api/types'

// Exercise the exact effect body without adding a DOM renderer dependency.
const source = readFileSync(new URL('./Spotlight.tsx', import.meta.url), 'utf8')
const match = source.match(/useEffect\(\(\) => \{([\s\S]*?)\n  \}, \[api, query, items, isOpen\]\)/)
if (!match) throw new Error('Spotlight search effect was not found')
const effect = new Function('api', 'query', 'items', 'isOpen', 'setResults', 'setIsSearching', 'setTimeout', 'clearTimeout', match[1])
const item = (id: string): Item => ({ id, item_type: 'knowledge', title: id, summary: '', content: '', tags: [], created_at: '2026-01-01T00:00:00Z' })
const A = item('a')
const B = item('b')
const RECENT = item('recent')
function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}
function harness() {
  let cleanup: (() => void) | undefined
  const state = { results: [] as Item[], searching: false }
  const searchItems = vi.fn()
  return {
    state, searchItems,
    render(query: string, isOpen = true) {
      cleanup?.()
      cleanup = effect({ searchItems }, query, [RECENT], isOpen,
        (value: Item[]) => { state.results = value },
        (value: boolean) => { state.searching = value }, setTimeout, clearTimeout)
    },
  }
}
const flush = async () => { await Promise.resolve(); await Promise.resolve() }
beforeEach(() => { vi.useFakeTimers() })
afterEach(() => { vi.useRealTimers() })

test('a superseded success cannot replace current results', async () => {
  const h = harness()
  const older = deferred<SearchResult>()
  const newer = deferred<SearchResult>()
  h.searchItems.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  h.render('alpha'); vi.advanceTimersByTime(140)
  h.render('beta'); vi.advanceTimersByTime(140)
  newer.resolve({ items: [B], total: 1 }); await flush()
  older.resolve({ items: [A], total: 1 }); await flush()
  expect(h.state.results).toEqual([B])
  expect(h.state.searching).toBe(false)
})

test('a superseded error cannot clear the active loading state', async () => {
  const h = harness()
  const older = deferred<SearchResult>()
  const newer = deferred<SearchResult>()
  h.searchItems.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  h.render('alpha'); vi.advanceTimersByTime(140)
  h.render('beta'); vi.advanceTimersByTime(140)
  older.reject(new Error('obsolete request')); await flush()
  expect(h.state.searching).toBe(true)
  newer.resolve({ items: [B], total: 1 }); await flush()
  expect(h.state.results).toEqual([B])
  expect(h.state.searching).toBe(false)
})

test('clearing or dismissing invalidates in-flight searches', async () => {
  for (const dismiss of [false, true]) {
    const h = harness()
    const older = deferred<SearchResult>()
    h.searchItems.mockReturnValueOnce(older.promise)
    h.render('alpha'); vi.advanceTimersByTime(140)
    h.render('', !dismiss)
    older.resolve({ items: [A], total: 1 }); await flush()
    expect(h.state.searching).toBe(false)
    expect(h.state.results).toEqual(dismiss ? [] : [RECENT])
    h.render('')
    expect(h.state.results).toEqual([RECENT])
  }
})
