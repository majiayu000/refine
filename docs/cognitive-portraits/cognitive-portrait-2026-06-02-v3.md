---
date: 2026-06-02
version: v3 (Codex threads)
generation_mode: 4 Codex threads in parallel + dispatcher merge
sessions: 8777
observations: 69751
decisions: 41527
bugfixes: 19447
projects: 84
baseline: 2026-03-21 (sessions=1565)
data_cutoff: 2026-05-27T13:38:00.891696+00:00
---

# 认知画像报告 — 2026-06-02 (v3)

> 基于 **8777 个 AI 编程会话、69751 条观测、41527 个决策、19447 个 bugfix、84 个项目** 的全量数据分析。
> **生成方式**：Codex Dispatcher + 4 个 Codex threads 并行写 L1/L2/L3/L4，主 agent 合并。
> **对比基线**：[2026-03-21 v0](./cognitive-portrait-2026-03-21-v0.md)（sessions=1565）。
> **数据截止**：2026-05-27T13:38:00.891696+00:00
> **写作立场**：统一第二人称“你”，每一段论断用 `[事实]` / `[推断，置信度：高/中/低]` / `[建议]` 三标签区分置信度。

---

## 总体判断（Executive Summary）

### 事实总览

[事实] 本次 Codex dispatcher 读取的是 `/Users/lifcc/Library/Application Support/refine/refine.db`，数据截止为 `2026-05-27T13:38:00.891696+00:00`，全量统计为 8777 sessions / 69751 observations / 41527 decisions / 19447 bugfixes / 84 个归一化项目。与 2026-03-21 v0 的 1565 sessions 相比，sessions 增量为 7212/1565(+460.5%)，说明当前画像已经不是早期小样本复盘，而是高密度长期工作系统的截面分析。[事实]

[事实] Dreyfus 分布为 novice 65、advanced_beginner 408、competent 2988、proficient 2361、expert 2955；中高阶 competent+proficient+expert 合计 7344/7779(94.4%)，expert 单档为 2955/7779(38.0%)。协作模式分布为 delegation 3660、review 2712、deep_inquiry 940、teaching 845、exploration 458、pair_programming 149、debugging 8，其中 delegation+review 合计 6372/9764(65.3%)，pair/debug 合计 157/9764(1.6%)。[事实]

[事实] 项目分布高度集中：x 为 22797 条观测，looper 为 9733 条观测，harness 为 2622 条观测，xhh 为 2026 条观测，前两项合计 32530/50332(64.6%)。Mirror score 显示认知深度为黄灯，战略广度为红灯，协作效能为黄灯；子指标包括 Dreyfus 3.9、探索率 5%、碎片化 24%、bug/决策 0.47。[事实]

### 推断主旋律

[推断，置信度：高] 当前画像的主旋律不是“能力不足”，而是“高阶能力集中在少数系统里，且治理型协作正在压过探索型协作”。x 与 looper 的合计占比达到 32530/50332(64.6%)，说明你的 expert 判断主要被内容自动化、调度、审核、发送、队列治理这些方向吸收；而 exploration 全量只有 458/9764(4.7%)，pair/debug 更低到 157/9764(1.6%)，说明你更常用 agent 推进再 review，而不是在复杂问题前段做共同建模。[推断，置信度：高]

[推断，置信度：中] 5 月 expert 峰值很高，但它可能混合了真实技能跃迁和数据分类偏置。`/tmp/cp_data_7.txt` 显示 2026-05 的 expert 为 1261，而 competent 只有 25、proficient 71；这个比例远高于 2026-03 与 2026-04，说明近期数据可能集中在成熟主线，不能简单解释为全域进入 expert。[推断，置信度：中]

[推断，置信度：高] 这次 Codex port 必须保留原 v3 的四层分治，是因为单线程自然叙事会把报告压成“可读周报”，但丢失原版的密度、标签、矩阵和分层长文。L1-L4 threads 的目的不是复刻 Claude Code 工具，而是复刻原架构的认知分工：L1 看演进，L2 看定位，L3 看工作健康，L4 给处方。[推断，置信度：高]

### 最优先建议

[建议] 未来 2 周的最高优先级是把“认知画像生成”本身纳入同一套 proof 标准：每次先生成 `/tmp/cp_data_1..8` 与 `/tmp/cp_mirror_score.txt`，再用 4 threads 生成 `/tmp/cp_v3_l1.md` 到 `/tmp/cp_v3_l4.md`，最后由 dispatcher 合并并在附录记录实际行数和标签计数；如果任一 layer 缺失，则禁止更新 INDEX 为通过。[建议]

[建议] 工作处方上，第一优先级不是继续增加产出，而是把 pair/debug 从 157/9764(1.6%) 提到至少 5%，触发条件限定为自动发送、schema、模型路由、数据口径、报告索引、后台服务这些高副作用任务。完成定义是连续 2 周 pair/debug ≥5%，且 bug/decision 从 0.47 向 0.40 以下移动。[建议]

[建议] 战略处方上，x/looper 仍可作为主航道，但每周必须把一个已验证的治理模式迁移到弱项目，例如把 looper 的 content contract / payload hash / safe action gate 迁移到 xhh 或 rss-scout。完成定义是下一个 7 天窗口里 xhh 或 rss-scout 不再为 0，且至少出现一个可复用 validator 或 proof 模板。[建议]

## L1：认知演进

### 1.1 认知等级时序分析

[事实] L1 输入来源为 `/tmp/cp_manifest.json`、`/tmp/cp_data_1.txt`、`/tmp/cp_data_2.txt`、`/tmp/cp_data_4.txt`、`/tmp/cp_data_7.txt`、`/tmp/cp_data_8.txt` 与 `/tmp/cp_mirror_score.txt`。
[事实] manifest 生成时间为 2026-06-02T06:02:16.679934+00:00，latest_observation 为 2026-05-27T13:38:00.891696+00:00。
[事实] 全量样本规模为 sessions 8777、observations 69751、decisions 41527、bugfixes 19447、projects 84。
[事实] mirror score 的数据范围为 2026-03-05 ~ 2026-05-27。
[事实] `/tmp/cp_data_7.txt` 提供月级 cognitive_level 分布，没有提供 2026-03-21 当日分布。
[推断，置信度：中] 因缺少 2026-03-21 单日明细，本节以 2026-03 月度桶作为包含 2026-03-21 的 baseline 代理，而不是伪造日级数值。
[事实] 2026-03 月度 baseline 总计为 2898/8777(33.0%) 条 cognitive_level 记录。
[事实] 2026-04 月度记录为 4512/8777(51.4%) 条。
[事实] 2026-05 月度记录为 1367/8777(15.6%) 条，且 latest_observation 截止 2026-05-27，不是完整自然月。

| [事实] 月份 | [事实] novice | [事实] advanced_beginner | [事实] competent | [事实] proficient | [事实] expert | [事实] 月总量 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| [事实] 2026-03 baseline proxy | 40/2898(1.4%) | 284/2898(9.8%) | 1591/2898(54.9%) | 831/2898(28.7%) | 152/2898(5.2%) | 2898/8777(33.0%) |
| [事实] 2026-04 | 21/4512(0.5%) | 118/4512(2.6%) | 1372/4512(30.4%) | 1459/4512(32.3%) | 1542/4512(34.2%) | 4512/8777(51.4%) |
| [事实] 2026-05 partial | 4/1367(0.3%) | 6/1367(0.4%) | 25/1367(1.8%) | 71/1367(5.2%) | 1261/1367(92.2%) | 1367/8777(15.6%) |

[推断，置信度：高] 从 2026-03 baseline proxy 到 2026-05 partial，expert 占比从 152/2898(5.2%) 升至 1261/1367(92.2%)，认知等级记录呈明显高阶化。
[推断，置信度：高] 2026-03 的主导等级是 competent 1591/2898(54.9%)，说明 baseline 时期主要还处在稳定执行与局部问题解决区间。
[推断，置信度：高] 2026-04 的 expert 1542/4512(34.2%) 与 proficient 1459/4512(32.3%) 接近，说明 4 月出现从熟练执行到系统判断的过渡期。
[推断，置信度：高] 2026-05 partial 的 expert 1261/1367(92.2%) 表明近期记录主要集中在架构合同、门禁、验证、治理与队列系统的高阶判断。
[推断，置信度：中] 2026-05 的记录量较 2026-04 少 1367/4512(30.3%)，因此 5 月 expert 占比很高但需要避免解释为完整月份稳定状态。
[事实] 全量 level_counts 为 novice 65、advanced_beginner 408、competent 2988、proficient 2361、expert 2955。

| [事实] 全量等级 | [事实] 数量 | [事实] 占比 | [推断，置信度：中] 认知含义 |
| --- | ---: | ---: | --- |
| [事实] novice | 65 | 65/8777(0.7%) | [推断，置信度：高] 新手型记录极少，说明大部分工作不是从零学习 |
| [事实] advanced_beginner | 408 | 408/8777(4.6%) | [推断，置信度：中] 低中阶探索存在，但不是主导形态 |
| [事实] competent | 2988 | 2988/8777(34.0%) | [推断，置信度：高] 稳定执行与问题拆解是长期底座 |
| [事实] proficient | 2361 | 2361/8777(26.9%) | [推断，置信度：高] 模式识别与跨任务迁移已大量出现 |
| [事实] expert | 2955 | 2955/8777(33.7%) | [推断，置信度：高] 高阶系统判断接近全量最高单项 |

[事实] competent/proficient/expert 合计为 7344/8777(83.7%)。
[事实] proficient/expert 合计为 5316/8777(60.6%)。
[事实] novice/advanced_beginner 合计为 473/8777(5.4%)。
[推断，置信度：高] 当前总体认知分布不是“偶发高阶”，而是以 competent 以上为主体的长期结构。
[推断，置信度：高] 2026-03 baseline proxy 中 competent/proficient/expert 合计为 2574/2898(88.8%)。
[推断，置信度：高] 2026-05 partial 中 competent/proficient/expert 合计为 1357/1367(99.3%)。
[推断，置信度：高] 与 2026-03 baseline proxy 相比，2026-05 partial 的高阶密度提高 10.5 个百分点。
[推断，置信度：中] 2026-05 的高阶密度提升，与样本集中在 Looper、x-reply、VibeGuard、Refine、aiproxy 等系统治理任务有关。

| [事实] 时序节点 | [事实] 主要项目/任务 | [事实] 记录特征 | [推断，置信度：高] 认知阶段 |
| --- | --- | --- | --- |
| [事实] 2026-03 baseline proxy | 多项目混合 | competent 1591/2898(54.9%) | [推断，置信度：中] 稳定执行与局部归因阶段 |
| [事实] 2026-04 | x/looper/工程治理扩张 | expert 1542/4512(34.2%) | [推断，置信度：高] 系统化门禁与架构判断形成期 |
| [事实] 2026-05 partial | looper、x-reply、Refine、infra | expert 1261/1367(92.2%) | [推断，置信度：高] 合同化、审计化、ROI 化认知阶段 |

[事实] 近期 observations overview 中，looper 项目记录 180/201(89.6%) 行。
[事实] 近期 observations overview 中，infra 记录 6/201(3.0%) 行。
[事实] 近期 observations overview 中，tool/tools 记录 8/201(4.0%) 行。
[事实] 近期 observations overview 中，refine 记录 6/201(3.0%) 行。
[推断，置信度：高] 近期认知时序不是平均覆盖 84 个项目，而是高度围绕一个主系统进行高密度迭代。
[推断，置信度：中] 这种集中会提升某一系统的 expert 占比，但也会压低探索广度。
[事实] mirror score 给出战略广度为红灯，探索率 5%，深耕率 19%，碎片化 24%。
[推断，置信度：高] 高阶认知密度与战略广度红灯同时出现，说明“做深”在短期内压过了“做宽”。
[事实] Mirror Weekly 2026-05-25 的认知深度为 green，Dreyfus=4.3、决策质量=66.1、知识获取=4.2。
[事实] Mirror Weekly 2026-05-31 的认知深度降为 yellow，Dreyfus=4.1、决策质量=58.9、知识获取=3.1。
[事实] mirror score 当前认知深度为 yellow，Dreyfus 3.9、决策质量 56%、知识获取 3.5。
[推断，置信度：中] 近期认知深度从周报绿灯回落到黄灯，可能不是能力退化，而是任务从建设转向重复审核与队列处理后，新增知识密度下降。
[推断，置信度：高] 认知等级上升和知识获取回落并存，说明当前高阶能力更多表现为治理、校验和边界控制，而不是持续开新知识域。
[建议] 后续 L1 追踪应把 expert 占比与探索率放在同一表中看，避免把高阶执行误判为全面认知扩张。
[建议] 对 2026-05 partial 的 expert 激增，应继续用 2026-06 第一周新数据复核，不应只凭 1367/8777(15.6%) 的部分月样本做长期定论。
[建议] 若要精确 baseline，应补充 2026-03-21 当日或当周 cognitive_level 数据，否则保留“2026-03 月度 baseline proxy”口径。

### 1.2 决策质量演进

[事实] manifest 中全量 decisions 为 41527，bugfixes 为 19447。
[事实] 全量 bugfixes/decisions 比例为 19447/41527(46.8%)。
[事实] mirror score 当前决策质量为 56%，标记为未达标。
[事实] Mirror Weekly 2026-05-24 决策质量为 71.4。
[事实] Mirror Weekly 2026-05-25 决策质量为 66.1。
[事实] Mirror Weekly 2026-05-31 决策质量为 58.9。
[事实] mirror score 当前决策质量为 56%。

| [事实] 时间/来源 | [事实] 决策质量 | [事实] 相关指标 | [推断，置信度：中] 变化解释 |
| --- | ---: | --- | --- |
| [事实] Mirror Weekly 2026-05-24 | 71.4 | Dreyfus=4.3, bug/决策=0.7 | [推断，置信度：中] 深度建设期决策质量较高 |
| [事实] Mirror Weekly 2026-05-25 | 66.1 | Dreyfus=4.3, bug/决策=0.6 | [推断，置信度：中] 质量仍高，但开始进入门禁与审核消耗 |
| [事实] Mirror Weekly 2026-05-31 | 58.9 | Dreyfus=4.1, bug/决策=0.4 | [推断，置信度：中] 重复审核和碎片化拉低综合评分 |
| [事实] Mirror score 2026-03-05~2026-05-27 | 56.0 | bug/决策=0.47 | [推断，置信度：中] 当前窗口低于 5 月下旬周报峰值 |

[推断，置信度：高] 决策质量的演进不是单向上升，而是出现“高阶化同时质量分回落”的张力。
[事实] 近期 decision patterns 中多次出现“写入后立即 read-back 验证”“payload_hash 绑定”“不依赖命令回显”。
[事实] 近期 decision patterns 中多次出现“队列为空则停止”“不伪造审核对象”“不写入无效 review decision”。
[事实] 近期 decision patterns 中多次出现“只使用 reply_review_queue.py list/show/review/show”“不调用发送命令”。
[事实] 近期 decision patterns 中多次出现“content contract 作为唯一权威 schema”“trigger_quote 必须进入 payload”。
[推断，置信度：高] 决策质量的核心改进方向已经从“做出选择”升级为“让选择可验证、可回读、可审计”。
[推断，置信度：高] 与 2026-03 baseline proxy 的 competent 主导相比，近期决策更多体现 expert 级别的约束设计。
[推断，置信度：中] 决策质量评分回落，可能来自任务重复度和战略广度不足，而不是单条决策严谨性下降。
[事实] mirror score 当前深度产出比为 234%，高于常规基线但仍标记未达标。
[推断，置信度：中] 深度产出比 234% 与决策质量 56% 并存，说明产出深度很高但质量评分可能被探索率、碎片化或协作摩擦惩罚。

| [事实] 决策模式 | [事实] 输入证据 | [推断，置信度：高] 认知进化点 | [推断，置信度：中] 风险 |
| --- | --- | --- | --- |
| [事实] read-back verification | 多个 Looper 审核记录强调 show 回读 | [推断，置信度：高] 从完成声明转向状态证明 | [推断，置信度：中] 容易增加操作摩擦 |
| [事实] payload_hash binding | 多个审核项以 exact payload_hash 绑定 | [推断，置信度：高] 从文本判断转向版本化判断 | [推断，置信度：低] 在简单任务中可能过重 |
| [事实] queue empty stop | 多次在 pending 为空时停止 | [推断，置信度：高] 从凑产量转向真实输入约束 | [推断，置信度：中] 容易牺牲短期吞吐 |
| [事实] no direct send | 多次拒绝调用发送脚本 | [推断，置信度：高] 从能力驱动转向边界驱动 | [推断，置信度：中] 自动化闭环依赖审核维护 |
| [事实] content contract | 多处强调唯一权威 schema | [推断，置信度：高] 从关键词规则转向语义合同 | [推断，置信度：中] 合同维护成本上升 |

