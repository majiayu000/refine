import { useEffect } from 'react'
import { Search, Sparkles } from 'lucide-react'
import { useStore } from './lib/store'
import { Spotlight } from './components/Spotlight'
import { ItemList } from './components/ItemList'
import { ItemDetail } from './components/ItemDetail'

export default function App() {
  const { loadItems, isSpotlightOpen, setSpotlightOpen } = useStore()

  useEffect(() => {
    loadItems()

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
    <div className="relative h-screen overflow-hidden text-slate-900">
      <div className="pointer-events-none absolute -left-20 -top-24 h-80 w-80 animate-pulse-soft rounded-full bg-brand-300/45 blur-3xl" />
      <div className="pointer-events-none absolute -right-24 top-0 h-96 w-96 animate-pulse-soft rounded-full bg-orange-300/35 blur-3xl [animation-delay:600ms]" />
      <div className="app-grain pointer-events-none absolute inset-0 opacity-50" />

      <div className="relative z-10 grid h-full w-full grid-rows-[minmax(17rem,0.95fr)_minmax(0,1.05fr)] gap-4 p-4 md:grid-cols-[22rem_minmax(0,1fr)] md:grid-rows-1 md:gap-6 md:p-6">
        <aside className="animate-rise-in flex min-h-0 flex-col rounded-[28px] border border-sand-200/70 bg-white/80 shadow-soft backdrop-blur-xl">
          <header className="border-b border-sand-200/80 px-5 py-5">
            <div className="flex items-start justify-between gap-3">
              <div>
                <p className="text-[11px] uppercase tracking-[0.24em] text-brand-700">Knowledge Console</p>
                <h1 className="mt-2 font-display text-3xl leading-none text-slate-900">Refine</h1>
              </div>
              <span className="inline-flex items-center gap-1 rounded-full border border-brand-200 bg-brand-50 px-2.5 py-1 text-[11px] font-semibold text-brand-700">
                <Sparkles className="h-3.5 w-3.5" />
                Live
              </span>
            </div>
            <p className="mt-3 text-sm leading-relaxed text-slate-600">
              让知识、技能与代码片段沉淀成可复用资产。
            </p>
          </header>

          <button
            onClick={() => setSpotlightOpen(true)}
            className="mx-4 mt-4 flex items-center gap-3 rounded-2xl border border-sand-200/90 bg-white/90 px-3.5 py-3 text-left text-sm shadow-sm transition-all hover:-translate-y-0.5 hover:border-brand-300 hover:shadow-glow"
          >
            <span className="flex h-8 w-8 items-center justify-center rounded-xl bg-brand-100 text-brand-700">
              <Search className="h-4 w-4" />
            </span>
            <span className="font-medium text-slate-700">搜索知识库</span>
            <kbd className="ml-auto rounded-md bg-sand-100 px-2 py-1 text-[11px] font-semibold text-sand-700">
              ⌘K
            </kbd>
          </button>

          <ItemList />
        </aside>

        <main className="animate-rise-in min-h-0 rounded-[28px] border border-sand-200/70 bg-white/78 shadow-soft backdrop-blur-xl [animation-delay:90ms]">
          <ItemDetail />
        </main>
      </div>

      <Spotlight isOpen={isSpotlightOpen} onClose={() => setSpotlightOpen(false)} />
    </div>
  )
}
