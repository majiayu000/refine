# Refine - React/Tauri 前端规范

> 基于 Vercel 性能优化指南的最佳实践

---

## 1. 项目结构

```
apps/desktop/ui/
├── src/
│   ├── main.tsx              # 入口
│   ├── App.tsx               # 根组件
│   │
│   ├── components/           # 通用组件
│   │   ├── ui/               # 基础 UI 组件
│   │   │   ├── Button.tsx
│   │   │   ├── Input.tsx
│   │   │   ├── Card.tsx
│   │   │   └── index.ts
│   │   ├── layout/           # 布局组件
│   │   │   ├── Sidebar.tsx
│   │   │   ├── Header.tsx
│   │   │   └── index.ts
│   │   └── search/           # 搜索相关
│   │       ├── SearchInput.tsx
│   │       ├── SearchResults.tsx
│   │       └── index.ts
│   │
│   ├── features/             # 功能模块
│   │   ├── knowledge/        # 知识管理
│   │   │   ├── KnowledgeList.tsx
│   │   │   ├── KnowledgeDetail.tsx
│   │   │   └── hooks.ts
│   │   ├── skills/           # 技能管理
│   │   │   ├── SkillEditor.tsx
│   │   │   ├── SkillRunner.tsx
│   │   │   └── hooks.ts
│   │   └── spotlight/        # 全局搜索
│   │       ├── Spotlight.tsx
│   │       └── hooks.ts
│   │
│   ├── hooks/                # 通用 Hooks
│   │   ├── useSearch.ts
│   │   ├── useTauri.ts
│   │   └── useKeyboard.ts
│   │
│   ├── lib/                  # 工具函数
│   │   ├── tauri.ts          # Tauri 调用封装
│   │   └── utils.ts
│   │
│   ├── stores/               # 状态管理 (Zustand)
│   │   ├── useItemStore.ts
│   │   └── useUIStore.ts
│   │
│   └── styles/               # 样式
│       ├── tokens.css
│       └── globals.css
│
├── index.html
├── vite.config.ts
├── tailwind.config.ts
└── tsconfig.json
```

---

## 2. 性能优化 (按优先级)

### 2.1 消除请求瀑布 (CRITICAL)

#### 并行获取独立数据

```tsx
// ❌ BAD - 串行请求
async function loadDashboard() {
  const items = await fetchItems();     // 等待
  const tags = await fetchTags();       // 再等待
  const stats = await fetchStats();     // 还要等
  return { items, tags, stats };
}

// ✅ GOOD - 并行请求
async function loadDashboard() {
  const [items, tags, stats] = await Promise.all([
    fetchItems(),
    fetchTags(),
    fetchStats(),
  ]);
  return { items, tags, stats };
}
```

#### 尽早发起请求，延迟 await

```tsx
// ❌ BAD - 立即 await
async function SearchPage({ query }: { query: string }) {
  const results = await search(query);  // 阻塞
  const suggestions = await getSuggestions(query);  // 再阻塞

  return <Results data={results} suggestions={suggestions} />;
}

// ✅ GOOD - 尽早发起，延迟 await
async function SearchPage({ query }: { query: string }) {
  // 立即发起两个请求
  const resultsPromise = search(query);
  const suggestionsPromise = getSuggestions(query);

  // 按需 await
  const results = await resultsPromise;
  const suggestions = await suggestionsPromise;

  return <Results data={results} suggestions={suggestions} />;
}
```

### 2.2 Bundle 优化 (CRITICAL)

#### 避免桶文件导入

```tsx
// ❌ BAD - 从桶文件导入 (可能导入整个包)
import { Button } from '@/components/ui';

// ✅ GOOD - 直接导入
import { Button } from '@/components/ui/Button';
```

#### 动态导入重型组件

```tsx
import { lazy, Suspense } from 'react';

// ❌ BAD - 静态导入大型组件
import { SkillEditor } from '@/features/skills/SkillEditor';

// ✅ GOOD - 动态导入
const SkillEditor = lazy(() => import('@/features/skills/SkillEditor'));

function SkillPage() {
  return (
    <Suspense fallback={<SkillEditorSkeleton />}>
      <SkillEditor />
    </Suspense>
  );
}
```

#### 条件加载模块