[事实] aiproxy/Wan 2.7 决策中，明确选择 direct route 的 `model_router.vsr_config` 后处理包装链路，而不是改 workflow/DAG。
[事实] aiproxy/Wan 2.7 决策中，明确首批只接入 text-to-video 与 image-to-video，将 reference-to-video 和 video-edit 后延。
[事实] aiproxy/Wan 2.7 决策中，明确 720p 跳过 VSR、1080p 走 720p -> FlashVSR -> 1080p、未传 resolution 按默认 1080p。
[推断，置信度：高] 这些决策表现出“先保护外部行为，再限制首版范围，再选择最低侵入链路”的高阶工程判断。
[事实] Refine 决策中，确认 `ingest-sessions --source codex` 读取本地 Codex sessions 后交给 Refine 配置的 LLM provider 总结。
[事实] Refine 决策中，确认当前环境必须显式设置 `REFINE_OPENAI_*` 或 `REFINE_ANTHROPIC_*`。
[事实] Refine 决策中，决定先用 `--latest 20/50/100` 小批量处理，而不是一次性处理 6859 个 backlog。
[推断，置信度：高] Refine 决策体现出成本控制、真实调用链识别和小批量验证的认知模式。
[事实] Loom 决策中，先只读评估 README、目录结构与技能相关代码，再用 cargo check/cargo test 验证真实能力。
[事实] Loom 决策中，将 MVP 收窄为 Skill Reliability / Projection Health，暂缓 marketplace、Tauri、RBAC、完整依赖求解。
[推断，置信度：高] Loom 决策体现出从“产品想象”回到“当前可验证核心能力”的收敛能力。

| [事实] 决策质量维度 | [事实] 近期证据 | [推断，置信度：高] 相对 2026-03 baseline proxy 的变化 |
| --- | --- | --- |
| [事实] 根因优先 | `$20` shell 插值丢失后用单引号重提审计字段 | [推断，置信度：高] 从修表象转向定位命令引用根因 |
| [事实] 边界优先 | 不调用发送脚本，只写审核决定 | [推断，置信度：高] 从能做转向该不该做 |
| [事实] 质量优先 | 6 个候选不自然合格则 0 queued | [推断，置信度：高] 从凑数转向质量门禁 |
| [事实] 小步验证 | Refine latest 20/50/100 | [推断，置信度：高] 从全量冲刺转向限额风险管理 |
| [事实] 首版收窄 | Wan 2.7 先接 text/image-to-video | [推断，置信度：高] 从全面覆盖转向语义风险分层 |

[推断，置信度：高] 近期决策质量的最强信号是“自我否决能力”：当输入缺失、队列为空、证据不足或风险过高时，停止比继续更常见。
[推断，置信度：中] 这种自我否决能力可能解释协作效能黄灯/红灯中的摩擦密度，因为每个执行动作都被更高标准审查。
[事实] mirror score 协作效能为黄灯，委派率 42%、模式多样性 7、bug/决策 0.47、摩擦密度 2.9。
[推断，置信度：中] 委派率 42% 表明仍高度依赖 AI 执行，但近期决策把 AI 输出纳入严格审核链，而不是直接采纳。
[建议] 决策质量应拆成“单条判断正确性”和“系统吞吐损耗”两个指标，否则 56% 会掩盖近期高质量门禁的进步。
[建议] 对重复审核任务，应增加批处理规则或抽样审计机制，避免高质量标准把认知资源锁死在低新颖度 review 上。
[建议] 对每个新系统决策继续保留“首版范围、可回滚点、验证命令、失败停止条件”四项，当前证据显示该模板有效。

### 1.3 知识积累与模式识别

[事实] `/tmp/cp_data_4.txt` 的 knowledge/pattern/architecture 样本覆盖 looper、tool、refine、aip、caff、litellm-rs、remem、vibeguard、douyin-mcp、harness 等项目。
[事实] 近期 pattern 高度集中在 review、delegation、deep_inquiry、exploration 四类。
[事实] manifest mode_counts 为 exploration 458、delegation 3660、teaching 845、review 2712、deep_inquiry 940、pair_programming 149、debugging 8。
[事实] 全量 delegation 为 3660/8777(41.7%)。
[事实] 全量 review 为 2712/8777(30.9%)。
[事实] 全量 deep_inquiry 为 940/8777(10.7%)。
[事实] 全量 teaching 为 845/8777(9.6%)。
[事实] 全量 exploration 为 458/8777(5.2%)。
[事实] 全量 pair_programming 为 149/8777(1.7%)。
[事实] 全量 debugging 为 8/8777(0.1%)。

| [事实] 协作/认知模式 | [事实] 全量数量 | [事实] 占比 | [推断，置信度：高] 知识积累含义 |
| --- | ---: | ---: | --- |
| [事实] delegation | 3660 | 3660/8777(41.7%) | [推断，置信度：高] 主要通过任务委派积累系统操作经验 |
| [事实] review | 2712 | 2712/8777(30.9%) | [推断，置信度：高] 大量知识来自审查、校正和门禁 |
| [事实] deep_inquiry | 940 | 940/8777(10.7%) | [推断，置信度：中] 深层探查为架构判断提供来源 |
| [事实] teaching | 845 | 845/8777(9.6%) | [推断，置信度：中] 方法论沉淀已形成但不是最高频 |
| [事实] exploration | 458 | 458/8777(5.2%) | [推断，置信度：高] 新方向探索偏低，与战略广度红灯一致 |
| [事实] pair_programming | 149 | 149/8777(1.7%) | [推断，置信度：中] 同步协作较少，异步委派更强 |
| [事实] debugging | 8 | 8/8777(0.1%) | [推断，置信度：中] debugging 可能被标签吸收到 review/delegation 中 |

[事实] mirror score 中知识获取为 3.5，Mirror Weekly 2026-05-25 知识获取为 4.2，Mirror Weekly 2026-05-31 知识获取为 3.1。
[推断，置信度：中] 知识获取在 5 月末下降，可能与近期重复处理 Looper 审核队列和合同化执行有关。
[事实] top_projects 中 x 为 22797 observations，looper 为 9733 observations。
[事实] x + looper observation 合计为 32530/69751(46.6%)。
[事实] top 15 projects 合计为 48485/69751(69.5%)。
[推断，置信度：高] 知识积累存在强主航道，近半 observation 指向 x 与 looper。
[推断，置信度：中] 主航道集中有利于模式识别深度，但会放大战略广度红灯。

| [事实] 项目/域 | [事实] observation 数 | [事实] 占全量 observations | [推断，置信度：高] 形成的模式识别 |
| --- | ---: | ---: | --- |
| [事实] x | 22797 | 22797/69751(32.7%) | [推断，置信度：高] 内容发布、回复质量、审计与平台风控模式 |
| [事实] looper | 9733 | 9733/69751(14.0%) | [推断，置信度：高] 调度、队列、审核、自动化 ROI 模式 |
| [事实] harness | 2622 | 2622/69751(3.8%) | [推断，置信度：中] Agent/调度工程治理模式 |
| [事实] xhh | 2026 | 2026/69751(2.9%) | [推断，置信度：中] 平台化内容包装与发布约束 |
| [事实] mutil-om | 1459 | 1459/69751(2.1%) | [推断，置信度：中] 产品/生成流程契约与 handoff 模式 |
| [事实] vibeguard | 1202 | 1202/69751(1.7%) | [推断，置信度：高] 反幻觉规则、审查矩阵与执行约束模式 |
| [事实] remem | 684 | 684/69751(1.0%) | [推断，置信度：中] 记忆检索、上下文预算和 hook 注入模式 |
| [事实] refine | 348 | 348/69751(0.5%) | [推断，置信度：中] 会话 ingest、指标镜像、认知报告口径 |

[事实] 近期知识样本中，content contract 被多次作为唯一权威 schema。
[事实] 近期知识样本中，safe_reply.py 被多次作为唯一发送入口。
[事实] 近期知识样本中，review queue 被多次作为发送前审核门禁。
[事实] 近期知识样本中，payload_hash 被多次作为审核绑定标识。
[事实] 近期知识样本中，trigger_quote 被多次作为 parent-triggered reply 的锚点。
[推断，置信度：高] 知识积累已经从“知道某个工具怎么用”转成“抽象出跨系统的合同、入口、状态、审计四件套”。
[推断，置信度：高] 这些模式可以迁移到 x-post、x-reply、reddit-monitor、Tool Scout、Refine ingest、aiproxy routing 等不同系统。
[推断，置信度：中] 跨系统模式高度一致，也可能让新问题被过早套入“门禁/合同/审计”框架，削弱早期探索自由度。

| [事实] 可迁移模式 | [事实] 近期证据 | [推断，置信度：高] 可复用认知 |
| --- | --- | --- |
| [事实] 单一权威入口 | safe_reply.py、content-contract.md、vsr_config | [推断，置信度：高] 降低状态分叉和语义漂移 |
| [事实] 审核门禁 | reply_review_queue、x-post-eval、PROMOTE | [推断，置信度：高] 把主观质量判断外置为流程 |
| [事实] 回读验证 | show 568/583/584、status/payload_hash 校验 | [推断，置信度：高] 把“完成”定义为状态可证明 |
| [事实] 分层流水线 | raw pool -> pre_filter -> contract -> queue -> safe_reply | [推断，置信度：高] 把召回、判断、执行职责拆开 |
| [事实] 首版收窄 | Wan 2.7 首批只接 text/image-to-video | [推断，置信度：高] 用语义风险决定上线顺序 |
| [事实] 小批量执行 | Refine latest 20/50/100 | [推断，置信度：高] 用批量控制成本与失败半径 |
| [事实] 只读复判 | Loom README/代码/cargo check/cargo test | [推断，置信度：高] 先验证事实，再做产品定位 |

[事实] 2026-05-25 Session Insights 记录过去 8724 个会话中 decisions 41284、bugfixes 19352、projects 78。
[事实] 当前 manifest 记录 sessions 8777、decisions 41527、bugfixes 19447、projects 84。
[事实] 两者之间新增 sessions 为 53，新增 decisions 为 243，新增 bugfixes 为 95，新增 projects 为 6。
[推断，置信度：中] 5 月 25 到 6 月 2 输入生成期间，项目覆盖继续扩大，但近期 L1 样本仍高度集中在 Looper/Refine/infra。
[事实] 2026-05-18 Session Insights 记录 sessions 8674、decisions 41043、bugfixes 19199、projects 71。
[事实] 从 2026-05-18 到当前 manifest，新增 sessions 103/8777(1.2%)，新增 decisions 484/41527(1.2%)，新增 bugfixes 248/19447(1.3%)，新增 projects 13/84(15.5%)。
[推断，置信度：中] 项目数增长速度高于 sessions/decisions 增长速度，说明知识边界继续扩张，但样本重心仍被主航道占据。
[推断，置信度：高] 与 2026-03-21 baseline proxy 相比，知识模式已经更明确地从“项目内经验”转向“跨项目治理原则”。

| [事实] 知识演进阶段 | [事实] 代表证据 | [推断，置信度：高] 认知特征 |
| --- | --- | --- |
| [事实] 2026-03 baseline proxy | competent 主导 1591/2898(54.9%) | [推断，置信度：中] 以稳定执行和局部问题解决为主 |
| [事实] 2026-04 | expert/proficient 接近 3001/4512(66.5%) | [推断，置信度：高] 架构原则开始跨任务迁移 |
| [事实] 2026-05 partial | expert 1261/1367(92.2%) | [推断，置信度：高] 以系统门禁、审计、ROI 和边界为核心 |

[事实] recent insights 中 Mirror Weekly 2026-05-25 写明“层1绿+层2红 → 深耕但视野收窄”。
[事实] recent insights 中 Mirror Weekly 2026-05-24 也写明同样张力。
[事实] Mirror Weekly 2026-05-31 的战略广度仍为 red，碎片化为 56.2。
[推断，置信度：高] 知识积累的主要瓶颈不是深度不足，而是深度与广度的比例失衡。
[推断，置信度：中] 当知识积累过度集中在 x/looper 时，模式识别会变强，但跨域迁移的新鲜样本不足。
[建议] 每周至少保留一个非 x/looper 的 exploration session，用于校准现有治理模式是否过拟合社媒自动化。
[建议] 对已经稳定的模式，应沉淀成可复用 checklist，而不是继续在每条审核任务中重新消耗高阶认知。
[建议] 对“合同/门禁/审计”模式，应建立适用边界，避免在低风险原型任务中过度套用。

### 1.4 认知瓶颈识别

[事实] mirror score 当前认知深度为黄灯，战略广度为红灯，协作效能为黄灯。
[事实] mirror score 当前探索率为 5%，深耕率为 19%，碎片化为 24%。
[事实] Mirror Weekly 2026-05-31 战略广度为红灯，探索率 12.1，深耕率 6.2，碎片化 56.2。
[事实] Mirror Weekly 2026-05-25 战略广度为红灯，探索率 8.7，深耕率 4.5，碎片化 45.5。
[事实] Mirror Weekly 2026-05-24 战略广度为红灯，探索率 9.6，深耕率 7.1，碎片化 50.0。
[推断，置信度：高] 战略广度连续红灯是 L1 中最稳定的瓶颈信号。
[推断，置信度：高] 该瓶颈与认知等级高阶化并不冲突；它表示高阶能力主要被用于既有主航道，而不是扩展问题空间。

| [事实] 瓶颈 | [事实] 指标证据 | [推断，置信度：高] 认知含义 | [建议] 干预方向 |
| --- | --- | --- | --- |
| [事实] 探索不足 | exploration 458/8777(5.2%)，mirror 探索率 5% | [推断，置信度：高] 新问题空间输入不足 | [建议] 固定探索配额，不与生产任务抢资源 |
| [事实] 主航道过密 | x+looper 32530/69751(46.6%) observations | [推断，置信度：高] 模式深，但易过拟合 | [建议] 用跨域样本检验治理模式 |
| [事实] 重复审核消耗 | 近期 looper 180/201(89.6%) observations | [推断，置信度：高] 高阶认知被低新颖度队列占用 | [建议] 批处理、抽样、自动拒绝规则前移 |
| [事实] 决策质量回落 | 71.4 -> 66.1 -> 58.9 -> 56.0 | [推断，置信度：中] 系统摩擦与碎片化稀释质量分 | [建议] 单独跟踪高风险决策质量 |
| [事实] 知识获取波动 | 4.4 -> 4.2 -> 3.1 -> 3.5 | [推断，置信度：中] 新知识输入不稳定 | [建议] 把学习型会话从执行型会话中分离 |

[事实] 近期样本中，队列为空后停止的决策重复出现。
[事实] 近期样本中，禁止发送命令的边界重复出现。
[事实] 近期样本中，read-back verification 的收尾方式重复出现。
[事实] 近期样本中，content contract 与 safe_reply 作为唯一入口重复出现。
[推断，置信度：高] 重复并非坏事，它说明系统原则稳定；但重复过多会降低认知新颖度。
[推断，置信度：中] 近期 L1 的最大风险是把“严格”误认为“持续进步”，而忽略严格规则的边际收益递减。
[事实] bugfixes/decisions 为 19447/41527(46.8%)，mirror bug/决策为 0.47。
[推断，置信度：中] 高 bug/decision 比例可能反映复杂系统维护强度，也可能反映自动化系统仍在高修正成本阶段。
[推断，置信度：中] 当 bugfix 成为大量认知输入时，认知会更偏向防御、校验和修复，而不是生成、探索和战略选择。

| [事实] 防御型认知信号 | [事实] 例子 | [推断，置信度：中] 可能副作用 |
| --- | --- | --- |
| [事实] 不伪造对象 | pending 队列为空即停止 | [推断，置信度：低] 在真实输入不足时产能下降 |
| [事实] 不绕过门禁 | approval 被拒后持久化 rejected | [推断，置信度：中] 主观判断被工具门禁限制 |
| [事实] 不直接发送 | review queue 先行 | [推断，置信度：中] 发布闭环依赖额外维护 |
| [事实] 不全量处理 | Refine backlog 小批量 | [推断，置信度：低] 历史积压清理速度慢 |
| [事实] 不改损坏 worktree | aiproxy 可用 checkout 上实施 | [推断，置信度：低] 分支/checkout 管理复杂 |

[推断，置信度：高] 这些“不要做什么”的规则显著提高了执行真实性和系统安全。
[推断，置信度：中] 但如果缺少“何时放松”的策略，防御型认知可能抑制原型速度。
[事实] L5 规则要求 Just do what is asked，不做 easy improvement。
[事实] L1 规则要求 Must search first before creating a new one。
[事实] L3/L4 规则要求禁 silent swallowing、No data = blank、no undeclared API/field。
[推断，置信度：高] 用户的工作环境把反幻觉规则内化为认知约束，这解释了近期“先查、先证、先停”的高频模式。
[推断，置信度：中] 这些约束对生产系统有高价值，但对开放探索任务可能带来摩擦。

| [事实] 2026-03-21 baseline proxy 对比项 | [事实] baseline proxy | [事实] 近期/当前 | [推断，置信度：高] 瓶颈变化 |
| --- | ---: | ---: | --- |
| [事实] expert 占比 | 152/2898(5.2%) | 1261/1367(92.2%) in 2026-05 partial | [推断，置信度：高] 高阶判断显著增强 |
| [事实] competent 占比 | 1591/2898(54.9%) | 25/1367(1.8%) in 2026-05 partial | [推断，置信度：高] 基础执行型记录显著减少 |
| [事实] exploration 占比 | 输入未给 2026-03 单月 mode | 458/8777(5.2%) 全量 | [推断，置信度：中] 探索偏低，无法证明随时间改善 |
| [事实] 战略广度 | 输入未给 2026-03-21 单日 score | 当前 red，探索率 5% | [推断，置信度：高] 当前主要瓶颈在广度 |
| [事实] 决策质量 | 输入未给 2026-03-21 单日 score | 当前 56% | [推断，置信度：中] 当前质量评分未匹配 expert 占比 |

