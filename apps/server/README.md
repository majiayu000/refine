# Refine Cloud Server (MVP)

Refine 浏览器插件的云端服务 Rust 实现（Axum + refine-core）。

## 启动

```bash
cargo run --package refine-server
```

默认监听 `http://127.0.0.1:21567`。首选端口被占用时，服务会检查已有实例的数据库身份、合同版本、鉴权方式和运行配置；只有这些条件匹配且授权探测成功时才复用已有实例并退出。其他情况继续尝试 `21567..21570` 中尚未尝试的端口。
服务端启动时会自动加载当前工作目录下的 `.env`。

例如可在仓库根目录创建 `.env`（或复制 `apps/server/.env.example`）：

```bash
REFINE_ANTHROPIC_API_KEY=your_key
REFINE_ANTHROPIC_BASE_URL=https://api.anthropic.com
REFINE_ANTHROPIC_MODEL=claude-opus-4-6
REFINE_API_TOKEN=replace_with_strong_random_token
```

生产环境建议直接使用 `apps/server/.env.production.example` 作为模板，默认启用鉴权：

```bash
cp apps/server/.env.production.example .env
```

## 环境变量

- `REFINE_SERVER_HOST`：监听地址（默认 `127.0.0.1`）
- `REFINE_SERVER_PORT`：首选服务端口（默认 `21567`，备用端口 `21568..21570`）
- `REFINE_ENV`：运行环境（`production` 时强制要求 `REFINE_API_TOKEN`）
- `REFINE_API_TOKEN`：鉴权令牌；设置后要求 `Authorization: Bearer <token>`
- `REFINE_DEV_ANON`：仅开发时显式启用匿名访问（`true/1/on`）；生产环境及非回环绑定仍要求 token
- `REFINE_TRUSTED_ORIGINS`：允许网页跨域访问的完整 origin，逗号分隔，默认空名单
- `REFINE_SERVER_DB_PATH`：SQLite 路径（默认 `$XDG_DATA_HOME/refine/refine.db`）
- `REFINE_ANTHROPIC_API_KEY` / `REFINE_OPENAI_API_KEY`：启用 LLM 提炼；未配置时仍可持久化采集，提炼需要可用的 LLM，未完成任务保留以便重试
- `REFINE_ANTHROPIC_MODEL`：Anthropic 模型名（默认 `claude-opus-4-6`）
- `REFINE_ANTHROPIC_BASE_URL`：Anthropic 兼容网关地址（默认 `https://api.anthropic.com`）
- `REFINE_ENABLE_FEATURE_HASH_SEARCH`：开启 FTS5 与哈希词项/字符特征混排（`true/1/on`，默认关闭）；旧 `REFINE_ENABLE_SEMANTIC_SEARCH` 是兼容别名，新名称优先。此实现不使用学习得到的语义 embedding
- `REFINE_MAX_ITEMS` / `REFINE_FREE_QUOTA_ITEMS`：条目上限（默认 `0` 不限制；设置为正数时启用限制）
- `REFINE_PREMIUM_USERS`：会员用户列表（逗号分隔，命中后忽略配额）；未设置时默认 `dev-user,token-user`（适配当前单用户自用场景）

示例（使用 Yunyi Claude 网关）：

```bash
export REFINE_ANTHROPIC_API_KEY=your_key
export REFINE_ANTHROPIC_BASE_URL=https://yunyi.cfd/claude
export REFINE_ANTHROPIC_MODEL=claude-opus-4-6
```

## 安全基线

- 当 `REFINE_ENV=production` 且未配置 `REFINE_API_TOKEN` 时，服务启动会直接失败。
- 当服务绑定到非回环地址（如 `0.0.0.0`、局域网 IP）且未配置 `REFINE_API_TOKEN` 时，服务启动会直接失败。
- 未授权请求统一返回 `401`，并提示使用 `Authorization: Bearer <token>`。

## API

- `GET /health`
- `POST /v1/conversations`
- `POST /v1/events`
- `GET /v1/events/summary?days=7`
- `POST /v1/extraction-jobs`
- `GET /v1/extraction-jobs/:id`
- `GET /v1/items?cursor=0&limit=20`
- `DELETE /v1/items/:item_id`
- `GET /v1/quota`
- `GET /v1/search?q=xxx&limit=20`
- `GET /v1/recommendations?q=xxx&limit=5`

## 存储说明

- `items` 由 `refine-core` 的 SQLite 存储持久化。
- `conversations`、`extraction_jobs`、`events` 已持久化到 SQLite（重启可恢复）。
- 运行时保留内存缓存用于快速读写，变更会回写数据库。

原生桌面通过同一个 `refine-server` library 提供 HTTP API，持久化回执、metadata、幂等键和 job ID 使用相同合同。中文子串检索、索引刷新、实例复用与固定语料评测的边界见 [检索与服务合同](../../docs/search-and-api-contracts.md)。

## 后期设计（暂缓）

- 当前仓库默认按单用户自用运行，`dev-user/token-user` 视作会员，不受配额限制。
- 多用户正式版再设计：会员体系、按用户计费/配额、后台管理页和精细化错误提示。
