/**
 * Spotlight - Global Search Component
 *
 * 全局搜索悬浮窗，支持键盘导航
 * 设计风格：Refined Dark Minimalism
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { Search, FileText, Zap, Code, Clock, ArrowRight } from 'lucide-react';
import { cn } from '@/lib/utils';

// ============================================================================
// Types
// ============================================================================

type ItemType = 'knowledge' | 'skill' | 'snippet';

interface SearchItem {
  id: string;
  type: ItemType;
  title: string;
  subtitle?: string;
  tags?: string[];
  timestamp?: string;
}

interface SpotlightProps {
  isOpen: boolean;
  onClose: () => void;
  onSelect?: (item: SearchItem) => void;
}

// ============================================================================
// Mock Data (替换为实际 Tauri 调用)
// ============================================================================

const mockResults: SearchItem[] = [
  {
    id: '1',
    type: 'knowledge',
    title: 'Python asyncio 最佳实践',
    subtitle: 'CPU密集用multiprocessing，IO密集用asyncio...',
    tags: ['python', 'asyncio'],
    timestamp: '12分钟前',
  },
  {
    id: '2',
    type: 'snippet',
    title: 'asyncio 并发请求示例',
    subtitle: 'async def fetch_all(urls): ...',
    tags: ['python'],
    timestamp: '12分钟前',
  },
  {
    id: '3',
    type: 'skill',
    title: '代码审查专家',
    subtitle: '对代码进行安全性、性能、可读性审查',
    tags: ['code-review'],
    timestamp: '2天前',
  },
  {
    id: '4',
    type: 'knowledge',
    title: 'Docker Compose 网络配置',
    subtitle: '使用自定义网络实现容器间通信...',
    tags: ['docker', 'devops'],
    timestamp: '1周前',
  },
  {
    id: '5',
    type: 'snippet',
    title: 'React useCallback 优化模式',
    subtitle: 'const handleClick = useCallback(() => {...}, []);',
    tags: ['react', 'performance'],
    timestamp: '2周前',
  },
];

// ============================================================================
// Sub Components
// ============================================================================

const TypeIcon = ({ type }: { type: ItemType }) => {
  const icons = {
    knowledge: FileText,
    skill: Zap,
    snippet: Code,
  };
  const Icon = icons[type];

  const colors = {
    knowledge: 'text-blue-400',
    skill: 'text-violet-400',
    snippet: 'text-emerald-400',
  };

  return <Icon className={cn('w-4 h-4', colors[type])} />;
};

const TypeBadge = ({ type }: { type: ItemType }) => {
  const labels = {
    knowledge: '知识',
    skill: '技能',
    snippet: '代码',
  };

  const colors = {
    knowledge: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
    skill: 'bg-violet-500/10 text-violet-400 border-violet-500/20',
    snippet: 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
  };

  return (
    <span
      className={cn(
        'px-1.5 py-0.5 text-[10px] font-medium rounded border',
        colors[type]
      )}
    >
      {labels[type]}
    </span>
  );
};

const Shortcut = ({ keys }: { keys: string[] }) => (
  <div className="flex items-center gap-0.5">
    {keys.map((key, i) => (
      <kbd
        key={i}
        className="px-1.5 py-0.5 text-[10px] font-mono bg-white/5 border border-white/10 rounded text-zinc-500"
      >
        {key}
      </kbd>
    ))}
  </div>
);

const ResultItem = ({
  item,
  isSelected,
  onSelect,
  onHover,
}: {
  item: SearchItem;
  isSelected: boolean;
  onSelect: () => void;
  onHover: () => void;
}) => {
  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -4 }}
      transition={{ duration: 0.15 }}
      onClick={onSelect}
      onMouseEnter={onHover}
      className={cn(
        'group relative px-3 py-2.5 cursor-pointer rounded-lg transition-all duration-150',
        isSelected
          ? 'bg-gradient-to-r from-primary-500/15 to-primary-500/5'
          : 'hover:bg-white/[0.03]'
      )}
    >
      {/* Selection indicator */}
      <AnimatePresence>
        {isSelected && (
          <motion.div
            layoutId="spotlight-selection"
            className="absolute inset-0 rounded-lg border border-primary-500/30"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
          />
        )}
      </AnimatePresence>

      <div className="relative flex items-start gap-3">
        {/* Icon */}
        <div
          className={cn(
            'mt-0.5 p-1.5 rounded-md transition-colors duration-150',
            isSelected ? 'bg-white/10' : 'bg-white/5'
          )}
        >
          <TypeIcon type={item.type} />
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h4
              className={cn(
                'text-sm font-medium truncate transition-colors duration-150',
                isSelected ? 'text-white' : 'text-zinc-200'
              )}
            >
              {item.title}
            </h4>
            <TypeBadge type={item.type} />
          </div>

          {item.subtitle && (
            <p className="mt-0.5 text-xs text-zinc-500 truncate font-mono">
              {item.subtitle}
            </p>
          )}

          {/* Tags & Timestamp */}
          <div className="mt-1.5 flex items-center gap-2">
            {item.tags?.map((tag) => (
              <span
                key={tag}
                className="text-[10px] text-zinc-600 font-medium"
              >
                #{tag}
              </span>
            ))}
            {item.timestamp && (
              <span className="flex items-center gap-1 text-[10px] text-zinc-600">
                <Clock className="w-3 h-3" />
                {item.timestamp}
              </span>
            )}
          </div>
        </div>

        {/* Action hint */}
        <div
          className={cn(
            'flex items-center gap-1 opacity-0 transition-opacity duration-150',
            isSelected && 'opacity-100'
          )}
        >
          <Shortcut keys={['↵']} />
        </div>
      </div>
    </motion.div>
  );
};

