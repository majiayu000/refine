import {
  BookOpenText,
  CalendarClock,
  Code2,
  Hash,
  NotebookPen,
  Trash2,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import { useStore } from '../lib/store'
import { cn } from '../lib/utils'
import type { Item } from '../lib/tauri'

const typeMeta: Record<
  Item['item_type'],
  { label: string; icon: LucideIcon; badgeClass: string }
> = {
  knowledge: {
    label: '知识文档',
    icon: BookOpenText,
    badgeClass: 'bg-brand-100 text-brand-800 border-brand-200',
  },
  skill: {
    label: '技能说明',
    icon: Wrench,
    badgeClass: 'bg-amber-100 text-amber-800 border-amber-200',
  },
  snippet: {
    label: '代码片段',
    icon: Code2,
    badgeClass: 'bg-slate-100 text-slate-700 border-slate-200',
  },
}

function formatFullDate(date: string): string {
  return new Date(date).toLocaleDateString('zh-CN', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  })
}

export function ItemDetail() {
  const { selectedItem, deleteItem } = useStore()

  if (!selectedItem) {
    return (
      <div className="flex h-full items-center justify-center p-6 md:p-8">
        <div className="w-full max-w-lg rounded-3xl border border-sand-200/85 bg-white/85 p-8 text-center shadow-soft">
          <span className="mx-auto flex h-16 w-16 items-center justify-center rounded-2xl border border-brand-200 bg-brand-50 text-brand-700">
            <NotebookPen className="h-8 w-8" />
          </span>
          <h2 className="mt-5 font-display text-2xl text-slate-900">选择一条知识</h2>
          <p className="mt-3 text-sm leading-relaxed text-slate-600">
            左侧列表用于快速切换，顶部快捷键 <kbd className="rounded bg-sand-100 px-1.5 py-0.5 text-xs">⌘K</kbd>
            可以全局搜索。
          </p>
        </div>
      </div>
    )
  }

  const meta = typeMeta[selectedItem.item_type]
  const Icon = meta.icon

  const handleDelete = async () => {
    if (window.confirm('确定要删除这条知识吗？')) {
      await deleteItem(selectedItem.id)
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="border-b border-sand-200/80 bg-gradient-to-br from-white/95 via-white/85 to-brand-50/45 px-5 py-5 md:px-8 md:py-7">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0 flex-1">
            <span
              className={cn(
                'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold',
                meta.badgeClass
              )}
            >
              <Icon className="h-4 w-4" />
              {meta.label}
            </span>
            <h1 className="mt-4 break-words font-display text-3xl leading-tight text-slate-900">{selectedItem.title}</h1>
            <p className="mt-3 max-w-3xl text-[15px] leading-relaxed text-slate-600">
              {selectedItem.summary || '暂无摘要描述。'}
            </p>
          </div>

          <button
            onClick={handleDelete}
            className="inline-flex items-center gap-1.5 rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-sm font-medium text-red-700 transition-colors hover:bg-red-100"
            title="删除"
          >
            <Trash2 className="h-4 w-4" />
            删除
          </button>
        </div>

        {selectedItem.tags.length > 0 && (
          <div className="mt-5 flex flex-wrap gap-2">
            {selectedItem.tags.map((tag) => (
              <span
                key={tag}
                className="rounded-full border border-sand-200 bg-sand-50 px-2.5 py-1 text-xs font-medium text-sand-700"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}

        <div className="mt-5 flex flex-wrap items-center gap-3 text-xs text-slate-500">
          <span className="inline-flex items-center gap-1.5 rounded-full border border-sand-200 bg-white/70 px-2.5 py-1">
            <Hash className="h-3.5 w-3.5" />
            {selectedItem.id.slice(0, 8)}
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full border border-sand-200 bg-white/70 px-2.5 py-1">
            <CalendarClock className="h-3.5 w-3.5" />
            {formatFullDate(selectedItem.created_at)}
          </span>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5 md:px-8 md:py-6">
        <section className="rounded-2xl border border-sand-200/80 bg-white/85 p-4 shadow-sm md:p-6">
          <h2 className="text-sm font-semibold uppercase tracking-[0.2em] text-slate-500">正文内容</h2>

          {selectedItem.item_type === 'snippet' ? (
            <pre className="mt-4 overflow-x-auto rounded-xl border border-slate-200 bg-slate-900 p-4 text-slate-100">
              <code className="font-mono text-sm leading-relaxed">
                {selectedItem.content || '// 暂无代码内容'}
              </code>
            </pre>
          ) : (
            <div className="mt-4 whitespace-pre-wrap break-words text-[15px] leading-8 text-slate-700">
              {selectedItem.content || '暂无内容'}
            </div>
          )}
        </section>
      </div>
    </div>
  )
}