[事实] 2026-05 partial expert 占比强，但 Mirror Weekly 2026-05-31 认知深度为 yellow。
[推断，置信度：高] expert 占比不是最终目标；持续学习、广度、决策质量和协作摩擦共同决定 L1 状态。
[推断，置信度：中] 当前认知瓶颈更像“高阶执行系统的运营瓶颈”，不是“理解能力不足”。
[推断，置信度：中] 如果继续把高阶认知投入重复审核，认知画像会呈现 expert 高、探索低、质量黄/红的稳定组合。
[建议] 将重复审核任务降级给规则化脚本或批处理，让 expert cognition 回到架构判断、异常处理和新域探索。
[建议] 对 x/looper 主航道设置“停止/继续”经济指标，例如每周新增知识、有效发布、误发率、人工审核分钟数。
[建议] 对非主航道每周至少选 1 个项目做 60-90 分钟 deep_inquiry，用于补战略广度。
[建议] 对已有治理模式做一次“反向审计”：列出哪些任务不需要 content contract、review queue 或 read-back verification。
[建议] 在 2026-06 复测时，优先观察 exploration 是否高于 458/8777(5.2%) 的全量水平，以及 expert 占比是否仍高于 2026-03 baseline proxy。

[事实] L1 结论 1：认知等级从 2026-03 baseline proxy 的 competent 主导，转向 2026-05 partial 的 expert 主导。
[事实] L1 结论 2：近期决策模式以验证、边界、审核、状态一致性为核心。
[事实] L1 结论 3：知识积累集中在 x/looper 主航道，并抽象出合同化、门禁化、审计化模式。
[事实] L1 结论 4：主要瓶颈不是深度不足，而是战略广度红灯、重复审核消耗和知识获取波动。
[推断，置信度：高] 与 2026-03-21 baseline proxy 相比，用户的认知演进已经从“会稳定完成复杂任务”进入“能定义系统边界并治理 AI 执行风险”的阶段。
[推断，置信度：中] 下一阶段是否继续进化，取决于能否把已经成熟的治理原则自动化/下放，并把认知资源重新释放给探索和新知识输入。

## L2：战略定位

### 2.1 项目投入分布与战略对齐

[事实] 数据窗口为 2026-03-05 至 2026-05-27，latest_observation 为 2026-05-27T13:38:00.891696+00:00。
[事实] 总量为 sessions=8777、observations=69751、decisions=41527、bugfixes=19447、projects=84。
[事实] 4 周镜像基线显示战略广度为红灯：探索率 5%、深耕率 19%、碎片化 24%。
[事实] Mirror Weekly 2026-05-31 显示战略广度仍为红灯：探索率 12.1%、深耕率 6.2%、碎片化 56.2%。
[事实] Mirror Weekly 2026-05-25 显示战略广度红灯：探索率 8.7%、深耕率 4.5%、碎片化 45.5%。
[事实] Mirror Weekly 2026-05-24 显示战略广度红灯：探索率 9.6%、深耕率 7.1%、碎片化 50.0%。
[推断，置信度：高] 与 2026-03-21 baseline 相比，当前不是“项目不够多”，而是“主线非常集中但探索质量不足”；84 个项目与红灯战略广度同时出现，说明广度问题来自有效探索/深耕比例，而不是项目数量。
[推断，置信度：高] 2026-03-21 baseline 之后，投入模式从“多项目发现”转向“内容/社媒自动化系统的长期治理”，战略中心更清晰，但机会搜索变窄。
[建议] 将 L2 战略目标设为“保留主航道、减少无效碎片、强制打开 1 个邻近机会窗口”，而不是平均削弱主线投入。

| 标签 | 维度 | 当前数值 | 2026-03-21 baseline 对比 | L2 解读 |
| --- | --- | --- | --- | --- |
| [事实] | 项目总数 | 84/84(100.0%) | baseline 后项目池已扩到高复杂度 | 项目组合不是单点创业，而是多系统经营 |
| [事实] | observations | 69751/69751(100.0%) | baseline 后可用行为证据明显充足 | 战略判断可基于真实轨迹 |
| [事实] | decisions | 41527/69751(59.5%) | baseline 后决策密度高 | 不是纯执行流，含大量选择与取舍 |
| [事实] | bugfixes | 19447/41527(46.8%) | baseline 后修复负载高 | 系统治理成本显著 |
| [事实] | top 项目 x | 22797/69751(32.7%) | baseline 后形成最大主航道 | 内容互动/社媒系统占主导 |
| [事实] | top 项目 looper | 9733/69751(14.0%) | baseline 后成为第二主航道 | 调度、发现、发布流水线为核心平台 |
| [事实] | unknown | 19307/69751(27.7%) | baseline 后仍存在归因黑洞 | 战略仪表盘存在解释损耗 |
| [事实] | x+looper | 32530/69751(46.6%) | baseline 后主航道集中 | 近半观测落入内容/自动化闭环 |
| [事实] | top3 含 unknown | 51837/69751(74.3%) | baseline 后高度集中 | 组合重心极强，但可解释性被 unknown 拉低 |
| [事实] | top10 具名项目合计 | 46297/69751(66.4%) | baseline 后形成稳定项目簇 | 主线可被经营，而非随机游走 |
| [事实] | top15 具名项目合计 | 48196/69751(69.1%) | baseline 后长尾仍存在 | 头部之外有少量工具/平台补位 |
| [推断，置信度：高] | x 战略角色 | 22797/69751(32.7%) | baseline 后从内容试验变成生产系统 | 这是当前最大业务/方法论载体 |
| [推断，置信度：高] | looper 战略角色 | 9733/69751(14.0%) | baseline 后从调度工具变成内容操作系统 | 它承接 x、tool-scout、knowledge-scout 等链路 |
| [推断，置信度：中] | unknown 风险 | 19307/69751(27.7%) | baseline 后仍未完全消除 | 项目归因不足会扭曲战略复盘 |
| [建议] | 投入规则 | top2 继续保留 | baseline 后主线已足够强 | 不建议砍主线，应补仪表盘与探索槽 |

[事实] 项目分布中，x、looper、harness、xhh、mutil-om、reddit-monitor、rss-scout、vibeguard、home、douyin-mcp 构成前 10 个具名项目。
[事实] 前 10 个具名项目合计 46297/69751(66.4%)，其中 x 与 looper 合计 32530/46297(70.3%)。
[事实] 前 10 具名项目中，内容/社媒/发现相关项目包括 x、looper、xhh、reddit-monitor、rss-scout、douyin-mcp，合计 38077/46297(82.2%)。
[事实] 前 10 具名项目中，工程治理/协作防护相关项目包括 harness、vibeguard、mutil-om，合计 5281/46297(11.4%)。
[推断，置信度：高] 当前战略实际不是“AI 工具广谱探索”，而是“内容自动化经营系统 + AI 协作治理系统”的双轴组合。
[推断，置信度：中] x 与 looper 的投入占比过高会带来同质化学习：大量经验集中在审核、发送、合同、队列、审计，而较少转化为新产品市场验证。
[建议] 每周保留 x/looper 的运营维护，但给 infra/model、skill registry、refine/memory 各至少一个可验证小实验，以避免战略广度继续红灯。

| 标签 | 战略簇 | 项目 | 观测占比 | 投入性质 | 战略对齐判断 |
| --- | --- | --- | --- | --- | --- |
| [事实] | 内容互动主航道 | x | 22797/69751(32.7%) | 自动回复、内容互动、审计 | 对齐最高 |
| [事实] | 内容操作平台 | looper | 9733/69751(14.0%) | 调度、候选、评估、发布 | 对齐最高 |
| [事实] | 工程基础设施 | harness | 2622/69751(3.8%) | Rust agent/调度维护 | 对齐中高 |
| [事实] | 内容分发 | xhh | 2026/69751(2.9%) | 小红书内容生产 | 对齐中高 |
| [事实] | 业务生成系统 | mutil-om | 1459/69751(2.1%) | OM 生成/QA/合同 | 对齐中 |
| [事实] | 社媒监听 | reddit-monitor | 1317/69751(1.9%) | Reddit 队列与回复 | 对齐中 |
| [事实] | 知识发现 | rss-scout | 1256/69751(1.8%) | RSS 采集与筛选 | 对齐中高 |
| [事实] | AI 防护治理 | vibeguard | 1202/69751(1.7%) | 反幻觉规则/审计 | 对齐高 |
| [事实] | 本地生活/环境 | home | 1180/69751(1.7%) | 环境/个人系统 | 对齐低中 |
| [事实] | 短视频分发 | douyin-mcp | 944/69751(1.4%) | 抖音选题到发布 | 对齐中 |
| [推断，置信度：高] | 主航道 | x+looper | 32530/69751(46.6%) | 内容系统经营 | 当前战略核心 |
| [推断，置信度：中] | 辅助平台 | harness+vibeguard+refine+remem | 4856/69751(7.0%) | 协作质量与记忆治理 | 是主航道的能力底座 |
| [推断，置信度：中] | 内容多平台 | xhh+douyin-mcp+reddit-monitor | 4287/69751(6.1%) | 平台侧扩展 | 可做跨平台验证 |
| [建议] | 资源分配 | 主航道/底座/探索 | 70/20/10 | 维持运营、修复测量、保留探索 | 比完全平均投入更符合现状 |

[事实] 2026-05-25 Session Insights 把 x/looper/rss-scout/reddit-monitor 标识为内容发现与互动系统，把 harness/vibeguard/om 标识为工程治理系统，把 xhh/douyin-mcp 标识为内容生产与分发系统。
[事实] 2026-05-18 Session Insights 指出 x 与 looper 是社媒自动化与内容系统主航道，且核心项目已进入系统运营 + 方法论沉淀阶段。
[推断，置信度：高] 2026-03-21 baseline 后，战略定位从“工具/agent 试验集合”演化为“可审计内容自动化操作系统”。
[推断，置信度：中] 当前最大的战略收益来自把 x/looper 的合同、审核、safe entry、audit 机制产品化，而不是再增加一个相似内容脚本。
[建议] 将战略叙事显式命名为“Agentic Content Ops + Reliability Guardrails”，避免所有项目在复盘中都被写成零散自动化。

### 2.2 技术栈演化路径

[事实] 近期技术栈证据横跨 Python 队列脚本、SQLite 持久化、Rust/Swift 工程、OpenAI-compatible 网关、aiproxy/model_router、FlashVSR 后处理、Remotion/内容发布链路。
[事实] looper/x reply 线反复出现 `reply_review_queue.py`、`safe_reply.py`、`review_result_json`、`payload_hash`、`content_contract`、`trigger_quote`。
[事实] Refine 线确认 `refine ingest-sessions --source codex` 读取本地 Codex sessions，再转交 Refine 配置的 LLM provider 总结。
[事实] aiproxy/Wan 2.7 线决定通过 `model_router.vsr_config` 后处理包装链路接入 FlashVSR，而不是改 workflow/DAG。
[事实] Loom 线被定位为 Git-backed skill registry + 多 target 投影/绑定的 operator control plane。
[推断，置信度：高] 技术栈正在从“脚本集合”走向“合同化控制平面”：队列、哈希、schema、审计、投影、rollback、doctor 成为共同关键词。
[推断，置信度：高] 2026-03-21 baseline 后，技术演化的关键不是语言迁移，而是边界迁移：从生成器直接行动，迁移到 review-gated / safe-entry / auditable state。
[建议] 技术路线应优先复用这套控制平面模式，避免每个新系统重新发明队列、审核、审计、回滚。

| 标签 | 技术层 | 证据 | 当前成熟度 | 战略意义 |
| --- | --- | --- | --- | --- |
| [事实] | 召回层 | raw pool、pre_filter、候选筛选 | 中高 | 控制 LLM 输入噪声 |
| [事实] | 语义合同层 | content_contract、trigger_quote | 高 | 防止 A 帖 B 回复与越界扩展 |
| [事实] | 审核层 | reply_review_queue.py、review_result_json | 高 | 生成与发送解耦 |
| [事实] | 身份绑定层 | payload_hash、exact payload | 高 | 避免 payload 漂移后沿用旧决策 |
| [事实] | 执行层 | safe_reply.py、safe_post | 中高 | 副作用收敛到唯一入口 |
| [事实] | 审计层 | reply_send_audit、round_log | 中高 | 支持复盘与质量门禁 |
| [事实] | 状态层 | SQLite、seen state、queue | 中 | 支持重启恢复与跨进程协作 |
| [事实] | 质量门禁 | template_shallow_anchor、parent_alignment | 中 | 阻止浅模板通过 |
| [事实] | 配额/风险 | like quota、reply quota、platform gate | 中 | 防止辅助动作阻塞主业务 |
| [事实] | 投影/回滚 | Loom projection、snapshot、rollback | 早中期 | 可扩展到 skills 控制平面 |
| [推断，置信度：高] | 架构主轴 | contract -> queue -> safe entry -> audit | 高 | 是当前最可复用的技术资产 |
| [推断，置信度：中] | 技术债主轴 | 重复 schema、分散副作用、自由文本 enum | 中高 | 已多次成为 bug 根因 |
| [建议] | 技术栈策略 | 抽象 shared contract/runtime | 中高 | 把重复修复转成平台能力 |

[事实] bugfix 证据中多次出现 content schema 复制导致版本漂移、reply 发送链路副作用分散、empty_reason 自由文本漂移、reply_text 冗余 @author、like 与 reply 强耦合。
[事实] 2026-05-25 与 2026-05-27 都重复出现同一组 x/looper 修复主题。
[推断，置信度：高] 重复修复不是偶发 bug，而是架构边界尚未完全固化：合同、枚举、发送入口、审计入口仍在从分散实现向中心化实现迁移。
[推断，置信度：中] 2026-03-21 baseline 后，系统可靠性意识显著提升，但 reusable runtime 的沉淀速度低于业务流程扩张速度。
[建议] 对所有内容自动化系统统一三类基础模块：`contract registry`、`queue identity`、`safe side-effect runner`。

| 标签 | 重复故障模式 | 出现证据 | 战略解释 | 下一步技术资产 |
| --- | --- | --- | --- | --- |
| [事实] | schema 漂移 | content schema 多处复制 | 规范未中心化 | contract registry |
| [事实] | 副作用分散 | reply 成功后记录/like/audit 分散 | 执行边界未统一 | safe side-effect runner |
| [事实] | 枚举漂移 | empty_reason 自由文本 | 状态口径不稳定 | typed status enum |
| [事实] | 平台语义误判 | @author 冗余 | 平台机制未进入合同 | platform adapter contract |
| [事实] | 辅助动作阻塞主动作 | like 配额阻塞 reply | 成功语义混淆 | core/aux action split |
| [事实] | 审核对象漂移 | payload_hash 必须绑定 | 决策身份需要强约束 | immutable payload version |
| [事实] | 质量门禁拦截 | template_shallow_anchor 拒绝 approval | 工具门禁优于主观判断 | gate-first review |
| [推断，置信度：高] | 共同根因 | contract 未完全平台化 | 业务扩张快于运行时抽象 | shared runtime |
| [建议] | 优先级 | 先抽象重复故障最多的三件事 | 投入收益高 | schema/side-effect/status |

[事实] aiproxy/Wan 2.7 线中，决策包括 720p 跳过 VSR、1080p 使用 720p -> FlashVSR -> 1080p、default_target_resolution、首批只接 I2V/T2V、reference-to-video 与 video-edit 延后。
[推断，置信度：高] 该线体现出成熟的技术路线取舍：保留外部默认行为、限制首批范围、避免把 direct route 错改成 workflow/DAG。
[推断，置信度：中] 这类 infra/model 路线与 x/looper 的内容系统不同，但战略能力同源：都依赖接口合同、风险分层、默认行为兼容、P0/P1 切分。
[建议] 把 infra/model 接入经验沉淀为“provider route contract checklist”，可反向服务 Atlas/aiproxy 与内容系统的 provider 抽象。

| 标签 | 技术路线 | 决策 | 风险控制 | L2 战略价值 |
| --- | --- | --- | --- | --- |
| [事实] | Wan 2.7 VSR | 使用 model_router.vsr_config 后处理 | 避免误改 workflow/DAG | 正确识别系统边界 |
| [事实] | 分辨率语义 | 720p 跳过、1080p 超分、默认 1080p | 保持外部行为兼容 | 保护产品承诺 |
| [事实] | 首批接入 | I2V/T2V 先行 | 延后语义风险更高模型 | 小范围验证 |
| [事实] | migration 策略 | 新增独立 SQL 配置 | 避免损坏 worktree 旧文件 | 低风险变更路径 |
| [推断，置信度：高] | 技术栈演化 | route contract 优先 | 比“能接上”更重要 | 可复制到其他模型接入 |
| [建议] | 下一步 | 建 route/VSR smoke matrix | 覆盖默认、720p、1080p | 防止默认行为回归 |

[事实] Refine 线确认当前环境必须显式映射 `REFINE_OPENAI_*` 或 `REFINE_ANTHROPIC_*`，不读取 `BASE_URL/BASE_API_KEY/BASE_MODEL`。
[事实] Refine 线确认 OpenAI-compatible base URL 要使用不带 `/v1` 的根地址，避免 `/v1/v1/chat/completions`。
[事实] Refine 线确认不要一次性处理 6859 个 Codex backlog，而是先执行 latest 20/50/100 小批量。
[推断，置信度：高] Refine 是整个认知画像与记忆系统的测量底座；如果 ingestion 配置错误，战略复盘会产生数据断层。
[建议] 将 Refine ingestion 从临时排错提升为固定 runbook：provider env、batch size、成本阈值、失败样本回放。

