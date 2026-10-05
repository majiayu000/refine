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
  const deletion = deferred<void>()
  api.deleteItem.mockReturnValueOnce(deletion.promise)
  api.getItems.mockResolvedValue(page([B]))
  const { useStore } = await import('./store')
  useStore.getState().selectItem(A)
  const pending = useStore.getState().deleteItem(A.id)
  useStore.getState().selectItem(B)
  deletion.resolve(undefined)
  await pending
  expect(useStore.getState().selectedItem).toEqual(B)
  useStore.getState().selectItem(A)
  await useStore.getState().deleteItem(A.id)
  expect(useStore.getState().selectedItem).toBeNull()
})
