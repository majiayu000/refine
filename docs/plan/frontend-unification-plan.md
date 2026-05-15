# Frontend Unification (Dashboard + Desktop UI) 执行计划

- 计划版本: v1
- 适用仓库: `/Users/lifcc/Desktop/code/AI/tools/refine`
- 执行模式: 每步改动 -> 立即测试 -> 回写计划 -> 下一步

## 0. 执行约束（DoR）

- 目标:
  - 用一套 React 前端代码同时承载:
    - 现有桌面 UI（知识浏览/搜索/详情/删除）
    - 现有 `/dashboard` 管理能力（Raw Inbox、漏斗、手动总结）
  - 消除 `apps/server/src/dashboard.html` 与 `apps/desktop/ui` 的并行实现。
- 兼容性: required（不能破坏现有 API 合约与桌面端基础能力）
- 提交策略: `per_step`
- 测试策略:
  - 步骤级: 每步至少 1 条定向测试 + 1 条健康检查
  - 最终: 运行 server + desktop 双路径验证
- 范围:
  - 包含: `apps/desktop/ui`, `apps/server`, `apps/desktop/src-tauri`（仅必要接缝）
  - 不包含: `apps/extension` 功能改造（只做兼容性回归）
- 基线:
  - 分支: `auto-optimize`
  - 工作区存在未提交改动（执行过程中不回滚）:
    - `.gitignore`
    - `.run/refine-server.log`
    - `apps/extension/src/popup.tsx`
    - `apps/server/README.md`
    - `apps/server/src/application/conversation.rs`
    - `apps/server/src/handlers.rs`
    - `apps/server/src/state.rs`

## 1. 分析结果（先于改动）

- 架构盘点摘要:
  - 模型/配置入口:
    - `apps/server/src/models.rs`
    - `apps/desktop/ui/src/lib/tauri.ts`
  - 工厂/注册表入口:
    - `apps/server/src/main.rs`（Axum 路由）
    - `apps/desktop/src-tauri/src/main.rs`（Tauri invoke + 本地 HTTP 启动）
  - 适配层入口:
    - `apps/server/src/handlers.rs`
    - `apps/desktop/ui/src/lib/store.ts`
  - 基础设施入口:
    - `apps/server/src/main.rs`（当前仅 API + 内嵌 HTML）
    - `apps/desktop/src-tauri/src/server/http.rs`（本地扩展专用 API）

### 1.1 重复/冗余候选

| id | 类别 | 文件与符号 | 证据 | 影响 | 风险 | 建议收敛方向 |
|----|------|------------|------|------|------|--------------|
| F1 | same-responsibility parallel implementations | `apps/server/src/dashboard.html` + `apps/desktop/ui/src/App.tsx` | 两套前端都承载知识列表/搜索展示，但技术栈和状态管理分离 | high | med | 统一到 `apps/desktop/ui` React，淘汰内嵌 HTML |
| F2 | duplicate adapter logic | `dashboard.html` `requestJson/authHeaders` + `apps/desktop/ui/src/lib/tauri.ts` | HTTP 调用、错误处理、token 管理重复 | high | med | 抽象统一 `ApiClient`（HttpAdapter/TauriAdapter） |
| F3 | multiple declaration sources for one entry | `apps/server/src/main.rs` `dashboard_page` + `include_str!` | server 页面发布路径与 desktop UI 构建链路独立漂移 | med | med | server 改为托管 React 构建产物 |
| F4 | capability asymmetry | `apps/server` 有 `/v1/conversations/events/extraction-jobs`，`apps/desktop/src-tauri/src/server/http.rs` 无对应能力 | 统一 UI 后在 Tauri 本地模式会出现功能缺口 | high | high | 先做能力探测 + 降级，再分阶段补齐后端能力 |
| F5 | auth behavior divergence | `dashboard.html` token 输入 + `apps/server/src/auth.rs` + Tauri invoke 无 token 流 | 用户路径与鉴权体验不一致 | med | low | 在统一 UI 中标准化 HTTP token 管理（仅 HTTP 模式） |

### 1.2 优先级评分

评分公式: `score = impact * confidence - (effort + risk)`

| id | finding | impact | effort | risk | confidence | score | phase |
|----|---------|--------|--------|------|------------|-------|-------|
| F1 | 前端实现重复 | 5 | 3 | 2 | 5 | 20 | P0 |
| F2 | API 适配重复 | 4 | 3 | 2 | 5 | 15 | P0 |
| F3 | 发布入口分裂 | 4 | 3 | 3 | 4 | 10 | P0 |
| F4 | 后端能力不对齐 | 5 | 5 | 4 | 5 | 16 | P1 |
| F5 | 鉴权体验分裂 | 3 | 2 | 2 | 4 | 8 | P1 |

