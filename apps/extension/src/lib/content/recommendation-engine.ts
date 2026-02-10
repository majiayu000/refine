import { fetchRecommendations, trackEvent, type RecommendationItem } from '../api'
import { markOnboardingTask } from '../onboarding'
import type { ConversationSource } from '../types'
import { injectStyleOnce } from './runtime'

interface RecommendationEngineOptions {
  providerId: string
  source: ConversationSource
  inputSelectors: string[]
  minChars?: number
  debounceMs?: number
  maxItems?: number
}

const DEFAULT_MIN_CHARS = 10
const DEFAULT_DEBOUNCE_MS = 300
const DEFAULT_MAX_ITEMS = 4
const DEFAULT_REQUEST_TIMEOUT_MS = 1_500
const SITE_SETTINGS_STORAGE_KEY = '__refine_recommendation_enabled_by_site'

export function initRecommendationEngine(options: RecommendationEngineOptions): void {
  if (!document.body) return

  const minChars = options.minChars ?? DEFAULT_MIN_CHARS
  const debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS
  const maxItems = options.maxItems ?? DEFAULT_MAX_ITEMS
  const styleId = `__refine_recommendation_style_${options.providerId}`

  injectStyleOnce(
    styleId,
    `
      .refine-reco-panel {
        position: fixed;
        right: 16px;
        bottom: 84px;
        width: min(380px, calc(100vw - 24px));
        border: 1px solid rgba(148, 163, 184, 0.34);
        border-radius: 14px;
        background: rgba(15, 23, 42, 0.94);
        color: #e2e8f0;
        box-shadow: 0 18px 35px rgba(2, 6, 23, 0.4);
        backdrop-filter: blur(6px);
        z-index: 999999;
        display: none;
      }

      .refine-reco-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 10px 12px;
        border-bottom: 1px solid rgba(148, 163, 184, 0.24);
        font-size: 12px;
      }

      .refine-reco-title {
        font-weight: 700;
        letter-spacing: 0.02em;
      }

      .refine-reco-close {
        border: 0;
        background: transparent;
        color: #94a3b8;
        cursor: pointer;
        font-size: 14px;
      }

      .refine-reco-close:hover {
        color: #e2e8f0;
      }

      .refine-reco-list {
        max-height: 260px;
        overflow: auto;
        padding: 8px;
        display: grid;
        gap: 8px;
      }

      .refine-reco-item {
        border: 1px solid rgba(100, 116, 139, 0.4);
        border-radius: 10px;
        background: rgba(30, 41, 59, 0.75);
        padding: 8px;
      }

      .refine-reco-item-title {
        font-size: 13px;
        font-weight: 700;
        color: #f8fafc;
        line-height: 1.3;
        margin-bottom: 5px;
      }

      .refine-reco-item-summary {
        font-size: 12px;
        line-height: 1.45;
        color: #cbd5e1;
        margin-bottom: 7px;
      }

      .refine-reco-item-meta {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-bottom: 7px;
      }

      .refine-reco-chip {
        border: 1px solid rgba(148, 163, 184, 0.45);
        border-radius: 999px;
        padding: 2px 8px;
        font-size: 10px;
        line-height: 1.4;
        color: #cbd5e1;
        background: rgba(15, 23, 42, 0.65);
      }

      .refine-reco-chip[data-kind="type"] {
        color: #bfdbfe;
        border-color: rgba(96, 165, 250, 0.48);
      }

      .refine-reco-chip[data-kind="reason"] {
        color: #fde68a;
        border-color: rgba(245, 158, 11, 0.5);
      }

      .refine-reco-actions {
        display: flex;
        gap: 6px;
        justify-content: flex-end;
      }

      .refine-reco-action {
        border: 1px solid rgba(148, 163, 184, 0.38);
        border-radius: 8px;
        padding: 4px 8px;
        background: rgba(30, 41, 59, 0.95);
        color: #e2e8f0;
        font-size: 11px;
        cursor: pointer;
      }

      .refine-reco-toggle {
        position: fixed;
        right: 16px;
        bottom: 20px;
        border: 1px solid rgba(148, 163, 184, 0.48);
        border-radius: 999px;
        padding: 6px 12px;
        background: rgba(15, 23, 42, 0.9);
        color: #e2e8f0;
        font-size: 11px;
        letter-spacing: 0.01em;
        cursor: pointer;
        z-index: 999999;
      }

      .refine-reco-toggle[data-state="off"] {
        color: #fda4af;
        border-color: rgba(244, 63, 94, 0.55);
        background: rgba(30, 41, 59, 0.88);
      }
    `
  )

  let panelEl: HTMLDivElement | null = null
  let listEl: HTMLDivElement | null = null
  let toggleEl: HTMLButtonElement | null = null
  let activeInputEl: HTMLElement | null = null
  let debounceTimer: number | undefined
  let requestId = 0
  let exposedKey = ''
  let enabled = true
  const siteSettingKey = `${options.providerId}:${window.location.hostname}`

  function setToggleState(nextEnabled: boolean): void {
    if (!toggleEl) return
    toggleEl.dataset.state = nextEnabled ? 'on' : 'off'
    toggleEl.textContent = nextEnabled ? 'Refine 推荐：开' : 'Refine 推荐：关'
    toggleEl.title = nextEnabled ? '点击关闭输入态推荐' : '点击重新开启输入态推荐'
  }

  function ensureToggle(): HTMLButtonElement | null {
    if (toggleEl) return toggleEl
    if (!document.body) return null

    const toggle = document.createElement('button')
    toggle.type = 'button'
    toggle.className = 'refine-reco-toggle'
    toggle.addEventListener('click', () => {
      void setEnabled(!enabled, true)
    })
    document.body.appendChild(toggle)
    toggleEl = toggle
    setToggleState(enabled)
    return toggle
  }

  async function readEnabledSetting(): Promise<boolean> {
    try {
      if (typeof chrome === 'undefined' || !chrome.storage?.local) return true
      const stored = await chrome.storage.local.get([SITE_SETTINGS_STORAGE_KEY])
      const bySite = stored[SITE_SETTINGS_STORAGE_KEY] as Record<string, unknown> | undefined
      const value = bySite?.[siteSettingKey]
      return typeof value === 'boolean' ? value : true
    } catch {
      return true
    }
  }

  async function persistEnabledSetting(nextEnabled: boolean): Promise<void> {
    try {
      if (typeof chrome === 'undefined' || !chrome.storage?.local) return
      const stored = await chrome.storage.local.get([SITE_SETTINGS_STORAGE_KEY])
      const raw = stored[SITE_SETTINGS_STORAGE_KEY]
      const bySite: Record<string, boolean> =
        raw && typeof raw === 'object' ? { ...(raw as Record<string, boolean>) } : {}
      bySite[siteSettingKey] = nextEnabled
      await chrome.storage.local.set({
        [SITE_SETTINGS_STORAGE_KEY]: bySite,
      })
    } catch {
      // ignore storage write errors to avoid breaking chat input
    }
  }

  async function setEnabled(nextEnabled: boolean, persist: boolean): Promise<void> {
    enabled = nextEnabled
    setToggleState(enabled)

    if (!enabled) {
      requestId += 1
      if (debounceTimer) {
        window.clearTimeout(debounceTimer)
        debounceTimer = undefined
      }
      hidePanel()
    }

    if (persist) {
      await persistEnabledSetting(nextEnabled)
    }
  }

  function ensurePanel(): { panel: HTMLDivElement; list: HTMLDivElement } | null {
    if (panelEl && listEl) return { panel: panelEl, list: listEl }
    if (!document.body) return null

    const panel = document.createElement('div')
    panel.className = 'refine-reco-panel'

    const head = document.createElement('div')
    head.className = 'refine-reco-head'

    const title = document.createElement('div')
    title.className = 'refine-reco-title'
    title.textContent = 'Refine 推荐'

    const close = document.createElement('button')
    close.type = 'button'
    close.className = 'refine-reco-close'
    close.textContent = '×'
    close.addEventListener('click', () => {
      panel.style.display = 'none'
    })

    head.appendChild(title)
    head.appendChild(close)

    const list = document.createElement('div')
    list.className = 'refine-reco-list'

    panel.appendChild(head)
    panel.appendChild(list)
    document.body.appendChild(panel)

    panelEl = panel
    listEl = list
    return { panel, list }
  }

  function hidePanel(): void {
    if (!panelEl) return
    panelEl.style.display = 'none'
  }

  function matchesSupportedInput(el: HTMLElement): boolean {
    return options.inputSelectors.some((selector) => {
      try {
        return el.matches(selector) || !!el.closest(selector)
      } catch {
        return false
      }
    })
  }

  function normalizeInputElement(target: EventTarget | null): HTMLElement | null {
    if (!(target instanceof HTMLElement)) return null
    if (matchesSupportedInput(target)) return target
    for (const selector of options.inputSelectors) {
      const found = target.closest(selector)
      if (found instanceof HTMLElement) return found
    }
    return null
  }

  function readInputText(inputEl: HTMLElement): string {
    if (inputEl instanceof HTMLTextAreaElement || inputEl instanceof HTMLInputElement) {
      return inputEl.value || ''
    }
    if (inputEl.isContentEditable) {
      return inputEl.innerText || inputEl.textContent || ''
    }
    return ''
  }

  function appendToInput(inputEl: HTMLElement, text: string): void {
    if (inputEl instanceof HTMLTextAreaElement || inputEl instanceof HTMLInputElement) {
      const separator = inputEl.value.trim().length > 0 ? '\n' : ''
      inputEl.value = `${inputEl.value}${separator}${text}`
      inputEl.dispatchEvent(new Event('input', { bubbles: true }))
      return
    }

    if (inputEl.isContentEditable) {
      inputEl.focus()
      const current = inputEl.innerText || inputEl.textContent || ''
      const separator = current.trim().length > 0 ? '\n' : ''
      inputEl.textContent = `${current}${separator}${text}`
      inputEl.dispatchEvent(new Event('input', { bubbles: true }))
    }
  }

  function reasonLabel(reason: string): string {
    if (reason === 'keyword_match') return '关键词匹配'
    if (reason === 'hybrid_match') return '混合匹配'
    if (reason === 'semantic_match') return '语义匹配'
    return reason || '命中'
  }

  async function reportExposed(query: string, items: RecommendationItem[]): Promise<void> {
    const key = `${query}:${items.map((item) => item.id).join(',')}`
    if (key === exposedKey) return
    exposedKey = key

    await trackEvent({
      event_name: 'recommendation_exposed',
      source: options.source,
      properties: {
        provider: options.source,
        query_length: query.length,
        item_count: items.length,
      },
      occurred_at: new Date().toISOString(),
    })
    void markOnboardingTask('searched')
  }

  async function reportClicked(item: RecommendationItem, action: 'insert' | 'copy'): Promise<void> {
    await trackEvent({
      event_name: 'recommendation_clicked',
      source: options.source,
      properties: {
        provider: options.source,
        action,
        item_id: item.id,
        item_type: item.item_type,
      },
      occurred_at: new Date().toISOString(),
    })

    await trackEvent({
      event_name: 'knowledge_reused',
      source: options.source,
      properties: {
        provider: options.source,
        action,
        item_id: item.id,
      },
      occurred_at: new Date().toISOString(),
    })
    void markOnboardingTask('reused')
  }

  async function copyRecommendation(item: RecommendationItem): Promise<void> {
    const text = item.content || item.summary || item.title
    try {
      await navigator.clipboard.writeText(text)
    } catch {
      // ignore clipboard error, interaction still tracked
    }
    await reportClicked(item, 'copy')
  }

  async function insertRecommendation(item: RecommendationItem): Promise<void> {
    const target = activeInputEl
    if (!target) return
    const text = item.content || item.summary || item.title
    appendToInput(target, text)
    await reportClicked(item, 'insert')
  }

  function renderRecommendations(items: RecommendationItem[], query: string): void {
    const view = ensurePanel()
    if (!view) return

    view.list.innerHTML = ''
    for (const item of items) {
      const container = document.createElement('article')
      container.className = 'refine-reco-item'

      const title = document.createElement('div')
      title.className = 'refine-reco-item-title'
      title.textContent = item.title

      const summary = document.createElement('div')
      summary.className = 'refine-reco-item-summary'
      summary.textContent = item.summary || '暂无摘要'

      const meta = document.createElement('div')
      meta.className = 'refine-reco-item-meta'

      const itemType = document.createElement('span')
      itemType.className = 'refine-reco-chip'
      itemType.dataset.kind = 'type'
      itemType.textContent = item.item_type || 'knowledge'
      meta.appendChild(itemType)

      const reason = document.createElement('span')
      reason.className = 'refine-reco-chip'
      reason.dataset.kind = 'reason'
      reason.textContent = reasonLabel(item.reason)
      meta.appendChild(reason)

      for (const tag of item.tags.slice(0, 3)) {
        const chip = document.createElement('span')
        chip.className = 'refine-reco-chip'
        chip.textContent = `#${tag}`
        meta.appendChild(chip)
      }

      const actions = document.createElement('div')
      actions.className = 'refine-reco-actions'

      const copyBtn = document.createElement('button')
      copyBtn.type = 'button'
      copyBtn.className = 'refine-reco-action'
      copyBtn.textContent = '复制'
      copyBtn.addEventListener('click', () => {
        void copyRecommendation(item)
      })

      const insertBtn = document.createElement('button')
      insertBtn.type = 'button'
      insertBtn.className = 'refine-reco-action'
      insertBtn.textContent = '插入'
      insertBtn.addEventListener('click', () => {
        void insertRecommendation(item)
      })

      actions.appendChild(copyBtn)
      actions.appendChild(insertBtn)
      container.appendChild(title)
      container.appendChild(summary)
      container.appendChild(meta)
      container.appendChild(actions)
      view.list.appendChild(container)
    }

    view.panel.style.display = 'block'
    void reportExposed(query, items)
  }

  async function queryRecommendations(inputEl: HTMLElement): Promise<void> {
    if (!enabled) {
      hidePanel()
      return
    }

    const query = readInputText(inputEl).trim()
    if (query.length < minChars) {
      hidePanel()
      return
    }

    const currentRequestId = ++requestId
    const response = await fetchRecommendations(query, {
      limit: maxItems,
      timeoutMs: DEFAULT_REQUEST_TIMEOUT_MS,
    })
    if (currentRequestId !== requestId) return

    if (!response?.success || !response.triggered || !Array.isArray(response.items) || response.items.length === 0) {
      hidePanel()
      return
    }

    activeInputEl = inputEl
    renderRecommendations(response.items.slice(0, maxItems), query)
  }

  function scheduleRecommendation(inputEl: HTMLElement): void {
    if (!enabled) {
      hidePanel()
      return
    }

    if (debounceTimer) window.clearTimeout(debounceTimer)
    debounceTimer = window.setTimeout(() => {
      void queryRecommendations(inputEl)
    }, debounceMs)
  }

  ensureToggle()
  void readEnabledSetting().then((storedEnabled) => {
    void setEnabled(storedEnabled, false)
  })

  document.addEventListener(
    'input',
    (event) => {
      const inputEl = normalizeInputElement(event.target)
      if (!inputEl) return
      scheduleRecommendation(inputEl)
    },
    true
  )

  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape') hidePanel()
  })
}