### 2.3 AI 协作模式战略评估

[事实] mode_counts 显示 delegation=3660/8777(41.7%)、review=2712/8777(30.9%)、deep_inquiry=940/8777(10.7%)、teaching=845/8777(9.6%)、exploration=458/8777(5.2%)、pair_programming=149/8777(1.7%)、debugging=8/8777(0.1%)。
[事实] 4 周镜像显示委派率 42%、模式多样性 7、bug/决策 0.47、摩擦密度 2.9。
[事实] Mirror Weekly 2026-05-31 显示委派率 31.8%、模式多样性 5.0、bug/决策 0.4、摩擦密度 2.3。
[推断，置信度：高] 当前 AI 协作主模式是“委派推进 + 审核收口”，不是 pair programming，也不是纯探索。
[推断，置信度：高] 2026-03-21 baseline 后，协作模式成熟度提升体现在边界与门禁，而不是更高的自动化放权。
[推断，置信度：中] review 占比 2712/8777(30.9%) 与 delegation 占比 3660/8777(41.7%) 叠加，说明人的战略角色正在变成“contract designer / reviewer / operator”，而非逐行实现者。
[建议] 协作优化不应追求更少 review，而应让 review 更结构化、更少重复、更接近产品指标。

| 标签 | 协作模式 | 数量 | 占比 | 战略含义 |
| --- | --- | --- | --- | --- |
| [事实] | delegation | 3660/8777 | 41.7% | AI 承担主要推进 |
| [事实] | review | 2712/8777 | 30.9% | 人类/二级门禁承担质量收口 |
| [事实] | deep_inquiry | 940/8777 | 10.7% | 用于架构、定位、根因问题 |
| [事实] | teaching | 845/8777 | 9.6% | 方法论输出与技能固化 |
| [事实] | exploration | 458/8777 | 5.2% | 探索占比偏低 |
| [事实] | pair_programming | 149/8777 | 1.7% | 逐行协作不是主模式 |
| [事实] | debugging | 8/8777 | 0.1% | debugging 标签显著低于实际 bugfix 量 |
| [推断，置信度：高] | 主协作结构 | delegation+review | 6372/8777(72.6%) | AI 先做，人类/工具后验 |
| [推断，置信度：中] | 探索缺口 | exploration | 458/8777(5.2%) | 与战略广度红灯一致 |
| [建议] | 协作策略 | 增加 plan_first exploration session | 每周至少 1 个 | 专门服务机会窗口 |

[事实] 多个 x-reply-review 会话都要求先 list pending、show exact payload、绑定 payload_hash、写入 review_result_json、再 show 回读。
[事实] 当 pending 队列为空时，多次选择停止而不是伪造审核对象。
[事实] 当 approval 被 template_shallow_anchor 拦截时，选择持久化 rejected，而不是绕过门禁。
[事实] 当 sibling x 仓库有无关脏文件时，选择不提交，避免越界混入。
[推断，置信度：高] AI 协作战略已经从“信任 agent 输出”转向“信任可审计流程”：真实输入、精确身份、结构化评分、新鲜回读。
[推断，置信度：高] 这套协作模式能降低幻觉、误发、越界修改，但会增加摩擦密度。
[建议] 把摩擦分成“必要控制摩擦”和“重复流程摩擦”；前者保留，后者用工具化减少。

| 标签 | 协作控制点 | 证据 | 保留/优化 | 理由 |
| --- | --- | --- | --- | --- |
| [事实] | exact payload | payload_json + payload_hash | 保留 | 防止审核漂移 |
| [事实] | read-back | show 回读状态与评分字段 | 保留 | W-03/W-16 新鲜验证 |
| [事实] | no send in review | 禁止 safe_reply.py/send_approved_replies.py | 保留 | 职责隔离 |
| [事实] | queue empty stop | pending 为空即停止 | 保留 | 防止伪造执行 |
| [事实] | tool gate override | template_shallow_anchor 拒绝 approval | 保留 | 门禁优先于主观判断 |
| [事实] | dirty worktree boundary | sibling x 脏文件不提交 | 保留 | 并发安全 |
| [推断，置信度：高] | 重复命令流程 | list/show/review/show | 可优化 | 适合封装成 audit CLI |
| [建议] | 自动化方向 | review macro 生成结构化审计 | 优化 | 减少重复但不降低门禁 |

[事实] level_counts 显示 expert=2955/8777(33.7%)、competent=2988/8777(34.0%)、proficient=2361/8777(26.9%)、advanced_beginner=408/8777(4.6%)、novice=65/8777(0.7%)。
[事实] competent+proficient+expert 合计 8304/8777(94.6%)。
[推断，置信度：高] 当前 AI 协作不处于学习工具阶段，而处于高阶系统经营阶段；瓶颈不是“能不能做”，而是“做什么更值得”。
[推断，置信度：中] expert 占比 2955/8777(33.7%) 与战略广度红灯并存，说明深度能力强但机会配置不足。
[建议] 把高阶能力从主航道中释放 10% 到探索：每周选择 1 个邻近市场/工具路线做可证伪实验。

| 标签 | 认知等级 | 数量 | 占比 | L2 战略解释 |
| --- | --- | --- | --- | --- |
| [事实] | expert | 2955/8777 | 33.7% | 大量工作已进入系统级判断 |
| [事实] | competent | 2988/8777 | 34.0% | 执行可靠性较高 |
| [事实] | proficient | 2361/8777 | 26.9% | 可处理复杂流程 |
| [事实] | advanced_beginner | 408/8777 | 4.6% | 少量新领域适应 |
| [事实] | novice | 65/8777 | 0.7% | 纯新手场景很少 |
| [事实] | 高阶合计 | 8304/8777 | 94.6% | 具备规模化委派基础 |
| [推断，置信度：高] | 战略瓶颈 | 非能力不足 | 94.6% 高阶 | 更像机会选择与资源分配问题 |
| [建议] | 能力迁移 | 用 expert 模式做新方向 preflight | 1 次/周 | 把高阶能力转化为广度修复 |

[事实] L2 数据中反复出现“只读评估”“不调用发送命令”“不伪造审核对象”“不依赖命令回显”“不碰无关脏文件”。
[推断，置信度：高] AI 协作的战略优势是高保真执行与边界意识；这使复杂系统可持续，但也让每个操作成本上升。
[推断，置信度：中] 如果没有工具化宏命令，review/delegation 占比越高，重复审计劳动越容易吞掉探索时间。
[建议] 对高频审查流程建立“不可降级自动化”：自动完成 list/show/hash/readback，但仍保留人工/agent 的语义判定与拒绝权。

### 2.4 战略盲区与机会窗口

[事实] 战略广度在 2026-05-24、2026-05-25、2026-05-31 均为红灯。
[事实] 4 周镜像战略广度为红灯，探索率 5%、深耕率 19%、碎片化 24%。
[事实] 2026-05-31 的碎片化为 56.2%，高于 4 周镜像摘录的 24%。
[事实] 2026-05-31 的深耕率为 6.2%，低于 4 周镜像摘录的 19%。
[事实] 2026-05-31 的探索率为 12.1%，高于 4 周镜像摘录的 5%，但战略广度仍是红灯。
[推断，置信度：高] 当前盲区不是完全没有探索，而是探索与深耕同时失衡：探索短促、深耕不足、碎片化偏高。
[推断，置信度：高] 2026-03-21 baseline 后，主航道沉淀很强，但邻近机会窗口没有形成稳定经营节奏。
[建议] 不要把“开更多项目”当作解法；应把探索限定为少数可验证窗口，并设置一周内可证伪指标。

| 标签 | 指标 | 4 周镜像 | 2026-05-31 | 对比 | 战略含义 |
| --- | --- | --- | --- | --- | --- |
| [事实] | 探索率 | 5% | 12.1% | +7.1pp | 探索活动增加但仍未转绿 |
| [事实] | 深耕率 | 19% | 6.2% | -12.8pp | 当前深耕不足更突出 |
| [事实] | 碎片化 | 24% | 56.2% | +32.2pp | 工作切换/散点显著上升 |
| [事实] | 战略广度灯号 | 红 | 红 | 不变 | baseline 后问题未解除 |
| [推断，置信度：高] | 主问题 | 非单一探索不足 | 探索+深耕+碎片 | 缺少受控探索组合 |
| [建议] | 修复方式 | 1 个主探索 + 1 个复盘门 | 每周固定 | 降碎片，增转化 |

[事实] unknown 项目有 19307/69751(27.7%) 观测，位列项目分布第二。
[推断，置信度：高] unknown 是 L2 最大测量盲区；它可能包含真实战略工作，也可能包含无效噪声，但当前难以分辨。
[推断，置信度：中] 如果 unknown 中有大量内容/agent 相关工作，则 x/looper 主航道占比被低估；如果 unknown 是分散任务，则碎片化问题被低估。
[建议] 优先修复项目归因：将 Codex session.meta.project、cwd fallback、basename 聚类纳入固定 ingestion，目标是 unknown 从 19307/69751(27.7%) 降到 6975/69751(10.0%) 以下。

| 标签 | 盲区 | 当前证据 | 风险 | 建议指标 |
| --- | --- | --- | --- | --- |
| [事实] | unknown 归因 | 19307/69751(27.7%) | 战略复盘失真 | <10.0% |
| [事实] | 探索率低 | 5% 4 周镜像 | 新机会不足 | >12.0% 且有产物 |
| [事实] | 深耕率下滑 | 6.2% 2026-05-31 | 主线产出不稳定 | >15.0% |
| [事实] | 碎片化升高 | 56.2% 2026-05-31 | 上下文切换损耗 | <35.0% |
| [事实] | 重复故障 | schema/side-effect/status 多次出现 | 平台化不足 | 3 类模块抽象 |
| [事实] | review 重复劳动 | list/show/review/show 多次重复 | 审核吞噬探索时间 | review macro |
| [推断，置信度：高] | 最大盲区 | measurement + opportunity cadence | 可解释性与探索节奏 | 先修测量再扩张 |
| [建议] | 第一优先级 | unknown 归因修复 | 1 周内完成 | 否则 L2 投入判断继续偏差 |

[事实] 机会窗口 1 来自 x/looper：把 content contract、review queue、safe entry、audit 抽象成可复用的 Agentic Content Ops runtime。
[事实] 机会窗口 2 来自 Loom/vibeguard：把 skill registry、projection doctor、observed import、managed projection、diff/snapshot/rollback 与反幻觉规则结合。
[事实] 机会窗口 3 来自 Refine/remem：把 session ingestion、memory routing、cognitive portrait、mirror weekly 变成战略测量产品。
[事实] 机会窗口 4 来自 aiproxy/infra：把 provider route contract、VSR config、default behavior compatibility 变成模型上线/路由治理能力。
[推断，置信度：高] 四个机会窗口并非随机分散，它们共享同一战略主题：AI 系统在真实操作中需要合同、投影、审计、回滚、测量。
[推断，置信度：中] 最适合短期突破的是机会窗口 1 与 3，因为它们直接服务当前最大投入和当前测量盲区。
[建议] 30 天内不应同时重推 4 个窗口；建议选 2 个：Agentic Content Ops runtime 与 Refine/Mirror measurement。

| 标签 | 机会窗口 | 证据来源 | 可验证 MVP | 30 天优先级 |
| --- | --- | --- | --- | --- |
| [事实] | Agentic Content Ops runtime | x/looper 高占比与重复合同 | shared contract + review macro | P0 |
| [事实] | Refine/Mirror measurement | unknown 27.7%、weekly red lights | project attribution + weekly action card | P0 |
| [事实] | Skill Reliability control plane | Loom + VibeGuard | projection doctor + rollback demo | P1 |
| [事实] | Model route governance | Wan 2.7 VSR | route contract smoke matrix | P1 |
| [推断，置信度：高] | P0 选择理由 | 最大投入 + 最大盲区 | 直接降低战略误判 | 优先 |
| [推断，置信度：中] | P1 选择理由 | 邻近平台化能力 | 可作为后续产品线 | 暂缓但保留 |
| [建议] | 组合策略 | P0 两条并行，P1 只做 preflight | 避免碎片化继续升高 | 30 天复盘 |

[事实] x/looper 近期多次选择质量优先于数量，例如只提交 2 条待审回复、0 queued 时停止、缺少一手证据时放弃第一人称经验型回复。
[推断，置信度：高] 内容自动化主航道的护城河不在“更快生成”，而在“更少误发、更高贴合、更可审计”。
[推断，置信度：中] 这一路线若产品化，卖点应是 reliability/quality gate，而非 generic social media automation。
[建议] 对外叙事避免“自动发帖工具”，改成“review-gated agent workflow for high-trust content operations”。

| 标签 | 产品化叙事 | 不建议说法 | 建议说法 | 理由 |
| --- | --- | --- | --- | --- |
| [推断，置信度：高] | x/looper | 自动回复机器人 | 高信任内容操作系统 | 真实优势是门禁与审计 |
| [推断，置信度：高] | VibeGuard/Loom | skill 安装器 | skill reliability control plane | 投影/回滚/doctor 更有差异 |
| [推断，置信度：中] | Refine/Mirror | 周报生成器 | agent work measurement layer | 价值在数据口径与行动卡 |
| [推断，置信度：中] | aiproxy route | 模型接入脚本 | provider route governance | 价值在合同与默认行为兼容 |
| [建议] | 总叙事 | 多个脚本 | AI operational reliability stack | 串起四个窗口 |

[事实] 当前协作与技术证据大量围绕“不直接发送”“不伪造”“不绕过门禁”“不吞错”“先验证再宣称”。
[推断，置信度：高] 这是一种稀缺的 operator-grade agent 使用方式，战略上可以形成差异：把 AI 从演示工具变成可审计执行系统。
[推断，置信度：中] 风险是过度内化为个人流程，缺少产品化接口、命名和用户可理解的仪表盘。
[建议] 下一阶段将“规则”翻译成“用户可见控制”：队列状态、审核原因、payload 版本、发送副作用、回滚点、成本/质量指标。

| 标签 | 战略盲区 | 机会转换 | 验证方法 | Done-when |
| --- | --- | --- | --- | --- |
| [事实] | 规则多但产品界面少 | Operator dashboard | 展示 queue/gate/audit | 用户看得懂当前卡在哪 |
| [事实] | 测量有但归因不足 | Project attribution repair | unknown 降到 <10.0% | 周报项目分布可解释 |
| [事实] | 审核严但重复 | Review macro | list/show/review/show 自动化 | 不降低门禁但减少手工步骤 |
| [事实] | 主线强但探索弱 | Weekly opportunity slot | 1 个 P0/P1 preflight | 有 yes/no 证据 |
| [事实] | 技术合同分散 | Shared contract registry | schema 单源 | 重复 schema bug 清零 |
| [推断，置信度：高] | 最大机会 | AI operational reliability stack | 两个 P0 MVP | 既服务自己也可对外叙事 |
| [建议] | 30 天检查 | P0 两项 + unknown 修复 | 指标复盘 | 红灯战略广度至少转黄 |

[事实] 2026-03-21 baseline 之后，数据总量、主航道集中度、协作门禁成熟度均已足够支撑战略复盘。
[推断，置信度：高] 当前不是“缺少能力”的阶段，而是“需要战略减法和窗口纪律”的阶段。
[推断，置信度：高] L2 总判断：战略定位应从“多个 AI 自动化项目”升级为“高信任 AI 操作系统栈”，其中内容运营是最大样板间，Refine/Mirror 是测量底座，VibeGuard/Loom 是治理底座，aiproxy/infra 是 provider 合同外延。
[建议] 未来 4 周用三个指标约束战略：unknown <10.0%、碎片化 <35.0%、每周 1 个可证伪机会实验；若不能同时满足，停止新增项目，只做主航道抽象与测量修复。

## L3：工作方式健康度
[事实] 数据窗口为 2026-03-05 至 2026-05-27，最新观测时间为 2026-05-27T13:38:00.891696+00:00。
[事实] 全量样本包含 8777 个 sessions、69751 条 observations、41527 条 decisions、19447 条 bugfixes、84 个 projects。
[事实] Mirror 当前协作效能为黄灯，委派率 42%，模式多样性 7，bug/决策 0.47，摩擦密度 2.9。
[事实] 最近 Weekly 2026-05-31 显示协作效能为红灯，委派率 31.8，模式多样性 5.0，bug/决策 0.4，摩擦密度 2.3。
[事实] 最近 Weekly 2026-05-25 显示协作效能为红灯，委派率 37.7，模式多样性 6.0，bug/决策 0.6，摩擦密度 2.8。
[事实] 最近 Weekly 2026-05-24 显示协作效能为红灯，委派率 38.5，模式多样性 6.0，bug/决策 0.7，摩擦密度 2.9。
[事实] 2026-03-21 baseline 未在输入中提供单独数值表，当前可比基线只能使用 Mirror 的个人 4 周均值与 2026-03-05 至 2026-05-27 窗口。
[推断，置信度：高] 与 2026-03-21 baseline 相比，当前工作方式的主要变化不是“更轻松”，而是“阻力更可见、门禁更硬、回读验证更多”。
[推断，置信度：中] 协作效能从 Weekly 红灯到 Mirror 黄灯，说明执行合同正在改善，但改善被碎片化、队列阻塞和重复修复抵消。
[建议] L3 的健康判断应以摩擦密度、bug/decision、工具闭环、节奏可持续性四个指标联合判断，避免只看产出量。

