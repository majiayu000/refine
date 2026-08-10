# 认知画像报告 — 2026-06-01 (v3)

> 基于 **8777 个 AI 编程会话、69751 条观测、41527 个决策、19447 个 bugfix、84 个归一化项目** 的认知画像。
>
> **数据截止**：Refine DB 最新 observation 为 `2026-05-27T13:38:00.891696+00:00`。本报告生成于 2026-06-01，但不假装覆盖 05-28 至 06-01 的未入库数据。
>
> **本周窗口**：以最新 observation 为锚，当前 7 天为 `2026-05-20T13:38:00Z → 2026-05-27T13:38:00Z`，上一 7 天为 `2026-05-13T13:38:00Z → 2026-05-20T13:38:00Z`。
>
> **基线对比**：上一版 Codex 试跑 [cognitive-portrait-2026-05-31-v3.md](./cognitive-portrait-2026-05-31-v3.md)，以及 04-27 v3 的旧版画像结构。

---

## 总体判断

这版画像比 05-31 更接近真实周报，因为它终于有了最近窗口和具体证据。

最重要的结论不是“你变弱了”或者“你少做了”，而是：**当前 7 天从高压修复周切回了治理收口周**。上一 7 天有 242 个会话、2579 条观测、1196 个决策、1141 个 bugfix；当前 7 天降到 91 个会话、720 条观测、429 个决策、200 个 bugfix。总量下降很大，但 bug/decision 从 **0.954 降到 0.466**，这不是单纯萎缩，而是工作形态发生了变化。

上一周像是 looper 压力周：大量 bugfix、schema 漂移、发送链路、内容合同和队列质量问题被集中修掉。当前窗口仍然由 looper 主导，但它不再只是修火。它在做更明确的治理收束：`safe_reply.py` 单入口、content contract 唯一权威、payload hash 回读、review queue 门禁、template_shallow_anchor 拒绝后持久化 rejected。

这几个证据很关键：

1. **looper 仍是当前窗口绝对中心。** 当前 7 天 720 条观测里，looper 合并后占 356 条，达到 **49.4%**。这不是全量 x 独大的延续，而是最近窗口已经切到 looper 治理。
2. **质量压力从爆炸转为可控。** bugfix 从 1141 降到 200，bug/decision 从 0.954 降到 0.466。不能把它解读为质量问题消失，但可以读成“上周集中修复后，本周进入收口”。
3. **探索率短期回升，但 pair 仍然太低。** 当前 7 天 exploration 为 10/91(11.0%)，明显高于全量 5.2%；但 pair_programming 只有 1/91(1.1%)。你开始看新边界，但仍然主要靠 delegation/review，不靠现场共推。
4. **x 在当前窗口缺席。** 全量里 x 仍占 22797 条观测、45.2%，但当前 7 天 x 是 0。说明这周不是 x 主航道周，而是 looper、caff、aip、refine、douyin-mcp、vibeguard 等横向治理周。
5. **数据新鲜度本身仍是风险。** 报告日期是 06-01，DB 最新 observation 停在 05-27。这不是报告文风问题，而是认知系统的 ingest/coverage 问题。

核心快照：

| 指标 | 上一 7 天 | 当前 7 天 | 变化 | 解释 |
|---|---:|---:|---:|---|
| Sessions | 242 | 91 | -62.4% | 活跃窗口明显收缩 |
| Observations | 2579 | 720 | -72.1% | 数据密度下降 |
| Decisions | 1196 | 429 | -64.1% | 决策量下降 |
| Bugfixes | 1141 | 200 | -82.5% | 修复负荷大幅下降 |
| bug/decision | 0.954 | 0.466 | -0.488 | 质量压力回落 |
| review | 84 | 33 | 占比 34.7% -> 36.3% | 收口仍强 |
| delegation | 57 | 31 | 占比 23.6% -> 34.1% | 委派回升 |
| exploration | 4 | 10 | 占比 1.7% -> 11.0% | 短期探索修复 |
| pair_programming | 1 | 1 | 占比仍约 1% | 现场共推没有改善 |

一句话概括：

> 当前窗口不是“继续堆产出”，而是一次从修复洪峰回落后的治理收束。你把 looper 的发送、内容合同、审核队列、payload 证明做得更硬了；但真正的短板仍然是 pair/debug 太低，以及认知底座的数据新鲜度没有跟上报告需求。

和 05-31 那版相比，这版判断更锋利：**这周的核心不是 x/looper 双主线，而是 looper 治理单峰；不是探索持续低迷，而是短期探索回升但 pair 没动；不是 bug 压力泛泛偏高，而是上一窗口 bug 洪峰后出现了明显回落。**

---

## L1：认知演进

### 1.1 Dreyfus 技能阶段迁移

全量 Dreyfus 分布仍然在高位：