```tsx
// ❌ BAD - 总是加载
import { analytics } from '@/lib/analytics';

function App() {
  useEffect(() => {
    if (settings.enableAnalytics) {
      analytics.track('app_open');
    }
  }, []);
}

// ✅ GOOD - 按需加载
function App() {
  useEffect(() => {
    if (settings.enableAnalytics) {
      import('@/lib/analytics').then(({ analytics }) => {
        analytics.track('app_open');
      });
    }
  }, []);
}
```

### 2.3 减少重渲染 (MEDIUM)

#### 不订阅回调中才用的状态

```tsx
// ❌ BAD - 订阅了只在回调中用的状态
function SearchInput() {
  const [query, setQuery] = useState('');
  const items = useItemStore(state => state.items);  // 触发重渲染

  const handleSearch = () => {
    const filtered = items.filter(i => i.title.includes(query));
    // ...
  };
}

// ✅ GOOD - 在回调中获取状态
function SearchInput() {
  const [query, setQuery] = useState('');

  const handleSearch = () => {
    const items = useItemStore.getState().items;  // 不触发重渲染
    const filtered = items.filter(i => i.title.includes(query));
    // ...
  };
}
```

#### 使用 useMemo 缓存计算

```tsx
// ❌ BAD - 每次渲染都计算
function KnowledgeList({ items, filter }: Props) {
  const filtered = items.filter(item =>
    item.tags.some(tag => filter.tags.includes(tag))
  );

  return <List items={filtered} />;
}

// ✅ GOOD - 缓存计算结果
function KnowledgeList({ items, filter }: Props) {
  const filtered = useMemo(() =>
    items.filter(item =>
      item.tags.some(tag => filter.tags.includes(tag))
    ),
    [items, filter.tags]
  );

  return <List items={filtered} />;
}
```

#### 使用基础类型作为依赖

```tsx
// ❌ BAD - 对象依赖导致每次都执行
function useSearch(options: { query: string; type: string }) {
  useEffect(() => {
    search(options);
  }, [options]);  // options 每次都是新对象
}

// ✅ GOOD - 基础类型依赖
function useSearch(options: { query: string; type: string }) {
  const { query, type } = options;
  useEffect(() => {
    search({ query, type });
  }, [query, type]);  // 只在值变化时执行
}
```

#### 使用函数式 setState

```tsx
// ❌ BAD - 依赖当前状态
function Counter() {
  const [count, setCount] = useState(0);

  const increment = useCallback(() => {
    setCount(count + 1);  // 需要 count 作为依赖
  }, [count]);
}

// ✅ GOOD - 函数式更新
function Counter() {
  const [count, setCount] = useState(0);

  const increment = useCallback(() => {
    setCount(c => c + 1);  // 不需要依赖
  }, []);
}
```

#### useState 延迟初始化

```tsx
// ❌ BAD - 每次渲染都调用
function Editor() {
  const [state, setState] = useState(parseLocalStorage());  // 每次都解析
}

// ✅ GOOD - 只初始化一次
function Editor() {
  const [state, setState] = useState(() => parseLocalStorage());  // 传函数
}
```

---

## 3. Tauri 集成

### 3.1 封装 Tauri 调用

```tsx
// src/lib/tauri.ts
import { invoke } from '@tauri-apps/api/core';

export interface Item {
  id: string;
  itemType: 'knowledge' | 'skill' | 'snippet';
  title: string;
  content: string;
  tags: string[];
  createdAt: string;
}

export interface SearchResult {
  items: Item[];
  total: number;
}

// 类型安全的 Tauri 调用
export const tauri = {
  // 知识管理
  async getItems(): Promise<Item[]> {
    return invoke('get_items');
  },

  async getItem(id: string): Promise<Item> {
    return invoke('get_item', { id });
  },

  async createItem(item: Omit<Item, 'id' | 'createdAt'>): Promise<Item> {
    return invoke('create_item', { item });
  },

  async updateItem(id: string, updates: Partial<Item>): Promise<Item> {
    return invoke('update_item', { id, updates });
  },

  async deleteItem(id: string): Promise<void> {
    return invoke('delete_item', { id });
  },

  // 搜索
  async search(query: string, options?: SearchOptions): Promise<SearchResult> {
    return invoke('search', { query, options });
  },

  // 提炼
  async extractKnowledge(content: string): Promise<ExtractedKnowledge> {
    return invoke('extract_knowledge', { content });
  },
};
```

### 3.2 React Query 集成