### 3.1 摩擦密度与阻力分析
[事实] 当前 Mirror 摩擦密度为 2.9。
[事实] 2026-05-31 Weekly 摩擦密度为 2.3。
[事实] 2026-05-25 Weekly 摩擦密度为 2.8。
[事实] 2026-05-24 Weekly 摩擦密度为 2.9。
[事实] 从 2026-05-24 到 2026-05-31，摩擦密度从 2.9 降至 2.3，变化为 -0.6/2.9(20.7%)。
[事实] 当前 Mirror 又回到 2.9，较 2026-05-31 Weekly 高 +0.6/2.3(26.1%)。
[推断，置信度：高] 摩擦不是稳定下降，而是在不同工作批次中波动。
[推断，置信度：高] 摩擦波动与 Looper 审核队列、schema 漂移、发送链路副作用、脏工作区和 provider 配置问题高度相关。
[推断，置信度：中] 当前摩擦密度 2.9 不是单纯坏信号；它同时反映了用户把隐性质量风险转成显性门禁。
[建议] 不要把摩擦密度目标设为 0；更合理目标是把可重复摩擦归并为 checklist 或脚本，把不可消除摩擦保留为显性门禁。

| 标签 | 指标 | 当前值 | 近周参照 | 2026-03-21 baseline 对比 |
| --- | --- | ---: | ---: | --- |
| [事实] | Mirror 摩擦密度 | 2.9 | 2026-05-31 为 2.3 | baseline 未给出单值 |
| [事实] | Weekly 低点 | 2.3 | 2026-05-31 | 只能与 4 周均值间接比较 |
| [事实] | Weekly 高点 | 2.9 | 2026-05-24 | 与当前 Mirror 相同 |
| [推断，置信度：高] | 摩擦形态 | 波动型 | 2.3 至 2.9 | baseline 后未形成稳定低摩擦状态 |
| [推断，置信度：中] | 健康含义 | 中性偏紧 | 阻力可见 | 比隐性风险更健康 |
| [建议] | 操作目标 | 降重复阻力 | 保质量门禁 | 与 baseline 比较时看“可解释摩擦”占比 |

[事实] `/tmp/cp_data_5.txt` 中 2026-05-27 的 Looper 样本多次标记为失败、错误、阻塞或 bugfix。
[事实] 阻塞样本包括 pending 队列为空时停止、不写 review decision、不伪造审核对象。
[事实] 错误样本包括 approval 写入被 template_shallow_anchor 拒绝后持久化 rejected。
[事实] 失败样本包括 aiproxy checkout 损坏、Refine provider key 不可用、OpenAI base URL 拼接风险。
[事实] bugfix 样本包括 raw pool JSON 结构误判、content schema 多处复制、reply_text 冗余 @author、like 与 reply 强耦合。
[推断，置信度：高] 摩擦主要来自“执行合同与真实环境之间的错配”，不是来自低技能重复试错。
[推断，置信度：高] 大量阻塞被正确保留为阻塞，而不是被包装成成功，这是工作方式健康的正向证据。
[推断，置信度：中] 用户已经从“靠执行者临场判断”转向“靠队列、payload_hash、contract、audit 让系统拒绝错误动作”。
[建议] 对摩擦分类时应把“质量门禁触发”与“实现缺陷触发”拆开，否则会误判高质量防线为低效率。

| 标签 | 摩擦来源 | 证据 | 健康影响 | 建议处理 |
| --- | --- | --- | --- | --- |
| [事实] | 外部队列为空 | 多次 pending 队列为空即停止 | 防止伪造工作 | 保留阻塞语义 |
| [事实] | 质量门禁拒绝 | template_shallow_anchor 拒绝 approval | 防止浅层模板通过 | 记录拒绝原因 |
| [事实] | schema 漂移 | content contract 多处复制 | 造成口径不一致 | 单一权威文件 |
| [事实] | 副作用分散 | reply 记录、like、audit 分散 | 状态可能不一致 | safe_reply 单入口 |
| [事实] | shell 引用错误 | `$20` 被 shell 插值吞掉 | 审计字段失真 | 参数引用规则化 |
| [事实] | provider 配置错配 | REFINE_OPENAI_BASE_URL 需要根地址 | 调用失败或双 `/v1` | 配置模板化 |
| [事实] | 脏工作区 | sibling x 仓库有无关脏文件 | 阻止安全提交 | 独立 worktree |
| [事实] | checkout 损坏 | aiproxy 损坏 worktree | 影响判断最新状态 | sibling repo 交叉验证 |
| [推断，置信度：高] | 结构性摩擦 | contract 与状态边界反复出现 | 中高风险 | 转成复用 preflight |
| [推断，置信度：中] | 偶发摩擦 | 单次参数引用或路径误用 | 中风险 | 加入命令模板 |
| [建议] | 降噪方向 | 保留门禁、移除重复手工检查 | 提升健康度 | 每类摩擦只保留一个入口 |

[事实] Looper 在 2026-05-25 与 2026-05-27 多次围绕同一组问题修复：reply_text、empty_reason、content contract、like/reply 耦合、safe_reply。
[事实] 同一问题跨日期重复出现，说明故障不是一次性 bug，而是执行边界在多个入口重复暴露。
[推断，置信度：高] 重复摩擦的根因是多入口、多脚本、多 schema 并存。
[推断，置信度：高] safe_reply.py 单入口、references/content-contract.md 单权威、review queue 强制门禁，是对重复摩擦的结构性治疗。
[推断，置信度：中] 如果这些结构性修复已完全执行并被自动化验证，未来摩擦密度应从 2.9 下降到接近 2.3。
[建议] 未来 7 天跟踪 safe_reply 入口外调用次数；目标为 0/N(0%)。
[建议] 未来 7 天跟踪 payload 缺少 content_contract 的拒发次数；目标不是 0，而是所有拒发都有可审计原因。
[建议] 未来 7 天跟踪 pending 队列为空时的报告格式；目标为所有空队列都给出同一枚举原因。

| 标签 | 重复摩擦主题 | 出现日期 | 出现形式 | 健康判断 |
| --- | --- | --- | --- | --- |
| [事实] | reply_text 冗余 @author | 2026-05-25、2026-05-27 | 多次 bugfix | 需要规则固化 |
| [事实] | empty_reason 自由文本 | 2026-05-25、2026-05-27 | 多次 bugfix | 需要中心 enum |
| [事实] | content contract 漂移 | 2026-05-25、2026-05-27 | 多处复制 schema | 需要单一权威 |
| [事实] | like/reply 强耦合 | 2026-05-25、2026-05-27 | 辅助动作阻塞主动作 | 需要审计降级 |
| [事实] | 发送链路分散 | 2026-05-25、2026-05-27 | 记录、like、audit 分散 | 需要原子入口 |
| [推断，置信度：高] | 重复密度 | 5/5(100%) 都属于边界问题 | 非普通 bug | 应优先治理边界 |
| [建议] | 处置优先级 | 先治理入口，再治理单点代码 | P0 | 减少反复修补 |

[事实] 阻塞并不总是低效；例如 pending 队列为空时停止，避免了对不存在 payload_hash 的无效审计。
[事实] 拒绝调用 twitter reply、safe_reply.py 或 send_approved_replies.py 的只读审核边界，在多个样本中反复出现。
[推断，置信度：高] 这类阻塞是“边界健康”的表现，防止 AI 在缺少真实输入时伪造进展。
[推断，置信度：中] 当前用户的工作方式已经接受“停止也是完成的一种”，这比追求每次都有输出更健康。
[建议] 在 L3 评分中将“正确停止”单列为正向指标。

| 标签 | 阻塞类型 | 样本行为 | 是否健康 | 原因 |
| --- | --- | --- | --- | --- |
| [事实] | 无 pending 项 | 不写 review decision | 是 | 避免虚构审核 |
| [事实] | 只读边界 | 不调用发送命令 | 是 | 遵守技能边界 |
| [事实] | 无 payload_json | 停止流程 | 是 | 避免无效审计 |
| [事实] | sibling 仓库脏 | 不提交代码 | 是 | 避免混入无关改动 |
| [事实] | provider key 缺失 | 不宣称 ingest 成功 | 是 | 避免隐藏失败 |
| [推断，置信度：高] | 正确停止率 | 已在多个样本中出现 | 偏高 | 门禁意识强 |
| [建议] | 后续指标 | stop_with_reason / all_stops | 目标 100/N(100%) | 所有停止都要枚举 |

### 3.2 Bug/Decision 比率健康度
[事实] 全量 decisions 为 41527。
[事实] 全量 bugfixes 为 19447。
[事实] 全量 bug/decision 比率为 19447/41527(46.8%)，与 Mirror 的 0.47 一致。
[事实] 2026-05-31 Weekly 的 bug/决策为 0.4。
[事实] 2026-05-25 Weekly 的 bug/决策为 0.6。
[事实] 2026-05-24 Weekly 的 bug/决策为 0.7。
[事实] 从 2026-05-24 到 2026-05-31，bug/决策从 0.7 降至 0.4，下降 0.3/0.7(42.9%)。
[事实] 当前 Mirror 0.47 低于 2026-05-24 的 0.7，约低 0.23/0.7(32.9%)。
[事实] 当前 Mirror 0.47 高于 2026-05-31 的 0.4，约高 0.07/0.4(17.5%)。
[推断，置信度：高] Bug/Decision 比率处于可控但不轻松的区间。
[推断，置信度：高] 0.47 表示每 100 个决策约对应 47 个修复事件，说明工作方式偏向高迭代、高校验。
[推断，置信度：中] 与 2026-03-21 baseline 相比，当前比率不能证明缺陷绝对减少，但能证明缺陷被记录为可复盘对象。
[建议] 下一阶段不要只追求 bug/decision 降低；应追求“重复 bugfix 占比”降低。

| 标签 | 指标 | 数值 | 计算 | 判断 |
| --- | ---: | ---: | --- | --- |
| [事实] | decisions | 41527 | manifest total | 总决策量大 |
| [事实] | bugfixes | 19447 | manifest total | 修复量大 |
| [事实] | bug/decision | 0.47 | 19447/41527(46.8%) | 中高 |
| [事实] | 2026-05-24 周值 | 0.7 | Weekly | 高压 |
| [事实] | 2026-05-25 周值 | 0.6 | Weekly | 高压 |
| [事实] | 2026-05-31 周值 | 0.4 | Weekly | 改善 |
| [推断，置信度：高] | 当前健康度 | 黄灯 | Mirror | 可控但紧 |
| [建议] | 目标 | 重复 bugfix 降低 | 需新增跟踪 | 比总 bug 数更重要 |

[事实] 近期 bugfix 高度集中在 Looper 的内容与发送系统。
[事实] 2026-05-27 的前 10 个 bugfix 中，5 个直接属于 reply 链路或 content contract。
[事实] 2026-05-25 的 bugfix 中，reply_text、empty_reason、content contract、like/reply、发送链路分散反复出现。
[推断，置信度：高] 这不是“bug 太多”的单一问题，而是“同一生产系统正在从脚本集合收敛为受控流水线”的转型期。
[推断，置信度：中] Bug/Decision 0.47 在这个阶段可以接受，但若同类 bug 在单入口治理后继续重复，则健康度应下调。
[建议] 对 Looper 建立 bugfix 分类表：schema、payload、queue、send、audit、quota、repo-state、provider。

| 标签 | Bug 类别 | 样本 | 决策侧对应动作 | 健康含义 |
| --- | --- | --- | --- | --- |
| [事实] | schema | content schema 多处复制 | references/content-contract.md | 消除漂移 |
| [事实] | payload | payload_hash 绑定审核 | show/review/show | 防止旧载荷复用 |
| [事实] | queue | pending 空即停止 | 不写 review decision | 防止伪造输入 |
| [事实] | send | safe_reply 单入口 | 原子处理 reply 与 audit | 状态一致性 |
| [事实] | audit | review_result_json 写入评分字段 | 回读校验 | 可审计 |
| [事实] | quota | like best-effort | reply 不被 like 阻塞 | 主辅动作解耦 |
| [事实] | repo-state | sibling x 脏文件 | 不提交 | 降低越界风险 |
| [事实] | provider | REFINE_OPENAI_* 映射 | 控制 API 调用路径 | 降低配置错配 |
| [推断，置信度：高] | 主因 | 边界治理 | 多类 bug 都指向边界 | 结构性修复有效 |
| [建议] | 后续 | bugfix 归因必须落到类别 | 6 至 8 类足够 | 防止自由文本漂移 |

[事实] 决策样本中大量出现“不是直接发送，而是提交 review queue”“不是凭记忆实现，而是定位权威文件”“不是绕过门禁，而是持久化 rejected”。
[事实] 决策样本中多次出现“回读验证”，包括 `reply_review_queue.py show`、payload_hash、status、reviewer_id、score 字段。
[推断，置信度：高] 决策质量的健康信号是“执行前先定边界，执行后回读验证”。
[推断，置信度：高] Bugfix 的健康信号是“每次修复都追溯根因”，例如 raw pool JSON 实际为 list、shell 参数未正确引用、SDK 常量类型不匹配。
[推断，置信度：中] 该工作方式在短期内提高了 bugfix 数量，但长期会降低不可解释失败。
[建议] 将决策记录分为三类：边界决策、实现决策、停止决策。
[建议] 将 bugfix 记录分为三类：根因修复、保护性修复、口径修复。

| 标签 | 决策类型 | 出现频率线索 | 健康度 | 下一步 |
| --- | --- | --- | --- | --- |
| [事实] | 边界决策 | 多次不调用发送命令 | 高 | 保留 |
| [事实] | 实现决策 | safe_reply、content contract | 高 | 自动化验证 |
| [事实] | 停止决策 | pending 空停止 | 高 | 枚举化 |
| [事实] | 迁移决策 | 独立 registry、小批量接管 | 中高 | 分阶段复盘 |
| [事实] | 配置决策 | REFINE_OPENAI_BASE_URL 根地址 | 中 | 模板化 |
| [推断，置信度：高] | 决策模式 | 保守收敛 | 健康 | 避免过早扩展 |
| [建议] | 指标 | stop_decisions / all_decisions | 新增 | 衡量正确停止能力 |

[事实] bugfix 重复暴露的主题包括 empty_reason、reply_text、content contract、like/reply、safe_reply。
[事实] 这些主题跨 2026-05-25 与 2026-05-27 重复。
[推断，置信度：高] 重复 bugfix 是当前健康度的最大扣分项。
[推断，置信度：高] 但重复 bugfix 已经触发结构性收敛动作，不是无方向修补。
[建议] 若 7 天后同类主题仍出现 3/N(>20%) 的 bugfix 占比，应优先审查入口覆盖率，而不是继续补单点。
[建议] 若 7 天后新 bugfix 主要来自新系统边界，则可认为重复问题下降。

| 标签 | 重复 bugfix | 根因方向 | 已有治理 | 健康判断 |
| --- | --- | --- | --- | --- |
| [事实] | empty_reason | 自由文本漂移 | 中心 enum | 可降 |
| [事实] | reply_text | 平台 mention 语义未编码 | @author 数量为 0 | 可降 |
| [事实] | content contract | 多处复制 schema | 单一权威 | 可降 |
| [事实] | like/reply | 主辅动作混淆 | best-effort audit | 可降 |
| [事实] | send path | 副作用分散 | safe_reply | 可降 |
| [推断，置信度：中] | 复发风险 | 中 | 需验证所有入口 | 未完全消除 |
| [建议] | 复盘口径 | 复发即查入口而非查单点 | P0 | 减少局部修补 |

### 3.3 工具使用模式与效率
[事实] collaboration_mode 全量计数为 delegation 3660、review 2712、deep_inquiry 940、teaching 845、exploration 458、pair_programming 149、debugging 8。
[事实] delegation 占 3660/8777(41.7%)。
[事实] review 占 2712/8777(30.9%)。
[事实] deep_inquiry 占 940/8777(10.7%)。
[事实] teaching 占 845/8777(9.6%)。
[事实] exploration 占 458/8777(5.2%)。
[事实] pair_programming 占 149/8777(1.7%)。
[事实] debugging 占 8/8777(0.1%)。
[事实] delegation + review 合计为 6372/8777(72.6%)。
[推断，置信度：高] 用户的主要工具使用模式不是“与 AI 同步写代码”，而是“委派执行 + 审核收口”。
[推断，置信度：高] 这与数据中的 review queue、payload_hash、show 回读、strict no-send 边界一致。
[推断，置信度：中] pair_programming 与 debugging 标记占比极低，可能是标签口径导致，不应理解为用户很少调试。
[建议] L3 应把工具效率定义为“AI 被约束在正确边界内完成真实状态变更”，而不是“AI 输出速度”。

| 标签 | 协作模式 | 数量 | 占比 | 健康解读 |
| --- | --- | ---: | ---: | --- |
| [事实] | delegation | 3660 | 3660/8777(41.7%) | 主模式 |
| [事实] | review | 2712 | 2712/8777(30.9%) | 第二模式 |
| [事实] | deep_inquiry | 940 | 940/8777(10.7%) | 深挖存在 |
| [事实] | teaching | 845 | 845/8777(9.6%) | 方法沉淀 |
| [事实] | exploration | 458 | 458/8777(5.2%) | 探索偏低 |
| [事实] | pair_programming | 149 | 149/8777(1.7%) | 标签较少 |
| [事实] | debugging | 8 | 8/8777(0.1%) | 标签极少 |
| [推断，置信度：高] | delegation+review | 6372 | 6372/8777(72.6%) | 已形成委派-审核系统 |
| [建议] | 模式目标 | 提升 exploration 质量 | 不必追求数量 | 每周保留小探索 |

