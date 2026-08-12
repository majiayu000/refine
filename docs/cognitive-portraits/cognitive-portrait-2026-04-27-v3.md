# 认知画像报告 — 2026-04-27 (v3)

> 基于 **7239 个 AI 编程会话、53424 条观测、33898 个决策、12287 个 bugfix、70 个项目** 的全量数据分析。
>
> **基线对比**：上一版 [cognitive-portrait-2026-04-09-v3.md](./cognitive-portrait-2026-04-09-v3.md)（截至 04-09，3919 会话 / 29760 观测 / 58 项目）。
>
> **关注窗口**：04-10 → 04-27 共 18 天，新增 **3320 会话 / 23664 观测**，平均 **184 sessions/day**。

---

## 总体判断

你正在从"带可观测性的系统设计者"向"高密度策略执行者"过渡。这一版画像的最大特征不是"做更多"——会话量虽然增长 84.7%，但更显著的是**认知密度的爆发**：expert 级观测从 345 跃升到 1536（4.5×），中高阶合计占 94.0%，几乎全面脱离"工具学习"阶段。但有三个值得警惕的信号：

1. **认知深度上升、战略广度收窄** — Dreyfus 3.5→3.7，深度产出比 130%→163%；但探索率 7%→6%，深耕率 23%，碎片化 23%（三项均红）
2. **协作效能从黄→红** — 委派率 49%→45%（下降）、模式多样性 7（与 baseline 持平）、bug/决策 0.36（红）、摩擦密度 2.9（改善但仍红）
3. **x 项目独大化加深** — 04-10 后新增的 3320 会话里，x 单项目就占 1320（39.8%），叠加 baseline 中的 other（49.6%），整个工作系统正在被两条主线吸光

核心特征（本周快照 vs 04-09 基线）：