| 阶段 | 数量 | 占比 | 判断 |
|---|---:|---:|---|
| Novice | 65 | 0.7% | 基本清空 |
| Advanced Beginner | 408 | 4.6% | 主要来自新领域和运营项目 |
| Competent | 2988 | 34.0% | 仍是最大单档 |
| Proficient | 2361 | 26.9% | 稳定系统判断 |
| Expert | 2955 | 33.7% | 与 competent 接近并列 |

但本周最有价值的不是全量分布，而是当前 7 天分布：

| 阶段 | 当前 7 天数量 | 当前 7 天占比 |
|---|---:|---:|
| Proficient | 38 | 41.8% |
| Expert | 34 | 37.4% |
| Competent | 16 | 17.6% |
| Advanced Beginner | 3 | 3.3% |

这比全量更健康。全量里 expert 和 competent 接近并列，当前窗口里 proficient + expert 合计 79.2%。这说明最近的工作不是低阶学习，也不是纯执行，而是在已有系统上做熟练判断。

同时要注意：上一 7 天 expert 占比是 220/242(90.9%)，当前 7 天降到 37.4%。这个下降不能简单读成认知退步。上一窗口高度集中在 looper 的成熟修复，标签容易被打成 expert；当前窗口项目更分散，proficient 上升反而说明任务从单一高熟系统切到多点治理。

按项目看，长期阶段仍然分层：

| 项目 | 全量观测 | 阶段判断 |
|---|---:|---|
| x | 22797 | Expert / 内容自动化主航道 |
| looper | 9733 | Expert / 调度与审核治理主航道 |
| harness | 2622 | Competent -> Proficient |
| xhh | 2026 | Competent，且存在运营语义断层 |
| mutil-om | 1459 | Proficient |
| reddit-monitor | 1317 | Proficient |
| rss-scout | 1256 | Competent 锁定 |
| vibeguard | 1202 | Proficient |
| douyin-mcp | 944 | Proficient，但发布副作用明显 |
| remem | 684 | 小体量高密度 |

当前窗口的项目阶段更能说明问题：

- looper 356 条，占当前窗口 49.4%，是本周的主战场。
- caff 37 条，aip 33 条，refine 27 条，douyin-mcp 24 条，vibeguard 17 条，说明这周有横向散点。
- x 全量第一，但当前窗口为 0，说明 x 不再是当周解释核心。

这改变了画像判断。不能继续沿用“x 独大”作为本周主标题。更准确的是：

> x 是长期重力中心，looper 是当前治理中心。

这也是 Dreyfus 演进的关键。你已经不只是某个项目 expert，而是在把 expert 方法迁移到调度、内容、发布、质量门禁、认知报告这些不同系统里。但迁移仍不均衡，xhh/rss-scout 还是没有进入同等高阶。

### 1.2 Bloom 认知层级

当前窗口的 Bloom 主档是 Evaluate + Analyze。

证据很密：

- Wan 2.7 VSR 决策里，你选择通过 `model_router.vsr_config` 后处理链路接入，而不是改 workflow/DAG，因为该模型属于 direct route。
- 同一组决策里，你把 `resolution=720p` 定义为跳过 VSR，把 `resolution=1080p` 定义为 `720p -> FlashVSR -> 1080p`，未传 resolution 时保留默认 1080p 对外行为。
- looper 审核里，你没有绕过 `template_shallow_anchor` 门禁强行批准，而是持久化 rejected。
- reply 审核里，你要求基于 exact payload_hash，而不是泛化判断。
- content contract 里，你拒绝关键词、白名单、热度替代语义理解，要求 payload 显式携带 reader/writer contract。
- safe reply 里，你把发送后记录、parent-like、S-tier like 和 audit 收敛到 `scripts/safe_reply.py` 单入口。

这些都不是 Apply。它们不是照着模式写功能，而是在判断系统边界、风险口径、谁是权威、哪些副作用允许发生。

粗略拆分：

| Bloom 层级 | 当前表现 | 判断 |
|---|---|---|
| Understand | 查权威文件、确认 queue 状态、确认 VSR route 形态 | 基础动作 |
| Apply | 复用 queue、contract、safe_reply、migration 模式 | 稳定存在 |
| Analyze | 拆出 schema 漂移、发送副作用分散、like/reply 耦合、HTML 错页误判 | 很强 |
| Evaluate | 决定不绕过门禁、不直接发、不改 DAG、不改坏默认行为 | 主档 |
| Create | 设计 VSR 配置语义、content contract 单权威、review-gated pipeline | 有，但不是最大占比 |

当前 Create 的质量不差，但它被 Evaluate 包住。你不是为了创造而创造，而是在“风险边界清楚后”创造一个更稳的结构。

这是一种成熟工程师的认知形态，但也会有代价：你很容易把所有新想法都拖进门禁、contract、validator、queue 体系里。这样能减少事故，但可能降低探索速度。

本周 Bloom 处方：