[事实] project 分布中，x 为 22797 observations，looper 为 9733，harness 为 2622，xhh 为 2026，mutil-om 为 1459。
[事实] top 5 项目 observation 合计为 38637/69751(55.4%)。
[事实] x 单项为 22797/69751(32.7%)。
[事实] looper 单项为 9733/69751(14.0%)。
[事实] x + looper 合计为 32530/69751(46.6%)。
[事实] unknown 为 19307/69751(27.7%)，是第二大类。
[推断，置信度：高] 工具效率高度依赖 x/looper 主线的稳定性。
[推断，置信度：中] unknown 占比 19307/69751(27.7%) 会降低项目级复盘效率。
[建议] 优先治理 unknown 归因；否则 L3 对节奏与摩擦的归因会被稀释。

| 标签 | 项目 | observations | 占比 | L3 含义 |
| --- | --- | ---: | ---: | --- |
| [事实] | x | 22797 | 22797/69751(32.7%) | 最大主战场 |
| [事实] | looper | 9733 | 9733/69751(14.0%) | 第二主战场 |
| [事实] | unknown | 19307 | 19307/69751(27.7%) | 归因风险 |
| [事实] | harness | 2622 | 2622/69751(3.8%) | 工程治理线 |
| [事实] | xhh | 2026 | 2026/69751(2.9%) | 内容生产线 |
| [事实] | mutil-om | 1459 | 1459/69751(2.1%) | 业务工具线 |
| [推断，置信度：高] | x+looper | 32530 | 32530/69751(46.6%) | 近半数工作压在自动化运营线 |
| [建议] | unknown | 19307 | 19307/69751(27.7%) | 应降至可解释区间 |

[事实] 近期工具链中反复出现 `reply_review_queue.py list/show/review/show`。
[事实] 近期工具链中反复出现 `safe_reply.py` 作为统一发送入口。
[事实] 近期工具链中反复出现 `references/content-contract.md` 作为唯一权威 schema。
[事实] 近期工具链中出现 `REFINE_OPENAI_BASE_URL`、`REFINE_OPENAI_*` 的配置映射。
[事实] 近期工具链中出现 `cargo check/cargo test` 作为 Loom 只读验证。
[事实] 近期工具链中出现 launchctl、端口、health/status/api/jobs 的 Looper 服务验证。
[推断，置信度：高] 工具使用模式已经从“命令执行”升级为“命令前置边界 + 命令后置回读”。
[推断，置信度：高] 回读验证是当前工作健康度的核心保护。
[推断，置信度：中] 由于工具入口多，若缺少统一索引，执行者仍会在命令选择上消耗认知负荷。
[建议] 建立每条主线的最小可执行工具表：发现、审查、写入、回读、发送、停止。

| 标签 | 工具动作 | 代表命令/对象 | 效率贡献 | 风险 |
| --- | --- | --- | --- | --- |
| [事实] | 队列发现 | list pending | 确认真实输入 | 队列空导致停止 |
| [事实] | 载荷核验 | show payload_hash | 防止旧 payload | 需精确绑定 |
| [事实] | 审核写入 | review result | 可审计决策 | 字段引用错误 |
| [事实] | 回读验证 | show status/score | 证明持久化 | 增加步骤 |
| [事实] | 发送入口 | safe_reply.py | 副作用原子化 | 入口覆盖需验证 |
| [事实] | 合同权威 | content-contract.md | 降低 schema 漂移 | 需禁止复制缩减版 |
| [事实] | provider 配置 | REFINE_OPENAI_* | 控制 LLM 调用 | base URL 易错 |
| [事实] | 构建验证 | cargo check/test | 验证真实能力 | 耗时 |
| [推断，置信度：高] | 总体效率 | 中高 | 真实状态可见 | 操作步骤偏重 |
| [建议] | 优化方向 | 把步骤封装，不删校验 | 保持回读 | 降手工负担 |

[事实] Mirror 模式多样性为 7。
[事实] 2026-05-31 Weekly 模式多样性为 5.0。
[事实] 2026-05-25 Weekly 模式多样性为 6.0。
[事实] 2026-05-24 Weekly 模式多样性为 6.0。
[推断，置信度：高] 当前模式多样性高于近周 Weekly，说明工作方式不是单一委派。
[推断，置信度：中] 模式多样性增加可能带来上下文切换成本，尤其在 x、looper、refine、infra、tool 同日交错时。
[建议] 每个工作日最多保留 2 条深度主线，其余工作进入 queue 或只读 triage。

| 标签 | 时间点 | 模式多样性 | 协作效能灯号 | L3 判断 |
| --- | ---: | ---: | --- | --- |
| [事实] | 2026-05-24 | 6.0 | 红灯 | 多样但压力高 |
| [事实] | 2026-05-25 | 6.0 | 红灯 | 多样但压力高 |
| [事实] | 2026-05-31 | 5.0 | 红灯 | 多样性下降但仍红 |
| [事实] | 当前 Mirror | 7.0 | 黄灯 | 多样性恢复 |
| [推断，置信度：中] | baseline 后 | 5 至 7 区间 | 不稳定 | 需避免切换过量 |
| [建议] | 节奏控制 | 2 主线 + 低风险维护 | 可执行 | 降低认知换挡 |

### 3.4 工作节奏与可持续性
[事实] 数据覆盖 84 个 projects。
[事实] observations 总量为 69751。
[事实] top 15 项目中同时包含内容运营、工程治理、基础设施、记忆工具、视频与部署方向。
[事实] x、looper、unknown 三项合计 51837/69751(74.3%)。
[事实] top 5 项目合计 38637/69751(55.4%)，说明主线集中但仍有长尾。
[事实] 工作节奏中存在多次“立即执行真实队列”“回读校验”“不提交无关脏文件”“不直接发送”的收尾动作。
[推断，置信度：高] 可持续性的优势是边界感强、验证习惯强、能正确停止。
[推断，置信度：高] 可持续性的风险是主线系统太多，且每条主线都要求高审计密度。
[推断，置信度：中] 2026-03-21 baseline 之后，工作方式从探索多项目逐步转向运营多个可治理系统；产出质量提高，但维护面扩大。
[建议] 可持续性不应靠减少 ambition，而应靠压缩“同时活跃的可写系统数”。

| 标签 | 节奏维度 | 当前证据 | 健康度 | 风险 |
| --- | --- | --- | --- | --- |
| [事实] | 主线集中 | x+looper 32530/69751(46.6%) | 高 | 对主线稳定性依赖大 |
| [事实] | 长尾项目 | 84 projects | 中 | 切换成本高 |
| [事实] | unknown | 19307/69751(27.7%) | 低 | 复盘归因弱 |
| [事实] | 回读验证 | 多次 show 校验 | 高 | 步骤耗时 |
| [事实] | 正确停止 | 队列空即停止 | 高 | 可能被误看作低产出 |
| [事实] | 脏区隔离 | 不提交 sibling x | 高 | 需要更多 worktree 管理 |
| [推断，置信度：高] | 总体节奏 | 高强度可控 | 中高 | 可持续性取决于降重复摩擦 |
| [建议] | 节奏目标 | 少开新入口，多封装旧入口 | 高 | 降低维护面 |

[事实] 近期多条工作线体现“质量优先于数量”：只提交 2 条待审回复、不为凑配额放宽候选门槛、6 个候选均不自然合格时记录 0 queued。
[事实] 这类行为说明节奏不是按吞吐最大化，而是按安全门槛推进。
[推断，置信度：高] 质量优先降低了短期产量，但提升了长期可持续性。
[推断，置信度：中] 如果用户同时要求多平台高频发布，这种高门槛会带来吞吐瓶颈。
[建议] 把“质量优先”拆成可调阈值：强制门禁不可降，候选池规模可调，发布目标可延后。

| 标签 | 节奏策略 | 样本 | 正向效果 | 代价 |
| --- | --- | --- | --- | --- |
| [事实] | 只提交待审 | 2 条待审回复 | 降低误发 | 吞吐低 |
| [事实] | 0 queued 可接受 | 6 个候选不合格 | 保质量 | 产出为空 |
| [事实] | 不直发 | review-gated | 可审计 | 周期变长 |
| [事实] | 不绕过门禁 | rejected 持久化 | 保合同 | 需要解释 |
| [事实] | 不伪造审核 | pending 空停止 | 保真实 | 无输出 |
| [推断，置信度：高] | 可持续性 | 质量优先 | 长期高 | 短期慢 |
| [建议] | 调节方式 | 调候选池，不降门禁 | 稳 | 降低系统性风险 |

[事实] 2026-05-27 同一天覆盖 infra、looper、tool、refine 多类任务。
[事实] 2026-05-25 同一天覆盖 looper、caff、aiproxy、refine、content systems 等多类任务。
[推断，置信度：高] 用户常在单日跨多个系统工作，节奏密度高。
[推断，置信度：中] 单日多系统切换会增加“路径/仓库/配置/队列状态”误判概率。
[建议] 高风险任务日应启用显式 preflight：cwd、git status、队列状态、provider env、允许写入路径。
[建议] 对并行 threads 明确文件所有权；本任务 L3 只写 `/tmp/cp_v3_l3.md` 是健康边界的正例。

| 标签 | 切换风险 | 证据 | 影响 | 控制动作 |
| --- | --- | --- | --- | --- |
| [事实] | 仓库误判 | 损坏 worktree、sibling repo | 最新状态误读 | 交叉核验 |
| [事实] | 脏工作区 | sibling x 脏文件 | 提交风险 | 不提交 |
| [事实] | 配置错配 | BASE_URL 不被 Refine 读取 | 调用失败 | env 显式映射 |
| [事实] | 队列状态 | pending 为空 | 无输入 | 停止 |
| [事实] | 路径边界 | 只读/只写约束 | 防越界 | 文件所有权 |
| [推断，置信度：高] | 切换成本 | 中高 | 影响可持续性 | preflight 必须常态化 |
| [建议] | 日内节奏 | 每次切 repo 先做 5 项核对 | 可执行 | 降错误密度 |

[事实] 当前认知级别分布中 competent 2988、proficient 2361、expert 2955，合计 8304/8777(94.6%)。
[事实] novice 65/8777(0.7%)，advanced_beginner 408/8777(4.6%)。
[推断，置信度：高] 高阶工作占比高，说明工作节奏更多受系统复杂度限制，而非基础能力限制。
[推断，置信度：中] 高阶任务长期占比过高会让休息与维护窗口被挤压。
[建议] 每周固定一个低认知负荷维护窗口，只处理文档索引、unknown 归因、队列清理和重复 bug 分类。

| 标签 | 认知级别 | 数量 | 占比 | L3 含义 |
| --- | --- | ---: | ---: | --- |
| [事实] | novice | 65 | 65/8777(0.7%) | 极少 |
| [事实] | advanced_beginner | 408 | 408/8777(4.6%) | 少 |
| [事实] | competent | 2988 | 2988/8777(34.0%) | 稳定执行 |
| [事实] | proficient | 2361 | 2361/8777(26.9%) | 系统判断 |
| [事实] | expert | 2955 | 2955/8777(33.7%) | 高阶主导 |
| [事实] | competent+proficient+expert | 8304 | 8304/8777(94.6%) | 高强度 |
| [推断，置信度：高] | 节奏风险 | 高阶占比过高 | 易累积维护债 | 需要低负荷窗口 |
| [建议] | 节奏安排 | 每周 1 个维护半天 | 不新增系统 | 降长期压力 |

[事实] 2026-05-31 Weekly 的战略广度为红灯，碎片化 56.2。
[事实] 2026-05-25 Weekly 的碎片化为 45.5。
[事实] 2026-05-24 Weekly 的碎片化为 50.0。
[事实] 当前 Mirror 的碎片化为 24%，明显低于 Weekly 45.5 至 56.2 区间。
[推断，置信度：中] 当前 Mirror 与 Weekly 的碎片化口径可能不同，不能直接线性比较。
[推断，置信度：高] 即便如此，工作方式仍同时存在“主线集中”和“项目长尾”两种状态。
[建议] 评价可持续性时同时看主线占比与 unknown 占比，不只看碎片化单值。

| 标签 | 指标 | 当前/近期值 | 解读 | 风险 |
| --- | ---: | ---: | --- | --- |
| [事实] | Mirror 碎片化 | 24% | 当前镜像较低 | 口径需注意 |
| [事实] | 2026-05-31 碎片化 | 56.2 | Weekly 高 | 红灯 |
| [事实] | 2026-05-25 碎片化 | 45.5 | Weekly 高 | 红灯 |
| [事实] | 2026-05-24 碎片化 | 50.0 | Weekly 高 | 红灯 |
| [推断，置信度：中] | 口径差异 | 存在 | 不可直接断言改善 | 需同口径追踪 |
| [建议] | 追踪方式 | 同一报告同一指标连续看 | 稳 | 避免误判 |

[事实] 与 2026-03-21 baseline 相比，当前输入可明确确认：工作已覆盖更多 projects，且 x/looper 成为主工作流中心。
[事实] 与 2026-03-21 baseline 相比，当前输入可明确确认：回读验证、payload_hash、review queue、safe_reply、content contract 成为重复出现的工作机制。
[推断，置信度：高] baseline 后的最大进步是“可审计执行”成为默认工作方式。
[推断，置信度：中] baseline 后的最大代价是每个系统都开始需要运营级维护，导致摩擦与切换成本上升。
[建议] baseline 对比不应写成单纯进步或退步；应写成“治理成熟度上升，维护复杂度同步上升”。

| 标签 | baseline 对比项 | 2026-03-21 baseline | 当前窗口 | 判断 |
| --- | --- | --- | --- | --- |
| [事实] | 可比数值 | 输入未提供单表 | Mirror/Weekly 可用 | 需注明限制 |
| [事实] | 数据窗口 | baseline 日期要求 | 2026-03-05 至 2026-05-27 | 当前覆盖 baseline 后 |
| [事实] | 工作机制 | 未在输入中详列 | review queue、safe_reply、contract | 当前机制清晰 |
| [事实] | 协作效能 | baseline 未给单值 | Mirror 黄灯、Weekly 红灯 | 波动 |
| [推断，置信度：高] | 成熟度 | baseline 后上升 | 可审计执行默认化 | 正向 |
| [推断，置信度：中] | 复杂度 | baseline 后上升 | 84 projects、unknown 高 | 风险 |
| [建议] | 结论写法 | 不编造 baseline 数值 | 用结构性对比 | 保真实 |

[事实] 当前 L3 的核心健康信号包括正确停止、回读验证、单入口收敛、质量门禁、拒绝伪造。
[事实] 当前 L3 的核心风险信号包括重复 bugfix、unknown 占比高、单日多系统切换、provider/env 配置易错、长尾项目维护。
[推断，置信度：高] 工作方式健康度总体为“中高，但处在高负荷治理期”。
[推断，置信度：高] 如果重复 bugfix 被单入口和单权威 schema 吸收，未来健康度会明显改善。
[推断，置信度：中] 如果继续增加新系统而不降低 unknown 与重复摩擦，摩擦密度会保持 2.8 至 2.9 区间。
[建议] 未来 2 周 L3 优先级：第一，降低 unknown 归因；第二，监控重复 bugfix；第三，固化 preflight；第四，限制同时可写主线。

| 标签 | 未来 2 周指标 | 当前值 | 目标 | 验证方式 |
| --- | --- | ---: | ---: | --- |
| [建议] | unknown 占比 | 19307/69751(27.7%) | 低于 20% | 项目归因统计 |
| [建议] | safe_reply 外发送入口 | 未给分母 | 0/N(0%) | rg 调用点 |
| [建议] | content_contract 缺失 payload | 未给分母 | 有拒绝即有原因 | queue audit |
| [建议] | 重复 bugfix 主题 | 5 类重复 | 低于 2 类 | bugfix 分类 |
| [建议] | pending 空停止 | 多次出现 | 100/N(100%) 有枚举 | queue log |
| [建议] | 回读验证 | 多次出现 | 100/N(100%) 关键写入后回读 | show 命令记录 |
| [建议] | 同时可写主线 | 未给单日上限 | 不超过 2 条 | 日计划 |
| [建议] | 摩擦密度 | 2.9 | 接近 2.3 | Mirror/Weekly 同口径 |

[事实] 本层只分析工作方式健康度，不评价 L1 认知深度、L2 战略广度或 L4 风险伦理的完整内容。
[事实] 本层没有写入 final report、INDEX 或其他 layer 文件。
[推断，置信度：高] L3 最重要的结论是：用户已经建立了高审计、高边界的 AI 协作方式，但系统数量与重复边界 bug 正在消耗可持续性。
[建议] 保持“真实输入、真实写入、真实回读”的原则；减少的是重复手工摩擦，不是减少验证。

