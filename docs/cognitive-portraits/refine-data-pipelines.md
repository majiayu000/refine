# refine 数据管道与产出链路

> 梳理日期：2026-04-09；链路 9 于 2026-08-28 更新为 v4
> 研究范围：refine Rust workspace (`apps/cli`, `apps/mirror`, `packages/core`) + 相关 launchd 任务 + Claude Code skill
> 研究方法：只读研究，不执行任何命令

---

## 摘要

共识别 **9 条产出链路**，其中 **6 条调 LLM**、**5 条写 refine.db**、其余写本地文件或 stdout：

- 1 条数据流入口（`refine ingest-sessions`，auto/remem/local provider → refine.db）
- 1 条核心 LLM 聚合链路（`refine insights --prescription`，~10+1 次 LLM 并发）
- 4 条 mirror 子命令（`score` / `motd` / `dashboard` / `weekly` / `profile`）
- 2 条由 launchd 调度的 shell 脚本（daily/weekly）
- 1 条 in-repo skill（`cognitive-portrait`，通过版本化 collector 间接读取同快照 event-time cohort）

**核心结论**：除 `ingest-sessions` 外，所有链路都是对已经落库的 Observation 数据做聚合加工。`refine insights --prescription` 是 LLM 调用最重、耗时最长的链路（~11 次 LLM，10 并发）；`mirror score` 在原始职责"纯 SQL 聚合"之外，会先为当前 score/cohort 生成确定性 advice，并可选调用一次 LLM 做结构化 policy 确认。

---

## 链路全景图

```
auto/remem: remem raw archive；local: filesystem scan
 (完整 user/assistant 消息)
       │
       │ parse + LLM facet extract (3 并发 × 最多 5 次重试)
       ▼
┌──────────────────────────────────────────────────────────────┐
│ 链路 1: refine ingest-sessions                               │
│   → refine.db: documents + items(type=observation)           │
└──────────────────────────────────────────────────────────────┘
       │                                   ▲
       │ (item_type='observation' rows)    │
       │                                   │ daily-refresh.sh
       ▼                                   │ (08:00 每天)
┌──────────────────────────────────────────────────────────────┐
│         cluster_observations()  纯 Rust 聚合                │
│           (packages/core/src/session)                        │
└──────────────────────────────────────────────────────────────┘
       │                 │                 │              │
       ▼                 ▼                 ▼              ▼
┌─────────────┐  ┌─────────────┐   ┌─────────────┐  ┌─────────────┐
│ 链路 2:     │  │ 链路 3:     │   │ 链路 4:     │  │ 链路 5:     │
│ insights    │  │ mirror      │   │ mirror      │  │ mirror      │
│ --prescrip. │  │ score       │   │ dashboard   │  │ weekly      │
│             │  │             │   │             │  │             │
│ 10+1 次 LLM │  │ 1 次 LLM    │   │ 0 LLM       │  │ 1 次 LLM    │
│ (10 并发)   │  │ (advice)    │   │             │  │             │
└──────┬──────┘  └──────┬──────┘   └─────────────┘  └──────┬──────┘
       │                │                                  │
       ▼                ▼                                  ▼
  documents        scores.jsonl                      documents
  (session-        + statusline.txt                  (mirror-weekly)
   insights-v2)    + advice.json                     + last-weekly.md
                                                     + weekly-history.jsonl
                                                            │
                                                            ▼
                                                      ┌─────────────┐
                                                      │ 链路 6:     │
                                                      │ mirror motd │
                                                      │ (读缓存)    │
                                                      │ 0 LLM       │
                                                      └─────────────┘

┌─────────────┐  ┌─────────────┐
│ 链路 7:     │  │ 链路 8:     │
│ mirror      │  │ weekly-     │
│ profile     │  │ insights.sh │
│ 1 次 LLM    │  │ (launchd)   │
│             │  │ = 链路1+链路2 │
└──────┬──────┘  └─────────────┘
       ▼
  documents
  (mirror-profile)
  + profile-summary.json

┌─────────────────────────────────────────────────────────────┐
│ 链路 9: cognitive-portrait skill (外部)                     │
│   deterministic bundle + 4 agents + evidence quality gate  │
│   → v4 report + bundle.json + quality.json                 │
└─────────────────────────────────────────────────────────────┘
```

