# Refine Cloud Server (MVP)

Refine 浏览器插件的云端服务最小实现（不依赖桌面端）。

## 启动

```bash
cd apps/server
bun install
bun run dev
```

默认监听 `http://localhost:8787`。

## 可选环境变量

- `PORT`：服务端口（默认 `8787`）
- `REFINE_API_TOKEN`：设置后要求 `Authorization: Bearer <token>`

## API

- `GET /health`
- `POST /v1/conversations`
- `POST /v1/extraction-jobs`
- `GET /v1/extraction-jobs/:id`
- `GET /v1/items?cursor=0&limit=20`
- `GET /v1/search?q=xxx&limit=20`

> 当前为内存存储，用于迁移联调。生产环境需替换为持久化存储与异步任务系统。
