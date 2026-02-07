# Refine Architecture V2 (Proposed)

> 目标：在不拆微服务的前提下，做到“强边界、弱耦合、可演进”。

## 1. Scope

本方案适用于当前 monorepo：

- `/Users/lifcc/Desktop/code/AI/tools/refine/packages/core`
- `/Users/lifcc/Desktop/code/AI/tools/refine/apps/server`
- `/Users/lifcc/Desktop/code/AI/tools/refine/apps/desktop/src-tauri`
- `/Users/lifcc/Desktop/code/AI/tools/refine/apps/cli`
- `/Users/lifcc/Desktop/code/AI/tools/refine/apps/extension`

架构风格：**Modular Monolith + Hexagonal (Ports & Adapters) + Clean Dependency Rule**。

## 2. Why Change

当前主要问题（有代码证据）：

1. 业务流程重复实现（云端和桌面本地各一套提炼流程）
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/server/src/extraction.rs`
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/desktop/src-tauri/src/server/extract.rs`
2. LLM 客户端装配逻辑重复
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/server/src/state.rs`
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/desktop/src-tauri/src/server/extract.rs`
3. Transport 层承担过多业务编排
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/server/src/handlers.rs`
   - `/Users/lifcc/Desktop/code/AI/tools/refine/apps/desktop/src-tauri/src/server/http.rs`

## 3. Target Architecture

### 3.1 Layer Model

```text
Adapters (HTTP / Tauri / CLI / Extension Client)
  -> Application (Use Cases)
    -> Domain (Entities / Policies / Value Objects)
      <- Ports (Repository / LLM / Job / Event)
        <- Adapters (SQLite / Anthropic / OpenAI / Gemini / Grok)
```

依赖规则：

- 外层可以依赖内层；内层不能依赖外层。
- Domain 不依赖 Axum/Tauri/Reqwest/SQLite。
- Application 只依赖 `ports` trait，不直接 new 具体客户端。

### 3.2 Bounded Contexts

建议按业务边界拆分，而不是按技术名词拆分：

1. `capture`：对话采集、去重、入队。
2. `extraction`：提炼任务、重试、降级策略。
3. `knowledge`：Item 聚合、标签、来源、生命周期。
4. `retrieval`：关键词/语义检索、排序、分页。
5. `delivery`：HTTP/Tauri/CLI 输出与契约。

### 3.3 Workspace Layout (V2)

```text
refine/
  packages/
    domain/                 # 纯领域（无 IO）
    application/            # 用例编排
    ports/                  # 入/出站接口
    adapters/               # SQLite + LLM providers + scheduler
    contracts/              # API DTO / OpenAPI / TS client
  apps/
    server/                 # Axum adapter only
    desktop/src-tauri/      # Tauri + local HTTP adapter only
    cli/                    # CLI adapter only
    extension/              # 使用 contracts client
```

> 过渡期可不立刻拆 crate。先在 `packages/core` 内引入 `application`、`ports`、`adapters` 子模块，稳定后再拆分成多 crate。

## 4. LLM Decoupling (Claude/Gemini/Grok/OpenAI)

### 4.1 Unified Port

定义统一端口（示意）：

```rust
#[async_trait]
pub trait LlmGateway: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
}
```

### 4.2 Provider Adapters

- `AnthropicAdapter`
- `OpenAIAdapter`
- `GeminiAdapter`
- `GrokAdapter`
- `RouterAdapter`（按模型路由、fallback、超时和重试）

业务用例只依赖 `LlmGateway`，这样 provider 可替换、可复用、可灰度。

## 5. Shared Use Cases (Eliminate Duplication)

抽离到 application 层的核心用例：

1. `CreateConversation`
2. `StartExtractionJob`
3. `RunExtraction`（含 JSON repair/fallback）
4. `GetJobStatus`
5. `ListItems`
6. `SearchItems`

效果：

- `apps/server` 和 `apps/desktop` 复用同一套用例代码。
- handlers 只做参数校验和 DTO 映射。

## 6. Data and State Strategy

### 6.1 Repositories

新增端口：

- `ConversationRepository`
- `JobRepository`
- `ItemRepository`（已有）

SQLite 实现统一放 adapters，内存实现仅用于测试。

### 6.2 Job Runtime

统一 `JobScheduler` 端口，避免各入口自行 `tokio::spawn` 编排。

建议能力：

- 并发上限
- 取消与超时
- 幂等处理
- 可观测状态（pending/running/succeeded/failed）

## 7. API Contract Strategy

1. `packages/contracts` 作为单一契约源。
2. server 与 desktop local HTTP 共享同一 DTO/错误码语义。
3. extension 通过生成 client 调用，减少接口漂移。

## 8. Migration Plan (Low Risk)

### Phase 1: Extract Use Cases (no API break)

1. 把 `/apps/server/src/extraction.rs` 迁到 core application。
2. `/apps/desktop/src-tauri/src/server/extract.rs` 改为调用同一用例。
3. 保持现有 HTTP 路由不变。

Done when:

- 云端和桌面提炼结果一致（同输入同输出）。
- 重复提炼逻辑删除至少一份。

### Phase 2: Port-ify Persistence

1. 引入 `ConversationRepository` / `JobRepository` trait。
2. 将 server persistence 和 desktop persistence 适配到同一端口。

Done when:

- handlers 不直接依赖具体 persistence struct。

### Phase 3: LLM Factory + Router

1. 提取统一 `LlmGateway` 和 provider factory。
2. server/desktop 使用同一装配入口。
3. 加入 provider fallback 策略（可配置）。

Done when:

- `apps/server/src/state.rs` 与 desktop LLM 构建重复逻辑消失。

### Phase 4: Transport Slimming

1. handlers/http 仅做 DTO <-> command。
2. 错误码映射集中化。

Done when:

- transport 文件不再承载业务规则分支。

### Phase 5: Contracts Unification

1. 统一 API schema。
2. extension 改为契约客户端。

Done when:

- extension 不再手写和后端耦合的字段解析分支。

## 9. Non-goals

- 不在此阶段拆微服务。
- 不在此阶段引入事件总线中间件（Kafka/NATS）。
- 不在此阶段改动用户可见 API 路径。

## 10. External References

- Hexagonal Architecture (Alistair Cockburn): https://alistair.cockburn.us/hexagonal-architecture
- Clean Architecture (Dependency Rule): https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html
- Bounded Context (Martin Fowler): https://martinfowler.com/bliki/BoundedContext.html
- Cargo Workspaces (Rust/Cargo): https://doc.rust-lang.org/cargo/reference/workspaces.html
- Axum shared state pattern (`State`, `FromRef`): https://docs.rs/axum/latest/axum/extract/struct.State.html
