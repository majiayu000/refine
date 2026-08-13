# SPEC: Mirror — 认知镜像 CLI

## 定位

从 AI 编程会话中提取认知指纹，追踪开发者成长轨迹。

## 形态

refine workspace 新 member `apps/mirror/`，纯 Rust CLI，只读 refine SQLite。

## 评分框架设计

### 设计原则（基于调研）

| 来源 | 原则 | 应用 |
|------|------|------|
| SPACE/DORA | 不追求单一分数，观察维度间张力 | 3 层信号灯而非 0-100 分 |
| Oura Ring | 个人基线 > 绝对标准 | 运行 4 周后切换为滑动窗口基线 |
| WHOOP | 非线性阈值，过度也是问题 | delegation >55% 是红灯，不是越低越好 |
| Apple Watch | 视觉隐喻清晰，低认知负担 | 3 个信号灯 + 一句话 |
| 游戏化论文 | 恢复机制比连续惩罚重要 | 从红灯恢复时给强正面反馈 |
| viaEDGE | Self-Awareness 是调节因子不是等权维度 | V2 加入元认知调节 |

### 3 层信号灯

#### 层 1: 认知深度（你在想什么层次的问题？）

| 指标 | 数据来源 | 计算 | 绿 | 黄 | 红 |
|------|---------|------|---|---|---|
| Dreyfus 加权分 | cognitive_level tags | novice=1,adv=2,comp=3,prof=4,exp=5 加权平均 | >3.5 | 2.5-3.5 | <2.5 |
| 理由显式率 | decision titles 含"因为/原因"比例 | 关键词匹配；不等同决策质量 | >60% | 40-60% | <40% |
| 深度思考产出比 | deep_inquiry 中 expert% vs delegation 中 expert% | 交叉统计 | 差>10% | 差0-10% | 反转 |

层 1 信号灯 = 3 个指标中最差的那个

#### 层 2: 战略广度（你在投资什么？）

| 指标 | 数据来源 | 计算 | 绿 | 黄 | 红 |
|------|---------|------|---|---|---|
| 探索率 | exploration / 总协作模式 | 百分比 | >15% | 8-15% | <8% |
| 成熟项目占比 | 20+ session 项目 / 总项目 | 百分比 | 15-30% | 10-15% | <10%或>30% |
| 一次性项目占比 | 1 session 项目 / 总项目 | 百分比；不等同上下文切换 | <20% | 20-40% | >40% |

层 2 信号灯 = 3 个指标中最差的那个

#### 层 3: 协作效能（你和 AI 的配合健康吗？）

| 指标 | 数据来源 | 计算 | 绿 | 黄 | 红 |
|------|---------|------|---|---|---|
| delegation 率 | delegation / 总协作模式 | 百分比 | <40% | 40-55% | >55% |
| 模式多样性 | 协作模式中 >0 的数量 | 整数 | >=4 | 2-3 | 1 |
| Bug/决策抽取比 | extracted bugfix / extracted decision | 提取量比率，不等同协作质量 | <0.6 | 0.6-0.8 | >0.8 |

层 3 信号灯 = 3 个指标中最差的那个

### 维度间张力分析

| 组合 | 含义 | 自动建议 |
|------|------|---------|
| 层1绿 + 层2红 | 深耕但视野收窄 | "开一个新方向的探索 session" |
| 层2绿 + 层3红 | 探索多但 delegation 过高 | "探索时用 pair 模式而非委托" |
| 层1红 + 层3绿 | 协作顺畅但认知没提升 | "你在舒适区，挑战更难的问题" |
| 全绿 | 健康成长 | 自动提升基线 |
| 全红 | 需要重新规划 | 触发完整 insights 报告 |

### 个人基线机制

- 前 4 周：使用固定阈值（上表）
- 4 周后：切换为滑动窗口基线
  - 基线 = 最近 4 周平均
  - 绿 = 优于基线, 黄 = 基线 ±10%, 红 = 低于基线 10%+
