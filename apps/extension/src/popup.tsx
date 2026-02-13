/**
 * Popup - 插件弹出窗口
 */

import { useState, useEffect } from 'react'
import './style.css'
import { readOnboardingTaskState, type OnboardingTaskState } from './lib/onboarding'
import type { SyncStatus } from './lib/types'

const POPUP_WIDTH_PX = 360

function enforcePopupViewportWidth(): void {
  if (typeof document === 'undefined') return

  const width = `${POPUP_WIDTH_PX}px`
  const root = document.documentElement
  root.style.setProperty('width', width, 'important')
  root.style.setProperty('min-width', width, 'important')
  root.style.setProperty('max-width', width, 'important')

  if (document.body) {
    document.body.style.setProperty('width', width, 'important')
    document.body.style.setProperty('min-width', width, 'important')
    document.body.style.setProperty('max-width', width, 'important')
    document.body.style.setProperty('margin', '0', 'important')
    document.body.style.setProperty('overflow-x', 'hidden', 'important')
  }

  const plasmoRoot = document.getElementById('__plasmo')
  if (plasmoRoot) {
    plasmoRoot.style.setProperty('width', width, 'important')
    plasmoRoot.style.setProperty('min-width', width, 'important')
    plasmoRoot.style.setProperty('max-width', width, 'important')
  }
}

enforcePopupViewportWidth()

interface Stats {
  totalItems: number
  todayExtracted: number
}

interface SyncStatusResponse {
  cloudHealthy: boolean
  status: SyncStatus
  remoteTotalItems?: number
  quota?: QuotaInfo
}

interface QuotaInfo {
  limit: number | null
  used: number
  remaining: number | null
  exceeded: boolean
}

interface ExtractResponse {
  success?: boolean
  length?: number
  message?: string
}

type MessageLevel = 'ok' | 'error' | ''

type IconProps = {
  className?: string
}

const DEFAULT_SYNC_STATUS: SyncStatus = {
  pending: 0,
  syncing: 0,
  failed: 0,
  sent: 0,
  apiBase: '',
}

const DEFAULT_ONBOARDING_STATE: OnboardingTaskState = {
  extracted: false,
  searched: false,
  reused: false,
  updatedAt: 0,
}

function isSupportedConversationUrl(url: string): boolean {
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'https:') return false

    const host = parsed.hostname.toLowerCase()
    const path = parsed.pathname.toLowerCase()

    if (host === 'chat.openai.com' || host === 'chatgpt.com' || host === 'claude.ai' || host === 'gemini.google.com') {
      return true
    }

    if (host === 'grok.com' || host.endsWith('.grok.com')) {
      return true
    }

    if ((host === 'x.com' || host === 'twitter.com') && path.startsWith('/i/grok')) {
      return true
    }

    if ((host === 'x.ai' || host.endsWith('.x.ai')) && path.startsWith('/grok')) {
      return true
    }

    return false
  } catch {
    return false
  }
}

async function safeRuntimeMessage<T>(message: unknown): Promise<T | null> {
  try {
    return (await chrome.runtime.sendMessage(message)) as T
  } catch {
    return null
  }
}

async function readStoredStats(): Promise<Stats> {
  const storage = await chrome.storage.local.get(['stats'])
  const stats = storage.stats as Partial<Stats> | undefined
  return {
    totalItems: typeof stats?.totalItems === 'number' ? stats.totalItems : 0,
    todayExtracted: typeof stats?.todayExtracted === 'number' ? stats.todayExtracted : 0,
  }
}

function LogoIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 3l1.9 4.1L18 9l-4.1 1.9L12 15l-1.9-4.1L6 9l4.1-1.9L12 3z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M6 15.8l1 2.1L9 19l-2 1-1 2.1-1-2.1-2-1 2-1.1 1-2.1z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function StackIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 4l7 4-7 4-7-4 7-4z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="M5 12l7 4 7-4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
      <path d="M5 16l7 4 7-4" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  )
}

function SparkIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M12 3l1.7 4 4 1.7-4 1.7-1.7 4-1.7-4-4-1.7 4-1.7L12 3z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M18.5 13.5l.9 1.8 1.8.9-1.8.9-.9 1.8-.9-1.8-1.8-.9 1.8-.9.9-1.8z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function CloudIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M7 18a4 4 0 0 1 0-8 5 5 0 0 1 9.8-1.5A3.5 3.5 0 1 1 18 18H7z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function BoltIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M13 2L5 13h6l-1 9 8-11h-6l1-9z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function RotateIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M20 11a8 8 0 1 0 2 5.2"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M20 4v7h-7"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

