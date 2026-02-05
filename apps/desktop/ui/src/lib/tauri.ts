import { invoke } from '@tauri-apps/api/core'

export interface Item {
  id: string
  item_type: 'knowledge' | 'skill' | 'snippet'
  title: string
  summary: string
  content: string
  tags: string[]
  created_at: string
}

export interface SearchResult {
  items: Item[]
  total: number
}

// 获取所有知识
export async function getItems(params?: {
  item_type?: string
  limit?: number
}): Promise<Item[]> {
  return invoke('get_items', params || {})
}

// 获取单个知识
export async function getItem(id: string): Promise<Item | null> {
  return invoke('get_item', { id })
}

// 搜索知识
export async function searchItems(
  query: string,
  limit?: number
): Promise<SearchResult> {
  return invoke('search_items', { query, limit })
}

// 创建知识
export async function createItem(params: {
  title: string
  summary: string
  content: string
  item_type?: string
  tags?: string[]
}): Promise<Item> {
  return invoke('create_item', params)
}

// 更新知识
export async function updateItem(params: {
  id: string
  title?: string
  summary?: string
  content?: string
}): Promise<Item> {
  return invoke('update_item', params)
}

// 删除知识
export async function deleteItem(id: string): Promise<boolean> {
  return invoke('delete_item', { id })
}