依赖关系核心一句话：**所有下游链路都依赖链路 1 (ingest-sessions) 写入的 `items.item_type='observation'`**。

---

## 链路清单

### 链路 1: refine ingest-sessions

| 字段 | 内容 |
|---|---|
| 命令入口 | `refine ingest-sessions [--provider auto\|remem\|local] [--limit N\|--latest N] [--dry-run]`；`--legacy-local-scan` 是 `--provider local` 的弃用别名 |
| 代码位置 | `apps/cli/src/cli.rs:74-88` → `apps/cli/src/handlers.rs:32-58` → `apps/cli/src/ingest_sessions.rs:41-199`（处理函数 `handle_ingest_sessions`，`process_single_session`，`llm_call_with_retry`，`extract_and_parse_facets_with_retry`）|
| 触发方式 | 手动；由 `scripts/daily-refresh.sh` 每天 08:00 调用；由 `scripts/weekly-insights.sh` 每周一 09:00 调用 |
| 数据来源 | `auto/remem` 使用 `remem raw sessions --json` 枚举 exact tuple、`remem raw messages --json` 读取完整快照分页；`local` 使用本地扫描；auto 仅在 remem 可执行文件缺失时回退到 local |
| 处理步骤 | 1) 校验 session summary；2) 按 `--latest` 或 `--limit` 裁剪；3) 逐页校验 selector/order/cursor/count/epoch；4) 用稳定 URL + raw 内容跳过或刷新；5) 保守匹配本地旧路径 identity；6) filter/chunk；7) 默认串行（`REFINE_INGEST_CONCURRENCY` 可配置）执行 LLM facet 抽取；8) 在同一事务保存 remem Document/Items 并删除已取代旧 Document/items |
| LLM 调用 | **是**；每会话 1 次（或每分块 1 次，需要 chunking 时可能 N 次）+ 1 次最终合并；默认并发度 1；最多 5 次重试，base delay 10s，退避 `10 * 2^attempt` 秒 |
| 输出目标 | **refine.db**：`documents` 表（每会话 1 行，source = `remem-raw-session`）+ `items` 表（每会话 N 行，`item_type='observation'`）|
| 输出 schema | `Document { id, source, url=remem-raw://v1/<hex tuple>, title, raw_content }`；`Item { id, item_type='observation', ..., document_id }`；内容变化时保留 Document identity 并替换关联 Items |
| 依赖 | `auto/remem` 需要兼容的 `remem` 二进制（`PATH` 或 `REFINE_REMEM_BIN`），`local` 不需要；所有 provider 都需要 LLM key；provider/契约错误会 fail closed，auto 仅对缺失可执行文件回退 |
| 已知问题 | 网络波动 / API cooldown 时单会话可能耗尽 5 次重试失败；`daily-refresh.sh` 会根据 exit code 记录 `~/.refine/last-refresh-ok`，失败时不更新时间戳 |

---

### 链路 2: refine insights (`--prescription` 为 L4 处方开关)

