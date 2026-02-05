/**
 * Spotlight - 全局搜索组件
 */

import { useState, useEffect, useRef, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import { useStore } from '../lib/store'
import { searchItems } from '../lib/tauri'
import type { Item } from '../lib/tauri'

interface SpotlightProps {
  isOpen: boolean
  onClose: () => void
}

const typeColors = {
  knowledge: 'text-blue-400 bg-blue-500/10',
  skill: 'text-violet-400 bg-violet-500/10',
  snippet: 'text-emerald-400 bg-emerald-500/10',
}

const typeLabels = {
  knowledge: '知识',
  skill: '技能',
  snippet: '代码',
}

export function Spotlight({ isOpen, onClose }: SpotlightProps) {
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<Item[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [isSearching, setIsSearching] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const { selectItem, items } = useStore()

  // 搜索
  useEffect(() => {
    if (!query.trim()) {
      setResults(items.slice(0, 5))
      return
    }

    const timer = setTimeout(async () => {
      setIsSearching(true)
      try {
        const result = await searchItems(query, 10)
        setResults(result.items)
      } catch {
        setResults([])
      }
      setIsSearching(false)
    }, 150)

    return () => clearTimeout(timer)
  }, [query, items])

  // 重置状态
  useEffect(() => {
    if (isOpen) {
      setQuery('')
      setSelectedIndex(0)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [isOpen])

  // 键盘导航
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault()
          setSelectedIndex((i) => Math.min(i + 1, results.length - 1))
          break
        case 'ArrowUp':
          e.preventDefault()
          setSelectedIndex((i) => Math.max(i - 1, 0))
          break
        case 'Enter':
          e.preventDefault()
          if (results[selectedIndex]) {
            selectItem(results[selectedIndex])
            onClose()
          }
          break
        case 'Escape':
          e.preventDefault()
          onClose()
          break
      }
    },
    [results, selectedIndex, selectItem, onClose]
  )

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          {/* 背景遮罩 */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50"
          />

          {/* 搜索面板 */}
          <motion.div
            initial={{ opacity: 0, scale: 0.96, y: -10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: -10 }}
            transition={{ duration: 0.2 }}
            className="fixed top-[15%] left-1/2 -translate-x-1/2 w-full max-w-xl z-50 px-4"
          >
            <div className="bg-gray-900/95 backdrop-blur-xl rounded-xl border border-gray-700/50 shadow-2xl overflow-hidden">
              {/* 搜索框 */}
              <div className="flex items-center px-4 py-3 border-b border-gray-800">
                <svg
                  className="w-5 h-5 text-gray-500 mr-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
                  />
                </svg>
                <input
                  ref={inputRef}
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="搜索知识、技能、代码片段..."
                  className="flex-1 bg-transparent text-white text-sm placeholder:text-gray-500 focus:outline-none"
                />
                {isSearching && (
                  <div className="w-4 h-4 border-2 border-brand-500 border-t-transparent rounded-full animate-spin" />
                )}
              </div>

              {/* 结果列表 */}
              <div className="max-h-80 overflow-y-auto py-2">
                {results.length > 0 ? (
                  <>
                    <div className="px-4 py-1 text-xs font-medium text-gray-500 uppercase">
                      {query ? '搜索结果' : '最近使用'}
                    </div>
                    {results.map((item, index) => (
                      <button
                        key={item.id}
                        onClick={() => {
                          selectItem(item)
                          onClose()
                        }}
                        onMouseEnter={() => setSelectedIndex(index)}
                        className={`w-full text-left px-4 py-2.5 flex items-start gap-3 transition-colors ${
                          index === selectedIndex
                            ? 'bg-brand-500/10'
                            : 'hover:bg-gray-800/50'
                        }`}
                      >
                        <span
                          className={`mt-0.5 text-xs px-1.5 py-0.5 rounded ${typeColors[item.item_type]}`}
                        >
                          {typeLabels[item.item_type]}
                        </span>
                        <div className="flex-1 min-w-0">
                          <div className="text-sm font-medium text-gray-200 truncate">
                            {item.title}
                          </div>
                          <div className="text-xs text-gray-500 truncate mt-0.5">
                            {item.summary}
                          </div>
                        </div>
                      </button>
                    ))}
                  </>
                ) : (
                  <div className="py-12 text-center text-gray-500">
                    <p className="text-sm">没有找到匹配的结果</p>
                    <p className="text-xs text-gray-600 mt-1">尝试其他关键词</p>
                  </div>
                )}
              </div>

              {/* 底部提示 */}
              <div className="flex items-center gap-4 px-4 py-2 border-t border-gray-800 text-xs text-gray-600">
                <span className="flex items-center gap-1">
                  <kbd className="px-1 bg-gray-800 rounded">↑↓</kbd>
                  <span>导航</span>
                </span>
                <span className="flex items-center gap-1">
                  <kbd className="px-1 bg-gray-800 rounded">↵</kbd>
                  <span>选择</span>
                </span>
                <span className="flex items-center gap-1">
                  <kbd className="px-1 bg-gray-800 rounded">esc</kbd>
                  <span>关闭</span>
                </span>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  )
}
