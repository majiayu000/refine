# Refine

> 从 AI 对话中提炼知识，在需要时主动推送

## 产品定位

**智能知识复用引擎** - 自动从与各种 AI 模型的对话中提炼可复用的知识片段和技能，下次遇到类似问题时主动推荐。

## 核心价值

- **不再重复问同样的问题** - 问过一次，永久可用
- **知识不再散落** - 跨平台对话统一管理
- **从被动搜索到主动推送** - 在你需要时自动出现

---

## 整体架构

```
┌─────────────────────────────────────────────────────────────────────┐
│                           采集层                                     │
├─────────────┬─────────────┬─────────────┬─────────────┬─────────────┤
│ 浏览器插件   │ 手动粘贴     │ API Hook    │ 文件导入     │ 剪贴板监听   │
│ (ChatGPT    │ (复制对话    │ (OpenAI/    │ (JSON/MD    │ (检测到对话  │
│  Claude等)  │  内容)       │  Claude)    │  导出)      │  格式自动问) │
└──────┬──────┴──────┬──────┴──────┬──────┴──────┬──────┴──────┬──────┘
       │             │             │             │             │
       └─────────────┴─────────────┼─────────────┴─────────────┘
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         处理层 (本地 LLM/API)                        │
├─────────────────────────────────────────────────────────────────────┤
│  1. 识别价值 → 判断这段对话是否值得保存                                │
│  2. 分类打标 → 自动识别领域（前端/后端/运维/设计...）                   │
│  3. 提炼转化 → 生成三种形态:                                          │
│     ┌──────────────┬──────────────┬──────────────┐                  │
│     │ 知识卡片      │ 可执行技能    │ 代码片段      │                  │
│     │ - 标题        │ - 名称        │ - 语言        │                  │
│     │ - 摘要        │ - 描述        │ - 代码        │                  │
│     │ - 关键词      │ - 参数模板    │ - 用途说明    │                  │
│     │ - 原文链接    │ - prompt模板  │ - 依赖/环境   │                  │
│     └──────────────┴──────────────┴──────────────┘                  │
│  4. 向量化 → 生成 embedding 存入本地向量库                            │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         存储层 (本地优先)                            │
├─────────────────────────────────────────────────────────────────────┤
│  SQLite + 向量索引 (sqlite-vss / faiss)                              │
│  ├── knowledge/          # 知识卡片                                  │
│  ├── skills/             # 可执行技能                                │
│  ├── snippets/           # 代码片段                                  │
│  ├── sources/            # 原始对话存档                              │
│  └── embeddings.db       # 向量索引                                  │
│                                                                      │
│  可选云同步: iCloud / Dropbox / 自建 WebDAV                          │
└─────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────┐
│                          消费层 (多端)                               │
├──────────────────┬──────────────────┬───────────────────────────────┤
│   桌面应用        │   浏览器插件       │   CLI                        │
│                  │                   │                              │
│ • 知识库浏览      │ • 侧边栏实时匹配   │ • refine search "关键词"     │
│ • 全局搜索        │ • 一键插入到对话   │ • refine run <skill>         │
│ • 悬浮窗快速访问  │ • 自动推荐弹窗     │ • refine add (从剪贴板)      │
│ • 技能管理/编辑   │                   │ • refine export              │
└──────────────────┴──────────────────┴───────────────────────────────┘
```

---

## 知识片段类型

### 1. 知识卡片 (Knowledge Card)

从对话中提炼的结构化知识点。

```yaml
type: knowledge
title: "Python asyncio vs threading 选择指南"
summary: |
  - CPU 密集型任务 → multiprocessing
  - IO 密集型任务 → asyncio (推荐) 或 threading
  - 混合场景 → asyncio + ProcessPoolExecutor
tags: ["python", "concurrency", "asyncio", "threading"]
source:
  platform: "claude"
  conversation_id: "xxx"
  created: "2024-01-15T10:30:00Z"
```

### 2. 可执行技能 (Executable Skill)

封装成可复用的 prompt 模板，带参数输入。

```yaml
type: skill
name: "代码审查专家"
description: "对代码进行安全性、性能、可读性审查"
tags: ["code-review", "security", "performance"]

parameters:
  - name: code
    type: text
    required: true
    description: "要审查的代码"
  - name: language
    type: select
    options: ["python", "javascript", "go", "rust", "typescript"]
  - name: focus
    type: multi-select
    options: ["security", "performance", "readability", "best-practices"]
    default: ["security", "best-practices"]

prompt_template: |
  请对以下 {{language}} 代码进行审查，重点关注: {{focus}}

  ```{{language}}
  {{code}}
  ```

  请从以下维度分析:
  1. 潜在问题
  2. 改进建议
  3. 示例修改

source:
  platform: "chatgpt"
  conversation_id: "xxx"
  created: "2024-01-15T10:30:00Z"
```