| 字段 | 内容 |
|---|---|
| 命令入口 | `refine insights [--period N] [--prescription]` |
| 代码位置 | `apps/cli/src/cli.rs:89-97` → `apps/cli/src/handlers.rs:59-76` → `apps/cli/src/insights.rs:24-125`（`handle_insights`，`llm_with_retry`）；路由规划在 `packages/core/src/session/analysis_routes.rs:18` 的 `plan_routes` |
| 触发方式 | 手动；由 `scripts/weekly-insights.sh` 每周日 09:00 通过 launchd 自动触发 |
| 数据来源 | `item_store.find_by_type(ItemType::Observation)` — 全量 Observation（当前实现**未按 `--period` 过滤**时间窗口） |
| 处理步骤 | 1) 加载所有 Observation；2) `cluster_observations()` 纯 Rust 本地聚类（按 project + facet 汇总）；3) `plan_routes()` 规划 N 路分析路由（项目总览 / 决策模式 / bug 模式 / 认知演化 / 技术雷达 / AI 协作 / 工作流 / 各项目深挖 / 知识网络 / 摩擦深挖，最少补齐到 10 路）；4) **10 路并发** LLM 调用 (`Semaphore::new(10)`)，system prompt = `ROUTE_SYSTEM_PROMPT`；5) `merge_route_results()` 合并；6) 调 1 次 LLM 做最终报告（system prompt = `INSIGHTS_SYSTEM_PROMPT`，是否含 L4 处方由 `with_prescription` 决定；大上下文合并请求的单次超时为 300 秒）；7) 保存 |
| LLM 调用 | **是**；一次运行 ≈ **N+1 次**（N 通常为 10 路）；并发度 10；每次独立 5 次重试（和链路 1 同样的 exponential backoff 策略） |
| 输出目标 | **stdout**（完整 markdown 报告）+ **refine.db** `documents` 表 (source=`session-insights-v2`，URL = `insights-v2://<rfc3339>`) |
| 输出 schema | Markdown 文档；`Document.title = "Session Insights v2 YYYY-MM-DD HH:MM"`；`raw_content` 为完整的合并报告 |
| 依赖 | 依赖链路 1 产出的 Observation；必须有 LLM key（否则提示"请配置 API Key"后直接返回） |
| 已知问题 | `InsightsOptions.period` 字段带 `#[allow(dead_code)]` — **声明但未生效**（U-26 声明-执行鸿沟）；任意单路失败时 content 置为字面量 "分析失败: ..."，不会让总流程失败，但会写入最终报告；单路失败后无 sleep 重试窗口外的恢复 |

---

### 链路 3: mirror score

| 字段 | 内容 |
|---|---|
| 命令入口 | `mirror score [--since YYYY-MM-DD\|--all]` |
| 代码位置 | `apps/mirror/src/cli.rs:22-30` → `apps/mirror/src/main.rs:45-48` → `apps/mirror/src/score.rs:66-186`（`handle_score`）；子模块 `score/{baseline,compute,display,indicators,persistence,streak,statusline,types}.rs`；LLM 部分在 `apps/mirror/src/advice.rs` |
| 触发方式 | 手动；由 `scripts/daily-refresh.sh` 每天 08:00 调用 |
| 数据来源 | `ItemRepository::find_since(cutoff)` (默认 90 天) 或 `find_all()`；再通过 `cluster_observations()` 聚合 |
| 处理步骤 | 1) 按 `--all`/`--since`/默认 90 天窗口加载 items；2) 确认有 observation；3) `cluster_observations()`；4) `score::compute(&cluster, &config.targets)` 算 3 层信号灯 + 9 指标 + tension；5) `load_recent_scores(365)` 读历史；6) `persist_score()` 写 jsonl；7) `print_score()` stdout；8) 读 `growth-tracker.json` 展示 pending_ingest 提示；9) 按事件时间分别计算滚动 90 天与 7 天 score，先把当前 portfolio policy 的确定性建议写入缓存，再可选调用 LLM 确认结构化 policy；10) `write_statusline()`。portfolio 计算失败时旧 advice 缓存会失效。 |
| LLM 调用 | **是（best-effort，可用 `--require-advice` 提升为必需）**；调用链 `advice::generate_and_cache` → `llm_with_retry`；单次调用，有 5 次重试退避。LLM 只返回 `policy` JSON，用户可见 short/full 均由服务端 deterministic renderer 产生，自由文本不会进入输出。`advice.json` v5 同时绑定当前 score timestamp、policy、90 天 cohort 和 7 天 cohort；LLM 关闭或失败时仍保留本次 score 的确定性建议。只有版本、新鲜度、严格 `sha256:<64hex>` identity 和预期 90 天 cohort 均可验证的 `profile-summary.json` 才会进入 prompt。 |
| 输出目标 | **stdout**（ASCII 信号灯 + 指标） + `~/.mirror/scores.jsonl`（历史） + `<db_parent>/statusline.txt` + `~/.mirror/advice.json`（LLM 缓存） |
| 输出 schema | `ScoreResult { layers: [LayerScore; 3], tension, timestamp }`，每 `LayerScore { name, signal(Red/Yellow/Green), indicators: [Indicator] }`；`CachedAdvice { advice, short, generated_at, cache_version, cache_key, score_timestamp, policy_key, long_cohort_identity, recent_cohort_identity }` |
| 依赖 | 依赖链路 1 的 Observation；LLM 调用为可选软依赖 |
| 已知问题 | `--period` 相关路径 `filter_since` 被标为 `#[allow(dead_code)]` |