// ============================================================================
// Main Component
// ============================================================================

export function Spotlight({ isOpen, onClose, onSelect }: SpotlightProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Filter results based on query
  const results = useMemo(() => {
    if (!query.trim()) return mockResults.slice(0, 3); // 显示最近使用
    return mockResults.filter(
      (item) =>
        item.title.toLowerCase().includes(query.toLowerCase()) ||
        item.tags?.some((tag) => tag.includes(query.toLowerCase()))
    );
  }, [query]);

  // Reset selection when results change
  useEffect(() => {
    setSelectedIndex(0);
  }, [results]);

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setSelectedIndex((i) => Math.min(i + 1, results.length - 1));
          break;
        case 'ArrowUp':
          e.preventDefault();
          setSelectedIndex((i) => Math.max(i - 1, 0));
          break;
        case 'Enter':
          e.preventDefault();
          if (results[selectedIndex]) {
            onSelect?.(results[selectedIndex]);
            onClose();
          }
          break;
        case 'Escape':
          e.preventDefault();
          onClose();
          break;
      }
    },
    [results, selectedIndex, onSelect, onClose]
  );

  // Scroll selected item into view
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;

    const selected = list.children[selectedIndex] as HTMLElement;
    if (selected) {
      selected.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }
  }, [selectedIndex]);

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          {/* Backdrop */}
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            onClick={onClose}
            className="fixed inset-0 bg-black/60 backdrop-blur-sm z-50"
          />

          {/* Spotlight Panel */}
          <motion.div
            initial={{ opacity: 0, scale: 0.96, y: -10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.96, y: -10 }}
            transition={{ duration: 0.2, ease: [0.23, 1, 0.32, 1] }}
            className="fixed top-[18%] left-1/2 -translate-x-1/2 w-full max-w-[580px] z-50"
          >
            <div
              className={cn(
                'relative overflow-hidden rounded-2xl',
                'bg-zinc-900/95 backdrop-blur-xl',
                'border border-white/[0.08]',
                'shadow-2xl shadow-black/50',
                // Subtle glow effect
                'before:absolute before:inset-0 before:rounded-2xl',
                'before:bg-gradient-to-b before:from-primary-500/5 before:to-transparent',
                'before:pointer-events-none'
              )}
            >
              {/* Search Input */}
              <div className="relative flex items-center px-4 py-3 border-b border-white/[0.06]">
                <Search className="w-5 h-5 text-zinc-500 mr-3" />
                <input
                  ref={inputRef}
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="搜索知识、技能、代码片段..."
                  className={cn(
                    'flex-1 bg-transparent text-white text-sm',
                    'placeholder:text-zinc-500',
                    'focus:outline-none',
                    'font-medium tracking-tight'
                  )}
                />
                <Shortcut keys={['⌘', 'K']} />
              </div>

              {/* Results */}
              <div
                ref={listRef}
                className="max-h-[360px] overflow-y-auto overscroll-contain py-2 px-2"
              >
                {results.length > 0 ? (
                  <>
                    {/* Section label */}
                    <div className="px-3 py-1.5 text-[10px] font-semibold text-zinc-600 uppercase tracking-wider">
                      {query ? '搜索结果' : '最近使用'}
                    </div>

                    {/* Items */}
                    {results.map((item, index) => (
                      <ResultItem
                        key={item.id}
                        item={item}
                        isSelected={index === selectedIndex}
                        onSelect={() => {
                          onSelect?.(item);
                          onClose();
                        }}
                        onHover={() => setSelectedIndex(index)}
                      />
                    ))}
                  </>
                ) : (
                  /* Empty state */
                  <div className="py-12 text-center">
                    <div className="inline-flex items-center justify-center w-12 h-12 rounded-full bg-white/5 mb-3">
                      <Search className="w-5 h-5 text-zinc-600" />
                    </div>
                    <p className="text-sm text-zinc-500">
                      没有找到匹配的结果
                    </p>
                    <p className="mt-1 text-xs text-zinc-600">
                      尝试其他关键词
                    </p>
                  </div>
                )}
              </div>

              {/* Footer */}
              <div className="flex items-center justify-between px-4 py-2.5 border-t border-white/[0.06] bg-white/[0.02]">
                <div className="flex items-center gap-4 text-[10px] text-zinc-600">
                  <span className="flex items-center gap-1.5">
                    <Shortcut keys={['↑', '↓']} />
                    <span>导航</span>
                  </span>
                  <span className="flex items-center gap-1.5">
                    <Shortcut keys={['↵']} />
                    <span>选择</span>
                  </span>
                  <span className="flex items-center gap-1.5">
                    <Shortcut keys={['esc']} />
                    <span>关闭</span>
                  </span>
                </div>

                <button
                  onClick={() => {
                    // Navigate to full search
                  }}
                  className="flex items-center gap-1 text-[10px] text-zinc-500 hover:text-zinc-300 transition-colors"
                >
                  <span>高级搜索</span>
                  <ArrowRight className="w-3 h-3" />
                </button>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}

export default Spotlight;