function LinkIcon({ className }: IconProps) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path
        d="M10 14l4-4"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M7 17a3 3 0 0 1 0-4.2l2.1-2.1a3 3 0 1 1 4.2 4.2L11 17"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M17 7a3 3 0 0 1 0 4.2l-2.1 2.1a3 3 0 1 1-4.2-4.2L13 7"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

export default function Popup() {
  const [stats, setStats] = useState<Stats>({ totalItems: 0, todayExtracted: 0 })
  const [remoteTotalItems, setRemoteTotalItems] = useState<number | null>(null)
  const [cloudHealthy, setCloudHealthy] = useState(false)
  const [extractMessage, setExtractMessage] = useState('')
  const [extractMessageLevel, setExtractMessageLevel] = useState<MessageLevel>('')
  const [syncStatus, setSyncStatus] = useState<SyncStatus>(DEFAULT_SYNC_STATUS)
  const [quota, setQuota] = useState<QuotaInfo | null>(null)
  const [onboarding, setOnboarding] = useState<OnboardingTaskState>(DEFAULT_ONBOARDING_STATE)

  const queueSize = syncStatus.pending + syncStatus.syncing
  const displayedTotalItems = remoteTotalItems ?? stats.totalItems
  const totalSourceText = remoteTotalItems == null ? '本地缓存' : '云端实时'
  const onboardingDoneCount = [onboarding.extracted, onboarding.searched, onboarding.reused].filter(Boolean).length
  const onboardingProgressPercent = Math.round((onboardingDoneCount / 3) * 100)

  const syncStateText = (() => {
    if (syncStatus.failed > 0) return `同步失败 ${syncStatus.failed} 条`
    if (queueSize > 0) {
      return `待同步 ${queueSize} 条`
    }
    return cloudHealthy ? '云端同步正常' : '云端不可达'
  })()

  const syncStateToneClass = (() => {
    if (syncStatus.failed > 0) return 'tone-error'
    if (queueSize > 0) return 'tone-warn'
    return cloudHealthy ? 'tone-ok' : 'tone-off'
  })()

  const syncCardToneClass = syncStatus.failed > 0 ? 'status-card-error' : cloudHealthy ? 'status-card-ok' : ''

  const syncCloudStatus = async () => {
    try {
      const result = await safeRuntimeMessage<SyncStatusResponse>({
        action: 'getSyncStatus',
      })

      if (result?.status) {
        setCloudHealthy(result.cloudHealthy)
        setSyncStatus(result.status)
        setRemoteTotalItems(typeof result.remoteTotalItems === 'number' ? result.remoteTotalItems : null)
        setQuota(result.quota || null)
      } else {
        setCloudHealthy(false)
        setSyncStatus(DEFAULT_SYNC_STATUS)
        setRemoteTotalItems(null)
        setQuota(null)
      }
    } catch {
      setCloudHealthy(false)
      setSyncStatus(DEFAULT_SYNC_STATUS)
      setRemoteTotalItems(null)
      setQuota(null)
    }

    try {
      setStats(await readStoredStats())
      setOnboarding(await readOnboardingTaskState())
    } catch {
      setStats({ totalItems: 0, todayExtracted: 0 })
      setOnboarding(DEFAULT_ONBOARDING_STATE)
    }
  }

  useEffect(() => {
    enforcePopupViewportWidth()

    void syncCloudStatus()
    const timer = setInterval(() => {
      void syncCloudStatus()
    }, 15000)

    return () => clearInterval(timer)
  }, [])

  useEffect(() => {
    const onStorageChanged: Parameters<typeof chrome.storage.onChanged.addListener>[0] = (
      changes,
      areaName
    ) => {
      if (areaName !== 'local') return

      if (changes.stats) {
        void readStoredStats()
          .then((next) => setStats(next))
          .catch(() => setStats({ totalItems: 0, todayExtracted: 0 }))
      }

      if (changes.onboardingTasks) {
        void readOnboardingTaskState()
          .then((next) => setOnboarding(next))
          .catch(() => setOnboarding(DEFAULT_ONBOARDING_STATE))
      }
    }

    chrome.storage.onChanged.addListener(onStorageChanged)
    return () => {
      chrome.storage.onChanged.removeListener(onStorageChanged)
    }
  }, [])

  useEffect(() => {
    const onVisibilityChanged = () => {
      if (document.visibilityState === 'visible') {
        void syncCloudStatus()
      }
    }

    document.addEventListener('visibilitychange', onVisibilityChanged)
    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChanged)
    }
  }, [])

  const handleExtract = async () => {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
    if (!tab?.id) {
      setExtractMessage('未找到活动标签页')
      setExtractMessageLevel('error')
      return
    }

    const url = tab.url || ''
    const isSupported = isSupportedConversationUrl(url)
    if (!isSupported) {
      setExtractMessage('当前页面不支持，请在 ChatGPT、Claude、Gemini 或 Grok 对话页使用')
      setExtractMessageLevel('error')
      return
    }

    let result: ExtractResponse
    try {
      result = ((await chrome.tabs.sendMessage(tab.id as number, {
        action: 'extract',
      })) as ExtractResponse) || { success: false, message: '提取失败' }
    } catch {
      result = {
        success: false,
        message: '页面脚本未就绪，请刷新当前对话页面后重试',
      }
    }

    if (result.success) {
      setExtractMessage(`提取成功，已加入同步队列（${result.length ?? 0} 字符）`)
      setExtractMessageLevel('ok')
      await syncCloudStatus()
      return
    }

    setExtractMessage(result.message || '提取失败')
    setExtractMessageLevel('error')
  }

  const handleForceSync = async () => {
    try {
      await safeRuntimeMessage({ action: 'forceSync' })
      await syncCloudStatus()
    } catch {
      setCloudHealthy(false)
      setRemoteTotalItems(null)
      setExtractMessage('扩展后台未就绪，请在扩展页点击“重新加载”')
      setExtractMessageLevel('error')
    }
  }

  return (
    <div className="popup-shell">
      <header className="popup-header">
        <div className="brand-wrap">
          <div className="brand-icon-wrap">
            <LogoIcon className="brand-icon" />
          </div>
          <div>
            <h1 className="brand-title">Refine</h1>
            <p className="brand-subtitle">智能知识复用引擎</p>
          </div>
        </div>
        <span className="brand-badge">EXT</span>
      </header>

      <section className="stats-grid">
        <article className="stats-card stats-card-brand">
          <div className="stats-card-top">
            <span className="stats-icon-box">
              <StackIcon className="stats-icon" />
            </span>
            <span className="stats-meta">{totalSourceText}</span>
          </div>
          <p className="stats-value">{displayedTotalItems}</p>
          <p className="stats-label">知识总数</p>
        </article>

        <article className="stats-card stats-card-green">
          <div className="stats-card-top">
            <span className="stats-icon-box stats-icon-box-green">
              <SparkIcon className="stats-icon" />
            </span>
            <span className="stats-meta">今日更新</span>
          </div>
          <p className="stats-value stats-value-green">{stats.todayExtracted}</p>
          <p className="stats-label">今日提炼</p>
        </article>
      </section>

      <section className={`status-card ${syncCardToneClass}`}>
        <div className={`status-dot ${syncStateToneClass}`} />
        <div className="status-content">
          <p className="status-title">
            <CloudIcon className="status-icon" />
            {syncStateText}
          </p>
          <p className="status-detail">已发送 {syncStatus.sent} 条，队列 {queueSize} 条</p>
          {quota ? (
            <p className={`status-detail ${quota.exceeded ? 'status-upgrade' : ''}`}>
              免费额度{' '}
              {typeof quota.limit === 'number'
                ? `${quota.used}/${quota.limit}`
                : `${quota.used}/无限制`}
              {quota.exceeded ? '（已超限，请升级）' : ''}
            </p>
          ) : null}
          {syncStatus.lastError ? <p className="status-error">{syncStatus.lastError}</p> : null}
        </div>
      </section>

      <section className="onboarding-card">
        <div className="onboarding-head">
          <p className="onboarding-title">首日任务流</p>
          <span className="onboarding-progress">{onboardingDoneCount}/3</span>
        </div>
        <div className="onboarding-progress-track" aria-hidden="true">
          <div
            className="onboarding-progress-fill"
            style={{ width: `${onboardingProgressPercent}%` }}
          />
        </div>
        <p className={`onboarding-item ${onboarding.extracted ? 'is-done' : ''}`}>1. 提取 1 条对话</p>
        <p className={`onboarding-item ${onboarding.searched ? 'is-done' : ''}`}>2. 搜索/触发推荐 1 次</p>
        <p className={`onboarding-item ${onboarding.reused ? 'is-done' : ''}`}>3. 复制或插入并复用 1 次</p>
      </section>

      <section className="actions">
        <button onClick={handleExtract} className="action-btn action-btn-primary">
          <BoltIcon className="action-icon" />
          提取当前对话
        </button>
        <button onClick={handleForceSync} className="action-btn action-btn-secondary">
          <RotateIcon className="action-icon" />
          立即同步队列
        </button>
      </section>

      {extractMessage && (
        <div className={`message-box ${extractMessageLevel === 'ok' ? 'message-success' : 'message-error'}`}>
          {extractMessage}
        </div>
      )}

      <footer className="popup-footer">
        <LinkIcon className="footer-icon" />
        <span>Cloud API:</span>
        <code className="footer-api">{syncStatus.apiBase || '(未配置)'}</code>
      </footer>
    </div>
  )
}