---

### 链路 4: mirror dashboard

| 字段 | 内容 |
|---|---|
| 命令入口 | `mirror dashboard [--since YYYY-MM-DD\|--all]` |
| 代码位置 | `apps/mirror/src/dashboard.rs:31-`（`handle_dashboard`） |
| 触发方式 | 手动 |
| 数据来源 | 与 score 相同：`find_since(cutoff)` 或 `find_all()` → `cluster_observations()` |
| 处理步骤 | 1) 加载 items；2) cluster；3) `score::compute(&cluster, &config.targets)`；4) `persist_score()`；5) 渲染 ASCII dashboard（3 层信号灯 / 认知等级分布 / 协作模式分布 / 项目排行等） |
| LLM 调用 | **否** |
| 输出目标 | **stdout**（76 字符宽的 ASCII 框） + `~/.mirror/scores.jsonl`（与 score 共用同一份 persistence） |
| 输出 schema | 纯终端文本 |
| 依赖 | 依赖链路 1 的 Observation；与 score 共享 compute 逻辑 |
| 已知问题 | 与 score 共用 `persist_score` — 一次运行 dashboard 也会污染分数历史（历史是按调用次数追加而非每天去重） |

---

### 链路 5: mirror weekly

| 字段 | 内容 |
|---|---|
| 命令入口 | `mirror weekly` |
| 代码位置 | `apps/mirror/src/weekly.rs:64-161`（`handle_weekly`），prompt 构造 `build_weekly_prompt`，持久化 `save_weekly_record` / `write_weekly_history_lines_atomically` |
| 触发方式 | 手动；`scripts/daily-refresh.sh` 检测到 `DOW==7`（周日）时调用；失败非致命 |
| 数据来源 | `find_by_type(ItemType::Observation)` → 按时间范围拆成 `this_week`（最近 7 天）和 `last_week`（7-14 天前） |
| 处理步骤 | 1) 拉全量 observation；2) 切分 this/last 两个时间窗；3) 分别 `cluster_observations` + `score::compute`；4) `load_last_weekly_record()` 读上周记录作为 prompt 的"上周建议"上下文；5) `build_weekly_prompt()` 拼接本/上周信号灯 + 上周建议 + Requirements；6) **单次** LLM 调用 `llm_with_retry`（5 次重试退避）；7) 保存 |
| LLM 调用 | **是**；单次调用（带 5 次重试）；system prompt = "认知成长分析师，生成差量报告，第二人称" |
| 输出目标 | **stdout** + `~/.mirror/last-weekly.md`（给周一 motd 读） + `~/.mirror/weekly-history.jsonl`（上限 52 行 = 52 周） + **refine.db** `documents` 表（source=`mirror-weekly`，URL=`mirror-weekly://<rfc3339>`；通过 `document_save::save_report_to_document`） |
| 输出 schema | `WeeklyRecord { week_end, scores:[LayerSignal; 3], suggestions: [String] }`；markdown 报告含"本周信号灯 / 上周信号灯 / 上周建议执行情况 / 下周 1-2 条建议" |
| 依赖 | 依赖链路 1；依赖 `~/.mirror/weekly-history.jsonl`（首次运行时为空）；需要 LLM key（否则 main.rs:52 直接 anyhow!error） |
| 已知问题 | **历史上存在 LLM 调用失败**（重试耗尽即整条命令 fail），`daily-refresh.sh` 对此命令做了 `|| echo "failed (non-fatal)"` 兜底；`extract_suggestions()` 依赖报告 markdown 包含"建议 / suggestion / 下周 / next week"等固定 section header，LLM 输出偏差时无法抽取 |

---

### 链路 6: mirror motd