```tsx
// src/hooks/useItems.ts
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { tauri, Item } from '@/lib/tauri';

export function useItems() {
  return useQuery({
    queryKey: ['items'],
    queryFn: tauri.getItems,
  });
}

export function useItem(id: string) {
  return useQuery({
    queryKey: ['items', id],
    queryFn: () => tauri.getItem(id),
    enabled: !!id,
  });
}

export function useCreateItem() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: tauri.createItem,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['items'] });
    },
  });
}

export function useDeleteItem() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: tauri.deleteItem,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['items'] });
    },
  });
}
```

### 3.3 搜索 Hook

```tsx
// src/hooks/useSearch.ts
import { useState, useDeferredValue } from 'react';
import { useQuery } from '@tanstack/react-query';
import { tauri } from '@/lib/tauri';

export function useSearch() {
  const [query, setQuery] = useState('');
  const deferredQuery = useDeferredValue(query);

  const { data: results, isLoading } = useQuery({
    queryKey: ['search', deferredQuery],
    queryFn: () => tauri.search(deferredQuery),
    enabled: deferredQuery.length > 0,
    staleTime: 1000 * 60,  // 1 分钟缓存
  });

  return {
    query,
    setQuery,
    results: results?.items ?? [],
    isLoading,
    isStale: query !== deferredQuery,
  };
}
```

---

## 4. 状态管理 (Zustand)

### 4.1 Store 定义

```tsx
// src/stores/useItemStore.ts
import { create } from 'zustand';
import { Item } from '@/lib/tauri';

interface ItemState {
  items: Item[];
  selectedId: string | null;
  filter: {
    type: string | null;
    tags: string[];
  };
}

interface ItemActions {
  setItems: (items: Item[]) => void;
  selectItem: (id: string | null) => void;
  setFilter: (filter: Partial<ItemState['filter']>) => void;
}

export const useItemStore = create<ItemState & ItemActions>((set) => ({
  // State
  items: [],
  selectedId: null,
  filter: {
    type: null,
    tags: [],
  },

  // Actions
  setItems: (items) => set({ items }),
  selectItem: (id) => set({ selectedId: id }),
  setFilter: (filter) => set((state) => ({
    filter: { ...state.filter, ...filter },
  })),
}));
```

### 4.2 派生状态选择器

```tsx
// src/stores/useItemStore.ts (续)
import { shallow } from 'zustand/shallow';

// 派生选择器 - 只在结果变化时触发重渲染
export const useFilteredItems = () => useItemStore((state) => {
  const { items, filter } = state;

  return items.filter(item => {
    if (filter.type && item.itemType !== filter.type) return false;
    if (filter.tags.length > 0 && !filter.tags.some(t => item.tags.includes(t))) return false;
    return true;
  });
}, shallow);

export const useSelectedItem = () => useItemStore((state) => {
  if (!state.selectedId) return null;
  return state.items.find(i => i.id === state.selectedId) ?? null;
});
```

### 4.3 UI 状态

```tsx
// src/stores/useUIStore.ts
import { create } from 'zustand';

interface UIState {
  spotlightOpen: boolean;
  sidebarCollapsed: boolean;
  theme: 'light' | 'dark' | 'system';
}

interface UIActions {
  toggleSpotlight: () => void;
  toggleSidebar: () => void;
  setTheme: (theme: UIState['theme']) => void;
}

export const useUIStore = create<UIState & UIActions>((set) => ({
  spotlightOpen: false,
  sidebarCollapsed: false,
  theme: 'dark',

  toggleSpotlight: () => set((state) => ({ spotlightOpen: !state.spotlightOpen })),
  toggleSidebar: () => set((state) => ({ sidebarCollapsed: !state.sidebarCollapsed })),
  setTheme: (theme) => set({ theme }),
}));
```

---

## 5. 键盘快捷键

### 5.1 全局快捷键 Hook

```tsx
// src/hooks/useKeyboard.ts
import { useEffect } from 'react';

type KeyHandler = (e: KeyboardEvent) => void;

interface Shortcut {
  key: string;
  ctrl?: boolean;
  meta?: boolean;
  shift?: boolean;
  handler: KeyHandler;
}

export function useKeyboard(shortcuts: Shortcut[]) {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      for (const shortcut of shortcuts) {
        const keyMatch = e.key.toLowerCase() === shortcut.key.toLowerCase();
        const ctrlMatch = shortcut.ctrl ? e.ctrlKey : true;
        const metaMatch = shortcut.meta ? e.metaKey : true;
        const shiftMatch = shortcut.shift ? e.shiftKey : !e.shiftKey;

        if (keyMatch && ctrlMatch && metaMatch && shiftMatch) {
          e.preventDefault();
          shortcut.handler(e);
          return;
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [shortcuts]);
}
```

