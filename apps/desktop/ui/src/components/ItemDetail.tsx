import { useStore } from '../lib/store'

const typeLabels = {
  knowledge: '知识',
  skill: '技能',
  snippet: '代码片段',
}

export function ItemDetail() {
  const { selectedItem, deleteItem } = useStore()

  if (!selectedItem) {
    return (
      <div className="h-full flex flex-col items-center justify-center text-gray-600">
        <svg className="w-16 h-16 mb-4 opacity-30" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
        </svg>
        <p>选择一个知识查看详情</p>
      </div>
    )
  }

  const handleDelete = async () => {
    if (window.confirm('确定要删除这个知识吗？')) {
      await deleteItem(selectedItem.id)
    }
  }

  return (
    <div className="h-full flex flex-col">
      {/* 头部 */}
      <header className="p-6 border-b border-gray-800">
        <div className="flex items-start justify-between">
          <div>
            <span className="text-xs text-brand-400 font-medium">
              {typeLabels[selectedItem.item_type]}
            </span>
            <h1 className="text-2xl font-semibold text-gray-100 mt-1">
              {selectedItem.title}
            </h1>
            <p className="text-gray-400 mt-2">{selectedItem.summary}</p>
          </div>
          <button
            onClick={handleDelete}
            className="p-2 text-gray-500 hover:text-red-400 hover:bg-red-500/10 rounded-lg transition-colors"
            title="删除"
          >
            <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
            </svg>
          </button>
        </div>

        {/* 标签 */}
        {selectedItem.tags.length > 0 && (
          <div className="flex gap-2 mt-4">
            {selectedItem.tags.map((tag) => (
              <span
                key={tag}
                className="text-sm text-gray-400 bg-gray-800 px-2 py-1 rounded"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}

        {/* 元信息 */}
        <div className="flex gap-4 mt-4 text-xs text-gray-600">
          <span>ID: {selectedItem.id.slice(0, 8)}</span>
          <span>创建于: {new Date(selectedItem.created_at).toLocaleDateString()}</span>
        </div>
      </header>

      {/* 内容 */}
      <div className="flex-1 overflow-y-auto p-6">
        <div className="prose prose-invert prose-sm max-w-none">
          {selectedItem.item_type === 'snippet' ? (
            <pre className="bg-gray-900 p-4 rounded-lg overflow-x-auto">
              <code className="text-sm font-mono text-gray-300">
                {selectedItem.content}
              </code>
            </pre>
          ) : (
            <div className="whitespace-pre-wrap text-gray-300 leading-relaxed">
              {selectedItem.content || '暂无内容'}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