| 字段 | 内容 |
|---|---|
| 命令入口 | `mirror motd`（一般写进 `~/.zshrc` 每次启动终端触发） |
| 代码位置 | `apps/mirror/src/motd.rs:233-328`（`handle_motd`），`weekly_reminder_from_path` 等 |
| 触发方式 | shell rc 手动调用；不调 launchd |
| 数据来源 | `~/.mirror/scores.jsonl`（通过 `load_recent_scores(2)`）+ `~/.mirror/advice.json`（LLM 缓存）+ `~/.mirror/last-weekly.md`（周一提醒）+ 内置 tips 列表（fallback） |
| 处理步骤 | 1) 读最近 2 次 score；2) 计算信号灯 / trend 箭头；3) 找最弱 indicator 的 dimension；4) 只读取 `advice::load_cached_for_score()` 返回的 v5 建议，即 cache 的 score timestamp 必须与当前 score 精确一致，否则用静态 tips；5) 检测 score 数据是否 >48h 过期；6) 追加 streak 信息；7) 如果今天是周一且 `last-weekly.md` 存在 → 追加一行周报提醒 |
| LLM 调用 | **否**（纯读缓存；不触发任何网络请求） |
| 输出目标 | **stdout**（单行 + 可选提醒行） |
| 输出 schema | 形如 `🪞 深度🟢↑ 广度🟡 协作🔴 | <tip> [⚠️ Data is stale...]` |
| 依赖 | 依赖链路 3 产生的 `scores.jsonl` 和 `advice.json`；依赖链路 5 产生的 `last-weekly.md` |
| 已知问题 | 无核心问题；对 LLM 内容有 `strip_ansi_escapes` 防注入 |

---

### 链路 7: mirror profile

| 字段 | 内容 |
|---|---|
| 命令入口 | `mirror profile` |
| 代码位置 | `apps/mirror/src/profile.rs:269-317`（`handle_profile`），`build_profile_prompt`，`extract_profile_data` |
| 触发方式 | 手动 |
| 数据来源 | `find_observations_by_event_range(now-90d, now)` 滚动 90 天 event-time observations → `cluster_observations` + `score::compute`，与 score advice 的长期窗口共用 `LONG_TERM_WINDOW_DAYS` |
| 处理步骤 | 1) 加载滚动 90 天 event-time cohort；2) cluster + score；3) `extract_profile_data()` 算出 Top 10 项目 + 复杂度分桶 + decision:bugfix 比；4) `build_profile_prompt()` 带 facet budget 4000 字符预算；5) **单次** `llm_with_retry` 调用；6) 保存同一 cohort identity 的画像与摘要 |
| LLM 调用 | **是**；单次（带 5 次重试）；system prompt = "认知画像艺术家，写叙事，第二人称，结尾 2-3 个反思问题" |
| 输出目标 | **stdout** + `~/.mirror/profile-summary.json`（带生成时间、窗口、schema/source revision 与 cohort identity 的短摘要，给 advice 流程做可验证 context 注入） + **refine.db** `documents` 表（source=`mirror-profile`，URL=`mirror-profile://<rfc3339>`） |
| 输出 schema | `profile-summary.json` 是版本化 JSON envelope；14 天过期、legacy 文本、未来时间、未知 schema/revision、非 `sha256:<64hex>` identity、与预期 90 天 cohort 不同或字段缺失时不注入 advice prompt。DB 里存完整叙事 markdown。 |
| 依赖 | 依赖链路 1；需要 LLM key |
| 已知问题 | facet 内容仍受 `FACET_BUDGET_CHARS=4000` 预算限制 |

---

### 链路 8: scripts/weekly-insights.sh (launchd 周报自动化)

| 字段 | 内容 |
|---|---|
| 命令入口 | `/Users/lifcc/Desktop/code/AI/tools/refine/scripts/weekly-insights.sh` |
| 代码位置 | `scripts/weekly-insights.sh`（52 行） |
| 触发方式 | launchd `com.lifcc.refine-weekly-insights.plist` — **每周日 09:00** |
| 数据来源 | 共享 LLM loader：当前进程 → `~/.refine/llm.env` → 显式传入的仓库 `.env` fallback |
| 处理步骤 | 1) 通过共享 loader 做凭据 preflight（不读取 `~/.zshrc`）；2) 打印仅含 `<set>/<unset>` 和来源的 preflight；3) `refine ingest-sessions`（链路 1）；4) `refine insights --prescription`（链路 2）；5) `osascript` 发送 macOS 通知 |
| LLM 调用 | **是（间接）**：= 链路 1 + 链路 2 的总和（单会话 N 次 + insights ≈ 11 次） |
| 输出目标 | `~/Library/Logs/refine-insights.log`（日志） + 链路 1/2 的所有写入目标 |
| 输出 schema | 无自身 schema，复用下游 |
| 依赖 | `~/.refine/llm.env` 内的 LLM API key（开发时可显式 fallback 到 `.env`）；依赖 `refine` 二进制绝对路径 `/Users/lifcc/.cargo/bin/refine` |
| 已知问题 | `set -euo pipefail` 但对子命令 exit code 做了 `if ... 2>&1; then ... else ... fi` 兜底，只记录日志不中断；launchd 不重试失败 |