- 保留 Evaluate 的硬度。
- 增加 Create 的前置空间。
- 让至少一个实验不以立即收口为目标。

换句话说，不是降低标准，而是别让标准吞掉试错。

### 1.3 双环学习

当前窗口的双环学习比 05-31 报告里说得更具体。

你不是只在修 bug，而是在改系统假设：

1. **从“能发出去”改成“唯一安全发送入口”。** reply 成功后的本地记录、like、audit 原来分散实现，修复后收敛到 `safe_reply.py`。这不是补一行代码，是改变执行合同。
2. **从“schema 可以复制”改成“contract 必须唯一权威”。** content schema 多处复制导致版本漂移，修复为 `references/content-contract.md` 单一权威，并要求 payload 显式携带。
3. **从“辅助动作必须成功”改成“辅助动作可审计降级”。** like 成功原来被硬绑定到 reply 成功，修复后 parent-like 可审计降级，S-tier like best-effort。
4. **从“追问容易得到答复”改成“追问不是主力回复骨架”。** 你识别出“会得到答复”不等于“值得放大”，把追问降级为例外。
5. **从“工具返回成功即可”改成“回读证明才算完成”。** reply_review_queue.py show 578 / show 584 被用来验证 payload_hash、score、状态是否落库。
6. **从“默认 720p 目标逻辑可复用”改成“必须保护 Wan 2.7 默认 1080p 外部语义”。** 这是对默认行为契约的保护。

这些是典型双环：你改的不是单点结果，而是系统运行规则。

但双环学习还有两个缺口。

第一，pair/debug 仍然极低。当前 7 天 pair_programming 只有 1 次，debugging 没进入模式表。你能在 review 中发现系统假设问题，但较少在执行前与 agent 一起把假设拆开。

第二，数据底座没有同步进化。报告生成在 06-01，数据停在 05-27。你已经很擅长要求 payload hash、queue show、exact cwd，但认知画像自己的 ingest 新鲜度还没进入同等标准。

这形成一个有点讽刺但重要的判断：

> 你对业务系统的 proof 要求已经高于对认知系统本身的 proof 要求。

这不是道德问题，是系统优先级问题。认知系统一旦 stale，周报就会把“数据未入库”误读成“人没活动”或“工作量下降”。当前窗口总量下降 62%-82%，其中可能有真实工作变化，也可能有 ingest coverage 问题。报告必须把这个不确定性放在首屏。

### 1.4 元认知成熟度

这次 Codex skill 的改动本身，就是元认知成熟度的一个样本。

05-31 的报告已经能读，但你觉得“不够大哥”。问题不是标题不对，而是证据不足。于是这次没有继续调文风，而是补 collector：

- 加 current 7d vs previous 7d。
- 加 recent decisions。
- 加 recent bugfixes。
- 加 project evidence。
- 加项目名归一化，避免 `looper` 被长路径和短名拆开。
- 修改 skill 指令，要求周报先用窗口数据，再用全量数据。

这说明你在修生成系统的输入，而不是只修输出。

这是一种很高阶的元认知动作：发现报告不够锋利时，先问“证据包够不够”，而不是先问“措辞能不能更像”。

但元认知闭环仍有三层未完成：

| 层级 | 当前状态 | 下一步 |
|---|---|---|
| 观测 | 已有 Refine DB、Mirror、collector | 修 ingest 新鲜度 |
| 解释 | 报告能从全量转为窗口解释 | 增加样本聚类和异常检测 |
| 行为改变 | 报告提出处方 | 处方还未进入下周任务预算 |

当前最该修的是第一层。因为 06-01 报告如果只能看到 05-27，它最多是“延迟周报”。延迟不是不能用，但必须明确写出来。

---

## L2：战略定位

### 2.1 个人技术雷达

长期雷达仍是 x/looper 双核：

| 项目 | 全量观测 | 占比 | 长期角色 |
|---|---:|---:|---|
| x | 22797 | 45.2% | 内容自动化与互动主航道 |
| looper | 9733 | 19.3% | 调度、审核、队列治理 |
| harness | 2622 | 5.2% | 工程质量与 PR/CI 治理 |
| xhh | 2026 | 4.0% | 内容生产与平台适配 |
| mutil-om | 1459 | 2.9% | 工作流与生成系统 |
| reddit-monitor | 1317 | 2.6% | Reddit 监控与回复 |
| rss-scout | 1256 | 2.5% | AI 资讯发现 |
| vibeguard | 1202 | 2.4% | 反幻觉规则治理 |
| douyin-mcp | 944 | 1.9% | 短视频发布链路 |
| remem | 684 | 1.4% | 记忆底座 |
| refine | 348 | 0.7% | 认知观测底座 |

但当前窗口雷达完全不同：

