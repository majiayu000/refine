# Refine - UI 设计系统

> 简洁、专业、高效的知识管理应用设计规范

---

## 1. 设计原则

```
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  1. 快速 (Fast)                                             │
│     • 所有交互响应 < 100ms                                   │
│     • 视觉反馈即时                                          │
│     • 减少不必要的动画                                       │
│                                                             │
│  2. 安静 (Quiet)                                            │
│     • 不打扰用户工作流                                       │
│     • 推荐可关闭/最小化                                      │
│     • 低饱和度配色                                          │
│                                                             │
│  3. 清晰 (Clear)                                            │
│     • 信息层级分明                                          │
│     • 操作可预期                                            │
│     • 状态一目了然                                          │
│                                                             │
│  4. 专业 (Professional)                                     │
│     • 面向开发者和知识工作者                                 │
│     • 代码友好的字体                                         │
│     • 深色模式优先                                          │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 设计 Tokens

### 2.1 颜色系统

#### 品牌色 (Primary)

```css
/* 主色调 - 冷静的蓝紫色，代表知识和智慧 */
--color-primary-50:  #f0f4ff;
--color-primary-100: #e0e8ff;
--color-primary-200: #c7d4fe;
--color-primary-300: #a4b8fc;
--color-primary-400: #8194f8;
--color-primary-500: #6366f1;  /* 主色 */
--color-primary-600: #5046e5;
--color-primary-700: #4338ca;
--color-primary-800: #3730a3;
--color-primary-900: #312e81;
```

#### 中性色 (Neutral)

```css
/* 深色主题 - 默认 */
--color-gray-50:  #fafafa;
--color-gray-100: #f4f4f5;
--color-gray-200: #e4e4e7;
--color-gray-300: #d4d4d8;
--color-gray-400: #a1a1aa;
--color-gray-500: #71717a;
--color-gray-600: #52525b;
--color-gray-700: #3f3f46;
--color-gray-800: #27272a;
--color-gray-900: #18181b;
--color-gray-950: #09090b;
```

#### 语义色 (Semantic)

```css
/* 成功 */
--color-success-500: #22c55e;
--color-success-600: #16a34a;

/* 警告 */
--color-warning-500: #f59e0b;
--color-warning-600: #d97706;

/* 错误 */
--color-error-500: #ef4444;
--color-error-600: #dc2626;

/* 信息 */
--color-info-500: #3b82f6;
--color-info-600: #2563eb;
```

#### 知识片段类型色

```css
/* 知识卡片 - 蓝色 */
--color-knowledge: #3b82f6;

/* 可执行技能 - 紫色 */
--color-skill: #8b5cf6;

/* 代码片段 - 绿色 */
--color-snippet: #22c55e;
```

### 2.2 深色主题 (默认)

```css
:root {
  /* 背景 */
  --bg-primary: #09090b;      /* 主背景 */
  --bg-secondary: #18181b;    /* 卡片/面板背景 */
  --bg-tertiary: #27272a;     /* 输入框/悬停背景 */
  --bg-elevated: #3f3f46;     /* 弹窗/下拉背景 */

  /* 文字 */
  --text-primary: #fafafa;    /* 主文字 */
  --text-secondary: #a1a1aa;  /* 次要文字 */
  --text-tertiary: #71717a;   /* 辅助文字 */
  --text-disabled: #52525b;   /* 禁用文字 */

  /* 边框 */
  --border-default: #27272a;
  --border-hover: #3f3f46;
  --border-focus: #6366f1;
}
```

### 2.3 浅色主题

```css
[data-theme="light"] {
  /* 背景 */
  --bg-primary: #ffffff;
  --bg-secondary: #fafafa;
  --bg-tertiary: #f4f4f5;
  --bg-elevated: #ffffff;

  /* 文字 */
  --text-primary: #18181b;
  --text-secondary: #52525b;
  --text-tertiary: #71717a;
  --text-disabled: #a1a1aa;

  /* 边框 */
  --border-default: #e4e4e7;
  --border-hover: #d4d4d8;
  --border-focus: #6366f1;
}
```

### 2.4 字体系统

```css
/* 字体家族 */
--font-sans: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
--font-mono: "JetBrains Mono", "Fira Code", "SF Mono", Consolas, monospace;

