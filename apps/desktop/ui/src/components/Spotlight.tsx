/**
 * Spotlight - 全局搜索组件
 */

import { useState, useEffect, useRef, useCallback } from 'react'
import { motion, AnimatePresence } from 'framer-motion'
import {
  ArrowDown,
  ArrowUp,
  CornerDownLeft,
  LoaderCircle,
  Search,
} from 'lucide-react'
import { useStore } from '../lib/store'
import { cn } from '../lib/utils'
import { getApiClient } from '../lib/api/client'
import type { Item } from '../lib/api/types'
import { getItemTypeMeta } from '../lib/item-type-meta'

interface SpotlightProps {
  isOpen: boolean
  onClose: () => void
}

export function Spotlight({ isOpen, onClose }: SpotlightProps) {
  const api = getApiClient()
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<Item[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const [isSearching, setIsSearching] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const { selectItem, items } = useStore()

  useEffect(() => {
    if (!query.trim()) {
      setResults(items.slice(0, 6))
      return
    }

    const timer = setTimeout(async () => {
      setIsSearching(true)
      try {
        const result = await api.searchItems(query, 10)
        setResults(result.items)
      } catch {
        setResults([])
      } finally {
        setIsSearching(false)
      }
    }, 140)

    return () => clearTimeout(timer)
  }, [api, query, items])

  useEffect(() => {
    if (isOpen) {
      setQuery('')
      setSelectedIndex(0)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [isOpen])

  useEffect(() => {
    setSelectedIndex((index) => {
      if (results.length === 0) {
        return 0
      }
      return Math.min(index, results.length - 1)
    })
  }, [results.length])

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault()
          setSelectedIndex((index) => Math.min(index + 1, results.length - 1))
          break
        case 'ArrowUp':
          e.preventDefault()
          setSelectedIndex((index) => Math.max(index - 1, 0))
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
    [onClose, results, selectItem, selectedIndex]
  )

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={onClose}
            className="fixed inset-0 z-50 bg-slate-900/48 backdrop-blur-sm"
          />

          <motion.div
            initial={{ opacity: 0, scale: 0.97, y: -12 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: -12 }}
            transition={{ duration: 0.2 }}
            className="fixed left-1/2 top-[12%] z-50 w-full max-w-2xl -translate-x-1/2 px-4"
          >
            <div className="overflow-hidden rounded-3xl border border-sand-200/85 bg-white/95 shadow-2xl">
              <div className="flex items-center gap-3 border-b border-sand-200/80 px-4 py-3.5 md:px-5">
                <span className="flex h-9 w-9 items-center justify-center rounded-xl border border-brand-200 bg-brand-50 text-brand-700">
                  <Search className="h-4.5 w-4.5" />
                </span>

                <input
                  ref={inputRef}
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="搜索知识、技能、代码片段..."
                  className="flex-1 bg-transparent text-sm text-slate-800 placeholder:text-slate-400 focus:outline-none"
                />

                {isSearching ? (
                  <LoaderCircle className="h-4.5 w-4.5 animate-spin text-brand-700" />
                ) : (
                  <kbd className="rounded-md bg-sand-100 px-2 py-1 text-[11px] font-semibold text-sand-700">
                    ESC
                  </kbd>
                )}
              </div>

              <div className="max-h-80 overflow-y-auto py-2">
                {results.length > 0 ? (
                  <>
                    <div className="px-4 py-1 text-[11px] font-semibold uppercase tracking-[0.15em] text-slate-500 md:px-5">
                      {query ? '搜索结果' : '最近条目'}
                    </div>

                    {results.map((item, index) => {
                      const meta = getItemTypeMeta(item.item_type)
                      const Icon = meta.icon

                      return (
                        <button
                          key={item.id}
                          onClick={() => {
                            selectItem(item)
                            onClose()
                          }}
                          onMouseEnter={() => setSelectedIndex(index)}
                          className={cn(
                            'flex w-full items-start gap-3 px-4 py-2.5 text-left transition-colors md:px-5',
                            index === selectedIndex ? 'bg-brand-100/70' : 'hover:bg-slate-100/70'
                          )}
                        >
                          <span
                            className={cn(
                              'mt-0.5 inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-semibold',
                              meta.chipClass
                            )}
                          >
                            <Icon className="h-3.5 w-3.5" />
                            {meta.label}
                          </span>

                          <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-semibold text-slate-800">{item.title}</div>
                            <div className="mt-0.5 truncate text-xs text-slate-500">
                              {item.summary || '暂无摘要描述'}
                            </div>
                          </div>
                        </button>
                      )
                    })}
                  </>
                ) : (
                  <div className="py-12 text-center text-slate-500">
                    <p className="text-sm font-medium text-slate-700">没有找到匹配结果</p>
                    <p className="mt-1 text-xs text-slate-500">换个关键词再试试</p>
                  </div>
                )}
              </div>

              <div className="flex items-center gap-4 border-t border-sand-200/80 px-4 py-2.5 text-xs text-slate-500 md:px-5">
                <span className="inline-flex items-center gap-1">
                  <ArrowUp className="h-3.5 w-3.5" />
                  <ArrowDown className="h-3.5 w-3.5" />
                  导航
                </span>
                <span className="inline-flex items-center gap-1">
                  <CornerDownLeft className="h-3.5 w-3.5" />
                  选择
                </span>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  )
}