| 项目 | 当前 7 天观测 | 占比 | 当周角色 |
|---|---:|---:|---|
| looper | 356 | 49.4% | 主治理对象 |
| caff | 37 | 5.1% | 小工具发布/维护 |
| aip | 33 | 4.6% | infra / VSR / 模型路由 |
| life | 31 | 4.3% | 生活侧散点任务 |
| product | 29 | 4.0% | 产品化工作 |
| refine | 27 | 3.8% | 认知底座回到视野 |
| douyin-mcp | 24 | 3.3% | 发布副作用治理 |
| codey | 22 | 3.1% | 开发工具方向 |
| litellm-rs | 17 | 2.4% | 模型/代理基础设施 |
| vibeguard | 17 | 2.4% | guard 规则和 hook 诊断 |

这个对比很重要。长期你看起来被 x 吸住，但当前窗口 x 没有新增观测。真正的问题变成：**当 x 暂时沉默时，looper 是否会替代 x 成为新的任务黑洞。**

looper 本周占 49.4%，说明它确实有这个趋势。

不过 looper 的性质和 x 不同。x 更像业务主航道，looper 更像调度治理层。looper 高占比不一定坏，因为它会沉淀 queue、contract、review、safe action 这类可迁移资产。

关键问题是：这些资产有没有迁出去？

从当前证据看，迁移还不够：

- xhh 当前 7 天为 0。
- rss-scout 当前 7 天为 0。
- reddit-monitor 当前 7 天为 0。
- remem/refine 有少量回到视野，但还不构成主线。

所以战略结论是：

> looper 本周不是问题本身，looper 方法未迁移到弱项目才是问题。

### 2.2 探索/利用比

当前 7 天协作模式：

| 模式 | 数量 | 占比 | 判断 |
|---|---:|---:|---|
| review | 33 | 36.3% | 第一模式，收口强 |
| delegation | 31 | 34.1% | 第二模式，吞吐仍高 |
| deep_inquiry | 15 | 16.5% | 深问比例不错 |
| exploration | 10 | 11.0% | 短期修复 |
| teaching | 1 | 1.1% | 沉淀少 |
| pair_programming | 1 | 1.1% | 仍是短板 |

和全量相比，本周探索率明显改善。全量 exploration 是 5.2%，当前窗口 11.0%。这说明你并不是持续完全不探索。

但这里不能乐观过头。探索回升的同时，pair 没有起来。也就是说你更多是在“看新问题/新方向”，不是“和 agent 一起拆复杂问题”。

这会导致一个熟悉的问题：探索进入系统后，很快被 review/delegation 吸收，最终变成任务处理，而不是共同建模。

当前窗口还有一个变化：teaching 从全量 9.6% 降到 1.1%。上一 7 天 teaching 占比 28.5%，当前几乎消失。这说明上一窗口可能在做大量规则沉淀或讲解，而当前窗口进入执行治理。

短期节奏可以这样读：

- 上一 7 天：looper 高压修复 + teaching/规则沉淀。
- 当前 7 天：looper 治理收口 + exploration 回升。
- 仍未发生：pair/debug 前置。

所以探索/利用处方要更精确：

不要泛泛说“增加探索”。当前探索已经回到 11%。真正要增加的是 pair/debug。

建议下个窗口的硬目标：

| 指标 | 当前 7 天 | 下个窗口目标 |
|---|---:|---:|
| exploration | 11.0% | 8%-12%，保持即可 |
| pair/debug | 1.1% | 至少 5% |
| review + delegation | 70.4% | 降到 60%-65% |
| deep_inquiry | 16.5% | 保持 12%+ |

pair/debug 的触发条件要具体：

- 涉及自动发送。
- 涉及 schema / data contract。
- 涉及认知报告数据口径。
- 涉及 launchd/后台服务。
- 涉及跨仓库状态。

这些任务不适合纯 delegation。

### 2.3 注意力分配

本周注意力的好消息是，x 没有继续吸走全部空间。

坏消息是，looper 几乎接管了 x 的位置。

这不是简单换了一个项目名。looper 的任务类型更危险，因为它看起来都是“治理基础设施”，很容易被合理化。x 至少有明确业务边界，looper 则可以无限延伸到队列、调度、审核、发布、日报、工具发现、内容契约。

当前窗口 looper 的 356 条里，证据集中在几类：

- reply queue 审核和 payload hash 回读。
- content contract 单一权威。
- safe_reply 单入口。
- like/reply 副作用拆分。
- round_log 和 empty_reason enum。
- template_shallow_anchor 门禁拒绝。

这些都值得做，但它们也会制造新维护面。

注意力分配的风险是：

> 你把“降低系统风险”的任务做得太多，最后系统本身变成最大的风险源。

下个窗口不建议继续把 looper 做成 50%。建议：

