# Refine Cloud Server (MVP)

Refine 浏览器插件的云端服务 Rust 实现（Axum + refine-core）。

## 启动

```bash
cargo run --package refine-server
```

默认监听 `http://localhost:8787`。
服务端启动时会自动加载当前工作目录下的 `.env`。

例如可在仓库根目录创建 `.env`（或复制 `apps/server/.env.example`）：

```bash
REFINE_ANTHROPIC_API_KEY=your_key
REFINE_ANTHROPIC_BASE_URL=https://yunyi.cfd/claude
REFINE_ANTHROPIC_MODEL=claude-opus-4-6
```

## 可选环境变量

- `HOST`：监听地址（默认 `0.0.0.0`）
- `PORT`：服务端口（默认 `8787`）
- `REFINE_API_TOKEN`：设置后要求 `Authorization: Bearer <token>`
- `REFINE_SERVER_DB_PATH`：SQLite 路径（默认 `$XDG_DATA_HOME/refine/server.db`）
- `REFINE_ANTHROPIC_API_KEY` / `REFINE_OPENAI_API_KEY`：启用 LLM 提炼（未配置时使用 fallback 提炼）
- `REFINE_ANTHROPIC_MODEL`：Anthropic 模型名（默认 `claude-opus-4-6`）
- `REFINE_ANTHROPIC_BASE_URL`：Anthropic 兼容网关地址（默认 `https://api.anthropic.com`）

示例（使用 Yunyi Claude 网关）：

```bash
export REFINE_ANTHROPIC_API_KEY=your_key
export REFINE_ANTHROPIC_BASE_URL=https://yunyi.cfd/claude
export REFINE_ANTHROPIC_MODEL=claude-opus-4-6
```

## API

- `GET /health`
- `POST /v1/conversations`
- `POST /v1/events`
- `GET /v1/events/summary?days=7`
- `POST /v1/extraction-jobs`
- `GET /v1/extraction-jobs/:id`
- `GET /v1/items?cursor=0&limit=20`
- `GET /v1/search?q=xxx&limit=20`
- `GET /v1/recommendations?q=xxx&limit=5`

## 存储说明

- `items` 由 `refine-core` 的 SQLite 存储持久化。
- `conversations` 与 `extraction_jobs` 当前在内存维护（进程重启后会重建）。
- 生产环境建议将会话与任务状态迁移到持久化存储，并接入独立任务队列。
