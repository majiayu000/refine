import { useEffect } from 'react'
import { useStore } from './lib/store'
import { Spotlight } from './components/Spotlight'
import { ItemList } from './components/ItemList'
import { ItemDetail } from './components/ItemDetail'

export default function App() {
  const { loadItems, isSpotlightOpen, setSpotlightOpen } = useStore()

  useEffect(() => {
    loadItems()

    // 全局快捷键
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault()
        setSpotlightOpen(true)
      }
      if (e.key === 'Escape') {
        setSpotlightOpen(false)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [loadItems, setSpotlightOpen])

  return (
    <div className="flex h-screen bg-gray-950">
      {/* 侧边栏 */}
      <aside className="w-72 border-r border-gray-800 flex flex-col">
        <header className="p-4 border-b border-gray-800">
          <h1 className="text-xl font-semibold text-brand-400">Refine</h1>
          <p className="text-sm text-gray-500 mt-1">智能知识复用引擎</p>
        </header>

        {/* 搜索按钮 */}
        <button
          onClick={() => setSpotlightOpen(true)}
          className="mx-4 mt-4 flex items-center gap-2 px-3 py-2 text-sm text-gray-400 bg-gray-900 rounded-lg border border-gray-800 hover:border-gray-700 transition-colors"
        >
          <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
          <span>搜索知识...</span>
          <kbd className="ml-auto text-xs bg-gray-800 px-1.5 py-0.5 rounded">⌘K</kbd>
        </button>

        {/* 知识列表 */}
        <ItemList />
      </aside>

      {/* 主内容区 */}
      <main className="flex-1 overflow-hidden">
        <ItemDetail />
      </main>

      {/* Spotlight */}
      <Spotlight
        isOpen={isSpotlightOpen}
        onClose={() => setSpotlightOpen(false)}
      />
    </div>
  )
}