| 类别 | 当前状态 | 下个窗口建议 |
|---|---|---|
| looper 主治理 | 49.4% | 限制到 30%-35% |
| 认知底座 refine/remem | refine 3.8%，remem 未进前 10 | 提到 10%-15% |
| 方法迁移 xhh/rss-scout | 当前 0 | 至少一个项目必须有新增 |
| infra/aip | 4.6% | 可保持，但限定交付面 |
| 发布链路 douyin/xhh | douyin 3.3%，xhh 0 | 选择一个做 contract 化 |

这不是为了平均主义。它是为了防止治理系统继续自我膨胀。

### 2.4 知识网络

当前知识网络出现了一个明显中心：contract。

不同项目里的关键词其实都在指向同一件事：

- looper：content contract、payload hash、review queue、safe_reply。
- aip：VSR 配置、default_target_resolution、direct route vs workflow。
- douyin-mcp：发布定时失败后不擅自立即发布，保留人工/修复重试选项。
- vibeguard：PostToolUse hook 状态判断、全局 hook timeout、不要把单次日志外推成持续故障。
- harness：fallback task_id 必须包含 project_id，避免跨 project 解析错 workflow。
- mutil-om：citation marker、blank-card、rent roll 判定，都是输出合同问题。

这说明你的知识网络正在围绕“副作用合同”收束。

这是好事。副作用合同是你近期最强的横向能力。

但它还没有被产品化成统一模板。每个项目都在局部解决：

- looper 有发送合同。
- aip 有模型路由合同。
- douyin 有发布合同。
- harness 有 workflow identity 合同。
- mutil-om 有渲染与引用合同。

下一步应该把这些抽象成一个跨项目模板：

```text
Action Contract
- input source
- authority file
- allowed side effects
- forbidden shortcuts
- proof command
- rollback / degraded mode
```

这会比继续堆规则更有价值。

---

## L3：工作方式健康度

### 3.1 协作模式光谱

当前协作光谱已经不是单纯委派型，而是 review-first：

| 模式 | 当前 7 天 | 全量 | 解读 |
|---|---:|---:|---|
| review | 36.3% | 30.9% | 本周更偏审核收口 |
| delegation | 34.1% | 41.7% | 委派仍高，但不再第一 |
| deep_inquiry | 16.5% | 10.7% | 深问增强 |
| exploration | 11.0% | 5.2% | 短期修复 |
| teaching | 1.1% | 9.6% | 本周沉淀少 |
| pair_programming | 1.1% | 1.7% | 继续不足 |

这张表说明：你这周不是无脑让 agent 跑。你在审、问、探索。

问题是 pair 仍然没有进入主流程。对你这种工作类型，pair/debug 不是“写代码时结对”，而是执行前共同拆证据：

- 当前状态是否 live。
- 哪个文件是权威。
- 哪个副作用不可发生。
- 哪个字段必须回读。
- 哪个默认行为不能改。

当前很多错误其实都说明 pair/debug 应该前置：

- `$20` 被 shell 插值吞掉。
- raw pool JSON 被当 dict 解析，实际是 list。
- reply_text 忽略 X 原生 reply 自动 mention。
- content schema 复制导致漂移。
- like 成功与 reply 成功被错误硬绑定。
- PR feedback fallback task_id 未包含 project_id。

这些问题不是靠更努力 review 就能彻底消灭。它们需要在写代码前把“真实结构”和“副作用合同”说清楚。

协作处方：

> 只要任务含有外部副作用或跨系统身份，就从 delegation 切到 pair/debug。

外部副作用包括发送、发布、写队列、改 schema、改路由、改自动化启动、写报告索引。

### 3.2 认知负荷

当前认知负荷的核心不是任务多，而是权威多。

最近证据里反复出现“哪个东西才是权威”：

- content contract 的权威从内嵌缩减版回到 `references/content-contract.md`。
- Wan 2.7 VSR 的权威不是 workflow/DAG，而是 direct route 的 router 后处理。
- reply 审核的权威不是主观判断，而是 exact payload_hash 和 queue show。
- 发布动作的权威不是临时改成立即发布，而是用户原定的定时发布边界。
- hook 卡住的权威不是 UI 感觉，而是进程状态、task_complete 和日志。
- 认知报告的权威不是生成日期，而是 DB latest observation。

这说明你的系统已经复杂到必须靠“权威定位”来降低负荷。

当前 Codex skill 的增强也符合这个方向：报告不再只靠全量总结，而是把窗口、证据、项目样本作为权威输入。

但负荷仍然有两个源头：

1. **数据源滞后。** 06-01 只能看到 05-27。报告必须持续提醒这一点。
2. **项目标签混乱。** 原 collector 把 `looper` 和长路径拆开，导致数据解释偏差。现在已归一化，项目数从 96 变成 84。

这两个都是认知负荷问题，不是格式问题。

下个技术动作应该是：

- 给 collector 输出 `data_freshness_status`。
- 如果 latest observation 距生成时间超过 48 小时，报告自动标记为 stale。
- 在索引里区分“生成日期”和“数据截止日期”。

