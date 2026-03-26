# Full Refactor (No Compatibility) 执行计划

- 计划版本: v1
- 适用仓库: /Users/lifcc/Desktop/code/AI/tools/refine
- 执行模式: 每步改动 -> 立即测试 -> 回写计划 -> 下一步

## 0. 执行约束（DoR）

- 目标: 完成 server/desktop/extension 关键链路的无兼容重构，消除双状态源、兼容分支和配置碎片
- 兼容性: not required（明确不做向后兼容）
- 提交策略: final_only（本轮不拆分提交）
- 测试策略:
  - 步骤级: 每步至少执行 1 条定向命令（`cargo check -p refine-server` / `cargo check -p refine-desktop` / `pnpm typecheck`）
  - 最终: `cargo test --workspace` + 前端 typecheck

## 1. 分析结果（先于改动）

- 架构盘点摘要:
  - API 入口: `apps/server/src/main.rs`, `apps/desktop/src-tauri/src/server/http.rs`
  - 提炼编排: `apps/server/src/extraction.rs`, `apps/desktop/src-tauri/src/server/extract.rs`
  - 状态存储: `apps/server/src/state.rs`, `apps/server/src/persistence.rs`
  - 契约/配置: `packages/core/src/infra/contract.rs`, `apps/extension/src/lib/config.ts`, `apps/desktop/ui/src/lib/api/adapters/http.ts`

| id | 类别 | 文件与符号 | 证据 | 影响 | 风险 | 建议收敛方向 |
|----|------|------------|------|------|------|--------------|
| F1 | single-source violation | `apps/server/src/state.rs::RuntimeState` | 同时持有 DB 仓储与内存 conversations/jobs/idempotency | high | high | 删除 RuntimeState，改为 repository 单一事实源 |
| F2 | duplicated flow | `apps/server/src/extraction.rs` + `apps/desktop/src-tauri/src/server/extract.rs` | 双端并行维护提炼+保存流程 | high | medium | 提炼严格模式收敛到 core 用例函数，双端调用同一条 |
| F3 | silent degradation | `packages/core/src/refinement/usecase.rs::extract_items_or_fallback` | 失败降级仍产出成功结果 | high | medium | server/desktop 改为 strict 提炼（失败即失败） |
| F4 | deprecated path | `apps/desktop/src-tauri/src/server/http.rs::/extract` + in-memory idempotency | 旧路径和内存去重并存 | medium | low | 删除 `/extract` 与内存 idempotency |
| F5 | config fragmentation | `apps/server/src/main.rs`, `apps/desktop/src-tauri/src/server/mod.rs`, `apps/extension/src/lib/config.ts`, `apps/desktop/ui/src/lib/api/adapters/http.ts` | 多端硬编码 8787 与默认基址 | high | low | 统一到新默认端口并移除旧默认值 |
| F6 | legacy compatibility code | `apps/extension/src/lib/api.ts::fetchCloudTotalItemsWithOptions` | 老接口 fallback 分支 | medium | low | 删除 legacy fallback，仅保留新契约 |

评分（impact/confidence - effort/risk）:

| id | impact | effort | risk | confidence | score | phase |
|----|--------|--------|------|------------|-------|-------|
| F1 | 5 | 4 | 4 | 5 | 17 | P0 |
| F2 | 5 | 3 | 3 | 4 | 14 | P0 |
| F3 | 5 | 2 | 3 | 5 | 20 | P0 |
| F4 | 3 | 2 | 2 | 5 | 11 | P1 |
| F5 | 4 | 2 | 2 | 5 | 16 | P1 |
| F6 | 3 | 1 | 1 | 5 | 13 | P1 |

## 2. 详细步骤（从分析映射而来）

### Step A1 移除 server 内存 RuntimeState（单一事实源）

- 状态: `completed`
- 目标: conversations/jobs/idempotency 全部改为 repository 查询，不再保留内存副本
- 预计改动文件:
  - `apps/server/src/state.rs`
  - `apps/server/src/application/ports.rs`
  - `apps/server/src/persistence.rs`
  - `apps/server/src/application/conversation.rs`
  - `apps/server/src/application/job.rs`
  - `apps/server/src/application/query.rs`
  - `apps/server/src/extraction.rs`
- 详细改动:
  - 扩展 repository 端口（按 id / idempotency / 分页查询）
  - 删除 RuntimeState 字段与初始化逻辑
  - 改写应用层读写路径，避免 state.runtime 访问
- 步骤级测试命令:
  - `cargo check -p refine-server`
- 完成判定:
  - `rg -n "state\\.runtime|RuntimeState" apps/server/src` 无业务引用
  - `cargo check -p refine-server` 通过

### Step A2 提炼链路改为 strict 模式并双端收敛

- 状态: `completed`
- 目标: 不再 fallback 成功；server/desktop 使用同一 strict 提炼入口
- 预计改动文件:
  - `packages/core/src/refinement/usecase.rs`
  - `packages/core/src/refinement/mod.rs`
  - `apps/server/src/extraction.rs`
  - `apps/desktop/src-tauri/src/server/extract.rs`
- 步骤级测试命令:
  - `cargo check -p refine-core`
  - `cargo check -p refine-server`
  - `cargo check -p refine-desktop`

### Step A3 删除桌面本地 API 兼容路径

