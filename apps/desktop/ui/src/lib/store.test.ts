import { beforeEach, expect, test, vi } from 'vitest'
import type { Item, ItemListResult, SearchResult } from './api/types'

const api = vi.hoisted(() => ({
  getCapabilities: vi.fn(() => ({})),
  getItems: vi.fn(),
  searchItems: vi.fn(),
  deleteItem: vi.fn(),
}))
vi.mock('./api/client', () => ({ getApiClient: () => api }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}
const item = (id: string): Item => ({ id, item_type: 'knowledge', title: id, summary: '', content: '', tags: [], created_at: '2026-01-01T00:00:00Z' })
const A = item('a')
const B = item('b')
const page = (items: Item[], nextCursor: number | null = null): ItemListResult => ({ items, total: items.length, nextCursor })

beforeEach(() => {
  vi.resetAllMocks()
  vi.resetModules()
})

test('a superseded list response cannot replace a newer snapshot', async () => {
  const older = deferred<ItemListResult>()
  const newer = deferred<ItemListResult>()
  api.getItems.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  const { useStore } = await import('./store')
  const first = useStore.getState().loadItems()
  const second = useStore.getState().loadItems()
  newer.resolve(page([B]))
  expect(await second).toBe(true)
  older.resolve(page([A]))
  expect(await first).toBe(false)
  expect(useStore.getState().items).toEqual([B])
  expect(useStore.getState().isLoading).toBe(false)
})

test('an old pagination response cannot append after a list refresh', async () => {
  const older = deferred<ItemListResult>()
  const newer = deferred<ItemListResult>()
  api.getItems.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  const { useStore } = await import('./store')
  useStore.setState({ items: [A], nextCursor: 1 })
  const more = useStore.getState().loadMoreItems()
  const refresh = useStore.getState().loadItems()
  newer.resolve(page([B]))
  await refresh
  older.resolve(page([item('obsolete')], 2))
  await more
  expect(useStore.getState().items).toEqual([B])
  expect(useStore.getState().nextCursor).toBeNull()
  expect(useStore.getState().isLoadingMore).toBe(false)
})

test('search commits only the current query, including clearing the query', async () => {
  const older = deferred<SearchResult>()
  const newer = deferred<SearchResult>()
  api.searchItems.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  const { useStore } = await import('./store')
  const first = useStore.getState().search('alpha')
  const second = useStore.getState().search('beta')
  newer.resolve({ items: [B], total: 1 })
  await second
  older.resolve({ items: [A], total: 1 })
  await first
  expect(useStore.getState().searchResults).toEqual([B])
  const late = deferred<SearchResult>()
  api.searchItems.mockReturnValueOnce(late.promise)
  const pending = useStore.getState().search('alpha')
  await useStore.getState().search('')
  late.resolve({ items: [A], total: 1 })
  await pending
  expect(useStore.getState().searchResults).toEqual([])
})

test('deleting A preserves a newer selection B but clears A when still selected', async () => {
  const deletion = deferred<boolean>()
  api.deleteItem.mockReturnValueOnce(deletion.promise).mockResolvedValue(true)
  api.getItems.mockResolvedValue(page([B]))
  const { useStore } = await import('./store')
  useStore.getState().selectItem(A)
  const pending = useStore.getState().deleteItem(A.id)
  useStore.getState().selectItem(B)
  deletion.resolve(true)
  await pending
  expect(useStore.getState().selectedItem).toEqual(B)
  useStore.getState().selectItem(A)
  await useStore.getState().deleteItem(A.id)
  expect(useStore.getState().selectedItem).toBeNull()
})

test('confirmed deletion stays removed when the follow-up refresh fails', async () => {
  api.deleteItem.mockResolvedValue(true)
  api.getItems.mockRejectedValue(new Error('offline'))
  const { useStore } = await import('./store')
  useStore.setState({ items: [A, B], searchResults: [A, B], selectedItem: A, totalItems: 5, nextCursor: 2 })
  const outcome = await useStore.getState().deleteItem(A.id)
  expect(outcome).toEqual({ status: 'deleted', refreshed: false })
  expect(useStore.getState().items).toEqual([B])
  expect(useStore.getState().searchResults).toEqual([B])
  expect(useStore.getState().selectedItem).toBeNull()
  expect(useStore.getState().totalItems).toBe(4)
  expect(useStore.getState().nextCursor).toBe(1)
  expect(useStore.getState().deleteFeedback?.kind).toBe('warning')
  expect(useStore.getState().deleteFeedback?.message).toContain('已删除')
  expect(useStore.getState().deletingItemId).toBeNull()
})

test('a false deletion receipt is explicit not-found and reconciles stale rows', async () => {
  api.deleteItem.mockResolvedValue(false)
  api.getItems.mockResolvedValue(page([B]))
  const { useStore } = await import('./store')
  useStore.setState({ items: [A, B], selectedItem: A })
  await expect(useStore.getState().deleteItem(A.id)).resolves.toEqual({ status: 'not_found', refreshed: true })
  expect(useStore.getState().items).toEqual([B])
  expect(useStore.getState().deleteFeedback?.message).toContain('已不存在')
})

test('failed deletion remains visible and preserves data and selection', async () => {
  api.deleteItem.mockRejectedValue(new Error('database unavailable'))
  const { useStore } = await import('./store')
  useStore.setState({ items: [A, B], selectedItem: A })
  await expect(useStore.getState().deleteItem(A.id)).resolves.toEqual({ status: 'failed', message: 'database unavailable' })
  expect(useStore.getState().items).toEqual([A, B])
  expect(useStore.getState().selectedItem).toEqual(A)
  expect(useStore.getState().deleteFeedback?.kind).toBe('error')
  expect(useStore.getState().deletingItemId).toBeNull()
  expect(api.getItems).not.toHaveBeenCalled()
})

test('pending deletion is exposed and duplicate submissions do not issue another mutation', async () => {
  const deletion = deferred<boolean>()
  api.deleteItem.mockReturnValue(deletion.promise)
  api.getItems.mockResolvedValue(page([]))
  const { useStore } = await import('./store')
  const pending = useStore.getState().deleteItem(A.id)
  expect(useStore.getState().deletingItemId).toBe(A.id)
  await expect(useStore.getState().deleteItem(A.id)).resolves.toEqual({ status: 'pending' })
  expect(api.deleteItem).toHaveBeenCalledTimes(1)
  deletion.resolve(true)
  await pending
  expect(useStore.getState().deletingItemId).toBeNull()
})

test('a pre-deletion search cannot restore a removed result', async () => {
  const search = deferred<SearchResult>()
  api.searchItems.mockReturnValueOnce(search.promise)
  api.deleteItem.mockResolvedValue(true)
  api.getItems.mockResolvedValue(page([B]))
  const { useStore } = await import('./store')
  useStore.setState({ items: [A, B], searchResults: [A, B] })
  const pending = useStore.getState().search('query')
  await useStore.getState().deleteItem(A.id)
  search.resolve({ items: [A, B], total: 2 })
  await pending
  expect(useStore.getState().searchResults).toEqual([B])
})
