import { useStore } from '../lib/store'
import type { Item } from '../lib/tauri'

const typeColors = {
  knowledge: 'bg-blue-500/20 text-blue-400',
  skill: 'bg-green-500/20 text-green-400',
  snippet: 'bg-purple-500/20 text-purple-400',
}

const typeLabels = {
  knowledge: '知识',
  skill: '技能',
  snippet: '片段',
}

export function ItemList() {
  const { items, selectedItem, selectItem, isLoading } = useStore()

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center text-gray-500">
        加载中...
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center text-gray-500 p-4">
        <svg className="w-12 h-12 mb-3 opacity-50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
        </svg>
        <p className="text-sm">暂无知识</p>
        <p className="text-xs text-gray-600 mt-1">使用 ⌘K 搜索或添加</p>
      </div>
    )
  }

  return (
    <div className="flex-1 overflow-y-auto p-2">
      {items.map((item) => (
        <ItemCard
          key={item.id}
          item={item}
          isSelected={selectedItem?.id === item.id}
          onClick={() => selectItem(item)}
        />
      ))}
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
  return (
    <button
      onClick={onClick}
      className={`w-full text-left p-3 rounded-lg mb-1 transition-colors ${
        isSelected
          ? 'bg-brand-500/20 border border-brand-500/50'
          : 'hover:bg-gray-900 border border-transparent'
      }`}
    >
      <div className="flex items-start gap-2">
        <span className={`text-xs px-1.5 py-0.5 rounded ${typeColors[item.item_type]}`}>
          {typeLabels[item.item_type]}
        </span>
      </div>
      <h3 className="font-medium text-gray-200 mt-1.5 line-clamp-1">{item.title}</h3>
      <p className="text-sm text-gray-500 mt-1 line-clamp-2">{item.summary}</p>
      {item.tags.length > 0 && (
        <div className="flex gap-1 mt-2 flex-wrap">
          {item.tags.slice(0, 3).map((tag) => (
            <span key={tag} className="text-xs text-gray-500 bg-gray-800 px-1.5 py-0.5 rounded">
              {tag}
            </span>
          ))}
        </div>
      )}
    </button>
  )
}