## 2. 详细步骤（从分析映射而来）

### Step A1 统一 API 适配层与能力探测

- 状态: `completed`
- 目标:
  - 在 `apps/desktop/ui` 建立单一 API 抽象，替换分散的 `tauri.ts + dashboard inline fetch` 逻辑。
  - 暴露能力探测（是否支持 conversations/funnel/extraction-jobs）。
- 预计改动文件:
  - `apps/desktop/ui/src/lib/api/types.ts`（新增）
  - `apps/desktop/ui/src/lib/api/client.ts`（新增）
  - `apps/desktop/ui/src/lib/api/adapters/http.ts`（新增）
  - `apps/desktop/ui/src/lib/api/adapters/tauri.ts`（新增）
  - `apps/desktop/ui/src/lib/tauri.ts`（精简或兼容导出）
- 详细改动:
  - 定义统一接口: `listItems/search/delete/listConversations/getFunnelSummary/createExtractionJob/getQuota`。
  - 按 runtime 选择适配器:
    - Tauri: 优先 invoke
    - Web/Server: HTTP + `Authorization` token
  - 能力探测用于 UI 降级（防止 Tauri 本地模式调用不存在 API）。
- 步骤级测试命令:
  - `bun --cwd apps/desktop/ui run build`
  - `cargo check -p refine-desktop`
- 完成判定:
  - UI 不再直接依赖旧 `tauri.ts` fetch 细节。
  - 能在运行时拿到能力矩阵并用于后续页面渲染。

### Step A2 迁移 Dashboard 功能到 React Ops 页面

- 状态: `in_progress`
- 目标: 用 React 页面承载原 `dashboard.html` 的会话列表、漏斗、手动总结和 token 设置。
- 预计改动文件:
  - `apps/desktop/ui/src/pages/OpsDashboard.tsx`（新增）
  - `apps/desktop/ui/src/components/ops/ConversationList.tsx`（新增）
  - `apps/desktop/ui/src/components/ops/FunnelCards.tsx`（新增）
  - `apps/desktop/ui/src/lib/store.ts`（扩展状态）
  - `apps/desktop/ui/src/App.tsx`（增加模式切换）
- 详细改动:
  - 将 `dashboard.html` 的状态机迁移为 Zustand + React 组件。
  - `Knowledge` 与 `Ops` 两个视图在同一应用中切换。
  - 支持删除入口（沿用现有 ItemDetail 删除流）。
- 步骤级测试命令:
  - `bun --cwd apps/desktop/ui run build`
  - `cargo check -p refine-server`
- 完成判定:
  - `dashboard.html` 核心功能在 React UI 可用。
  - 旧 UI 与新 UI 数据输出一致（items/conversations/funnel）。

### Step A3 Server 托管统一前端构建产物

- 状态: `pending`
- 目标: `refine-server` 不再返回内嵌 HTML，改为托管 React dist（`/` 与 `/dashboard` 共用）。
- 预计改动文件:
  - `apps/server/Cargo.toml`（`tower-http` 增加 `fs`）
  - `apps/server/src/main.rs`
  - `apps/server/src/handlers.rs`
  - `apps/server/README.md`
- 详细改动:
  - 引入静态文件服务（`ServeDir` + SPA fallback）。
  - API 路由优先，其余页面路由落到前端入口。
  - 保留 `/dashboard` 路径兼容，内部由 React 控制页面模式。
- 步骤级测试命令:
  - `cargo check -p refine-server`
  - `cargo test -p refine-server`
- 完成判定:
  - `http://127.0.0.1:21567/` 与 `/dashboard` 都加载统一 React UI。
  - `/v1/*` 接口行为与原先一致。

### Step A4 清理旧实现与文档对齐

- 状态: `pending`
- 目标: 删除/废弃 `dashboard.html` 旧实现，避免双实现继续漂移。
- 预计改动文件:
  - `apps/server/src/dashboard.html`（删除或标记废弃）
  - `apps/server/src/handlers.rs`
  - `README.md`
  - `docs/11_API_SPEC.md`
- 详细改动:
  - 移除 `include_str!("dashboard.html")` 依赖。
  - 更新运行说明与发布清单。
- 步骤级测试命令:
  - `cargo check -p refine-server`
  - `bun --cwd apps/desktop/ui run build`