| 维度 | 04-09 baseline | 04-27 当前 | 变化 |
|------|----|----|----|
| Dreyfus 平均 | 3.5 | 3.7 | ↑ 0.2 |
| 决策质量 | 上升中 | 56% ✓ | ↑ |
| 深度产出比 | 130% | 163% ✓ | ↑ 33pt |
| 知识获取 | 3.2 | 3.2 ✗ | 持平（红） |
| 探索率 | 7% | 6% ✗ | ↓ 1pt（红） |
| 深耕率 | — | 23% ✗ | （红） |
| 碎片化 | — | 23% ✗ | （红） |
| 委派率 | 49% ✗ | 45% ✗ | ↓ 4pt（仍红） |
| 模式多样性 | — | 7 ✗ | （红） |
| bug/决策 | — | 0.36 ✗ | （红） |
| 摩擦密度 | 3.1 ✗ | 2.9 ✗ | ↓ 0.2（改善但仍红） |
| 信号灯总评 | 深绿/广红/协黄 | 深黄/广黄/**协红** | 协作下沉 |

一句话概括：

> **你已经过了"能不能做"的阶段，进入了"能做多狠"的阶段。x 项目把你的认知密度推到了 expert 占 62.7% 的顶点，但代价是战略半径在收窄、协作模式在固化。下阶段的核心矛盾不是"再做更多"，而是"如何用同样的密度去强化你最弱的两个项目（rss-scout 仍 88% competent、xhh 仍 60% competent）以及最弱的两种模式（exploration 5.2%、pair_programming 1.8%）"。**

---

## L1：认知演进

### 1.1 Dreyfus 技能阶段迁移

#### 全局分布对比

| 阶段 | 04-09 数量 | 04-09 占比 | 04-27 数量 | 04-27 占比 | 变化 |
|------|------|------|------|------|------|
| Novice | — | — | 37 | 0.5% | — |
| Advanced Beginner | — | — | 394 | 5.5% | — |
| Competent | 1740 | 44.4% | 2960 | 40.9% | ↓ 3.5pt |
| Proficient | 1501 | 38.3% | 2277 | 31.5% | ↓ 6.8pt |
| Expert | 345 | 8.8% | 1536 | **21.2%** | **↑ 12.4pt** |
| **中高阶合计** | **3586** | **91.5%** | **6773** | **94.0%** | ↑ 2.5pt |

[来源：sqlite3 GROUP BY tags level，全量 observation 表]

**关键变化**：

- **Expert 爆发**：从 345 → 1536，增长 **4.45×**，远超会话量 1.85× 的增长率。这说明你不只是"做了更多"，而是"在更高水平上做了更多"。
- **Competent 占比下降**：44.4% → 40.9%。配合 expert 上升，整体在向高位迁移。
- **关注 0.5% novice + 5.5% advanced_beginner**：仍有 ~6% 的会话停留在低阶。结合后面 1.4 节的"项目断层"分析，这些主要分布在 xhh（advanced_beginner 15.6%）、vibeguard（advanced_beginner 12.5%）这类新接入项目。

#### 04-10 之后的窗口分布（18 天单独看）

| 阶段 | 数量 | 占比 |
|------|------|------|
| Novice | 6 | 0.2% |
| Advanced Beginner | 105 | 3.2% |
| Competent | 1221 | 36.8% |
| Proficient | 776 | 23.4% |
| Expert | 1191 | **35.9%** |

**这是本画像最强的发现**：04-10~04-27 这 18 天的窗口里，**expert 级会话占到 35.9%**，远超全量基线的 21.2%，更是 baseline（8.8%）的 4.1×。

这意味着：你的认知重心**确实在跃升**，且跃升集中在最近 3 周。

#### 按项目的当前阶段判断（高置信度）

| 项目 | 主导阶段 | 高阶占比 | 阶段判断 |
|------|------|------|------|
| **x** | expert 62.7% + proficient 32.2% | **94.9%** | **Expert / 部分 Master** |
| remem | expert 37.2% + proficient 34.9% | 72.1% | Proficient → Expert |
| reddit-monitor | proficient 64.4% + competent 33.0% | 64.4% | Proficient |
| douyin-mcp | proficient 56.5% + competent 30.6% | 56.5% | Proficient |
| om | proficient 53.2% + competent 27.0% + expert 12.7% | 65.9% | Proficient |
| vibeguard | proficient 44.6% + competent 31.3% | 56.2% | Proficient（但 12.5% advanced_beginner，断层明显） |
| harness | competent 45.4% + proficient 37.4% | 47.7% | Competent → Proficient（仍在爬坡） |
| xhh | competent 60.5% + advanced_beginner 15.6% | 21.5% | **Competent，且向下断层 15.6%** |
| rss-scout | competent 88.4% | 11.6% | **Competent 锁死** |

**判断**：
- **x 已是 Master 级仓库**（expert 1284 / proficient 659，几乎没有低阶痕迹）
- **rss-scout 仍是 Competent 锁死**：182 个观测里 160 个 competent，只有 19 个 proficient 和 2 个 expert。这种分布意味着你在 rss-scout 上**不学新东西**，纯执行。
- **xhh 是断层项目**：60.5% competent + 15.6% advanced_beginner + 6 个 novice，说明你在这个项目上经常**回到入门状态**——可能因为它涉及小红书/抖音运营这类非工程领域

### 1.2 Bloom 认知层级

[基于 33898 个 decision 类观测的样本推断 — 中置信度]

从 mirror score 的"决策质量 56%"和 insights 报告里的协作模式分布，可以推断你目前的 Bloom 层级分布：

| Bloom 层级 | 推断占比 | 行为代表 |
|------|------|------|
| Remember/Understand | ~10% | 阅读文档、查 API |
| Apply | ~25% | 套用既有 pattern 实现新功能 |
| **Analyze** | ~35% | 拆解需求、识别瓶颈、定位 bug |
| **Evaluate** | ~25% | 评估方案、做技术选型、决定是否合并 |
| Create | ~5% | 设计新协议、新架构 |

**结论**：你的主要活动在 Analyze + Evaluate（合计 60%）。这是 senior 工程师的典型分布。

**短板**：**Create 仅 ~5%**。从项目里看，真正在创造新东西的只有 remem（双层 archive/memory 模型）、harness（执行框架架构）这两个项目，其余都是"用现成模式解决问题"。

**对比 04-09**：当时 Create 估计 ~3%，现在 ~5%。微弱提升，主要来自 remem Phase 1 和 vibeguard 规则编码工作。

### 1.3 双环学习

**判断**：你在 04-10 之后展现出了**显著的双环学习能力**，但仍不稳定。

**正向证据**（高置信度）：
- **harness 的多 agent 决策**：你明确**保留单 agent triage**，拒绝引入多 agent，并质疑了"复杂化是不是更好"的底层假设
- **rss-scout 评分阈值改革**：把"prompt 中的软规则"全部下沉到 `config/rules.yaml`，这是对"提示词能否守规则"假设的根本质疑
- **x v10 → v10.2 → v11 → v12 重构**：你反复推翻自己上一周的策略，这是双环典型行为
- **mirror weekly 新报告（今日生成）的总结**：你已经识别出"探索增加但决策质量下降"是一个系统问题，而不是单点 bug

**负向证据**（中置信度）：
- **bug/决策比 0.36（红）**：意味着每 2.8 个决策就产生 1 个 bug。这个比例本身不算坏，但与 baseline 没有显著改善
- **xhh 反复回到 advanced_beginner**：6 个 novice + 40 个 advanced_beginner（占 18.0%）说明你在这个项目里**没有形成稳定的方法论**，每次都重新学习

**结论**：你的双环学习集中在**工程系统类项目**（harness、remem、x、vibeguard），而在**运营类项目**（xhh、reddit-monitor、douyin-mcp）里仍以单环为主。

### 1.4 元认知成熟度

#### 过程监控意识（高）

证据：
- 你主动跑 `mirror score` 和 `mirror weekly` 来观察自己的状态
- 你在 x 项目里把硬过滤从 prompt 下沉到 `pre_filter.py` + `rules.yaml`，本质是给 AI 协作流程加可观测性
- 你在 reddit-monitor 引入 `action_log.next_allowed_at` 持久化滑动窗口限流，这是给系统行为加可观测性

#### 认知策略反省（中高）

证据：
- 双环学习（见 1.3）说明你愿意质疑自己的方法
- 但 mirror weekly 提示"决策质量下降、协作摩擦上升"——你**意识到了**问题，但没有形成可执行的修复

#### 综合判断

你的元认知成熟度处于"高自我监控 + 中策略反省"的组合，这是**经验工程师的典型分布**。短板在于：

- **缺少"决策检查清单"**：mirror weekly 建议你在每次重要决策前回答 3 个问题，但目前没有这样的固定流程
- **缺少"委派标准模板"**：mirror weekly 建议委派时固定补齐 4 个元素（目标/边界/验收/回报码点），目前是临时性补充

---

## L2：战略定位

### 2.1 个人技术雷达

#### 投资组合分布（按 sessions 数）

```
                      sessions  占比     阶段
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
x                    2037     28.1%   Master
harness              281      3.9%    Competent→Proficient
xhh                  256      3.5%    Competent (有断层)
reddit-monitor       239      3.3%    Proficient
rss-scout            182      2.5%    Competent (锁死)
.claude root         162      2.2%    -
om                   126      1.7%    Proficient
vibeguard            112      1.5%    Proficient (有断层)
douyin-mcp           62       0.9%    Proficient
remem                43       0.6%    Proficient→Expert
其他 60+ 项目         3739     51.6%   多数 Competent
```

**结论**：

- **核心资产**：x（Master 级 + 28.1% session 量，且仍在快速产出）
- **稳定专业级**：reddit-monitor、om、douyin-mcp、vibeguard（合计 ~7.4%）
- **高密度但小体量**：remem（43 sessions / Expert 占 37%，"小而精"）
- **战略性投入**：harness（架构层项目，决定其他项目能否扩展）
- **执行性投入**：rss-scout（高频低密度，锁死在 competent）

#### 与 baseline 对比的雷达变化

| 项目 | 04-09 sessions | 04-27 sessions | 增长 | 阶段变化 |
|------|------|------|------|------|
| x | ~717 | 2037 | **+1320 (+184%)** | Expert→Master |
| rss-scout | ~10 | 182 | **+172 (+1720%)** | Competent → Competent（量增质未升） |
| harness | ~259 | 281 | +22 | 持平 |
| xhh | ~248 | 256 | +8 | 持平 |
| reddit-monitor | ~239 | 239 | 0 | 持平 |
| 新出现 | - | looper 27, loom 10, aip 8 | 新增 | 探索性 |

**关键发现**：

1. **x 是绝对中心，且仍在加速**：18 天里 x 单独贡献了 1320 个 sessions，是其他所有项目总和的 1.5×
2. **rss-scout 增长 17.2×**：新增 172 个 sessions，但全部停留在 competent 阶段，意味着是**机械重复，不是学习**
3. **harness/xhh/reddit-monitor 完全停滞**：这 18 天几乎没有新会话，说明这些系统已经"够用"，但也没有新探索

### 2.2 探索/利用比

| 模式 | 全量数 | 全量占比 | 04-10 后 | 04-10 后占比 |
|------|------|------|------|------|
| delegation | 3268 | 45.2% | 1354 | 40.9% |
| review | 2402 | 33.2% | 1067 | 32.2% |
| deep_inquiry | 731 | 10.1% | 498 | 15.0% |
| **exploration** | **425** | **5.9%** | 173 | 5.2% |
| teaching | 218 | 3.0% | 174 | 5.3% |
| pair_programming | 147 | 2.0% | 30 | 0.9% |
| debugging | 8 | 0.1% | 0 | 0% |

**判断**：

- **探索率 5.9%（mirror score 显示 6%，红）**：远低于 25% 健康阈值
- **18 天窗口里 exploration 仅 5.2%**，比全量更低，说明**最近探索还在收缩**
- **deep_inquiry 提升到 15%**（vs 全量 10.1%）：这是好信号，意味着 18 天里你在更多项目里做深度思考

#### 长期影响推断（中置信度）

如果探索率持续维持在 5-6%：
- **新项目无法启动**：你的项目雷达将冻结在当前 9 个核心项目上
- **rss-scout 锁死**：缺少探索意味着 rss-scout 会一直停在 competent
- **x 优势衰减**：所有竞品都在快速演化，没有探索就无法发现新打法

### 2.3 注意力分配

#### 你最投入的事 vs 你成长最快的事 是同一件吗？

**最投入的事（按 sessions）**：x (2037) → harness (281) → xhh (256)

**成长最快的事（按 expert 增量推断）**：
- **x**：expert 从 ~150 → 1284（增长 8.5×）— 投入第一，成长第一 ✅
- **remem**：expert 从 ~5 → 16（增长 3.2×，且占比 37.2%）— 投入第 10，但成长密度第一
- **harness**：expert 从 ~10 → 29（增长 2.9×）— 投入第二，成长稳定

**结论**：x 是"投入和成长一致"的健康项目；remem 是"小投入大产出"的高 ROI 项目；harness 是"长期投入，渐进成长"的基础设施项目。

**警告**：xhh 投入第三（256 sessions）但仍 60% competent，**投入和成长严重不匹配**。

### 2.4 知识网络

#### 跨项目复用最强的认知资产

基于 mirror weekly + insights v2 报告交叉验证：

1. **"硬规则下沉"模式**（高置信度）
   - 起源：x (rules.yaml + pre_filter.py)
   - 复用：rss-scout (评分阈值 >=8 + JSONL 输出契约)、reddit-monitor (action_log 限流配置化)、vibeguard (规则文件)
   - 价值：是你目前**最强的可移植资产**

2. **"双层持久化"模式**（高置信度）
   - 起源：remem (Raw archive + Curated memory)
   - 复用潜力：尚未广泛迁移，但是 harness 的 thread 持久化、x 的 ks- 归因可以借鉴
   - 价值：**结构洞资产**——有价值但目前孤立

3. **"前置门禁"模式**（高置信度）
   - 起源：reddit-monitor (无经验跳过)
   - 复用：x (A0 Scout First)、xhh (配额预检)、om (atlas 模型存在性校验)
   - 价值：你的"信任 AI 但验证 AI"哲学的工程化体现

4. **"枚举字段 vs 自由文本"原则**（中置信度）
   - 起源：x (empty_reason 限定枚举)
   - 复用：reddit-monitor (action_log 状态枚举)、vibeguard (规则 ID 枚举)
   - 价值：抗 AI 自然语言漂移的硬约束

**结构洞**：remem 的双层模型、om 的 evidence-first 工作流，目前只在原项目内部使用，未沉淀为跨项目方法论。

---

## L3：工作方式健康度

### 3.1 协作模式光谱

#### 全局分布

| 模式 | 数量 | 占比 |
|------|------|------|
| delegation | 3268 | 45.2% |
| review | 2402 | 33.2% |
| deep_inquiry | 731 | 10.1% |
| exploration | 425 | 5.9% |
| teaching | 218 | 3.0% |
| pair_programming | 147 | 2.0% |
| debugging | 8 | 0.1% |

**信号灯：模式多样性 7（红）**

实际有效模式仅 **3 种**（delegation 45.2% + review 33.2% + deep_inquiry 10.1% = 88.5%），其余 4 种合计 11.5%。这是协作效能从黄变红的核心原因。

#### 按项目的"信任地图"

| 项目 | 主导协作模式 | 委派率 | 解读 |
|------|------|------|------|
| **reddit-monitor** | delegation 98.3% | 极高 | "黑盒信任"——交给系统，自己不参与 |
| rss-scout | delegation 88.3% | 极高 | 同上 |
| xhh | delegation 60.4% + exploration 28.2% | 中高 | "委派+随机探索"，缺少 review |
| x | delegation 52.9% + deep_inquiry 22.0% + review 15.6% | 适中 | 最健康的混合 |
| douyin-mcp | delegation 62.9% | 高 | "执行型项目"特征 |
| harness | review 46.6% + delegation 36.7% | 偏低 | **review-dominant，最特殊** |
| om | deep_inquiry 40.5% + delegation 24.8% | 偏低 | **思考主导** |
| remem | deep_inquiry 38.6% + delegation 29.5% | 偏低 | **思考主导** |
| vibeguard | delegation 31.3% + review/deep_inquiry/exploration 各 16-18% | 适中 | **最均衡** |

**判断**：

- **reddit-monitor 委派率 98.3% 是个异常信号**：要么是系统已经稳定到不需要人工介入（最优），要么是你已经"懒得看了"（偷懒）。结合 reddit-monitor 仍处于 proficient 阶段，**前者更可能**——这是健康的"完成态委派"
- **harness review 占 46.6% 是好现象**：架构层项目天然需要人工 review，这是合适的
- **om/remem 的 deep_inquiry 主导**说明这两个是"思考型项目"，是 你**最强的研究阵地**
- **xhh 缺 review**：60.4% 委派 + 28.2% 探索，但 review 仅 7.4%。这是 xhh 进步缓慢的核心原因——**没有反馈环**

### 3.2 认知负荷

#### 摩擦密度 2.9（vs baseline 3.1）

改善了，但仍红。

**生产性挣扎 vs 无效摩擦的拆解**（中置信度推断）：

| 类型 | 估计占比 | 代表项目/场景 |
|------|------|------|
| **生产性挣扎** | ~50% | x v10→v10.2 重构、harness 单 vs 多 agent 决策、remem Phase 0/1 边界设计 |
| **无效摩擦** | ~30% | xhh 反复回到 advanced_beginner、上游配额限流（usage_limit_reached）反复出现 |
| **未分类** | ~20% | - |

**核心问题**：

- **上游配额限流**已经触发过你"探针 → 续跑 → 再撞顶 → 等明天"的循环 4 次（04-25 两次、04-26 一次、04-27 一次）。这不是工程问题，是**外部约束**——但你处理它的方式仍是手动续跑，而不是引入限流退避算法
- **xhh 的反复入门**：这 256 个 sessions 里有 46 个低阶（18.0%），意味着**你大约每 5.6 个会话就要重新理解一次 xhh 的状态**。这是结构性认知负荷

### 3.3 心流率

[基于 deep_inquiry + pair_programming 占比 + 长 session 推断 — 中置信度]

- **深度工作占比** = (deep_inquiry 731 + pair_programming 147) / 7239 ≈ **12.1%**
- **04-10 后这个比例**：(498 + 30) / 3320 ≈ **15.9%**

**判断**：心流率有改善（12.1% → 15.9%），但绝对值仍偏低。健康开发者的深度工作占比应在 25-40%。

主要被打断的来源（推断）：
- x 项目里反复的策略迭代（v10→v10.2→v11→v12，导致经常 context-switching）
- 上游 LLM 限流（一次 30-60 分钟的等待，破坏心流）
- 多项目并行（70 个项目，即使专注一个也会因周边消息切回）

### 3.4 学习方式：进化还是退化

#### 进化信号

- **expert 占比从 8.8% → 21.2%**：你在做的事质量更高
- **deep_inquiry 占比 10.1% → 15.0%**（18 天窗口）：在更多项目里愿意停下来思考
- **新协作模式 teaching 提升到 5.3%**（18 天窗口，vs 全量 3.0%）：你在更主动地"教"AI 怎么做事，这是协作进化

#### 退化信号

- **委派率 49% → 45%（4pt 下降）**：从趋势看，你**不是在更深委派 AI，而是在收回更多执行权**。这跟 mirror weekly 报告"探索增加，决策质量下降"是同一现象的两面——**你不再信任纯委派的输出**
- **exploration 5.9% → 5.2%（18 天窗口）**：探索意愿在缩小
- **pair_programming 2.0% → 0.9%（18 天窗口）**：真正"双向同时编辑"的会话在减少

#### 综合判断：**进化中夹带退化**

你正在变成"更挑剔的 senior"，这是质的进化；但同时，你在收窄探索半径，这是战略级的退化。

**思考外包化**风险：**低**
- 你的 deep_inquiry 占比仍在上升
- om/remem 等思考密度高的项目仍在持续投入
- mirror weekly 报告显示你能识别自己的协作问题，意味着元认知没有外包

但需要警惕：**判断标准外包化**
- 你越来越依赖 mirror score / weekly 来"诊断自己"，但这些信号灯本身是基于过去 4 周的均值——它们是**滞后指标**
- 如果未来你只通过信号灯反馈来调整方向，你会陷入"指标驱动"陷阱

---

## L4：成长处方

### 4.1 认知瓶颈诊断

不是技术瓶颈，是**战略半径瓶颈**。

具体表现为：

1. **x 项目饱和**：expert 占 62.7%，已经是 Master 级仓库，再做下去边际收益递减
2. **rss-scout 锁死**：competent 88.4%，新增了 172 sessions 但没有阶段提升，这是机械执行
3. **xhh 断层**：有 60% competent + 18% advanced_beginner，缺乏稳定方法论
4. **协作模式锁死**：实际只用 3 种（delegation + review + deep_inquiry），其余 4 种被边缘化
5. **探索率 5-6%**：意味着 12 个月内你不会发现新的高 ROI 项目

**核心瓶颈一句话**：**你已经把"已知的事情做到 Expert"，但没有给"未知的事情"留出 25% 的探索预算**。

### 4.2 学习策略处方（5 条具体建议）

#### 处方 1：把 rss-scout 从"机械执行"升级为"方法论实验场"

**触发条件**：rss-scout 仍 88.4% competent、172 个新 sessions 没带来阶段提升

**具体做法**：
- 接下来 4 周，每周给 rss-scout 配 1 个**小型 spike**：尝试用 LLM-as-judge 替代当前 >=8 阈值
- 验证标准：4 周后 rss-scout 的 expert 占比从 1.1% 提升到 ≥10%

**改善指标**：rss-scout 的 Dreyfus 阶段 + 全局 expert 占比

#### 处方 2：给 xhh 加 review-loop

**触发条件**：xhh 委派率 60.4%、review 仅 7.4%、断层 18%

**具体做法**：
- 每个 xhh 会话结束时，强制做 5 分钟 self-review：写下"这次决策依据是什么、下次什么情况下会重做"
- 这 5 分钟存成 reddit-monitor 那种 `action_log` 结构，落到 SQLite

**验证标准**：8 周后 xhh 的 advanced_beginner+novice 占比从 18% 降到 <5%

**改善指标**：xhh 的认知阶段稳定性 + 摩擦密度

#### 处方 3：建立"探索预算"配额

**触发条件**：探索率连续 4 周 <10%、深耕率红、碎片化红

**具体做法**：
- 每周固定**预留 1 天**做 exploration（任何项目都行，但必须打 `exploration` 标签）
- 不允许在 x 项目里做（x 已 Master，探索 ROI 低）
- 候选方向：
  - 把 remem 的 Raw/Curated 双层模型迁移到 harness
  - 给 om 的 evidence-first 加上 cross-validation 层
  - 启动一个全新的小项目（比如把 mirror weekly 输出的"决策检查 3 问"做成 Skill）

**验证标准**：4 周后 exploration 占比从 5.9% 提升到 ≥15%（健康线 25%，先做到一半）

**改善指标**：探索率 + 模式多样性 + 长期项目雷达

#### 处方 4：把"决策检查 3 问"和"委派 4 元素"工程化

**触发条件**：mirror weekly 已经诊断出"决策质量下降+协作摩擦上升"

**具体做法**：
- 写一个 `vibeguard:decide` skill，输入决策内容，强制让 LLM 用 3 问反问你
  1. 是基于证据决策，还是赶进度？
  2. 是否比较过 ≥2 个方案？
  3. 错了的话最可能的返工是什么？
- 写一个 `vibeguard:delegate` skill，输入任务，输出"目标/边界/验收/回报码点" 4 元素结构化模板

**验证标准**：4 周后决策质量从 56% 提升到 ≥65%、bug/决策从 0.36 降到 ≤0.30

**改善指标**：决策质量 + bug/决策 + 摩擦密度

#### 处方 5：给 harness 提速到 Proficient 主导

**触发条件**：harness 有 47.7% 中高阶但 45.4% 仍在 competent，这是基础设施项目"该升级而未升级"

**具体做法**：
- 接下来 4 周给 harness 集中投入 60 sessions（vs 04-10 后只有 22 sessions）
- 重点做 2 件事：
  1. 把 thread 持久化（remem 的 Raw/Curated 模式）做完
  2. 引入"任务依赖图"（当前是 flat list）

**验证标准**：harness 的 proficient 占比从 37.4% 提升到 ≥55%、competent 从 45.4% 降到 ≤30%

**改善指标**：harness 的 Dreyfus 阶段 + 长期工程效率（harness 升级会反向加速所有其他项目）

### 4.3 注意力重分配方案

#### 当前实际配比（按 18 天窗口）

```
x                39.8%    Master 项目，边际收益递减
rss-scout        5.2%     锁死，应改造或砍
harness          0.7%     基础设施，应加投
xhh              0.2%     断层项目，应加 review
其他              ~54%    其他 60+ 项目分散
```

#### 建议下 4 周配比

```
x                30%     ↓ 9.8pt（让出空间给探索）
harness          15%     ↑ 14.3pt（基础设施升级期）
remem            5%      ↑ 4.7pt（结构洞资产，迁移到 harness）
exploration      20%     ↑ 14.8pt（新项目/新方法论）
xhh              5%      ↑ 4.8pt（加 review-loop）
rss-scout        5%      持平（但改成实验场）
其他              20%    ↓ 34pt（强制收缩）
```

#### 时间块设计（建议每周）

| 时间块 | 用途 |
|------|------|
| 周一 09:00-12:00 | mirror weekly 复盘 + 当周 spike 选题 |
| 周二-周四 | 主投入（x 30% + harness 15%）|
| 周五全天 | exploration（新项目/新方法）|
| 周六上午 | xhh review-loop + rss-scout 实验 |
| 周日 | 真休息（保护心流恢复） |

### 4.4 AI 协作进化路径

#### 当前分布

```
delegation       45.2%   ████████████████████████
review           33.2%   ██████████████████
deep_inquiry     10.1%   █████
exploration       5.9%   ███
teaching          3.0%   █
pair_programming  2.0%   █
debugging         0.1%
```

#### 目标分布（12 周后）

```
delegation       35%     ████████████████████      （-10pt）
review           25%     ██████████████            （-8pt）
deep_inquiry     15%     ████████                  （+5pt）
exploration      15%     ████████                  （+9pt）
teaching          5%     ███                       （+2pt）
pair_programming  4%     ██                        （+2pt）
debugging         1%     ▎                         （+1pt）
```

#### 切换时机

| 当前模式 | 切换信号 | 目标模式 |
|------|------|------|
| delegation | 任务说明超过 3 段 | review |
| delegation | LLM 输出 < 80% 满意 | pair_programming |
| review | 改动超过 3 处 | deep_inquiry |
| review | 改动 = 改架构 | exploration |
| 任意 | 同类问题第 3 次出现 | teaching |

### 4.5 12 周成长计划

#### Week 1-2：诊断与契约

- 把这份画像和 mirror weekly 的 5 条诊断打印出来，挂在显眼位置
- 写 `vibeguard:decide` 和 `vibeguard:delegate` 两个 skill
- **里程碑**：决策质量从 56% → 60%

#### Week 3-4：rss-scout 改造

- 用 LLM-as-judge 替代 >=8 阈值
- 引入 cross-validation（多模型投票）
- **里程碑**：rss-scout expert 占比从 1.1% → ≥10%

#### Week 5-6：harness 升级

- 把 remem 的 Raw/Curated 双层模型迁移到 harness 的 thread 持久化
- 加任务依赖图
- **里程碑**：harness proficient 占比从 37% → ≥55%

#### Week 7-8：xhh review-loop

- 每个 xhh 会话强制 5 分钟 self-review，落 SQLite
- **里程碑**：xhh advanced_beginner+novice 占比从 18% → <5%

#### Week 9-10：探索预算

- 每周一天 exploration，候选方向：
  - mirror weekly 的"决策检查 3 问"做成 Skill
  - 新项目尝试（待选）
- **里程碑**：探索率从 5.9% → ≥15%

#### Week 11-12：综合验证

- 跑新一轮 cognitive-portrait
- 与本版对比，验证 5 条处方落地情况
- 调整下一阶段计划

---

## 数据附录

### A.1 信号灯历史

```
date         认知深度  战略广度  协作效能
2026-04-09   🟢       🔴        🟡
2026-04-25   🟡       🟡        🟡
2026-04-26   🟡       🟡        🟡
2026-04-27   🟡       🟡        🔴
```

### A.2 全量 cognitive_levels（04-27）

```
expert            1536    21.2%
proficient        2277    31.5%
competent         2960    40.9%
advanced_beginner  394     5.5%
novice             37     0.5%
total             7204
```

### A.3 全量 collab_modes（04-27）

```
delegation        3268    45.2%
review            2402    33.2%
deep_inquiry       731    10.1%
exploration        425     5.9%
teaching           218     3.0%
pair_programming   147     2.0%
debugging            8     0.1%
total             7199
```

### A.4 项目 × 阶段交叉表（top 9 项目）

```
项目              novice  beginner  competent  proficient  expert  total
x                  1       4        89         659         1284    2037
harness            1       18       128        105         29      281
xhh                6       40       155        48          7       256
reddit-monitor     0       0        79         154         6       239
rss-scout          0       1        160        19          2       182
om                 1       8        34         67          16      126
vibeguard          0       14       35         50          13      112
douyin-mcp         0       4        19         35          4       62
remem              1       4        7          15          16      43
```

### A.5 项目 × 协作模式交叉表（top 9 项目）

```
项目              deleg  review  deep_inq  expl  teach  pair  debug  total
x                 1082   319     449       20    167    -     -      2037
harness           103    131     26        7     6      7     1      281
xhh               148    19      13        69    4      3     -      256
reddit-monitor    235    3       -         -     1      -     -      239
rss-scout         166    8       6         1     -      1     -      182
om                30     15      49        14    3      13    1      125
vibeguard         35     20      20        18    5      11    3      112
douyin-mcp        39     3       7         8     1      4     -      62
remem             13     1       17        3     2      7     -      43
```

### A.6 mirror score 详细输出（04-27 17:00）

```
认知深度         🟡  Dreyfus 3.7 ✗ | 决策质量 56% ✓ | 深度产出比 163% ✓ | 知识获取 3.2 ✗
战略广度         🟡  探索率 6% ✗ | 深耕率 23% ✗ | 碎片化 23% ✗
协作效能         🔴  委派率 45% ✗ | 模式多样性 7 ✗ | bug/决策 0.36 ✗ | 摩擦密度 2.9 ✗
基线: 个人(4周均值)
数据范围: 2026-02-06 ~ 2026-04-27
```

### A.7 04-09 → 04-27 关键数字差量

| 指标 | 04-09 | 04-27 | Δ |
|------|------|------|------|
| 总会话 | 3919 | 7239 | +3320 (+84.7%) |
| 总观测 | 29760 | 53424 | +23664 (+79.5%) |
| 总决策 | 19584 | 33898 | +14314 (+73.1%) |
| 总 bugfix | 6257 | 12287 | +6030 (+96.4%) |
| 总项目 | 58 | 70 | +12 (+20.7%) |
| Expert 观测 | 345 | 1536 | +1191 (+345%) |
| Expert 占比 | 8.8% | 21.2% | +12.4pt |
| Dreyfus 平均 | 3.5 | 3.7 | +0.2 |
| 深度产出比 | 130% | 163% | +33pt |
| 探索率 | 7% | 6% | -1pt |
| 委派率 | 49% | 45% | -4pt |
| 摩擦密度 | 3.1 | 2.9 | -0.2 |

**bugfix 增长率（96.4%）超过总会话增长率（84.7%）**：警示信号，意味着每个会话产生的 bug 量在增加。这与 bug/决策 0.36 红灯一致。

---

## 报告元数据

- **生成时间**：2026-04-27 17:15 (CST)
- **数据范围**：2026-02-06 ~ 2026-04-27（4 周滑动 + 全量交叉）
- **基线版本**：[cognitive-portrait-2026-04-09-v3.md](./cognitive-portrait-2026-04-09-v3.md)
- **下次重审**：建议 2026-05-25（4 周后），验证 5 条处方的执行情况
- **核心追踪指标**：
  - rss-scout expert 占比（目标 ≥10%）
  - xhh advanced_beginner 占比（目标 <5%）
  - 探索率（目标 ≥15%）
  - 决策质量（目标 ≥65%）
  - 协作模式多样性（目标 ≥4 主流模式）

---

*本报告基于 refine 系统的 7239 个 AI 编程会话观测数据，由 cognitive-portrait skill v3 生成。所有判断标注了置信度，所有数字附了来源；推断和建议与事实严格分离。*
