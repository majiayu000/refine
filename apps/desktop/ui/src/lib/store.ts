import { create } from 'zustand'
import type { Item } from './tauri'
import * as api from './tauri'

interface AppState {
  // 状态
  items: Item[]
  selectedItem: Item | null
  searchQuery: string
  searchResults: Item[]
  isLoading: boolean
  isSpotlightOpen: boolean

  // 操作
  loadItems: () => Promise<void>
  selectItem: (item: Item | null) => void
  search: (query: string) => Promise<void>
  createItem: (params: { title: string; summary: string; content: string }) => Promise<void>
  deleteItem: (id: string) => Promise<void>
  setSpotlightOpen: (open: boolean) => void
}

export const useStore = create<AppState>((set, get) => ({
  items: [],
  selectedItem: null,
  searchQuery: '',
  searchResults: [],
  isLoading: false,
  isSpotlightOpen: false,

  loadItems: async () => {
    set({ isLoading: true })
    try {
      const items = await api.getItems()
      set({ items, isLoading: false })
    } catch (error) {
      console.error('加载失败:', error)
      set({ isLoading: false })
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