/* 字体大小 - Modular Scale (1.25) */
--text-xs:   0.75rem;   /* 12px */
--text-sm:   0.875rem;  /* 14px */
--text-base: 1rem;      /* 16px */
--text-lg:   1.125rem;  /* 18px */
--text-xl:   1.25rem;   /* 20px */
--text-2xl:  1.5rem;    /* 24px */
--text-3xl:  1.875rem;  /* 30px */

/* 行高 */
--leading-tight: 1.25;
--leading-normal: 1.5;
--leading-relaxed: 1.75;

/* 字重 */
--font-normal: 400;
--font-medium: 500;
--font-semibold: 600;
--font-bold: 700;
```

### 2.5 间距系统 (8pt Grid)

```css
--space-0:  0;
--space-1:  0.25rem;  /* 4px */
--space-2:  0.5rem;   /* 8px */
--space-3:  0.75rem;  /* 12px */
--space-4:  1rem;     /* 16px */
--space-5:  1.25rem;  /* 20px */
--space-6:  1.5rem;   /* 24px */
--space-8:  2rem;     /* 32px */
--space-10: 2.5rem;   /* 40px */
--space-12: 3rem;     /* 48px */
--space-16: 4rem;     /* 64px */
```

### 2.6 圆角

```css
--radius-sm:   0.25rem;  /* 4px */
--radius-md:   0.375rem; /* 6px */
--radius-lg:   0.5rem;   /* 8px */
--radius-xl:   0.75rem;  /* 12px */
--radius-2xl:  1rem;     /* 16px */
--radius-full: 9999px;
```

### 2.7 阴影

```css
/* 深色主题阴影 - 更subtle */
--shadow-sm:  0 1px 2px rgba(0, 0, 0, 0.3);
--shadow-md:  0 4px 6px rgba(0, 0, 0, 0.4);
--shadow-lg:  0 10px 15px rgba(0, 0, 0, 0.5);
--shadow-xl:  0 20px 25px rgba(0, 0, 0, 0.6);

/* 悬浮窗专用 */
--shadow-floating: 0 16px 48px rgba(0, 0, 0, 0.6),
                   0 0 0 1px rgba(255, 255, 255, 0.05);
```

### 2.8 动画

```css
/* 时长 */
--duration-fast:   100ms;
--duration-normal: 200ms;
--duration-slow:   300ms;

/* 缓动函数 */
--ease-default: cubic-bezier(0.4, 0, 0.2, 1);
--ease-in:      cubic-bezier(0.4, 0, 1, 1);
--ease-out:     cubic-bezier(0, 0, 0.2, 1);
--ease-bounce:  cubic-bezier(0.34, 1.56, 0.64, 1);
```

---

## 3. 组件规范

### 3.1 按钮 (Button)

```
┌─────────────────────────────────────────────────────────────┐
│  变体                                                        │
│  ─────                                                      │
│  Primary   [████████]  主要操作                              │
│  Secondary [▒▒▒▒▒▒▒▒]  次要操作                              │
│  Ghost     [        ]  辅助操作                              │
│  Danger    [████████]  危险操作                              │
│                                                             │
│  尺寸                                                        │
│  ─────                                                      │
│  sm: height 32px, padding 0 12px, text-sm                   │
│  md: height 36px, padding 0 16px, text-sm (默认)            │
│  lg: height 40px, padding 0 20px, text-base                 │
│                                                             │
│  状态                                                        │
│  ─────                                                      │
│  default → hover (+亮度) → active (-亮度) → disabled (50%透明)│
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**CSS 示例:**

```css
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  font-weight: var(--font-medium);
  border-radius: var(--radius-md);
  transition: all var(--duration-fast) var(--ease-default);
  cursor: pointer;
}

.btn-primary {
  background: var(--color-primary-500);
  color: white;
}

.btn-primary:hover {
  background: var(--color-primary-400);
}

.btn-primary:active {
  background: var(--color-primary-600);
}
```

### 3.2 输入框 (Input)