- 全绿持续 2 周 → 自动提升基线标准
- 从红灯恢复 → 给强正面反馈

### V2 维度（当前不实现）

| 维度 | 来源框架 | 需要什么 |
|------|---------|---------|
| 元认知信号 | MAI (Schraw & Dennison) | 改 facet prompt 增加 planning/monitoring/evaluation |
| 自我调节循环 | Zimmerman SRL | 检测 session 的 forethought→performance→reflection |
| 知识迁移率 | Near/Far Transfer | 跨项目知识复用检测 |
| ZPD 追踪 | Vygotsky | 同类问题 AI 依赖度的时间趋势 |
| Self-Awareness 调节因子 | viaEDGE | 用户对自身局限的识别频率，调节其他维度 |

## 4 个命令

### `mirror score`

计算 3 层信号灯 + 9 个子指标 + 张力分析。

输出：
```
Mirror 认知镜像

  认知深度   🟢  Dreyfus 3.6 | 决策质量 65% | 深度产出比 +14%
  战略广度   🟡  探索 16% ✓ | 深耕 19% ✓ | 碎片化 14% ✓ (深耕率偏低)
  协作效能   🟡  delegation 45% ✗ | 多样性 6种 ✓ | bug/决策 0.50 ✓

  张力: 层1绿+层3黄 → 认知在提升但 delegation 偏高，试试 review 模式
  基线: 默认阈值（还需 3 周建立个人基线）
```

持久化到 `~/.mirror/scores.jsonl`。

### `mirror motd`

一行输出，加到 .zshrc。

```
🪞 深度🟢 广度🟡 协作🟡 | delegation 45%↓ 试试让 AI 找反例而不是直接写代码
```

基于最弱维度选择建议，每天轮换。

### `mirror dashboard`

完整 ASCII 仪表盘：信号灯 + 9 个子指标进度条 + 张力分析 + 历史趋势 + 本周数据。

### `mirror weekly`

本周 vs 上周差量分析（需要 LLM）：
- 各指标变化方向和幅度
- 上周建议执行情况
- 新出现的模式
- 下周 1-2 条建议

## 文件结构

```
apps/mirror/
├── Cargo.toml
└── src/
    ├── main.rs         # 入口 (~40 行)
    ├── cli.rs          # clap 命令定义 (~30 行)
    ├── config.rs       # ~/.mirror/config.toml + 默认阈值 (~80 行)
    ├── score.rs        # 3 层信号灯算法 + 持久化 (~190 行)
    ├── motd.rs         # 一行简报 + tips 轮换 (~100 行)
    ├── dashboard.rs    # ASCII 仪表盘 (~190 行)
    └── weekly.rs       # 差量分析 + LLM (~190 行)
```

## 数据存储

```
~/.mirror/
├── scores.jsonl         # 信号灯历史（JSONL 追加）
├── weekly-history.jsonl # 周报历史
├── tips.json           # 建议库（首次生成 ~20 条）
└── config.toml         # 可选（目标阈值覆盖）
```

## 依赖

只读 refine SQLite，通过 refine-core 的 ItemRepository trait。
不修改 refine-core 和 refine-cli 的任何代码。

## 实现步骤

| Step | 内容 | 验证 |
|------|------|------|
| 1 | 项目骨架 + workspace 注册 | cargo check |
| 2 | config.rs（阈值 + 默认值） | cargo test |
| 3 | score.rs（3 层信号灯 + 张力 + 持久化） | cargo test + 手动运行 |
| 4 | motd.rs（弱项感知建议） | cargo test + 手动运行 |
| 5 | dashboard.rs（ASCII 仪表盘） | cargo test + 手动运行 |
| 6 | weekly.rs（差量 + LLM） | cargo test + 手动运行 |
| 7 | 安装 + zshrc 集成 | 端到端验证 |
