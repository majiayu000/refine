import { useEffect, useRef, useState } from 'react'
import type { RecommendationItem, RecommendationResponse } from './api'
import { trackContentEvent } from './content-events'
import { markOnboardingTask } from './onboarding'
import { SITE_SETTINGS_STORAGE_KEY, type RecommendationContext } from './content/recommendation-engine'

export function Recommendations() {
  const [context, setContext] = useState<(RecommendationContext & { tabId: number }) | null>(null)
  const [items, setItems] = useState<RecommendationItem[]>([])
  const [message, setMessage] = useState('正在读取当前输入…')
  const generation = useRef(0)

  async function refresh() {
    const request = ++generation.current
    setItems([])
    setContext(null)
    setMessage('正在读取当前输入…')
    try {
      const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
      if (tab?.id === undefined) throw new Error('未找到活动标签页')
      const next: RecommendationContext | null = await chrome.tabs.sendMessage(tab.id, { action: 'getRecommendationContext' })
      if (request !== generation.current) return
      if (!next) throw new Error('无法读取推荐设置')
      const current = { ...next, tabId: tab.id }
      setContext(current)
      if (!next.enabled) { setMessage('此网站推荐已关闭'); return }
      if (next.query.length < 10) { setMessage('在对话输入框输入至少 10 个字符，再刷新推荐'); return }
      setMessage('正在查找推荐…')
      const response: RecommendationResponse | null = await chrome.runtime.sendMessage({
        action: 'fetchRecommendations', query: next.query, options: { limit: 4, timeoutMs: 1500 },
      })
      if (request !== generation.current) return
      if (!response) { setMessage('推荐服务暂不可达，请重试'); return }
      const recommendations = response.triggered ? response.items.slice(0, 4) : []
      setItems(recommendations)
      setMessage(recommendations.length ? '' : '暂无相关推荐')
      if (recommendations.length) {
        void trackContentEvent({ event_name: 'recommendation_exposed', source: next.source,
          properties: { query_length: next.query.length, item_count: recommendations.length } }, tab.id)
        void markOnboardingTask('searched')
      }
    } catch {
      if (request === generation.current) setMessage('当前页面无法读取推荐，请在支持的对话页面使用')
    }
  }

  useEffect(() => {
    void refresh()
    return () => { generation.current += 1 }
  }, [])

  async function toggle() {
    if (!context) return
    generation.current += 1
    setItems([])
    try {
      const stored = await chrome.storage.local.get([SITE_SETTINGS_STORAGE_KEY])
      const key = `${context.source}:${new URL(context.url).hostname}`
      await chrome.storage.local.set({ [SITE_SETTINGS_STORAGE_KEY]: {
        ...stored[SITE_SETTINGS_STORAGE_KEY], [key]: !context.enabled,
      } })
      await refresh()
    } catch { setMessage('无法保存网站推荐设置，请重试') }
  }

  async function reuse(item: RecommendationItem, action: 'copy' | 'insert') {
    if (!context) return
    const text = item.content || item.summary || item.title
    try {
      if (action === 'copy') await navigator.clipboard.writeText(text)
      else {
        const result = await chrome.tabs.sendMessage(context.tabId, {
          action: 'insertRecommendation', text, url: context.url, query: context.query,
        })
        if (!result?.success) { setMessage(result?.message || '插入失败，请重试'); return }
        setItems([])
      }
      setMessage(action === 'copy' ? '已复制选中片段' : '已插入选中片段')
      for (const event_name of ['recommendation_clicked', 'knowledge_reused']) {
        void trackContentEvent({ event_name, source: context.source,
          properties: { action, item_id: item.id, item_type: item.item_type } }, context.tabId)
      }
      void markOnboardingTask('reused')
    } catch { setMessage(action === 'copy' ? '复制失败，请重试' : '插入失败，请重试') }
  }

  return <section className="recommendations-card" aria-label="私有知识推荐">
    <div className="recommendations-head">
      <h3>私有知识推荐</h3>
      <button type="button" onClick={() => void refresh()}>刷新</button>
      {context && <button type="button" onClick={() => void toggle()} aria-pressed={context.enabled}>
        {context.enabled ? '此网站：开' : '此网站：关'}
      </button>}
    </div>
    <p className="recommendations-note">推荐仅在扩展中预览；点击插入后，选中片段会交给当前网站。</p>
    {message && <p role="status">{message}</p>}
    {items.map((item) => <article className="recommendations-item" key={item.id}>
      <h4>{item.title}</h4>
      <p>{item.summary || '暂无摘要'}</p>
      <div className="recommendations-meta">
        <span>{item.item_type}</span><span>{item.match_strategy}</span>
        {item.tags.slice(0, 3).map((tag) => <span key={tag}>#{tag}</span>)}
      </div>
      <div className="recommendations-actions">
        <button type="button" onClick={() => void reuse(item, 'copy')}>复制</button>
        <button type="button" onClick={() => void reuse(item, 'insert')}>插入</button>
      </div>
    </article>)}
  </section>
}