```
┌─────────────────────────────────────────────────────────────┐
│  结构                                                        │
│  ─────                                                      │
│  ┌──────────────────────────────────────┐                   │
│  │ 🔍  placeholder text              ⌘K │                   │
│  └──────────────────────────────────────┘                   │
│    ↑                                  ↑                     │
│  前缀图标                          后缀/快捷键               │
│                                                             │
│  尺寸                                                        │
│  ─────                                                      │
│  sm: height 32px                                            │
│  md: height 36px (默认)                                     │
│  lg: height 40px                                            │
│                                                             │
│  状态                                                        │
│  ─────                                                      │
│  default → focus (primary border + glow) → error (red)      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 3.3 卡片 (Card)

```
┌─────────────────────────────────────────────────────────────┐
│  知识卡片                                                    │
│  ─────────                                                  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 📄  Python asyncio 最佳实践                     12分钟前 │  │
│  │                                                      │  │
│  │  CPU密集用multiprocessing，IO密集用asyncio...        │  │
│  │                                                      │  │
│  │  #python  #asyncio  #threading                       │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  技能卡片                                                    │
│  ─────────                                                  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ ⚡  代码审查专家                              [执行]  │  │
│  │                                                      │  │
│  │  对代码进行安全性、性能、可读性审查                    │  │
│  │                                                      │  │
│  │  参数: code, language, focus                         │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  代码片段卡片                                                │
│  ─────────────                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 📋  asyncio 并发请求示例                   python [复制]│  │
│  │  ┌────────────────────────────────────────────────┐  │  │
│  │  │ async def fetch_all(urls):                     │  │  │
│  │  │     async with aiohttp.ClientSession() as s:   │  │  │
│  │  │         ...                                    │  │  │
│  │  └────────────────────────────────────────────────┘  │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 3.4 搜索结果项 (Search Result Item)

```
┌─────────────────────────────────────────────────────────────┐
│  紧凑模式 (悬浮窗)                                           │
│  ─────────────────                                          │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 📄  Python asyncio 最佳实践                      ⏎   │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 📋  asyncio 示例代码                            ⌘C   │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
│  扩展模式 (主界面)                                           │
│  ─────────────────                                          │
│  ┌──────────────────────────────────────────────────────┐  │
│  │ 📄  Python asyncio 最佳实践                          │  │
│  │     CPU密集用multiprocessing，IO密集用asyncio...      │  │
│  │     #python #asyncio                       — 12分钟前 │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 3.5 标签 (Tag)

```
┌─────────────────────────────────────────────────────────────┐
│  样式                                                        │
│  ─────                                                      │
│  ┌─────────┐  背景: var(--bg-tertiary)                      │
│  │ #python │  文字: var(--text-secondary)                   │
│  └─────────┘  圆角: var(--radius-full)                      │
│               padding: 2px 8px                              │
│               font-size: var(--text-xs)                     │
│                                                             │
│  类型标签 (带颜色)                                            │
│  ─────────────────                                          │
│  ┌────────┐  知识: #3b82f6 (蓝)                             │
│  │ 📄 知识 │                                                │
│  └────────┘                                                 │
│  ┌────────┐  技能: #8b5cf6 (紫)                             │
│  │ ⚡ 技能 │                                                │
│  └────────┘                                                 │
│  ┌────────┐  代码: #22c55e (绿)                             │
│  │ 📋 代码 │                                                │
│  └────────┘                                                 │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 4. 布局规范

### 4.1 主界面布局

```
┌─────────────────────────────────────────────────────────────┐
│  Refine                                      [搜索]  ⚙️     │  ← 顶栏 48px
├──────────────┬──────────────────────────────────────────────┤
│              │                                              │
│  📚 全部      │  ┌────────────────────────────────────────┐ │
│  📄 知识  (12)│  │                                        │ │
│  ⚡ 技能  (5) │  │         内容区域                        │ │
│  📋 代码  (8) │  │                                        │ │
│              │  │         - 知识列表                      │ │
│  ─────────── │  │         - 详情面板                      │ │
│  标签        │  │         - 技能编辑器                    │ │
│  #python     │  │                                        │ │
│  #react      │  └────────────────────────────────────────┘ │
│  #docker     │                                              │
│              │                                              │
│  ← 侧边栏    │  ← 主内容区                                   │
│    200px     │    flex: 1                                   │
└──────────────┴──────────────────────────────────────────────┘
```

### 4.2 悬浮搜索窗

```
┌─────────────────────────────────────────┐
│  🔍  搜索知识...                    ⌘K  │  ← 输入框 44px
├─────────────────────────────────────────┤
│                                         │
│  📄  Python asyncio 最佳实践       ⏎    │  ← 结果项 40px
│  📋  asyncio 示例代码              ⌘C   │
│  ⚡  并发方案分析                  ⌘⏎   │
│                                         │
│  ─────────────────────────────────────  │
│  最近使用                               │  ← 分组标题 28px
│  📄  Docker Compose 网络配置            │
│                                         │
└─────────────────────────────────────────┘

尺寸: 600px × auto (最大 400px)
位置: 屏幕中央偏上 (top: 20%)
```

