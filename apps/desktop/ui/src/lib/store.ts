import { create } from 'zustand'
import type { Item } from './tauri'
import * as api from './tauri'

const PAGE_SIZE = 50

interface AppState {
  // 状态
  items: Item[]
  totalItems: number
  nextCursor: number | null
  selectedItem: Item | null
  searchQuery: string
  searchResults: Item[]
  isLoading: boolean
  isLoadingMore: boolean
  isSpotlightOpen: boolean

  // 操作
  loadItems: () => Promise<void>
  loadMoreItems: () => Promise<void>
  selectItem: (item: Item | null) => void
  search: (query: string) => Promise<void>
  createItem: (params: { title: string; summary: string; content: string }) => Promise<void>
  deleteItem: (id: string) => Promise<void>
  setSpotlightOpen: (open: boolean) => void
}

export const useStore = create<AppState>((set, get) => ({
  items: [],
  totalItems: 0,
  nextCursor: null,
  selectedItem: null,
  searchQuery: '',
  searchResults: [],
  isLoading: false,
  isLoadingMore: false,
  isSpotlightOpen: false,

  loadItems: async () => {
    set({ isLoading: true, isLoadingMore: false })
    try {
      const result = await api.getItems({ cursor: 0, limit: PAGE_SIZE })
      set({
        items: result.items,
        totalItems: result.total,
        nextCursor: result.nextCursor,
        isLoading: false,
      })
    } catch (error) {
      console.error('加载失败:', error)
      set({ isLoading: false, isLoadingMore: false })
    }
  },

  loadMoreItems: async () => {
    const { isLoading, isLoadingMore, nextCursor } = get()
    if (isLoading || isLoadingMore || nextCursor == null) {
      return
    }

    set({ isLoadingMore: true })
    try {
      const result = await api.getItems({ cursor: nextCursor, limit: PAGE_SIZE })
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
      console.error('加载更多失败:', error)
      set({ isLoadingMore: false })
    }
  },

  selectItem: (item) => {
    set({ selectedItem: item })
  },

  search: async (query) => {
    set({ searchQuery: query })
    if (!query.trim()) {
      set({ searchResults: [] })
      return
    }
    try {
      const result = await api.searchItems(query)
      set({ searchResults: result.items })
    } catch (error) {
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
    try {
      await api.deleteItem(id)
      set({ selectedItem: null })
      await get().loadItems()
    } catch (error) {
      console.error('删除失败:', error)
    }
  },

  setSpotlightOpen: (open) => {
    set({ isSpotlightOpen: open })
  },
}))