- 状态: `completed`
- 目标: 移除 `/extract` 和内存 idempotency，统一只走 `/v1/conversations`
- 预计改动文件:
  - `apps/desktop/src-tauri/src/server/http.rs`
- 步骤级测试命令:
  - `cargo check -p refine-desktop`

### Step A4 统一默认端口与 API 基址

- 状态: `completed`
- 目标: 替换多端 8787 默认值，统一为新端口（server=5567, desktop local=5568）
- 预计改动文件:
  - `apps/server/src/main.rs`
  - `apps/desktop/src-tauri/src/server/mod.rs`
  - `apps/extension/src/lib/config.ts`
  - `apps/extension/package.json`
  - `apps/desktop/ui/src/lib/api/adapters/http.ts`
- 步骤级测试命令:
  - `cargo check -p refine-server`
  - `cargo check -p refine-desktop`
  - `cd apps/extension && pnpm typecheck`
  - `cd apps/desktop/ui && pnpm build`

### Step A5 删除 extension legacy fallback 分支

- 状态: `completed`
- 目标: 删除旧接口兜底扫描逻辑，保持单一新契约实现
- 预计改动文件:
  - `apps/extension/src/lib/api.ts`
- 步骤级测试命令:
  - `cd apps/extension && pnpm typecheck`

### Step A6 全量回归与计划收尾

- 状态: `completed`
- 目标: 完成 workspace 验证并回写执行日志
- 步骤级测试命令:
  - `cargo test --workspace`
  - `cd apps/extension && pnpm typecheck`
  - `cd apps/desktop/ui && pnpm build`

## 3. 回归测试矩阵

- 阶段完成检查:
  - `cargo check -p refine-server`
  - `cargo check -p refine-desktop`
  - `cargo check -p refine-core`
- 最终检查:
  - `cargo test --workspace`
  - `cd apps/extension && pnpm typecheck`
  - `cd apps/desktop/ui && pnpm build`

## 4. 执行日志（每步完成后追加）

- 2026-03-26
  - Step A1: `completed`
    - 修改文件:
      - `apps/server/src/application/ports.rs`
      - `apps/server/src/persistence.rs`
      - `apps/server/src/state.rs`
      - `apps/server/src/application/conversation.rs`
      - `apps/server/src/application/job.rs`
      - `apps/server/src/application/query.rs`
      - `apps/server/src/extraction.rs`
      - `apps/server/src/handlers.rs`
    - 主要改动:
      - 删除 `RuntimeState` 与 `state.runtime` 全路径读写
      - repository 端口扩展为按 id/idempotency/分页查询
      - 应用层改为 repository 单一事实源
      - 修复文档列表计数中的静默吞错
    - 执行测试:
      - `cargo check -p refine-server` -> pass
  - Step A2: `in_progress`
    - 修改文件:
      - `packages/core/src/refinement/usecase.rs`
      - `packages/core/src/refinement/mod.rs`
      - `apps/server/src/extraction.rs`
      - `apps/desktop/src-tauri/src/server/extract.rs`
      - `apps/server/src/main.rs`
    - 主要改动:
      - 新增 `extract_items_with_strict_defaults`（失败直接 error）
      - server/desktop 提炼改为 strict 模式，LLM 缺失直接失败
      - 清理“fallback extraction”运行时文案
    - 执行测试:
      - `cargo check -p refine-core -p refine-server -p refine-desktop` -> pass
  - Step A3: `completed`
    - 修改文件:
      - `apps/desktop/src-tauri/src/server/http.rs`
      - `apps/desktop/src-tauri/src/server/extract.rs`
      - `apps/desktop/src-tauri/src/server/mod.rs`
    - 主要改动:
      - 删除 `/extract` 路由
      - 删除 in-memory idempotency 缓存逻辑
      - 保留 `/v1/conversations` 作为唯一入队路径
    - 执行测试:
      - `cargo check -p refine-desktop`（由联合 check 覆盖） -> pass
  - Step A4: `completed`
    - 修改文件:
      - `apps/server/src/main.rs`
      - `apps/desktop/src-tauri/src/server/mod.rs`
      - `apps/extension/src/lib/config.ts`
      - `apps/extension/package.json`
      - `apps/desktop/ui/src/lib/api/adapters/http.ts`
    - 主要改动:
      - 默认端口改为 server `5567`、desktop local `5568`
      - extension/UI 默认 API 基址切到 `5567`
    - 执行测试:
      - `cargo check -p refine-server -p refine-desktop` -> pass
      - `cd apps/desktop/ui && pnpm build` -> pass
  - Step A5: `completed`
    - 修改文件:
      - `apps/extension/src/lib/api.ts`
      - `apps/extension/src/background/index.ts`
    - 主要改动:
      - 删除 `fetchCloudTotalItemsWithOptions` 及 legacy fallback 扫描
      - background 改为调用严格契约版本 `fetchCloudTotalItems`
    - 执行测试:
      - `cd apps/extension && pnpm typecheck` -> pass
  - Step A6: `completed`
    - 修改文件:
      - `docs/plan/full-refactor-no-compat-2026-03-26.md`
    - 主要改动:
      - 完成全量回归并收尾
    - 执行测试:
      - `cargo test --workspace` -> pass（含 server/core/desktop/cli/mirror）
      - `cd apps/extension && pnpm typecheck` -> pass
      - `cd apps/desktop/ui && pnpm build` -> pass