### 3. 代码片段 (Code Snippet)

带上下文说明的代码片段。

```yaml
type: snippet
title: "Python asyncio 并发请求示例"
language: python
description: "使用 asyncio + aiohttp 并发请求多个 URL"
tags: ["python", "asyncio", "aiohttp", "http"]

code: |
  import asyncio
  import aiohttp

  async def fetch(session, url):
      async with session.get(url) as response:
          return await response.json()

  async def fetch_all(urls):
      async with aiohttp.ClientSession() as session:
          tasks = [fetch(session, url) for url in urls]
          return await asyncio.gather(*tasks)

  # 使用
  urls = ["https://api.example.com/1", "https://api.example.com/2"]
  results = asyncio.run(fetch_all(urls))

dependencies:
  - aiohttp

source:
  platform: "claude"
  conversation_id: "xxx"
  created: "2024-01-15T10:30:00Z"
```

---

## 核心用户流程

### 流程 1: 采集入库

```
在 ChatGPT 问了一个 Python 并发的问题
        │
        ▼
浏览器插件检测到对话 → 弹出「是否保存？」
        │
        ▼ 点击保存
        │
后台自动提炼:
  ├─ 知识卡片: "Python asyncio vs threading 选择指南"
  ├─ 技能: "分析并发场景并推荐方案"
  └─ 代码片段: asyncio 示例代码
        │
        ▼
入库完成 → 显示「已保存 3 个片段」
```

### 流程 2: 自动推荐

```
在 Claude 输入: "我有一个 IO 密集型任务..."
        │
        ▼
浏览器插件实时监听输入
        │
        ▼
本地向量搜索匹配到相关知识
        │
        ▼
侧边栏浮现推荐:
  ┌────────────────────────────────┐
  │ 💡 相关知识                     │
  ├────────────────────────────────┤
  │ 📄 Python asyncio vs threading │
  │ ⚡ [执行] 并发方案分析技能       │
  │ 📋 [复制] asyncio 示例代码      │
  └────────────────────────────────┘
        │
        ▼ 点击「执行技能」
        │
弹出参数填写 → 生成完整 prompt → 粘贴到对话框
```

### 流程 3: 主动搜索

```
按下全局快捷键 (Cmd+Shift+K)
        │
        ▼
桌面悬浮窗弹出搜索框
        │
        ▼ 输入 "docker compose 网络"
        │
显示匹配结果:
  • 3 个知识卡片
  • 1 个可执行技能
  • 5 个代码片段
        │
        ▼ 选中一个
        │
复制到剪贴板 / 直接执行 / 查看详情
```

---

## 产品形态

### 桌面应用 (主体)

- 知识库管理界面
- 全局快捷键唤起搜索
- 悬浮窗快速访问
- 技能编辑器
- 设置与同步

### 浏览器插件

- 支持 ChatGPT、Claude、Gemini 等平台
- 侧边栏显示匹配知识
- 一键保存对话
- 一键插入到输入框

### CLI 工具

```bash
# 搜索知识
refine search "docker compose"

# 执行技能
refine run code-review --code "$(cat main.py)" --lang python

# 添加知识 (从剪贴板)
refine add --from-clipboard

# 导出
refine export --format markdown --output ./export
```

---

## 数据存储

### 本地优先

- 所有数据存储在本地
- SQLite 存储结构化数据
- sqlite-vss 或 faiss 存储向量索引
- 支持可选云同步 (iCloud/Dropbox/WebDAV)

### 目录结构

```
~/.refine/
├── config.yaml          # 配置文件
├── refine.db            # SQLite 主数据库
├── vectors.db           # 向量索引
├── sources/             # 原始对话存档
│   └── {id}.json
└── exports/             # 导出文件
```

---

## 与现有产品的差异

| 维度 | Mem.ai / Notion AI | Refine |
|------|-------------------|--------|
| 输入源 | 手动记录为主 | AI 对话自动采集 |
| 处理 | 原样存储 | 智能提炼 + 技能化 |
| 输出 | 被动搜索 | 主动推荐 + 可执行 |
| 场景 | 通用笔记 | AI 对话复用专用 |
| 存储 | 云端为主 | 本地优先 |

---

## 参考产品

- [Mem.ai](https://get.mem.ai/) - AI 驱动的笔记应用
- [Notæ](https://notae.app/) - AI 对话历史分析工具
- [Second Brain](https://www.thesecondbrain.io/) - 可视化知识管理
- [MemGPT](https://github.com/cpacker/MemGPT) - LLM 长期记忆框架
- [Dev2ndBrain](https://github.com/ewceniza9009/Dev2ndBrain) - 开发者知识管理