### 4.3 浏览器插件侧边栏

```
┌────────────────────────┐
│  Refine           [×]  │  ← 标题栏 36px
├────────────────────────┤
│  💡 相关知识            │  ← 分组标题
├────────────────────────┤
│  📄 asyncio 最佳实践    │
│     [查看] [复制]       │  ← 结果项 60px
├────────────────────────┤
│  📋 示例代码            │
│     [复制]             │
├────────────────────────┤
│  ⚡ 并发分析技能        │
│     [执行]             │
└────────────────────────┘

宽度: 280px
位置: 页面右侧
```

---

## 5. 图标系统

### 5.1 图标库

使用 **Lucide Icons** (开源、一致、轻量)

### 5.2 核心图标

```
导航
─────
Home        🏠  home
Search      🔍  search
Settings    ⚙️  settings

类型
─────
Knowledge   📄  file-text
Skill       ⚡  zap
Snippet     📋  code

操作
─────
Add         ➕  plus
Edit        ✏️  pencil
Delete      🗑️  trash-2
Copy        📋  copy
Execute     ▶️  play
Save        💾  save

状态
─────
Success     ✓  check
Error       ✗  x
Warning     ⚠  alert-triangle
Info        ℹ  info
```

### 5.3 图标尺寸

```css
--icon-sm: 16px;  /* 列表、标签 */
--icon-md: 20px;  /* 按钮、输入框 (默认) */
--icon-lg: 24px;  /* 标题、空状态 */
```

---

## 6. 响应式设计

### 6.1 断点 (桌面应用)

```css
/* Tauri 桌面应用窗口尺寸 */
--bp-compact: 800px;   /* 紧凑模式 */
--bp-normal: 1200px;   /* 正常模式 */
--bp-wide: 1600px;     /* 宽屏模式 */
```

### 6.2 布局适配

```
紧凑模式 (< 800px)
─────────────────
• 侧边栏收起为图标
• 隐藏次要信息
• 单栏布局

正常模式 (800px - 1200px)
────────────────────────
• 侧边栏展开
• 双栏布局 (列表 + 详情)

宽屏模式 (> 1200px)
──────────────────
• 三栏布局
• 更多信息展示
```

---

## 7. 无障碍设计

### 7.1 键盘导航

```
全局快捷键
──────────
Cmd+Shift+K    唤起搜索
Cmd+N          新建知识
Cmd+,          打开设置
Esc            关闭弹窗

搜索框内
────────
↑/↓            选择结果
Enter          打开/执行
Cmd+C          复制内容
Cmd+Enter      执行技能
```

### 7.2 焦点管理

```css
/* 焦点样式 */
:focus-visible {
  outline: 2px solid var(--color-primary-500);
  outline-offset: 2px;
}

/* 焦点顺序 */
/* 弹窗打开时 focus 到第一个输入框 */
/* Esc 关闭后 focus 返回触发元素 */
```

### 7.3 颜色对比度

- 正文文字对比度 ≥ 4.5:1
- 大字 (18px+) 对比度 ≥ 3:1
- 交互元素对比度 ≥ 3:1

---

## 8. 设计资源导出

### 8.1 CSS 变量文件

```css
/* tokens.css */
:root {
  /* Colors */
  --color-primary-500: #6366f1;
  /* ... 完整 tokens */
}
```

### 8.2 Tailwind 配置

```js
// tailwind.config.js
module.exports = {
  theme: {
    extend: {
      colors: {
        primary: {
          500: '#6366f1',
          // ...
        }
      },
      fontFamily: {
        sans: ['Inter', 'sans-serif'],
        mono: ['JetBrains Mono', 'monospace'],
      }
    }
  }
}
```

### 8.3 组件 Checklist

- [ ] Button (Primary, Secondary, Ghost, Danger)
- [ ] Input (Text, Search)
- [ ] Card (Knowledge, Skill, Snippet)
- [ ] Tag
- [ ] Modal
- [ ] Dropdown
- [ ] Toast
- [ ] Tooltip
- [ ] Sidebar
- [ ] SearchResult
