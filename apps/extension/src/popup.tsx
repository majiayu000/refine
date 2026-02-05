/**
 * Popup - 插件弹出窗口
 */

import { useState, useEffect } from 'react'
import './style.css'

interface Stats {
  totalItems: number
  todayExtracted: number
}

export default function Popup() {
  const [stats, setStats] = useState<Stats>({ totalItems: 0, todayExtracted: 0 })
  const [isConnected, setIsConnected] = useState(false)

  useEffect(() => {
    // 从 storage 加载统计
    chrome.storage.local.get(['stats', 'connected'], (result) => {
      if (result.stats) setStats(result.stats)
      if (result.connected) setIsConnected(result.connected)
    })
  }, [])

  const handleExtract = async () => {
    // 发送消息到 content script 提取对话
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true })
    if (tab.id) {
      chrome.tabs.sendMessage(tab.id, { action: 'extract' })
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
        <div className={`w-2 h-2 rounded-full ${isConnected ? 'bg-green-500' : 'bg-red-500'}`} />
        <span className="text-sm text-gray-400">
          {isConnected ? '已连接桌面应用' : '未连接桌面应用'}
        </span>
      </div>

      {/* Actions */}
      <button
        onClick={handleExtract}
        className="w-full py-2.5 bg-brand-500 hover:bg-brand-600 text-white font-medium rounded-lg transition-colors"
      >
        提取当前对话
      </button>

      {/* Footer */}
      <div className="mt-4 pt-3 border-t border-gray-800 text-center">
        <p className="text-xs text-gray-600">
          支持 ChatGPT、Claude 等 AI 对话页面
        </p>
      </div>
    </div>
  )
}
