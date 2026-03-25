# Mirror v0.2.0 Release Plan

## Overview

从 v0.1.0（初始实现）升级到 v0.2.0（生产就绪），包含技术债清理、产品功能、核心管道升级。

## Release Target

- **v0.2.0-alpha**: Phase 1 完成后（技术债清理）
- **v0.2.0-beta**: Phase 2 完成后（产品功能）
- **v0.2.0**: Phase 3 完成后（核心管道升级）

---

## Phase 1: 技术债清理（v0.2.0-alpha）

### 1.1 删除废弃命令
- [ ] 删除 `apps/cli/src/growth.rs`（267 行）
- [ ] 删除 `cli.rs` 中 `Growth` / `Explore` / `DeepInquiry` 枚举变体
- [ ] 删除 `main.rs` 中对应的 handler 分发
- [ ] 验证：`cargo check --workspace && cargo test --workspace`

### 1.2 statusLine 预计算缓存
- [ ] `mirror score` 运行后写 `~/.mirror/statusline.txt`（纯文本一行）
- [ ] statusLine 改为 `cat ~/.mirror/statusline.txt`（从 ~80ms → ~1ms）
- [ ] 验证：statusLine 显示正确

### 1.3 session-tagger 增量扫描
- [ ] `growth-tracker.json` 新增 `last_scan_ts` 字段
- [ ] 扫描时只看 `mtime > last_scan_ts` 的文件
- [ ] 周轮转时重置 `last_scan_ts`
- [ ] 验证：Stop hook < 1s

### 1.4 监控告警
- [x] `daily-refresh.sh` 成功后写 `~/.refine/last-refresh-ok`（时间戳）
- [x] `cognitive-reminder.sh` 检测该文件，超 36h 未更新则显示警告
- [x] Hook 错误从 `2>/dev/null` 改为 `2>> ~/.refine/hooks-error.log`
- [ ] 验证：手动删除 last-refresh-ok，SessionStart 看到警告

### 1.5 版本提升 + CHANGELOG
- [ ] workspace version: 0.1.0 → 0.2.0-alpha
- [ ] 创建 CHANGELOG.md
- [ ] git tag v0.2.0-alpha

---

## Phase 2: 产品功能（v0.2.0-beta）

### 2.1 Streaks + 里程碑
- [ ] 从 `scores.jsonl` 计算连续有记录的天数
- [ ] `motd` 输出加 `🔥 连续 N 天`
- [ ] `statusLine` 加 streak 数字
- [ ] 里程碑触发（7/30/100/365 天）：motd 显示特殊消息
- [ ] 测试：streak 计算、跨天边界、中断后重置

### 2.2 自动周报 + motd 推送
- [ ] `daily-refresh.sh` 周日额外触发 `mirror weekly`
- [ ] 周报保存到 `~/.mirror/last-weekly.md`
- [ ] 周一 motd 检测到新周报时追加提示 `📋 本周周报已生成，运行 mirror weekly 查看`
- [ ] 验证：周日 daily-refresh 生成周报，周一 motd 提醒

### 2.3 Stop hook 异步增量 ingest
- [ ] `refine ingest-sessions` 支持 `--latest N` 参数（只处理最新 N 个文件）
- [ ] Stop hook 末尾追加：`nohup refine ingest-sessions --latest 1 >> ~/.refine/incremental.log 2>&1 &`
- [ ] 验证：session 结束后 30s 内新 observation 出现在 DB

### 2.4 版本提升
- [ ] workspace version: 0.2.0-alpha → 0.2.0-beta
- [ ] 更新 CHANGELOG.md
- [ ] git tag v0.2.0-beta

---

## Phase 3: 核心管道升级（v0.2.0）

### 3.1 ItemRepository 加 find_by_date_range
- [ ] `knowledge/repository.rs` 新增 `find_by_date_range(start, end)` 方法
- [ ] `infra/sqlite/ops.rs` 实现 SQL: `WHERE created_at BETWEEN ? AND ?`
- [ ] `mirror score/dashboard` 默认最近 90 天（`--all` 全量）
- [ ] 测试：时间过滤准确性

### 3.2 聚类利用缺失 facet 维度
- [ ] `clustering.rs` 的 `ProjectCluster` 新增：questions, project_progress, code_artifacts
- [ ] `cluster_observations` 填充这 3 个字段
- [ ] `mirror profile` 的 prompt 包含 questions 和 progress 数据
- [ ] 测试：新字段被正确聚合

### 3.3 聚类性能优化
- [ ] 合并 doc_project_map 构建和主遍历为单次
- [ ] tags 转换 `Vec<&str>` 只做一次
- [ ] 验证：benchmark 对比前后耗时

### 3.4 Facet prompt 优化
- [ ] 每个维度加数量限制（decisions max 5, patterns max 3 等）
- [ ] 添加优先级指导（decisions > bugs > patterns > friction）
- [ ] 明确 patterns vs knowledge_gained vs architecture 的边界
- [ ] 验证：对比 prompt 优化前后的提取质量

### 3.5 版本提升 + Release
- [ ] workspace version: 0.2.0-beta → 0.2.0
- [ ] 更新 CHANGELOG.md
- [ ] git tag v0.2.0
- [ ] GitHub Release

---

## Phase 4: 后续方向（v0.3.0 规划）

不在本次 release 范围，仅记录：
- [ ] `mirror serve` 本地 Web 仪表盘
- [ ] desktop 移出 workspace，CI 去掉 GTK 依赖
- [ ] CI 加 `cargo fmt --check` + `cargo audit`
- [ ] V2 指标：元认知/ZPD 追踪（需改 facet prompt）
- [ ] 匿名基准线比较（需多用户数据）

---

## Harness 集成

Phase 1-3 中的代码改动优先通过 Harness 自动执行：
1. 将每个子任务创建为 GitHub Issue
2. Harness 创建 worktree → agent 修复 → validator → review → PR
3. 人工审查 PR 后 merge

纯配置/脚本改动（statusLine、hooks、launchd）手动完成。
