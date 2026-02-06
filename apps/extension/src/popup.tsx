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

export default function Popup() {
  const [stats, setStats] = useState<Stats>({ totalItems: 0, todayExtracted: 0 })
  const [cloudHealthy, setCloudHealthy] = useState(false)
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
      const result = (await chrome.runtime.sendMessage({
        action: 'getSyncStatus',
      })) as SyncStatusResponse

      if (result?.status) {
        setCloudHealthy(result.cloudHealthy)
        setSyncStatus(result.status)
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
    // 发送消息到 content script 提取对话
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
    if (tab.id) {
      chrome.tabs.sendMessage(tab.id, { action: 'extract' })
    }
  }

  const handleForceSync = async () => {
    await chrome.runtime.sendMessage({ action: 'forceSync' })
    const result = (await chrome.runtime.sendMessage({
      action: 'getSyncStatus',
    })) as SyncStatusResponse
    if (result?.status) {
      setCloudHealthy(result.cloudHealthy)
      setSyncStatus(result.status)
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

      {/* Footer */}
      <div className="mt-4 pt-3 border-t border-gray-800 text-center">
        <p className="text-xs text-gray-600">
          Cloud API: {syncStatus.apiBase || '(未配置)'}
        </p>
      </div>
    </div>
  )
}