否则认知周报会不断发生“今天生成，但不是今天数据”的误读。

### 3.3 心流率

这周的心流更像“治理心流”，不是“创造心流”。

治理心流的典型表现：

- 把发送入口收敛。
- 把 schema 权威收敛。
- 把审核状态回读。
- 把失败状态持久化。
- 把默认行为保护起来。
- 把副作用变成可审计。

这种心流非常适合你。它给你强控制感，也能明显降低系统事故。

但它也容易上瘾，因为每一个治理动作都看起来正确。

这周 looper 的证据就是这样。每个修复都合理：

- round_log list/dict 解析修复合理。
- @author 冗余修复合理。
- empty_reason enum 合理。
- content contract 单权威合理。
- safe_reply 单入口合理。
- like/reply 解耦合理。

问题不在单个动作，而在总和。治理动作太多，会把你困在“更稳一点”的连续小闭环里。

成长型心流应该至少包含一个“新能力闭环”：

- 新评估口径。
- 新跨项目模板。
- 新自动 proof。
- 新数据新鲜度检查。

所以本周最值得延续的不是继续修 looper，而是把 looper 治理抽象成 Action Contract 模板，然后迁移到 xhh/rss-scout/douyin-mcp 中一个。

### 3.4 学习方式：进化还是退化

本周学习方式整体是进化的，但有一个危险分叉。

进化的证据：

- 从全量画像转为窗口画像。
- 从文风修正转为 collector 证据增强。
- 从“报告像不像”转为“数据弹药够不够”。
- 从泛泛说 exploration 低，改成识别当前窗口 exploration 已回升但 pair 没动。
- 从全量 x 主导，改成识别当前 looper 单峰。

这些都是更准确的学习。

危险的分叉：

- 你越来越擅长把复杂系统合同化。
- 这会让你更愿意继续治理复杂系统。
- 复杂系统越治理越大。
- 越大越需要治理。

这就是治理型能力的自我强化。

它不是坏能力，但如果没有预算边界，会吞掉探索。

所以学习方式的下一步不是“再学一个工具”，而是学会对治理说停：

> 一个治理系统连续两周超过 35% 注意力，就必须产出一个跨项目模板，或者主动降速。

looper 当前 49.4%，已经触发这个规则。

---

## L4：成长处方

### 4.1 核心瓶颈诊断

本周核心瓶颈有四个：

| 瓶颈 | 当前证据 | 影响 |
|---|---|---|
| looper 单峰 | 当前 7 天 49.4% | 治理系统可能替代业务系统成为任务黑洞 |
| pair/debug 过低 | 当前 7 天 1.1% | 复杂副作用问题仍偏事后发现 |
| 数据新鲜度不足 | 06-01 报告只到 05-27 | 周报可能误读近期变化 |
| 方法迁移不足 | xhh/rss-scout 当前 7 天为 0 | 强方法没有进入弱项目 |

这四个瓶颈比 05-31 的“探索低、协作红”更具体。

探索当前已经修复到 11.0%，所以不要继续把探索当第一矛盾。第一矛盾是：**治理能力太集中在 looper，且没有通过 pair/debug 前置成更低返工的工作方式。**

### 4.2 学习策略处方（5 条具体建议）

**1. 给 looper 设置注意力上限。**

下个窗口 looper 不超过 35%。如果超过，必须说明是事故响应还是主动治理。

如果不是事故响应，就停止新增 looper 规则，只允许抽象模板。

**2. 把 looper 的 Action Contract 抽出来。**

模板必须来自真实证据：

- safe_reply 单入口。
- content contract 单权威。
- payload hash 回读。
- review queue 门禁。
- rejected 持久化。
- 辅助动作可审计降级。

抽出来后，不要留在 looper 文档里，放到能迁移的通用位置。

**3. 选择 xhh 或 rss-scout 做迁移。**

当前 7 天这两个项目都是 0。下个窗口必须选择一个。

建议优先 xhh。原因是它的长期分布仍有 advanced_beginner 断层，而且内容平台副作用更适合 Action Contract。

最低交付：

- 一个 xhh content/action contract。
- 一个发布前 validator。
- 一个 proof 模板。

**4. pair/debug 提到 5%。**

只对高副作用任务强制。

触发条件：

- 自动发送。
- 发布。
- 数据口径。
- schema。
- route。
- launchd/后台服务。
- 报告索引。

执行方式不是开会，而是写三行：

```text
真实结构是什么？
权威文件是什么？
完成后如何回读证明？
```

**5. 给 cognitive portrait 加数据新鲜度门禁。**

collector 已经增强，但还缺 freshness。

下一步：

- 输出生成时间和 latest observation 的小时差。
- 超过 48 小时标记 stale。
- 报告标题或 header 显示“数据延迟”。
- INDEX 增加 data_cutoff 列，或至少在生成模式里注明。

这会直接减少“这周周报出来了吗”和“为什么不像最新”的摩擦。

