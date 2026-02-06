/**
 * Popup - 插件弹出窗口
 */

import { useState, useEffect } from 'react'
import './style.css'
import type { SyncStatus } from './lib/types'

interface Stats {
  totalItems: number
  todayExtracted: number
}

interface SyncStatusResponse {
  cloudHealthy: boolean
  status: SyncStatus
}

interface ExtractResponse {
  success?: boolean
  length?: number
  message?: string
}

async function safeRuntimeMessage<T>(message: unknown): Promise<T | null> {
  try {
    return (await chrome.runtime.sendMessage(message)) as T
  } catch {
    return null
  }
}

export default function Popup() {
  const [stats, setStats] = useState<Stats>({ totalItems: 0, todayExtracted: 0 })
  const [cloudHealthy, setCloudHealthy] = useState(false)
  const [extractMessage, setExtractMessage] = useState('')
  const [extractMessageLevel, setExtractMessageLevel] = useState<'ok' | 'error' | ''>('')
  const [syncStatus, setSyncStatus] = useState<SyncStatus>({
    pending: 0,
    syncing: 0,
    failed: 0,
    sent: 0,
    apiBase: '',
  })

  const syncStateText = (() => {
    if (syncStatus.failed > 0) return `同步失败 ${syncStatus.failed} 条`
    if (syncStatus.pending + syncStatus.syncing > 0) {
      return `待同步 ${syncStatus.pending + syncStatus.syncing} 条`
    }
    return cloudHealthy ? '云端同步正常' : '云端不可达'
  })()

  const syncStateDotClass = (() => {
    if (syncStatus.failed > 0) return 'bg-red-500'
    if (syncStatus.pending + syncStatus.syncing > 0) return 'bg-yellow-500'
    return cloudHealthy ? 'bg-green-500' : 'bg-gray-500'
  })()

  useEffect(() => {
    // 从 storage 加载统计
    chrome.storage.local.get(['stats'], (result) => {
      if (result.stats) setStats(result.stats)
    })

    const syncCloudStatus = async () => {
      const result = await safeRuntimeMessage<SyncStatusResponse>({
        action: 'getSyncStatus',
      })

      if (result?.status) {
        setCloudHealthy(result.cloudHealthy)
        setSyncStatus(result.status)
      } else {
        setCloudHealthy(false)
        setExtractMessage('扩展后台未就绪，请在扩展页点击“重新加载”')
        setExtractMessageLevel('error')
      }

      const storage = await chrome.storage.local.get(['stats'])
      if (storage.stats) {
        setStats(storage.stats as Stats)
      }
    }

    void syncCloudStatus()
    const timer = setInterval(() => {
      void syncCloudStatus()
    }, 15000)

    return () => clearInterval(timer)
  }, [])

  const handleExtract = async () => {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
    if (!tab?.id) {
      setExtractMessage('未找到活动标签页')
      setExtractMessageLevel('error')
      return
    }

    const url = tab.url || ''
    const isSupported = /^https:\/\/(chat\.openai\.com|chatgpt\.com|claude\.ai|gemini\.google\.com)\//.test(url)
    if (!isSupported) {
      setExtractMessage('当前页面不支持，请在 ChatGPT、Claude 或 Gemini 对话页使用')
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
      return
    }

    setExtractMessage(result.message || '提取失败')
    setExtractMessageLevel('error')
  }

  const handleForceSync = async () => {
    await safeRuntimeMessage({ action: 'forceSync' })
    const result = await safeRuntimeMessage<SyncStatusResponse>({
      action: 'getSyncStatus',
    })
    if (result?.status) {
      setCloudHealthy(result.cloudHealthy)
      setSyncStatus(result.status)
    } else {
      setCloudHealthy(false)
      setExtractMessage('扩展后台未就绪，请在扩展页点击“重新加载”')
      setExtractMessageLevel('error')
    }
  }

  return (
    <div className="w-80 bg-gray-950 text-gray-100 p-4">
      {/* Header */}
      <div className="flex items-center gap-2 mb-4">
        <div className="w-8 h-8 bg-brand-500 rounded-lg flex items-center justify-center">
          <span className="text-white font-bold">R</span>
        </div>
        <div>
          <h1 className="text-lg font-semibold">Refine</h1>
          <p className="text-xs text-gray-500">智能知识复用引擎</p>
        </div>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        <div className="bg-gray-900 rounded-lg p-3">
          <div className="text-2xl font-bold text-brand-500">{stats.totalItems}</div>
          <div className="text-xs text-gray-500">知识总数</div>
        </div>
        <div className="bg-gray-900 rounded-lg p-3">
          <div className="text-2xl font-bold text-green-500">{stats.todayExtracted}</div>
          <div className="text-xs text-gray-500">今日提炼</div>
        </div>
      </div>

      {/* Connection Status */}
      <div className="flex items-center gap-2 mb-4 px-3 py-2 bg-gray-900 rounded-lg">
        <div className={`w-2 h-2 rounded-full ${syncStateDotClass}`} />
        <span className="text-sm text-gray-400">
          {syncStateText}
        </span>
      </div>

      {/* Actions */}
      <button
        onClick={handleExtract}
        className="w-full py-2.5 bg-brand-500 hover:bg-brand-600 text-white font-medium rounded-lg transition-colors"
      >
        提取当前对话
      </button>
      <button
        onClick={handleForceSync}
        className="w-full mt-2 py-2 bg-gray-800 hover:bg-gray-700 text-gray-200 text-sm rounded-lg transition-colors"
      >
        立即同步队列
      </button>
      {extractMessage && (
        <div
          className={`mt-2 px-3 py-2 rounded-lg text-xs ${
            extractMessageLevel === 'ok'
              ? 'bg-emerald-950 text-emerald-300 border border-emerald-800'
              : 'bg-red-950 text-red-300 border border-red-800'
          }`}
        >
          {extractMessage}
        </div>
      )}

      {/* Footer */}
      <div className="mt-4 pt-3 border-t border-gray-800 text-center">
        <p className="text-xs text-gray-600">
          Cloud API: {syncStatus.apiBase || '(未配置)'}
        </p>
      </div>
    </div>
  )
}