## L4：成长处方
[事实] 数据窗口为 2026-03-05 到 2026-05-27，最新观测为 2026-05-27T13:38:00Z。
[事实] 本层只处理成长处方，不重写 L1/L2/L3 的画像、机制或诊断。
[事实] 总样本为 8777 个 sessions、69751 条 observations、41527 个 decisions、19447 个 bugfixes、84 个 projects。
[事实] 认知等级中 expert 为 2955/8777(33.7%)，proficient 为 2361/8777(26.9%)，competent 为 2988/8777(34.0%)。
[事实] 高阶区间 competent+proficient+expert 为 8304/8777(94.6%)。
[事实] 协作模式中 delegation 为 3660/8777(41.7%)，review 为 2712/8777(30.9%)，exploration 为 458/8777(5.2%)。
[事实] Mirror 当前信号为认知深度 yellow、战略广度 red、协作效能 yellow。
[事实] Mirror 当前指标为 Dreyfus 3.9、决策质量 56%、深度产出比 234%、知识获取 3.5。
[事实] Mirror 当前指标为探索率 5%、深耕率 19%、碎片化 24%。
[事实] Mirror 当前指标为委派率 42%、模式多样性 7、bug/决策 0.47、摩擦密度 2.9。
[事实] Mirror Weekly 2026-05-31 显示认知深度 yellow、战略广度 red、协作效能 red。
[事实] Mirror Weekly 2026-05-31 指标为 Dreyfus 4.1、决策质量 58.9、深度产出比 250.0、知识获取 3.1。
[事实] Mirror Weekly 2026-05-31 指标为探索率 12.1、深耕率 6.2、碎片化 56.2。
[事实] Mirror Weekly 2026-05-31 指标为委派率 31.8、模式多样性 5.0、bug/决策 0.4、摩擦密度 2.3。
[事实] 2026-05 月认知等级中 expert 为 1261/1367(92.2%)，proficient 为 71/1367(5.2%)，competent 为 25/1367(1.8%)。
[事实] 2026-03 月认知等级中 expert 为 152/2898(5.2%)，proficient 为 831/2898(28.7%)，competent 为 1591/2898(54.9%)。
[事实] 2026-03-21 baseline 本层采用 2026-03 月早期观测作为可用基线代理，因为输入只提供月度等级分布和后续 weekly 指标。
[推断，置信度：高] 与 2026-03-21 baseline 相比，当前能力瓶颈不再是能否完成复杂任务，而是复杂任务组合后的战略配额、沉淀节奏和运行健康。
[推断，置信度：高] L4 处方应优先约束工作组合，而不是继续增加新 workflow。
[建议] 90 天内将成长目标定义为：战略广度从 red 拉到 yellow，协作效能从 red/yellow 稳定到 yellow，认知深度保持 yellow 以上。

### 4.1 核心处方清单（5 条）
[事实] 4.1 必须 exactly 5 条处方；以下 5 条分别对应探索、深耕、碎片化、协作、质量兜底。

| 编号 | 处方名称 | 主靶点 | 当前证据 | 90 天目标 |
| --- | --- | --- | --- | --- |
| RX-1 | 探索配额处方 | 战略广度 | [事实] exploration 458/8777(5.2%)，Mirror 探索率 5% | [建议] 每周 2 个轻量探索 session，探索率稳定到 8%-12% |
| RX-2 | 深耕复利处方 | 深耕率 | [事实] Mirror 深耕率 19%，2026-05-31 weekly 深耕率 6.2 | [建议] 每周 1 个主线复盘/产品化沉淀块 |
| RX-3 | 碎片化闸门处方 | 注意力分散 | [事实] 84 个 projects，top15 observations 为 65658/69751(94.1%) | [建议] 主线外新增工作必须先通过 1 页 intent gate |
| RX-4 | 委派-审核再平衡处方 | 协作效能 | [事实] delegation 3660/8777(41.7%)，review 2712/8777(30.9%) | [建议] 对高风险任务采用 1:1 委派-审核闭环 |
| RX-5 | 质量兜底处方 | bug/决策与摩擦 | [事实] bugfixes 19447/decisions 41527(46.8%)，Mirror bug/决策 0.47 | [建议] 对重复 bug 建立 stop-rule 和 postmortem template |

#### RX-1：探索配额处方
[事实] 触发条件：Mirror 战略广度为 red，或探索率低于 8%，或一周内 exploration 低于 2 次。
[事实] 触发条件：当前 exploration 为 458/8777(5.2%)，低于 delegation 3660/8777(41.7%) 和 review 2712/8777(30.9%)。
[事实] 触发条件：2026-05-31 weekly 探索率为 12.1，但 mirror 当前 4 周均值探索率为 5%，说明短期改善未稳定。
[推断，置信度：高] 探索不足不是能力不足，而是执行系统过度占用认知带宽。
[建议] 行动步骤 1：每周固定 2 个 45 分钟 exploration session，只允许回答“是否值得进入主线”。
[建议] 行动步骤 2：每个 exploration session 只产出 3 个字段：机会、证据、下一步最小验证。
[建议] 行动步骤 3：探索结束后必须进入 stop/continue/park 三分支，不允许直接变成新项目。
[建议] 行动步骤 4：每周最多 1 个 exploration 进入 7 天 pilot，其他全部进入 backlog。
[建议] 行动步骤 5：pilot 必须绑定可观测指标，例如真实用户反馈、运行成功率、成本、发布结果。
[事实] 验证方法：统计下一周 exploration session 数是否达到 2/2(100.0%)。
[事实] 验证方法：统计每个 exploration 是否都有 opportunity/evidence/min-test 三字段。
[事实] 验证方法：统计进入 pilot 的探索是否不超过 1/2(50.0%)。
[事实] 验证方法：Mirror 探索率连续 4 周不低于 8% 即视为稳定改善。
[建议] 完成定义：连续 4 周 exploration 配额达成 8/8(100.0%)，且没有新增无验证项目。
[建议] 完成定义：战略广度从 red 至少提升到 yellow，或 red 但探索率与碎片化同时改善。
[推断，置信度：中] 该处方的核心不是“多试”，而是恢复高质量方向搜索。

#### RX-2：深耕复利处方
[事实] 触发条件：Mirror 深耕率低于 20%，或 weekly 深耕率低于 10%，或主线产物没有被二次复用。
[事实] 触发条件：Mirror 当前深耕率为 19%，2026-05-31 weekly 深耕率为 6.2。
[事实] 触发条件：top projects 中 x 为 22797/69751(32.7%)，looper 为 9733/69751(14.0%)，但周度深耕仍为 red。
[推断，置信度：高] 项目集中不等于深耕；深耕需要沉淀为可复用 contract、skill、evaluation 或发布资产。
[建议] 行动步骤 1：每周选择 1 条主线，只做一个“可复用资产”。
[建议] 行动步骤 2：可复用资产类型限定为 SKILL.md、contract.md、eval harness、runbook、metric dashboard、case study。
[建议] 行动步骤 3：每个资产必须绑定一个真实使用场景，不允许为整理而整理。
[建议] 行动步骤 4：资产写完后必须在同周真实调用 1 次，记录是否减少决策成本。
[建议] 行动步骤 5：如果不能在 7 天内复用，则降级为 note，不进入主线资产库。
[事实] 验证方法：每周检查可复用资产产出是否为 1/1(100.0%)。
[事实] 验证方法：每个资产是否有真实调用记录，目标为 1/1(100.0%)。
[事实] 验证方法：每月统计可复用资产中被二次调用的数量，目标为 3/4(75.0%)。
[事实] 验证方法：Mirror 深耕率连续 4 周不低于 15%，且 weekly 深耕率不再低于 6.2。
[建议] 完成定义：90 天内形成 10 个可复用资产，其中 6/10(60.0%) 被二次调用。
[建议] 完成定义：任一主线项目能明确展示“执行 → 复盘 → 工具化 → 复用”的闭环。
[推断，置信度：高] 对你而言，深耕处方应该从“项目时长”转向“资产复用率”。

#### RX-3：碎片化闸门处方
[事实] 触发条件：Mirror 碎片化高于 30%，或 weekly 碎片化高于 45%，或当天跨 4 个以上项目。
[事实] 触发条件：2026-05-31 weekly 碎片化为 56.2，2026-05-25 weekly 碎片化为 45.5，2026-05-24 weekly 碎片化为 50.0。
[事实] 触发条件：项目总数为 84/84(100.0%)，top15 占 65658/69751(94.1%)，存在长尾项目切换成本。
[推断，置信度：高] 碎片化的风险不是项目太多，而是无闸门切换导致“完成感”替代“复利”。
[建议] 行动步骤 1：每天设定最多 3 个 writable workstreams，其他请求只允许只读排查或排队。
[建议] 行动步骤 2：新增 writable workstream 前填写 intent gate：目标、证据、风险、done-when。
[建议] 行动步骤 3：如果 intent gate 不能在 5 分钟内写清，默认 park 24 小时。
[建议] 行动步骤 4：当天第 4 个项目只能进入 triage，不允许进入 implementation。
[建议] 行动步骤 5：每晚用 5 分钟归档“继续/停止/等待外部输入”的状态。
[事实] 验证方法：当天 writable workstreams 是否小于等于 3/3(100.0%)。
[事实] 验证方法：新增 workstream 是否有 intent gate，目标为 100%。
[事实] 验证方法：weekly 碎片化从 56.2 降到 40 以下即视为第一阶段有效。
[事实] 验证方法：主线项目 x/looper/harness/vibeguard/refine 的完成定义是否清晰可查。
[建议] 完成定义：连续 4 周 weekly 碎片化低于 45，且没有因切换导致的重复修复。
[建议] 完成定义：90 天内 top5 主线之外的新项目进入 implementation 不超过 6 次。
[推断，置信度：中] 这个处方会短期降低“响应所有机会”的速度，但会提高主线复利。

#### RX-4：委派-审核再平衡处方
[事实] 触发条件：delegation 占比高于 40%，review 占比低于 35%，或高风险任务没有新鲜验证输出。
[事实] 触发条件：delegation 为 3660/8777(41.7%)，review 为 2712/8777(30.9%)。
[事实] 触发条件：2026-05-31 weekly 委派率为 31.8，模式多样性为 5.0，协作效能为 red。
[推断，置信度：高] 委派已经是主力模式，但审核没有稳定成为等权闭环。
[建议] 行动步骤 1：把任务分成 low-risk、normal、high-risk 三档。
[建议] 行动步骤 2：high-risk 包括 auth、secrets、payments、发布、自动发送、数据库 migration、生产配置。
[建议] 行动步骤 3：high-risk 必须先写执行合同，再执行，再读回验证。
[建议] 行动步骤 4：normal 任务至少保留一个 fresh command 或 screenshot 作为完成证据。
[建议] 行动步骤 5：low-risk 任务允许快速执行，但交付时仍要说明验证缺口。
[事实] 验证方法：抽样 10 个 delegation 任务，检查是否有验证证据，目标 8/10(80.0%)。
[事实] 验证方法：抽样 high-risk 任务，检查是否都有 preflight 和 read-back，目标 100%。
[事实] 验证方法：review 占比从 30.9% 提升到 35% 左右，但不超过 delegation 造成瓶颈。
[事实] 验证方法：bug/decision 从 0.47 降到 0.40 以下。
[建议] 完成定义：连续 30 天高风险任务未出现“已声明完成但无新鲜验证”的交付。
[建议] 完成定义：每周至少 1 次对 agent 输出做 adversarial review，而不是仅做顺手收尾。
[推断，置信度：高] 你最需要的不是减少委派，而是让审核成为委派系统的同级组件。

#### RX-5：质量兜底处方
[事实] 触发条件：同一问题连续 2 次修复失败，或 bug/decision 高于 0.45，或出现 silent degradation 风险。
[事实] 触发条件：bugfixes 为 19447/decisions 41527(46.8%)，Mirror bug/决策为 0.47。
[事实] 触发条件：输入样本反复出现 schema 漂移、发送链路分散、empty_reason 自由文本、@author 冗余、like/reply 强耦合等缺陷。
[推断，置信度：高] 质量问题的主要来源是接口合同和副作用边界，而不是单点技术能力。
[建议] 行动步骤 1：每个重复 bug 必须写 root cause，不允许只写 symptom fix。
[建议] 行动步骤 2：同类 bug 第 2 次出现时，将修复升级为 contract/eval/test，而不是再补丁。
[建议] 行动步骤 3：同类 bug 第 3 次出现时停止实现，重审架构或入口边界。
[建议] 行动步骤 4：对状态写入、发送、发布、migration、ingest 等路径增加 read-back 验证。
[建议] 行动步骤 5：对 silent fallback 改成 visible error、explicit skipped state 或 audit record。
[事实] 验证方法：重复 bug 是否有 root cause 字段，目标 100%。
[事实] 验证方法：二次重复 bug 是否升级为 test/contract，目标 80%。
[事实] 验证方法：三次失败是否触发 stop-rule，目标 100%。
[事实] 验证方法：bug/decision 连续 4 周低于 0.40。
[建议] 完成定义：90 天内至少 5 个高频缺陷类别被转成 contract/eval/test。
[建议] 完成定义：无 silent swallowing、无无证据成功声明、无绕过门禁发布。
[推断，置信度：高] 质量兜底的成长收益最高，因为它直接降低认知摩擦和协作摩擦。

### 4.2 决策树：下一步行动选择
[事实] 决策树输入变量为：战略广度信号、协作效能信号、bug/decision、碎片化、探索配额、深耕资产复用率。
[建议] 决策树只用于选择下一步行动，不用于评价人格或能力。

| 节点 | 条件 | 下一步 | 标签 |
| --- | --- | --- | --- |
| D1 | [事实] 战略广度为 red 且探索率低于 8% | [建议] 先执行 RX-1，不新增主线 | 探索优先 |
| D2 | [事实] 战略广度为 red 但 weekly 碎片化高于 45 | [建议] 先执行 RX-3，冻结第 4 个 writable 项目 | 收敛优先 |
| D3 | [事实] 认知深度为 green/yellow 且深耕率低于 15 | [建议] 执行 RX-2，产出可复用资产 | 复利优先 |
| D4 | [事实] 协作效能为 red/yellow 且 high-risk 任务增多 | [建议] 执行 RX-4，补 preflight/read-back | 审核优先 |
| D5 | [事实] bug/decision 高于 0.45 | [建议] 执行 RX-5，暂停同类补丁扩散 | 质量优先 |
| D6 | [事实] exploration 已达 2/2(100.0%) 但无 pilot | [建议] 选 1 个机会做 7 天验证 | 选择优先 |
| D7 | [事实] 可复用资产低于 1/周 | [建议] 把本周最大修复转成 runbook/eval | 沉淀优先 |
| D8 | [事实] 当天已跨 3 个 writable 项目 | [建议] 第 4 个请求只读 triage，不写代码 | 边界优先 |
| D9 | [事实] 同一 bug 二次出现 | [建议] 升级为 contract/test，不再只修 symptom | 结构优先 |
| D10 | [事实] 同一 bug 三次出现 | [建议] 停止实现，重审架构假设 | 停止优先 |

[推断，置信度：高] 如果 D1、D2、D5 同时触发，优先级应为 D5 > D2 > D1。
[推断，置信度：高] 原因是质量红灯会污染探索结果，碎片化会吞噬探索收益。
[建议] 每天开工前只问 4 个问题：今天主线是什么、今天 writable 上限是多少、今天是否需要探索、今天哪个风险必须 read-back。
[建议] 每周复盘只问 4 个问题：探索是否达标、深耕是否复用、碎片化是否下降、bug/decision 是否下降。
[事实] 决策树完成信号不是“执行了更多任务”，而是“下一步行动变少且更确定”。

### 4.3 90 天成长路线图
[事实] 90 天路线图按 13 周设计，分为校准、收敛、复利、稳态四段。
[事实] 当前基线可用指标包括 8777 sessions、69751 observations、84 projects、Mirror 3 维信号、monthly level counts、weekly metrics。
[建议] 路线图只设少量可核验指标，避免把成长计划变成新项目。

| 周期 | 周数 | 主目标 | 核心动作 | 验证指标 |
| --- | --- | --- | --- | --- |
| P1 | W1 | [建议] 建立当周 workstream 闸门 | 每天最多 3 个 writable workstreams | [事实] 5/5 工作日记录主线与第 4 项处理 |
| P1 | W1 | [建议] 建立探索配额 | 2 个 45 分钟 exploration | [事实] exploration 2/2(100.0%) |
| P1 | W2 | [建议] 建立质量 stop-rule | 重复 bug 二次升级 contract/test | [事实] 重复 bug root cause 100% |
| P1 | W2 | [建议] 建立 high-risk 审核 | high-risk 必须 preflight/read-back | [事实] high-risk 任务 100% 有证据 |
| P2 | W3 | [建议] 第一份复用资产 | 从 x/looper/refine 中选一条主线 | [事实] 资产 1/1(100.0%) |
| P2 | W3 | [建议] 第一轮碎片化压降 | 第 4 项只读 triage | [事实] writable 项目 <=3/日 |
| P2 | W4 | [建议] 第二份复用资产 | 把一个 repeated fix 转成 runbook/eval | [事实] 资产被真实调用 1 次 |
| P2 | W4 | [建议] 4 周检查点 | 对比 Mirror 探索率、深耕率、碎片化 | [事实] 至少 2/3 指标改善 |
| P3 | W5 | [建议] Pilot 选择周 | 只允许 1 个 exploration 进入 pilot | [事实] pilot 1/2(50.0%) |
| P3 | W6 | [建议] 审核强度校准 | 抽样 10 个 delegation 任务 | [事实] 证据合格 8/10(80.0%) |
| P3 | W7 | [建议] 质量资产化 | 第 3 个 contract/eval/test | [事实] 高频缺陷资产 3/5(60.0%) |
| P3 | W8 | [建议] 中期复盘 | 停止一个低 ROI 工作流 | [事实] stop/continue/park 全部明确 |
| P4 | W9 | [建议] 主线复利加速 | x/looper/refine 任选一条做案例化沉淀 | [事实] 产物被二次调用 |
| P4 | W10 | [建议] 降摩擦周 | 聚焦 bug/decision 与摩擦密度 | [事实] bug/decision 低于 0.45 |
| P4 | W11 | [建议] 战略广度回测 | 检查探索是否带来真实新方向 | [事实] 4 周探索 8/8(100.0%) |
| P4 | W12 | [建议] 协作效能回测 | 检查 high-risk 完成证据 | [事实] 无无证据完成声明 |
| P4 | W13 | [建议] 90 天总结 | 只保留有效处方，废弃无效处方 | [事实] 6 项完成定义至少达成 4/6(66.7%) |

