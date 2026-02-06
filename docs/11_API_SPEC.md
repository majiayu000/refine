# Refine - API 规格

> Tauri 命令和 HTTP API 规格

---

## 1. Tauri 命令 (IPC)

桌面应用前端通过 `@tauri-apps/api` 调用后端命令。

### 1.1 get_items

获取知识条目列表。

```typescript
invoke('get_items', {
  item_type?: 'knowledge' | 'skill' | 'snippet',
  limit?: number  // 默认 50
}): Promise<ItemDto[]>
```

**返回**：
```typescript
interface ItemDto {
  id: string
  item_type: string
  title: string
  summary: string
  content: string
  tags: string[]
  created_at: string  // ISO 8601
}
```

---

### 1.2 get_item

获取单个知识条目。

```typescript
invoke('get_item', {
  id: string
}): Promise<ItemDto | null>
```

---

### 1.3 search_items

搜索知识条目。

```typescript
invoke('search_items', {
  query: string,
  limit?: number  // 默认 20
}): Promise<SearchResultDto>
```

**返回**：
```typescript
interface SearchResultDto {
  items: ItemDto[]
  total: number
}
```

---

### 1.4 create_item

创建新知识条目。

```typescript
invoke('create_item', {
  title: string,
  summary: string,
  content: string,
  item_type?: 'knowledge' | 'skill' | 'snippet',
  tags?: string[]
}): Promise<ItemDto>
```

---

### 1.5 update_item

更新知识条目。

```typescript
invoke('update_item', {
  id: string,
  title?: string,
  summary?: string,
  content?: string
}): Promise<ItemDto>
```

---

### 1.6 delete_item

删除知识条目。

```typescript
invoke('delete_item', {
  id: string
}): Promise<boolean>
```

---

## 2. HTTP API

桌面应用提供本地 HTTP API，供浏览器扩展调用。

**基础 URL**: `http://localhost:19527`

### 2.1 健康检查

```http
GET /health
```

**响应**：
```json
{
  "success": true,
  "message": "Refine is running"
}
```

---

### 2.2 提取对话

```http
POST /extract
Content-Type: application/json
X-Refine-Client: extension

{
  "content": "对话内容文本",
  "url": "https://claude.ai/chat/xxx",
  "source": "claude"
}
```

**响应**：
```json
{
  "success": true,
  "ids": [
    "550e8400-e29b-41d4-a716-446655440000"
  ]
}
```

**错误响应**：
```json
{
  "success": false,
  "message": "错误描述"
}
```

说明：
- `/extract` 仅接受浏览器扩展来源请求（`chrome-extension://` / `moz-extension://`）
- 请求头必须包含 `X-Refine-Client: extension`
- 桌面端需配置 LLM Key（`REFINE_ANTHROPIC_API_KEY` 或 `REFINE_OPENAI_API_KEY`）

---

## 3. CLI 命令

### 3.1 extract

从对话中提炼知识。

```bash
refine extract --stdin
echo "对话内容" | refine extract --stdin
```

---

### 3.2 search

搜索知识。

```bash
refine search <QUERY> [-l, --limit <N>]
```

**示例**：
```bash
refine search "rust error handling" --limit 5
```

---

### 3.3 list

列出知识条目。

```bash
refine list [-t, --type <TYPE>] [-l, --limit <N>]
```

**示例**：
```bash
refine list --type skill --limit 10
```

---

### 3.4 show

显示知识详情。

```bash
refine show <ID>
```

---

### 3.5 delete

删除知识。

```bash
refine delete <ID>
```

---

### 3.6 add

手动添加知识。

```bash
refine add --title <TITLE> --summary <SUMMARY> [--type <TYPE>]
```

**示例**：
```bash
refine add --title "Git 技巧" --summary "常用 Git 命令" --type knowledge
```

---

## 4. 错误码

| 错误 | 说明 | HTTP 状态 |
|------|------|----------|
| `NotFound` | 资源不存在 | 404 |
| `InvalidInput` | 参数无效 | 400 |
| `DatabaseError` | 数据库错误 | 500 |
| `SerializationError` | 序列化错误 | 500 |

---

## 5. 前端 TypeScript API

### 5.1 Tauri 封装

```typescript
// lib/tauri.ts
import { invoke } from '@tauri-apps/api/core'

export interface Item {
  id: string
  item_type: 'knowledge' | 'skill' | 'snippet'
  title: string
  summary: string
  content: string
  tags: string[]
  created_at: string
}

export interface SearchResult {
  items: Item[]
  total: number
}

export async function getItems(
  item_type?: string,
  limit?: number
): Promise<Item[]> {
  return invoke('get_items', { item_type, limit })
}

export async function getItem(id: string): Promise<Item | null> {
  return invoke('get_item', { id })
}

export async function searchItems(
  query: string,
  limit?: number
): Promise<SearchResult> {
  return invoke('search_items', { query, limit })
}

export async function createItem(
  title: string,
  summary: string,
  content: string,
  item_type?: string,
  tags?: string[]
): Promise<Item> {
  return invoke('create_item', { title, summary, content, item_type, tags })
}

export async function deleteItem(id: string): Promise<boolean> {
  return invoke('delete_item', { id })
}
```

---

### 5.2 扩展 API 客户端

```typescript
// lib/api.ts
const API_BASE = 'http://localhost:19527'

export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/health`)
    const data = await res.json()
    return data.success
  } catch {
    return false
  }
}

export async function sendToDesktop(
  content: string,
  url: string,
  source: string
): Promise<{ success: boolean; id?: string; message?: string }> {
  const res = await fetch(`${API_BASE}/extract`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content, url, source }),
  })
  return res.json()
}
```

---

## 6. CORS 配置

HTTP API 支持跨域请求：

```
Access-Control-Allow-Origin: chrome-extension://<extension-id> 或 moz-extension://<extension-id>
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Content-Type, X-Refine-Client
```