---

### 链路 9: cognitive-portrait v4 skill（in-repo Codex/Claude skill）

| 字段 | 内容 |
|---|---|
| 命令入口 | Claude Code skill 触发词："认知画像" / "cognitive portrait" / "认知分析" / "分析我的成长" |
| 代码位置 | `skills/cognitive-portrait/SKILL.md` + `prompts/`（在 refine 代码库中；`~/.claude/skills/cognitive-portrait` 为符号链接，见 `docs/setup-skills.md`）|
| 触发方式 | `scripts/cognitive-portrait.sh` 双周调度或用户交互触发 |
| 数据来源 | `refine cognitive-portrait collect` 复用 Session Insights 的同一 SQLite read snapshot、source allowlist、strict eligible cohort 和 manifest builder；默认 current/previous 90d event time |
| 处理步骤 | 1) 无 LLM collector 输出 deterministic JSON bundle；2) 4 个 agent 只读同一 bundle 并行写 L1-L4；3) 合并 v4 candidate；4) validator 检查证据、数字、可比性、novelty 和 action；5) 通过后才写 INDEX，失败隔离 |
| LLM 调用 | collector/validator **0 次**；L1-L4 生成 **4 路并行**，不经过 Refine `LlmClient` |
| 输出目标 | `cognitive-portrait-<date>-v4.md` + `evidence/*.bundle.json` + `evidence/*.quality.json` |
| 输出 schema | 4 层 Markdown；事实绑定 evidence ID/JSON pointer；处方绑定 owner/due/verify；不设行数门槛 |
| 依赖 | 依赖链路 1 的 first-class session Observation；不依赖链路 2 的叙事输出或链路 3 的 score 文本 |
| 已知边界 | remem upstream platform 暂为 unknown；Grok/Gemini knowledge-only 不进入 session 分母；unsupported/detached 数据使趋势不可比较 |

---

## 关键发现

### 1. 数据流入点

**只有链路 1 `refine ingest-sessions` 是数据流入口**（auto 默认探测 remem raw archive，缺少可执行文件时回退到 local；也可显式选择 remem/local）。所有其他链路（2~9）都是在 refine.db 里对已有 Observation 做聚合/加工/叙事。因此：

- **链路 1 质量决定整条管道质量**。facet prompt 改动、chunking 策略、filter 策略的变化会扩散到下游所有报告。
- 断掉链路 1（如 API key 失效或 provider 契约漂移）会让导入显式失败；auto 仅在 remem 可执行文件缺失时回退到 local，不会把 provider 错误降级成空输入。

### 2. LLM 成本分布

按每次运行的 LLM 调用数排序：

| 链路 | LLM 次数 | 并发度 | 场景 |
|---|---|---|---|
| 链路 2 `insights --prescription` | **≈11 次**（10 路 + 1 合并） | 10 | 最重 |
| 链路 1 `ingest-sessions` | **每会话 1~N 次**（N = chunk 数量） | 默认 1 | 批量时总量最大；可通过 env 配置 |
| 链路 5 `weekly` | 1 次 | 1 | |
| 链路 7 `profile` | 1 次 | 1 | |
| 链路 3 `score` (advice) | 最多 1 次（当前 score/cohort 缓存） | 1 | 先写确定性建议；相同 policy、score timestamp、90d/7d cohort 和 model 命中时为 0 次 |
| 链路 4 `dashboard` / 链路 6 `motd` | **0 次** | — | 纯本地 |