[推断，置信度：高] W1-W2 的重点是控制面，不是产出量。
[推断，置信度：高] W3-W8 的重点是把已经存在的强执行力转成复用资产。
[推断，置信度：中] W9-W13 的重点是检查处方是否真的降低摩擦，而不是检查文档是否完整。
[建议] 每周固定输出一个 10 行以内的 growth checkpoint：指标、完成、阻塞、下周唯一改变。
[建议] 如果某周爆发生产事故，则路线图自动暂停 48 小时，优先 RX-5。
[建议] 如果某周出现重大机会，则先做 RX-1 exploration，不允许直接吞并整个路线图。
[事实] 90 天路线图的最小成功标准为：探索达标 8/13 周、复用资产 8/13 周、碎片化改善 2 次、bug/decision 下降 1 次。
[建议] 90 天路线图的理想成功标准为：战略广度 yellow、协作效能 yellow、认知深度不低于 yellow。

### 4.4 风险兜底方案
[事实] 风险兜底至少覆盖 5 类风险；本层覆盖 8 类。

| 风险 | 触发信号 | 缓解措施 | 兜底方案 | 验证 |
| --- | --- | --- | --- | --- |
| R1 处方变成新负担 | [事实] 每周复盘超过 30 分钟 | [建议] checkpoint 限制 10 行 | [建议] 删除最低价值指标 | [事实] 复盘时长 <=30 分钟 |
| R2 探索变成新项目泛滥 | [事实] 2 个探索都进入 pilot | [建议] 每周最多 1 个 pilot | [建议] 多余机会 park 7 天 | [事实] pilot <=1/周 |
| R3 深耕资产无人复用 | [事实] 资产 2 周未调用 | [建议] 降级为 note | [建议] 删除或合并到 runbook | [事实] 复用率 >=60% |
| R4 碎片化压制机会 | [事实] 重要机会无法进入执行 | [建议] 开 45 分钟 exploration | [建议] 替换最低 ROI 当前项目 | [事实] 替换记录明确 |
| R5 审核过重拖慢吞吐 | [事实] review 队列堆积超过 3 天 | [建议] 按风险分层审核 | [建议] low-risk 快速通过但标注缺口 | [事实] high-risk 仍 100% read-back |
| R6 bug stop-rule 被忽视 | [事实] 同类 bug 第 3 次继续补丁 | [建议] 强制停止实现 | [建议] 只允许架构复盘 | [事实] 有 stop 记录 |
| R7 指标口径漂移 | [事实] weekly 与 mirror 口径不一致 | [建议] 标注来源与窗口 | [建议] 只比较同源指标 | [事实] 每个指标有来源 |
| R8 并行 thread 越界 | [事实] L1/L2/L3/L4 写入边界混乱 | [建议] 每层只写自有文件 | [建议] 缺层由 dispatcher 处理 | [事实] L4 只写 /tmp/cp_v3_l4.md |

[推断，置信度：高] 最大风险不是执行失败，而是处方体系本身变成额外复杂度。
[建议] 所有兜底方案都遵循“先降级，再停止，再重审”，不把失败伪装成迭代。
[建议] 对任何 red 指标，不允许用单次高光任务抵消；必须用连续 4 周趋势验证。
[建议] 对任何 missing data，不补写假数字；用 no data 标注并降低置信度。
[事实] 输入已显示 weekly 与 mirror 的探索率、深耕率、碎片化存在不同窗口口径。
[推断，置信度：中] 因此 4.5 的 baseline 对比应把“同源周报”和“总量月度”分开解释。

### 4.5 与上版本对比（时间序列）
[事实] 本节包含 2026-03-21 baseline 对比；输入未给出 2026-03-21 当日 mirror score，因此使用 2026-03 月观测和后续 weekly 作为基线代理。
[事实] 2026-03 月等级总量为 2898/8777(33.0%)，2026-04 为 4512/8777(51.4%)，2026-05 为 1367/8777(15.6%)。
[事实] 2026-05-24、2026-05-25、2026-05-31 weekly 指标可用于最近三次时间序列。

| 指标 | 2026-03-21 baseline/代理 | 2026-05-24 | 2026-05-25 | 2026-05-31/当前 | 变化判断 |
| --- | --- | --- | --- | --- | --- |
| Expert 占比 | [事实] 152/2898(5.2%) | [事实] no weekly level data | [事实] no weekly level data | [事实] 1261/1367(92.2%) in 2026-05 | [推断，置信度：高] 高阶执行显著上升 |
| Proficient 占比 | [事实] 831/2898(28.7%) | [事实] no weekly level data | [事实] no weekly level data | [事实] 71/1367(5.2%) in 2026-05 | [推断，置信度：中] 中高阶被 expert 吸收 |
| Competent 占比 | [事实] 1591/2898(54.9%) | [事实] no weekly level data | [事实] no weekly level data | [事实] 25/1367(1.8%) in 2026-05 | [推断，置信度：高] 基础执行已非瓶颈 |
| Dreyfus | [事实] no 03-21 value | [事实] 4.3 | [事实] 4.3 | [事实] 4.1 weekly / 3.9 mirror | [推断，置信度：中] 从高位回落到 yellow |
| 决策质量 | [事实] no 03-21 value | [事实] 71.4 | [事实] 66.1 | [事实] 58.9 weekly / 56 mirror | [推断，置信度：高] 最近三次下降 |
| 深度产出比 | [事实] no 03-21 value | [事实] 118.6 | [事实] 93.1 | [事实] 250.0 weekly / 234 mirror | [推断，置信度：中] 产出深度高但可能不稳 |
| 知识获取 | [事实] no 03-21 value | [事实] 4.4 | [事实] 4.2 | [事实] 3.1 weekly / 3.5 mirror | [推断，置信度：高] 知识摄入回落 |
| 探索率 | [事实] no 03-21 value | [事实] 9.6 | [事实] 8.7 | [事实] 12.1 weekly / 5 mirror | [推断，置信度：中] 周度好转但 4 周均值不足 |
| 深耕率 | [事实] no 03-21 value | [事实] 7.1 | [事实] 4.5 | [事实] 6.2 weekly / 19 mirror | [推断，置信度：中] 口径差异大，仍需资产复用验证 |
| 碎片化 | [事实] no 03-21 value | [事实] 50.0 | [事实] 45.5 | [事实] 56.2 weekly / 24 mirror | [推断，置信度：高] weekly 层面仍为核心风险 |
| 委派率 | [事实] no 03-21 value | [事实] 38.5 | [事实] 37.7 | [事实] 31.8 weekly / 42 mirror | [推断，置信度：中] 协作口径波动 |
| 模式多样性 | [事实] no 03-21 value | [事实] 6.0 | [事实] 6.0 | [事实] 5.0 weekly / 7 mirror | [推断，置信度：中] 短期多样性下降 |
| bug/决策 | [事实] no 03-21 value | [事实] 0.7 | [事实] 0.6 | [事实] 0.4 weekly / 0.47 mirror | [推断，置信度：中] 周度改善但总量仍高 |
| 摩擦密度 | [事实] no 03-21 value | [事实] 2.9 | [事实] 2.8 | [事实] 2.3 weekly / 2.9 mirror | [推断，置信度：中] 周度改善未转化为均值改善 |

[事实] 与 2026-03-21 baseline 代理相比，最确定的变化是 expert 占比从 152/2898(5.2%) 到 1261/1367(92.2%)。
[事实] 与 2026-03-21 baseline 代理相比，competent 占比从 1591/2898(54.9%) 到 25/1367(1.8%)。
[推断，置信度：高] 这表示能力结构已经从“学习/执行”迁移到“系统治理/复杂委派”。
[推断，置信度：高] 但 2026-05-31 决策质量 58.9 低于 2026-05-24 的 71.4，说明高阶任务密度上升后质量控制压力变大。
[推断，置信度：高] 2026-05-31 碎片化 56.2 高于 2026-05-25 的 45.5，也高于 2026-05-24 的 50.0，是最直接的近期风险。
[推断，置信度：中] 深度产出比 250.0 很高，但如果深耕率只有 6.2，则可能代表“局部深产出”而非“长期复利”。
[建议] 与上版本相比，下一版不要再主要追求更高 expert 占比，而要追求战略广度与协作效能信号转黄。
[建议] 下次复盘必须保留 2026-03-21 baseline 代理说明，除非有当日 mirror 原始数据可替换。

### 4.6 执行优先级矩阵
[事实] 执行优先级按 impact/effort 2x2 矩阵组织。
[事实] impact 取决于是否能改善战略广度、协作效能、bug/decision、碎片化。
[事实] effort 取决于是否需要新增系统、跨项目协调、长期维护。

| 象限 | 定义 | 处方/行动 | 理由 |
| --- | --- | --- | --- |
| 高影响/低成本 | [事实] 不新增系统，只改变每日选择 | [建议] RX-3 每天最多 3 个 writable workstreams | [推断，置信度：高] 直接压低碎片化 56.2 |
| 高影响/低成本 | [事实] 不新增系统，只增加验证句柄 | [建议] RX-4 high-risk read-back | [推断，置信度：高] 降低无证据完成与协作摩擦 |
| 高影响/低成本 | [事实] 不新增系统，只设停止条件 | [建议] RX-5 三次失败 stop-rule | [推断，置信度：高] 阻断重复 bug 扩散 |
| 高影响/高成本 | [事实] 需要持续 13 周执行 | [建议] RX-2 每周复用资产 | [推断，置信度：高] 能把强执行转成复利 |
| 高影响/高成本 | [事实] 需要选择与放弃 | [建议] 每月停止一个低 ROI 工作流 | [推断，置信度：中] 可释放探索与深耕带宽 |
| 低影响/低成本 | [事实] 轻量辅助 | [建议] 每周 10 行 growth checkpoint | [推断，置信度：中] 提供趋势可见性但不能替代执行 |
| 低影响/低成本 | [事实] 轻量辅助 | [建议] 每个 exploration 填 3 字段 | [推断，置信度：中] 降低探索发散 |
| 低影响/高成本 | [事实] 容易变成额外工程 | [建议] 暂缓新 dashboard、新 automation、新 INDEX 改造 | [推断，置信度：高] 当前瓶颈不是缺少更多系统 |

| 优先级 | 行动 | 启动时间 | 停止条件 | 成功信号 |
| --- | --- | --- | --- | --- |
| P0 | [建议] 三次失败 stop-rule | 今天 | [事实] 同类 bug 不再重复 | [事实] bug/decision <0.45 |
| P0 | [建议] 第 4 个 writable 项目闸门 | 今天 | [事实] weekly 碎片化 <45 连续 4 周 | [事实] 当天项目切换可解释 |
| P1 | [建议] high-risk preflight/read-back | 本周 | [事实] high-risk 100% 有验证 | [事实] 无无证据完成声明 |
| P1 | [建议] 每周 2 个 exploration | 本周 | [事实] 探索率稳定 8%-12% | [事实] 战略广度不再 red |
| P2 | [建议] 每周 1 个复用资产 | W3 开始 | [事实] 复用资产 6/10(60.0%) 被调用 | [事实] 深耕率不再低于 10 |
| P2 | [建议] 每月停止低 ROI 工作流 | W8 开始 | [事实] stop/continue/park 明确 | [事实] 主线带宽恢复 |

[事实] P0 行动不依赖外部工具，也不需要改 repo。
[事实] P1 行动依赖日常执行纪律，但不要求新增自动化。
[事实] P2 行动需要持续复盘，适合在 P0/P1 稳定后执行。
[推断，置信度：高] 最合理的启动顺序是 RX-5、RX-3、RX-4、RX-1、RX-2。
[推断，置信度：高] 先降质量与碎片化风险，探索和深耕才不会被新噪声吞掉。
[建议] 今天的唯一 P0 执行清单：设置 3 个 writable 上限、标记 high-risk 任务、同类 bug 二次即转 contract/test。
[建议] 本周的唯一 P1 执行清单：完成 2 个 exploration、抽样 10 个 delegation、交付 1 个复用资产候选。
[建议] 30 天检查：战略广度不再连续 red，weekly 碎片化至少低于 45 一次。
[建议] 60 天检查：复用资产达到 5 个，其中 3/5(60.0%) 被真实调用。
[建议] 90 天检查：6 个完成定义至少达成 4/6(66.7%)，否则删除最低价值处方。
[事实] 执行记录字段 1：date，必须记录处方执行日期。
[事实] 执行记录字段 2：rx_id，必须为 RX-1 到 RX-5 之一。
[事实] 执行记录字段 3：trigger，必须引用本层触发条件之一。
[事实] 执行记录字段 4：action_taken，必须描述当天实际动作。
[事实] 执行记录字段 5：evidence，必须包含命令输出、读回结果、产物路径或指标截图之一。
[事实] 执行记录字段 6：result，必须为 complete、partial、blocked、stopped 之一。
[建议] 如果 result=partial，则下一步只能是补验证或降级，不能直接声明完成。
[建议] 如果 result=blocked，则记录外部依赖和下次可重试条件。
[建议] 如果 result=stopped，则记录停止原因和被释放的时间/注意力预算。
[推断，置信度：高] 这 6 个字段能把处方从建议变成可审计执行记录。
[推断，置信度：中] 如果 30 天内记录字段被省略，说明处方仍停留在理解层而非运行层。
[建议] 处方执行不需要新自动化；先用手工记录验证 4 周，再决定是否技能化。
[建议] 技能化门槛为：同一记录格式连续使用 8/8(100.0%)，且至少减少 1 类重复 bug。
[事实] 若无当日数据，则记录 no data，不补写估计值。
[事实] 若指标来自不同窗口，则必须标注 mirror、weekly、monthly 或 manifest。
[事实] L4 处方完成定义：5 条处方齐全、每条包含触发条件/行动步骤/验证方法/完成定义、包含决策树、90 天路线图、风险兜底、baseline 对比、2x2 矩阵。
[事实] 本文件写入目标为 /tmp/cp_v3_l4.md。

---

## 附录 A：Mirror Score

```text
Mirror 认知镜像

  认知深度         🟡  Dreyfus 3.9 ✗ | 决策质量 56% ✗ | 深度产出比 234% ✗ | 知识获取 3.5 ✗
  战略广度         🔴  探索率 5% ✗ | 深耕率 19% ✗ | 碎片化 24% ✗
  协作效能         🟡  委派率 42% ✗ | 模式多样性 7 ✗ | bug/决策 0.47 ✗ | 摩擦密度 2.9 ✗
  基线: 个人(4周均值)
  数据范围: 2026-03-05 ~ 2026-05-27
```

## 附录 B：数据文件

| 文件 | 内容 |
|---|---|
| `/tmp/cp_data_1.txt` | Recent observations overview |
| `/tmp/cp_data_2.txt` | Decision patterns |
| `/tmp/cp_data_3.txt` | Bug fix patterns |
| `/tmp/cp_data_4.txt` | Knowledge and patterns acquired |
| `/tmp/cp_data_5.txt` | Friction and collaboration mode |
| `/tmp/cp_data_6.txt` | Project distribution |
| `/tmp/cp_data_7.txt` | Cognitive level distribution over time |
| `/tmp/cp_data_8.txt` | Recent session insights documents |

## 附录 C：生成元数据

| 字段 | 值 |
|---|---|
| 生成日期 | 2026-06-02 |
| Skill 版本 | cognitive-portrait v3 (Codex threads) |
| Dispatcher | 主 Codex 会话 |
| Threads | 4 |
| 并行方式 | Codex multi_agent threads |
| 数据采集方式 | Dispatcher 本地数据准备 → `/tmp/cp_data_*.txt` |
| 数据库 | `/Users/lifcc/Library/Application Support/refine/refine.db` |
| Mirror score | `/tmp/cp_mirror_score.txt` |
| 基线报告 | `docs/cognitive-portraits/cognitive-portrait-2026-03-21-v0.md` |
| 写作规范 | 三分离标签（事实/推断/建议）+ 精确数值驱动 + 矩阵化 |
| 行数下限 | L1/L2/L3 ≥250 / L4 ≥280 |
| 实际行数 | L1=284 / L2=278 / L3=390 / L4=284 |
| W-14 文件所有权 | 每个 Codex thread 只写自己的 `/tmp/cp_v3_l{N}.md` |

### C.1 Thread 摘要

| Thread | 文件 | 行数 | [事实] | [推断] | [建议] |
|---|---|---:|---:|---:|---:|
| L1 认知演进 | `/tmp/cp_v3_l1.md` | 284 | 188 | 133 | 20 |
| L2 战略定位 | `/tmp/cp_v3_l2.md` | 278 | 126 | 57 | 30 |
| L3 工作方式健康度 | `/tmp/cp_v3_l3.md` | 390 | 185 | 71 | 52 |
| L4 成长处方 | `/tmp/cp_v3_l4.md` | 284 | 205 | 49 | 121 |
| **合计** | — | **1236** | **704** | **310** | **223** |
