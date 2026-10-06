import { create } from 'zustand'
import { getApiClient } from './api/client'
import type { ApiCapabilities, Item } from './api/types'

const PAGE_SIZE = 50
const api = getApiClient()
let loadItemsRequest = 0
let searchRequest = 0

export type DeleteItemOutcome =
  | { status: 'deleted' | 'not_found'; refreshed: boolean }
  | { status: 'failed'; message: string }
  | { status: 'pending' }

interface DeleteFeedback {
  kind: 'success' | 'warning' | 'error'
  message: string
}

interface AppState {
  // 状态
  apiCapabilities: ApiCapabilities
  items: Item[]
  totalItems: number
  nextCursor: number | null
  selectedItem: Item | null
  searchQuery: string
  searchResults: Item[]
  isLoading: boolean
  isLoadingMore: boolean
  isSpotlightOpen: boolean
  deletingItemId: string | null
  deleteFeedback: DeleteFeedback | null

  // 操作
  loadItems: () => Promise<boolean>
  loadMoreItems: () => Promise<void>
  selectItem: (item: Item | null) => void
  search: (query: string) => Promise<void>
  createItem: (params: { title: string; summary: string; content: string }) => Promise<void>
  deleteItem: (id: string) => Promise<DeleteItemOutcome>
  clearDeleteFeedback: () => void
  setSpotlightOpen: (open: boolean) => void
}

export const useStore = create<AppState>((set, get) => ({
  apiCapabilities: api.getCapabilities(),
  items: [],
  totalItems: 0,
  nextCursor: null,
  selectedItem: null,
  searchQuery: '',
  searchResults: [],
  isLoading: false,
  isLoadingMore: false,
  isSpotlightOpen: false,
  deletingItemId: null,
  deleteFeedback: null,

  loadItems: async () => {
    const request = ++loadItemsRequest
    set({ isLoading: true, isLoadingMore: false })
    try {
      const result = await api.getItems({ cursor: 0, limit: PAGE_SIZE })
      if (request !== loadItemsRequest) return false
      set({
        items: result.items,
        totalItems: result.total,
        nextCursor: result.nextCursor,
        isLoading: false,
      })
      return true
    } catch (error) {
      if (request !== loadItemsRequest) return false
      console.error('加载失败:', error)
      set({ isLoading: false, isLoadingMore: false })
      return false
    }
  },

  loadMoreItems: async () => {
    const { isLoading, isLoadingMore, nextCursor } = get()
    if (isLoading || isLoadingMore || nextCursor == null) {
      return
    }

    const request = loadItemsRequest
    set({ isLoadingMore: true })
    try {
      const result = await api.getItems({ cursor: nextCursor, limit: PAGE_SIZE })
      if (request !== loadItemsRequest) return
      set((state) => {
        const existingIds = new Set(state.items.map((item) => item.id))
        const appended = result.items.filter((item) => !existingIds.has(item.id))
        return {
          items: state.items.concat(appended),
          totalItems: result.total,
          nextCursor: result.nextCursor,
          isLoadingMore: false,
        }
      })
    } catch (error) {
      if (request !== loadItemsRequest) return
      console.error('加载更多失败:', error)
      set({ isLoadingMore: false })
    }
  },

  selectItem: (item) => {
    set({ selectedItem: item })
  },

  search: async (query) => {
    const request = ++searchRequest
    set({ searchQuery: query })
    if (!query.trim()) {
      set({ searchResults: [] })
      return
    }
    try {
      const result = await api.searchItems(query)
      if (request !== searchRequest) return
      set({ searchResults: result.items })
    } catch (error) {
      if (request !== searchRequest) return
      console.error('搜索失败:', error)
    }
  },

  createItem: async (params) => {
    try {
      await api.createItem(params)
      await get().loadItems()
    } catch (error) {
      console.error('创建失败:', error)
    }
  },

  deleteItem: async (id) => {
    if (get().deletingItemId !== null) return { status: 'pending' }
    set({ deletingItemId: id, deleteFeedback: null })
    try {
      const deleted = await api.deleteItem(id)
      if (typeof deleted !== 'boolean') throw new Error('服务未返回有效的删除结果，请刷新后确认。')
      const status = deleted ? 'deleted' : 'not_found'
      const message = deleted ? '知识已删除。' : '该知识已不存在，已移除过期显示。'
      // No earlier page/search snapshot may resurrect a confirmed deletion.
      loadItemsRequest += 1
      searchRequest += 1
      set((state) => {
        const wasLoaded = state.items.some((item) => item.id === id)
        const wasKnown = wasLoaded || state.searchResults.some((item) => item.id === id) || state.selectedItem?.id === id
        return {
          items: state.items.filter((item) => item.id !== id),
          searchResults: state.searchResults.filter((item) => item.id !== id),
          selectedItem: state.selectedItem?.id === id ? null : state.selectedItem,
          totalItems: Math.max(0, state.totalItems - (wasKnown ? 1 : 0)),
          nextCursor: state.nextCursor === null ? null : Math.max(0, state.nextCursor - (wasLoaded ? 1 : 0)),
          isLoading: false,
          isLoadingMore: false,
          deleteFeedback: { kind: 'success' as const, message },
        }
      })
      const refreshed = await get().loadItems()
      if (!refreshed) {
        set({ deleteFeedback: { kind: 'warning', message: `${message}列表刷新未完成，可稍后刷新。` } })
      }
      return { status, refreshed }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      set({ deleteFeedback: { kind: 'error', message: `删除未确认：${message}` } })
      return { status: 'failed', message }
    } finally {
      set({ deletingItemId: null })
    }
  },

  clearDeleteFeedback: () => set({ deleteFeedback: null }),

  setSpotlightOpen: (open) => {
    set({ isSpotlightOpen: open })
  },
}))
