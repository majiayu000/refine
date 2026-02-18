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

const DEFAULT_API_BASE = 'http://127.0.0.1:8787'

function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && typeof (window as any).__TAURI_INTERNALS__ !== 'undefined'
}

function getApiBase(): string {
  const envBase = (import.meta as any)?.env?.VITE_REFINE_API_BASE
  if (typeof envBase === 'string' && envBase.trim()) {
    return envBase
  }
  return DEFAULT_API_BASE
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${getApiBase()}${path}`, {
    ...init,
    headers: {
      ...(init?.headers || {}),
    },
  })

  const data = await res.json()
  if (!res.ok || data?.success === false) {
    throw new Error(data?.message || `HTTP ${res.status}`)
  }
  return data as T
}

// 获取所有知识
export async function getItems(params?: {
  item_type?: string
  limit?: number
}): Promise<Item[]> {
  if (!isTauriRuntime()) {
    const limit = params?.limit ?? 50
    const query = new URLSearchParams({
      cursor: '0',
      limit: String(limit),
    })
    if (params?.item_type) {
      query.set('item_type', params.item_type)
    }

    const data = await requestJson<{ items: Item[] }>(`/v1/items?${query.toString()}`)
    return data.items || []
  }
  return invoke('get_items', params || {})
}

// 获取单个知识
export async function getItem(id: string): Promise<Item | null> {
  if (!isTauriRuntime()) {
    const data = await requestJson<{ items: Item[] }>(`/v1/items?cursor=0&limit=200`)
    return (data.items || []).find((item) => item.id === id) ?? null
  }
  return invoke('get_item', { id })
}

// 搜索知识
export async function searchItems(
  query: string,
  limit?: number
): Promise<SearchResult> {
  if (!isTauriRuntime()) {
    const qs = new URLSearchParams({ q: query })
    if (typeof limit === 'number') {
      qs.set('limit', String(limit))
    }
    const data = await requestJson<{ items: Item[]; total: number }>(`/v1/search?${qs.toString()}`)
    return {
      items: data.items || [],
      total: data.total || 0,
    }
  }
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
  if (!isTauriRuntime()) {
    throw new Error('HTTP fallback 模式暂不支持 create_item')
  }
  return invoke('create_item', params)
}

// 更新知识
export async function updateItem(params: {
  id: string
  title?: string
  summary?: string
  content?: string
}): Promise<Item> {
  if (!isTauriRuntime()) {
    throw new Error('HTTP fallback 模式暂不支持 update_item')
  }
  return invoke('update_item', params)
}

// 删除知识
export async function deleteItem(id: string): Promise<boolean> {
  if (!isTauriRuntime()) {
    const data = await requestJson<{ deleted?: boolean }>(`/v1/items/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    })
    return Boolean(data.deleted)
  }
  return invoke('delete_item', { id })
}
