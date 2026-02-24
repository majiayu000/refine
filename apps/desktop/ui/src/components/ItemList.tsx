import {
  BookOpenText,
  Clock3,
  Code2,
  Inbox,
  LoaderCircle,
  Wrench,
  type LucideIcon,
} from 'lucide-react'
import { useStore } from '../lib/store'
import { cn } from '../lib/utils'
import type { Item } from '../lib/tauri'

const typeMeta: Record<
  Item['item_type'],
  { label: string; icon: LucideIcon; chipClass: string }
> = {
  knowledge: {
    label: '知识',
    icon: BookOpenText,
    chipClass: 'bg-brand-100 text-brand-800 border border-brand-200',
  },
  skill: {
    label: '技能',
    icon: Wrench,
    chipClass: 'bg-amber-100 text-amber-800 border border-amber-200',
  },
  snippet: {
    label: '片段',
    icon: Code2,
    chipClass: 'bg-slate-100 text-slate-700 border border-slate-200',
  },
}

function formatShortDate(date: string): string {
  return new Date(date).toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
  })
}

export function ItemList() {
  const {
    items,
    totalItems,
    nextCursor,
    selectedItem,
    selectItem,
    isLoading,
    isLoadingMore,
    loadMoreItems,
  } = useStore()

  if (isLoading) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center text-sand-700">
        <LoaderCircle className="h-6 w-6 animate-spin text-brand-700" />
        <p className="mt-3 text-sm font-medium">正在加载知识资产...</p>
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center px-6 text-center text-slate-500">
        <span className="flex h-14 w-14 items-center justify-center rounded-2xl border border-sand-200 bg-white text-brand-700 shadow-sm">
          <Inbox className="h-7 w-7" />
        </span>
        <p className="mt-4 text-sm font-semibold text-slate-700">知识库还是空的</p>
        <p className="mt-1 text-xs leading-relaxed text-slate-500">按下 ⌘K 搜索，或先导入一条知识作为起点。</p>
      </div>
    )
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col px-3 pb-3 pt-2 md:px-4">
      <div className="mb-2 flex items-center justify-between px-1 text-xs text-slate-500">
        <span>已加载 {items.length} / 共 {totalItems} 条</span>
        <span>{nextCursor === null ? '已全部加载' : `剩余 ${Math.max(totalItems - items.length, 0)} 条`}</span>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="space-y-2">
          {items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              isSelected={selectedItem?.id === item.id}
              onClick={() => selectItem(item)}
            />
          ))}
        </div>
      </div>

      {nextCursor !== null && (
        <button
          onClick={() => void loadMoreItems()}
          disabled={isLoadingMore}
          className="mt-3 inline-flex items-center justify-center gap-2 rounded-xl border border-sand-200 bg-white/85 px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-white disabled:cursor-not-allowed disabled:opacity-60"
        >
          {isLoadingMore && <LoaderCircle className="h-4 w-4 animate-spin" />}
          {isLoadingMore ? '正在加载...' : '加载更多'}
        </button>
      )}
    </div>
  )
}

function ItemCard({
  item,
  isSelected,
  onClick,
}: {
  item: Item
  isSelected: boolean
  onClick: () => void
}) {
  const meta = typeMeta[item.item_type]
  const Icon = meta.icon

  return (
    <button
      onClick={onClick}
      className={cn(
        'group w-full rounded-2xl border p-3.5 text-left transition-all',
        isSelected
          ? 'border-brand-400/50 bg-brand-50/90 shadow-soft'
          : 'border-sand-200/85 bg-white/72 hover:-translate-y-0.5 hover:border-brand-300/70 hover:bg-white hover:shadow-soft'
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span
          className={cn('inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-[11px] font-semibold', meta.chipClass)}
        >
          <Icon className="h-3.5 w-3.5" />
          {meta.label}
        </span>
        <span className="inline-flex items-center gap-1 text-[11px] text-slate-500">
          <Clock3 className="h-3.5 w-3.5" />
          {formatShortDate(item.created_at)}
        </span>
      </div>

      <h3 className="mt-3 line-clamp-1 text-[15px] font-semibold text-slate-800">{item.title}</h3>
      <p className="mt-1.5 line-clamp-2 text-sm leading-relaxed text-slate-600">{item.summary || '暂无摘要描述'}</p>

      {item.tags.length > 0 && (
        <div className="mt-3 flex flex-wrap gap-1.5">
          {item.tags.slice(0, 4).map((tag) => (
            <span
              key={tag}
              className="rounded-full border border-sand-200 bg-sand-50 px-2 py-0.5 text-[11px] font-medium text-sand-700"
            >
              #{tag}
            </span>
          ))}
          {item.tags.length > 4 && (
            <span className="rounded-full border border-sand-200 bg-sand-50 px-2 py-0.5 text-[11px] font-medium text-sand-700">
              +{item.tags.length - 4}
            </span>
          )}
        </div>
      )}
    </button>
  )
}