- 完成判定:
  - 仓库仅保留一套前端源代码。
  - 文档中不再描述旧内嵌页面实现。

### Step B1 Tauri 本地模式能力降级与提示

- 状态: `pending`
- 目标: 在 desktop 本地模式下，当缺失 ops API 时显示明确降级提示，不出现空白/报错。
- 预计改动文件:
  - `apps/desktop/ui/src/pages/OpsDashboard.tsx`
  - `apps/desktop/ui/src/lib/api/adapters/tauri.ts`
  - `apps/desktop/ui/src/lib/store.ts`
- 详细改动:
  - 基于 capability 判断展示:
    - 支持: 正常展示 Ops 模块
    - 不支持: 提示“当前运行模式未启用会话/漏斗能力”
  - 维持 knowledge 主链路可用。
- 步骤级测试命令:
  - `cargo check -p refine-desktop`
  - `cd apps/desktop/src-tauri && cargo tauri dev`（手工验证）
- 完成判定:
  - Tauri 本地模式不报错，行为可预期。

### Step B2（可选里程碑）Desktop/Server 后端能力对齐

- 状态: `pending`
- 目标: 让 desktop 本地后端具备 `conversations/events/extraction-jobs` 能力，实现真正全功能统一。
- 预计改动文件:
  - `apps/desktop/src-tauri/src/server/http.rs`
  - `apps/desktop/src-tauri/src/server/mod.rs`
  - `apps/desktop/src-tauri/src/app/*`（必要时）
  - `packages/core`（如需抽取共享应用层）
- 详细改动:
  - 优先避免复制 `apps/server/src/application` 逻辑，抽公共层复用。
  - 对齐状态机和响应契约，减少双端分叉。
- 步骤级测试命令:
  - `cargo check -p refine-desktop`
  - `cargo test -p refine-core`
- 完成判定:
  - desktop 本地模式下 Ops 模块功能完整可用。
  - server 与 desktop API 契约差异最小化。

## 3. 回归测试矩阵

- 阶段完成检查:
  - `bun --cwd apps/desktop/ui run build`
  - `cargo check -p refine-server`
  - `cargo check -p refine-desktop`
- 最终检查:
  - `cargo test -p refine-server`
  - `cargo test -p refine-core`
  - 手工回归:
    - Server 模式: 启动 `cargo run --package refine-server`，验证 `/dashboard`、`/v1/items`、`/v1/conversations`、`/v1/events/summary`
    - Desktop 模式: 启动 `cd apps/desktop/src-tauri && cargo tauri dev`，验证知识列表/搜索/删除，以及 Ops 降级提示
    - Extension 基础连通: 验证 `/health` 与 `/v1/conversations` 不回归

## 4. 风险与回滚

- 风险:
  - 静态资源托管路径配置错误导致 server 空白页
  - 统一 API 适配后，Tauri/HTTP 分支判断错误导致功能不可用
  - token 注入逻辑迁移导致 401 行为变化
- 回滚策略:
  - 保留 `dashboard.html` 分支直到 Step A3 验收通过
  - 每步 `per_step` 提交，异常时按步骤回退
  - 如出现线上阻塞，临时恢复 `handlers::dashboard_page -> include_str!` 兜底

## 5. 执行日志（每步完成后追加）

- 2026-02-24
  - Step A1: `completed`
    - 修改文件:
      - `apps/desktop/ui/src/lib/api/types.ts`
      - `apps/desktop/ui/src/lib/api/client.ts`
      - `apps/desktop/ui/src/lib/api/adapters/http.ts`
      - `apps/desktop/ui/src/lib/api/adapters/tauri.ts`
      - `apps/desktop/ui/src/lib/tauri.ts`
      - `apps/desktop/ui/src/lib/store.ts`
      - `apps/desktop/ui/src/components/Spotlight.tsx`
      - `apps/desktop/ui/src/components/ItemList.tsx`
      - `apps/desktop/ui/src/components/ItemDetail.tsx`
    - 主要改动:
      - 新增统一 API 抽象层（HTTP/Tauri 双 adapter）。
      - 在 store 中注入 `apiCapabilities`，可在运行时读取能力矩阵。
      - 现有 UI 调用从旧 `tauri.ts` 直接请求逻辑迁移到新 client。
      - 旧 `tauri.ts` 保留为兼容转发层，避免外部调用直接断裂。
    - 执行测试:
      - `cd apps/desktop/ui && bun run build` -> pass
      - `cargo check -p refine-desktop` -> pass
  - Step A2: `in_progress`
    - 当前状态: 待实现 React Ops 页面与状态迁移
