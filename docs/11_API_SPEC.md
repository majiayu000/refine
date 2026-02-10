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

## 2. Unified HTTP API

浏览器扩展调用统一 `v1` HTTP 协议：
- 桌面本地模式：`http://localhost:8787`（由 desktop 内置服务提供）
- 独立服务模式：`http://localhost:8787` 或部署域名（由 `refine-server` 提供）

**基础 URL（开发）**: `http://localhost:8787`  
**基础 URL（生产）**: `https://api.refine.so`（示例）

扩展会附带请求头：`X-Refine-Client: extension`。

### 2.1 健康检查

```http
GET /health
```

**响应**：
```json
{
  "success": true,
  "message": "Refine cloud API (Rust) is running"
}
```

---

### 2.2 创建会话（扩展上传）

```http
POST /v1/conversations
Content-Type: application/json
X-Refine-Client: extension
Authorization: Bearer <token>  // 可选，服务端开启鉴权时需要

{
  "content": "对话内容文本",
  "url": "https://claude.ai/chat/xxx",
  "source": "claude",
  "title": "Claude Conversation",
  "captured_at": "2026-02-06T12:00:00.000Z",
  "idempotency_key": "uuid"
}
```

**响应**：
```json
{
  "success": true,
  "conversation_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "queued",
  "job_id": "9cc2d6f9-b8c7-4ed2-a401-5b2d2ab6d4fc"
}
```

---

### 2.3 创建提炼任务

```http
POST /v1/extraction-jobs
Content-Type: application/json

{
  "conversation_id": "550e8400-e29b-41d4-a716-446655440000",
  "mode": "auto"
}
```

**响应**：
```json
{
  "success": true,
  "job_id": "9cc2d6f9-b8c7-4ed2-a401-5b2d2ab6d4fc",
  "status": "succeeded"
}
```

---

### 2.4 查询提炼任务状态

```http
GET /v1/extraction-jobs/:job_id
```

---

### 2.5 列出知识条目

```http
GET /v1/items?cursor=0&limit=20
```

**响应**：
```json
{
  "success": true,
  "items": [],
  "total": 128,
  "next_cursor": 20
}
```

---

### 2.6 删除知识条目

```http
DELETE /v1/items/:item_id
```

**响应**：
```json
{
  "success": true,
  "deleted": true,
  "id": "item-id"
}
```

---

### 2.7 搜索知识

```http
GET /v1/search?q=rust&limit=20
```

---

### 2.8 上报事件

```http
POST /v1/events
Content-Type: application/json
X-Refine-Client: extension

{
  "event_name": "conversation_extracted",
  "source": "chatgpt",
  "properties": {
    "content_length": 1024
  },
  "occurred_at": "2026-02-10T12:00:00.000Z"
}
```

---

### 2.9 查询漏斗汇总

```http
GET /v1/events/summary?days=7
```

**响应**：
```json
{
  "success": true,
  "days": 7,
  "since": "2026-02-03T10:00:00Z",
  "counts": {
    "conversation_extracted": 120,
    "conversation_synced": 118,
    "recommendation_exposed": 0,
    "recommendation_clicked": 0,
    "knowledge_reused": 0
  }
}
```

---

### 2.10 推荐候选（输入态调用）

```http
GET /v1/recommendations?q=如何做 Rust 鉴权中间件&limit=5
```

**响应**：
```json
{
  "success": true,
  "triggered": true,
  "query": "如何做 Rust 鉴权中间件",
  "total": 3,
  "items": [
    {
      "id": "item-id",
      "item_type": "knowledge",
      "title": "Axum 鉴权中间件实践",
      "summary": "使用 tower layer 组织鉴权链路",
      "content": "可直接复用的完整实现片段...",
      "tags": ["rust", "axum", "auth"],
      "score": 1.0,
      "reason": "semantic_match"
    }
  ],
  "meta": {
    "latency_ms": 12,
    "strategy": "semantic_vector"
  }
}
```

说明：
- 当 `q` 长度小于 10 字符时，返回 `triggered=false`，避免高频无效请求。
- 扩展侧默认 `300ms` 输入去抖、`1.5s` 请求超时；超时或服务不可达时自动静默隐藏面板。
- 推荐面板支持 `复制` 与 `插入输入框`，并按站点记忆“推荐开关”状态。
- 推荐策略由服务端配置决定：默认 `keyword_search`，开启 `REFINE_ENABLE_SEMANTIC_SEARCH` 后为 `semantic_vector`。

---

说明：
- `idempotency_key` 用于去重，避免重复上传生成重复记录。
- 生产环境建议启用 `Authorization: Bearer` 并绑定用户身份。
- Claude 提炼可通过 `REFINE_ANTHROPIC_MODEL` 指定模型（默认 `claude-opus-4-6`），并通过 `REFINE_ANTHROPIC_BASE_URL` 对接 Anthropic 兼容网关。
- 可通过 `REFINE_ENABLE_SEMANTIC_SEARCH=true` 开启语义向量检索。
- 当前 `apps/server` 使用 Rust + Axum；`items`、`conversations`、`extraction_jobs`、`events` 已持久化到 SQLite。
- 生产建议接入更细粒度鉴权（用户级身份）与独立异步任务队列。

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
const API_BASE = process.env.PLASMO_PUBLIC_REFINE_API_BASE || 'http://localhost:8787'

export async function checkCloudHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/health`)
    const data = await res.json()
    return data.success
  } catch {
    return false
  }
}

export async function uploadConversation(
  content: string,
  url: string,
  source: string
): Promise<{ success: boolean; conversation_id?: string; message?: string }> {
  const res = await fetch(`${API_BASE}/v1/conversations`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      content,
      url,
      source,
      captured_at: new Date().toISOString(),
      idempotency_key: crypto.randomUUID(),
    }),
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
Access-Control-Allow-Headers: Content-Type, Authorization, X-Refine-Client
```