**每周日 09:00 的 launchd 任务（链路 8）= 链路 1 + 链路 2 的合计**，是 LLM 预算最集中的时间窗口。

### 3. 稳定性风险

- **链路 5 `mirror weekly`** 历史上最脆弱：单次 LLM 调用，重试耗尽即整个命令失败；被 `daily-refresh.sh` 周日路径用 `|| echo` 兜底；`extract_suggestions()` 还依赖固定的 markdown section header 匹配，LLM 措辞漂移会让"上周建议"回填失败。
- 链路 9 不再消费链路 2 的叙事报告；其事实层由 deterministic bundle 独立生成，因此链路 2 的 LLM 文本失败不会污染画像事实证据。
- **链路 2 的 `period` 参数是死代码**：`#[allow(dead_code)]` 标注，但 CLI 仍暴露 `--period` — 用户设置后静默被忽略（U-26 声明-执行鸿沟）。
- **链路 3 和链路 4 共用 `persist_score`**：每次运行 dashboard 或 score 都追加一行历史，personal baseline 计算会被"频繁运行 dashboard"污染。

### 4. 潜在重复 / 重构机会

- **三条链路重复调用 `cluster_observations + score::compute`**：链路 3 `score` / 链路 4 `dashboard` / 链路 5 `weekly` / 链路 7 `profile` 都做这一步（weekly 还做两次，this_week + last_week）。目前是每次同步调用纯函数，没有命中问题，但重构时应考虑提取为共享助手。
- **两处 `save_report_to_document` 的调用点** (链路 5 weekly、链路 7 profile) 已经被抽到 `document_save.rs`，但链路 2 `insights` 仍直接 `Document::new + set_title + set_url + doc_store.save`，可以统一。
- **链路 3 advice + 链路 5 weekly suggestions + 链路 7 profile** 都是 "用户可见的短叙事"，三者用的 system prompt / 调用约定完全独立，没有公共 prompt helper。
- 链路 9 与链路 2 共享 manifest/cohort 口径，但不共享叙事输出：Insights 负责短周期 delta，画像负责四层长期综合，避免一份 LLM 文本成为另一份报告的事实来源。

---

## 附录 A：env 变量清单

### LLM API 配置

优先级：Anthropic 优先，其次 OpenAI（`packages/core/src/infra/llm.rs:20-48`）。

| 变量 | 用途 | 备注 |
|---|---|---|
| `REFINE_ANTHROPIC_API_KEY` | Anthropic API key（最高优先级） | 或 `ANTHROPIC_AUTH_TOKEN`、`ANTHROPIC_API_KEY`（顺序兜底） |
| `REFINE_ANTHROPIC_MODEL` | Claude 模型名 | 默认 `claude-opus-4-6` |
| `REFINE_ANTHROPIC_BASE_URL` | Claude API base URL | 或 `ANTHROPIC_BASE_URL`；默认 `https://api.anthropic.com` |
| `REFINE_OPENAI_API_KEY` | OpenAI API key（次优先级） | 或 `OPENAI_API_KEY` |
| `REFINE_OPENAI_MODEL` | OpenAI 模型名 | |
| `REFINE_OPENAI_BASE_URL` | OpenAI API base URL | |
| `BASE_API_KEY` | 兼容网关 API key | 与 `BASE_URL`、`BASE_MODEL` 配套 |
| `BASE_URL` | 兼容网关 URL | 由共享 loader 解析 |
| `BASE_MODEL` | 兼容网关模型名 | 由共享 loader 解析 |

### 数据库 / 路径

| 变量 | 用途 | 备注 |
|---|---|---|
| `REFINE_DB_PATH` | 统一覆盖 DB 路径（最高优先级） | 见 `packages/core/src/infra/paths.rs:20` |

其他 fallback key（`resolve_db_path` 的 `fallback_keys` 参数）由各 binary 传入，当前 CLI / mirror 都传空数组。

### 分组使用