### 5.2 使用示例

```tsx
// src/App.tsx
import { useKeyboard } from '@/hooks/useKeyboard';
import { useUIStore } from '@/stores/useUIStore';

function App() {
  const toggleSpotlight = useUIStore((s) => s.toggleSpotlight);

  useKeyboard([
    {
      key: 'k',
      meta: true,
      shift: true,
      handler: () => toggleSpotlight(),
    },
    {
      key: ',',
      meta: true,
      handler: () => navigate('/settings'),
    },
    {
      key: 'Escape',
      handler: () => {
        // 关闭任何打开的弹窗
      },
    },
  ]);

  return <RouterProvider router={router} />;
}
```

---

## 6. 组件规范

### 6.1 组件文件结构

```tsx
// components/ui/Button.tsx

// 1. 导入
import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

// 2. 变体定义
const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 disabled:pointer-events-none disabled:opacity-50',
  {
    variants: {
      variant: {
        default: 'bg-primary-500 text-white hover:bg-primary-400',
        secondary: 'bg-gray-700 text-white hover:bg-gray-600',
        ghost: 'hover:bg-gray-700',
        danger: 'bg-red-500 text-white hover:bg-red-400',
      },
      size: {
        sm: 'h-8 px-3 text-sm',
        md: 'h-9 px-4 text-sm',
        lg: 'h-10 px-5 text-base',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'md',
    },
  }
);

// 3. Props 类型
export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

// 4. 组件实现
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => {
    return (
      <button
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  }
);

Button.displayName = 'Button';
```

### 6.2 提取静态 JSX

```tsx
// ❌ BAD - 每次渲染都创建新的 JSX
function Card({ title, content }: Props) {
  return (
    <div className="card">
      <div className="card-decoration" />  {/* 静态元素 */}
      <h3>{title}</h3>
      <p>{content}</p>
    </div>
  );
}

// ✅ GOOD - 提取静态部分
const CardDecoration = <div className="card-decoration" />;

function Card({ title, content }: Props) {
  return (
    <div className="card">
      {CardDecoration}
      <h3>{title}</h3>
      <p>{content}</p>
    </div>
  );
}
```

### 6.3 条件渲染用三元

```tsx
// ❌ BAD - && 可能渲染 0 或 false
function List({ items }: { items: Item[] }) {
  return (
    <div>
      {items.length && <ItemList items={items} />}
    </div>
  );
}

// ✅ GOOD - 三元表达式
function List({ items }: { items: Item[] }) {
  return (
    <div>
      {items.length > 0 ? <ItemList items={items} /> : null}
    </div>
  );
}
```

---

## 7. 依赖配置

### 7.1 package.json

```json
{
  "dependencies": {
    "react": "^18.2.0",
    "react-dom": "^18.2.0",
    "@tauri-apps/api": "^2.0.0",
    "@tanstack/react-query": "^5.0.0",
    "zustand": "^4.5.0",
    "react-router-dom": "^6.22.0",
    "class-variance-authority": "^0.7.0",
    "clsx": "^2.1.0",
    "tailwind-merge": "^2.2.0",
    "lucide-react": "^0.344.0"
  },
  "devDependencies": {
    "@types/react": "^18.2.0",
    "@types/react-dom": "^18.2.0",
    "@vitejs/plugin-react": "^4.2.0",
    "typescript": "^5.3.0",
    "vite": "^5.1.0",
    "tailwindcss": "^3.4.0",
    "autoprefixer": "^10.4.0",
    "postcss": "^8.4.0"
  }
}
```

### 7.2 vite.config.ts

```ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'path';

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // Tauri 配置
  clearScreen: false,
  server: {
    port: 7788,
    strictPort: true,
  },
  build: {
    target: 'esnext',
    minify: 'esbuild',
  },
});
```

---

## 8. 常见反模式

| 反模式 | 正确做法 |
|-------|---------|
| 从桶文件导入 | 直接导入具体文件 |
| 串行请求 | Promise.all 并行 |
| 组件内定义组件 | 提取到外部 |
| 对象作为依赖 | 解构为基础类型 |
| 在渲染中订阅 store | 用选择器或 getState |
| 静态导入重型库 | lazy + Suspense |
| && 条件渲染 | 三元表达式 |
| 每次都计算派生值 | useMemo 缓存 |