### 4.3 注意力重分配方案

下个窗口建议使用 35/25/20/10/10：

| 类别 | 比例 | 说明 |
|---|---:|---|
| looper 维护/治理 | 35% | 只做事故、关键门禁、模板抽象 |
| 方法迁移 | 25% | xhh 或 rss-scout 二选一 |
| 主业务/发布链路 | 20% | x、douyin、caff 等真实输出 |
| 认知底座 | 10% | refine/remem/Mirror freshness |
| unknown 探索 | 10% | 一个新边界，不进 looper |

和 05-31 的建议相比，这次把 looper 单独列出来。因为当前窗口已经证明它会吃到 49.4%。

执行时只看一个指标：

> 下个窗口 looper 是否低于 35%，且 xhh/rss-scout 是否至少一个不为 0。

如果做不到，说明报告没有改变行为。

### 4.4 AI 协作进化路径

你当前的 AI 协作已经到“契约治理”阶段。

阶段表：

| 阶段 | 行为 | 当前状态 |
|---|---|---|
| 工具使用 | 让 AI 写代码 | 已过 |
| 委派执行 | AI 完成明确任务 | 成熟 |
| 审核收口 | 你用 review 控制结果 | 主模式 |
| 契约治理 | queue/contract/validator/safe action | 正在成熟 |
| 组合调度 | 报告改变下周注意力 | 尚未稳定 |

这周的具体进化是：Codex skill 从“能写报告”进化为“能提供证据包”。

但还没有到组合调度。报告还没有自动影响下周任务。

下一阶段应该是 action card：

```text
下周停止：looper 非事故型规则堆叠
下周迁移：Action Contract -> xhh
下周修底座：cognitive portrait data freshness
下周 pair/debug：所有 schema/route/publish 任务
```

这里不需要马上自动化。先连续两周人工执行，检查是否真的改变了任务分布。

### 4.5 12 周成长计划

#### 第 1-2 周：把周报变成控制面板

目标：报告不只描述状态，而是改变下周配比。

动作：

- collector 增加 freshness。
- INDEX 标注 data cutoff。
- 每份报告输出 4 项 action card。
- 下周报告回查 action card 是否执行。

验收：

- 不再把生成日期误读成数据日期。
- looper 占比从 49.4% 降到 35% 以下。
- xhh/rss-scout 至少一个有当前窗口观测。

#### 第 3-4 周：迁移 Action Contract

目标：把 looper 的强方法迁出。

动作：

- 抽象 Action Contract 模板。
- 在 xhh 建 content/action contract。
- 在 xhh 建发布前 validator。
- 每次发布要求 proof。

验收：

- xhh advanced_beginner 占比下降。
- xhh review 不再只靠人工感觉。
- 至少一种平台风险被前置拦截。

#### 第 5-6 周：修 pair/debug

目标：把高副作用任务从 delegation/review 改为 pair/debug。

动作：

- 给自动发送、发布、schema、route、报告索引加 pair 触发条件。
- 失败两轮后禁止继续委派。
- 每个 pair 任务记录真实结构、权威文件、回读命令。

验收：

- pair/debug 达到 5%。
- bug/decision 低于 0.40。
- 同类 schema/contract 漂移减少。

#### 第 7-8 周：修认知底座

目标：让认知报告足够新鲜。

动作：

- 修 ingest pending sessions。
- 明确 event-time vs ingest-time。
- collector 输出 stale 状态。
- 报告根据 stale 状态调整口吻。

验收：

- 报告数据延迟不超过 48 小时。
- ingest 失败会出现在元数据，而不是被静默吞掉。
- 周报能解释真实当前周。

#### 第 9-10 周：跨项目模板化

目标：把 contract 方法从 looper/xhh 推到 rss-scout/douyin。

动作：

- rss-scout 建新意评分 proof。
- douyin 建发布副作用 proof。
- vibeguard/harness 的规则作为模板校验层。

验收：

- 至少 3 个项目复用同一 Action Contract。
- 至少 2 个项目有 validator。
- 弱项目出现 proficient/expert 占比提升。

#### 第 11-12 周：反审计

目标：确认报告没有美化自己。

动作：

- 抽查 20 个 expert 标签。
- 抽查 20 个 bugfix 是否可被 preflight 避免。
- 抽查 10 个完成声明是否有 fresh proof。
- 对比 action card 和实际任务分布。

验收：

- 找出至少 3 个指标误导点。
- 修 collector 或 scoring。
- 下一版画像能解释修正。

---

## 数据附录

### A. 全量数据

| 指标 | 数值 |
|---|---:|
| Sessions | 8777 |
| Observations | 69751 |
| Decisions | 41527 |
| Bugfixes | 19447 |
| Projects | 84 |
| Bug/decision | 0.468 |
| Latest observation | 2026-05-27T13:38:00.891696+00:00 |