| 链路 | 使用 env |
|---|---|
| 链路 1 ingest-sessions | `--provider auto\|remem` 时为 `remem`（或 `REFINE_REMEM_BIN`），`--provider local` 时为本地扫描；LLM key；可选 `REFINE_DB_PATH`、`REFINE_INGEST_CONCURRENCY` |
| 链路 2 insights | LLM key；可选 `REFINE_DB_PATH` |
| 链路 3 score | 可选 LLM key（advice 可跳过）；可选 `REFINE_DB_PATH` |
| 链路 4 dashboard | 可选 `REFINE_DB_PATH` |
| 链路 5 weekly | **必需** LLM key；可选 `REFINE_DB_PATH` |
| 链路 6 motd | 无 env 要求 |
| 链路 7 profile | **必需** LLM key；可选 `REFINE_DB_PATH` |

---

## 附录 B：launchd 任务清单

所有 refine 相关的 launchd plist（位于 `~/Library/LaunchAgents/`）：

| Label | plist 路径 | 触发 | 当前状态（`launchctl list`） |
|---|---|---|---|
| `com.lifcc.refine-server` | `~/Library/LaunchAgents/com.lifcc.refine-server.plist` | 常驻 | **PID 4706 running** |
| `com.lifcc.refine-ui-dev` | `~/Library/LaunchAgents/com.lifcc.refine-ui-dev.plist` | 常驻 | **PID 4696 running** |
| `com.lifcc.refine-daily-ingest` | `~/Library/LaunchAgents/com.lifcc.refine-daily-ingest.plist` | **每天 08:00** | 未运行（调度中）；**last exit=1**（最近一次 ingest 失败） |
| `com.lifcc.refine-weekly-insights` | `~/Library/LaunchAgents/com.lifcc.refine-weekly-insights.plist` | **每周日 09:00** | 未运行（调度中）；last exit=0 |

### 任务详情

#### `com.lifcc.refine-daily-ingest`

- 执行：`/bin/bash /Users/lifcc/Desktop/code/AI/tools/refine/scripts/daily-refresh.sh`
- 工作目录：`/Users/lifcc/Desktop/code/AI/tools/refine`
- 触发：每天 08:00
- 日志：`~/Library/Logs/refine-daily-ingest.log`
- 脚本做的事：
  1. `refine ingest-sessions`（链路 1）
  2. `mirror score`（链路 3，包含 advice LLM 调用）
  3. 周日额外跑 `mirror weekly`（链路 5，非致命）
  4. 成功写 `~/.refine/last-refresh-ok` 时间戳
  5. ingest 失败时以 `exit 1` 让 launchd 感知

#### `com.lifcc.refine-weekly-insights`

- 执行：`/bin/bash /Users/lifcc/Desktop/code/AI/tools/refine/scripts/weekly-insights.sh`
- 工作目录：`/Users/lifcc/Desktop/code/AI/tools/refine`
- 触发：每周日 09:00（`Weekday=0`，macOS launchd）
- 日志：`~/Library/Logs/refine-insights.log`
- 脚本做的事：
  1. 通过共享 loader 加载 LLM env；缺少 unattended key 时在 ingest 前失败
  2. `refine ingest-sessions`（链路 1，补捕最近 24h 新会话）
  3. `refine insights --prescription`（链路 2）
  4. `osascript` 通知 "Weekly insights 报告已生成"

其他 shell 脚本（非 launchd）：
- `scripts/import_claude_code.sh` — 独立的 Claude Code JSONL 选择性导入器，通过 HTTP POST 到 `refine-server` API（不走 CLI，不属于 Rust binary 的任何链路；属于 refine-server 链路，本次研究未展开）。
- `scripts/eval_recommendations.mjs` — 不在本次研究范围（`.mjs` 说明是 Node 脚本，非产出链路）。

---

## 范围外说明

本次梳理**未包含**以下产品面组件（只列出以备后查）：

- **`apps/server`** — refine HTTP 后端（默认 `http://127.0.0.1:21567`，备用端口 `21568..21570`），`scripts/import_claude_code.sh` 是其客户端
- **`apps/desktop`** — Tauri 桌面应用，通常消费 refine-server API
- **`apps/extension`** — 浏览器 extension
- **`packages/core/src/hook_session`** — Claude Code hook ingestion 相关（见 `docs/13_CLAUDE_HOOK_INGESTION.md`）

这些组件与"记录/报告产出链路"没有直接关系（它们是展示/输入层），不属于本次研究范围。