项目数从 05-31 报告里的 96 变为 84，是因为本版 collector 对长路径项目标签做了归一化，例如把 `-users-lifcc-desktop-code-work-life-looper` 和 `looper` 合并。这个变化改善可读性，但项目数不能和上一版直接横比。

### B. 当前 7 天 vs 上一 7 天

| metric | previous_7d | current_7d | delta | delta_pct |
|---|---:|---:|---:|---:|
| sessions | 242 | 91 | -151 | -62.4% |
| observations | 2579 | 720 | -1859 | -72.1% |
| decisions | 1196 | 429 | -767 | -64.1% |
| bugfixes | 1141 | 200 | -941 | -82.5% |
| bug_per_decision | 0.954 | 0.466 | -0.488 |  |

### C. 当前 7 天协作模式

| mode | count | pct |
|---|---:|---:|
| review | 33 | 36.3% |
| delegation | 31 | 34.1% |
| deep_inquiry | 15 | 16.5% |
| exploration | 10 | 11.0% |
| teaching | 1 | 1.1% |
| pair_programming | 1 | 1.1% |

### D. 当前 7 天 Top Projects

| project | observations | pct |
|---|---:|---:|
| looper | 356 | 49.4% |
| caff | 37 | 5.1% |
| aip | 33 | 4.6% |
| life | 31 | 4.3% |
| product | 29 | 4.0% |
| refine | 27 | 3.8% |
| douyin-mcp | 24 | 3.3% |
| codey | 22 | 3.1% |
| litellm-rs | 17 | 2.4% |
| vibeguard | 17 | 2.4% |

### E. Mirror Score

| 维度 | 信号 | 子指标 |
|---|---|---|
| 认知深度 | yellow | Dreyfus 3.9 / 决策质量 56% / 深度产出比 234% / 知识获取 3.5 |
| 战略广度 | red | 探索率 5% / 深耕率 19% / 碎片化 24% |
| 协作效能 | yellow | 委派率 42% / 模式多样性 7 / bug/决策 0.47 / 摩擦密度 2.9 |

Mirror 数据范围：2026-03-03 ~ 2026-05-27。

### F. 近期关键证据

| 类型 | 项目 | 证据 |
|---|---|---|
| decision | aip/infra | Wan 2.7 VSR 选择 router 后处理链路，而不是改 workflow/DAG |
| decision | aip/infra | `resolution=720p` 跳过 VSR，`1080p` 走 `720p -> FlashVSR -> 1080p` |
| decision | looper | approval 被 `template_shallow_anchor` 拒绝后持久化 rejected，不绕过门禁 |
| decision | looper | reply 审核基于 exact payload_hash，而不是泛化判断 |
| decision | looper | content 判定改为 agent-first，关键词/白名单/热度不能替代语义判断 |
| decision | looper | 所有 reply 唯一发送入口收敛为 `scripts/safe_reply.py` |
| bugfix | looper | raw pool JSON 实际为 list，不是 dict，修复 round_log 首次写入失败 |
| bugfix | looper | reply_text 不应包含 `@author`，利用 X 原生 reply 自动 mention |
| bugfix | looper | content schema 多处复制导致漂移，改为 `references/content-contract.md` 唯一权威 |
| bugfix | looper | like 成功不应硬绑定 reply 成功，改为可审计降级 |
| bugfix | looper | `$20` 被 shell 插值吞掉，暴露命令引用问题 |
| bugfix | harness | fallback task_id 未包含 `project_id`，可能跨 project 解析错 workflow |

---

## 报告元数据

| 字段 | 值 |
|---|---|
| 报告日期 | 2026-06-01 |
| 数据截止 | 2026-05-27T13:38:00.891696+00:00 |
| 当前窗口 | 2026-05-20T13:38:00Z -> 2026-05-27T13:38:00Z |
| 上一窗口 | 2026-05-13T13:38:00Z -> 2026-05-20T13:38:00Z |
| 生成入口 | Codex `cognitive-portrait` skill 增强试跑 |
| 数据收集 | `/Users/lifcc/.codex/skills/cognitive-portrait/scripts/collect_data.py` |
| 数据包 | `/tmp/codex_cognitive_portrait/data.md` / `/tmp/codex_cognitive_portrait/data.json` |
| collector 增强 | 7d delta / recent decisions / recent bugfixes / project evidence / project canonicalization |
| 校验器 | `/Users/lifcc/.codex/skills/cognitive-portrait/scripts/validate_report.py` |
| Claude Code skill | 未调用 |
| Claude Code sub-agent | 未调用 |
| 报告状态 | 已通过 validator，已进入索引 |

本版相对 05-31 的改进不是换文风，而是换证据结构：先读当前窗口，再读全量画像；先看具体 decision/bugfix，再做高层判断。这样生成出来的认知周报更接近旧版 v3 的密度，也更适合继续演进为 Codex 原生版本。
