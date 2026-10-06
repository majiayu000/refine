# 检索与服务合同

## 中文查询与索引迁移

Items 和本地 Documents 同时维护词项 FTS5 索引与 trigram 外部内容索引。
英文保留前缀查询；含中日韩文字的词条按字段内连续子串匹配；多个词条仍为 AND。
因此 `灰度发布` 可以命中 `使用灰度发布降低部署风险`，`部署 Rust` 可以跨标题与正文匹配。
所有查询参数绑定，标点不会作为 FTS 操作符执行，相同排名以完整 ID 稳定排序。

启动迁移在同一个 IMMEDIATE 事务内创建触发器、回填旧记录，并补齐旧版 Documents
全文索引的回填。后续 insert/update/delete 在业务事务内维护索引。
trigram 使用 external content，不另存一份正文。

SQLite 的 trigram MATCH 至少需要三个 Unicode 字符。一至两个字的查询使用
字面 LIKE 回退，保证召回；这类查询可能扫描全表。不能据此宣称所有中文查询都有
相同的延迟。参见 [SQLite FTS5 trigram 文档](https://www.sqlite.org/fts5.html#the_trigram_tokenizer)。

Remem 引用文档仍不复制 raw transcript。Document 搜索覆盖保存在 Refine 中的标题、
来源和本地正文；读取某个 Remem 文档可按已有 snapshot hash 按需加载，但没有把
Remem 全部原文隐式纳入本地全文索引。

## 可选哈希特征检索

`REFINE_ENABLE_FEATURE_HASH_SEARCH=1` 启用 FTS5 与 256 维哈希词项/字符特征混排。
默认关闭。旧名称 `REFINE_ENABLE_SEMANTIC_SEARCH` 作为兼容别名保留，新名称优先。
该实现没有训练过的 embedding 模型，不应被描述为已具备语义理解能力。
推荐响应 `meta.backend` 明确区分 `fts5` 与 `feature_hash`。

所有 Item 提交通过 SQLite 触发器更新 generation。搜索读取 generation 与 Items 的
一致数据库快照，在发生变更时替换内存索引；没有变更时只读 generation。
CLI 和桌面写入都能被观察。数据库索引只从已提交内容派生，迟到的 index/remove
回调不会覆盖较新快照。

发生变更后的首次哈希搜索会重建整个派生索引，仍是 O(N) 工作。此处解决跨进程
陈旧结果；大库的持久化向量索引和增量更新是后续可独立评测的工作。

## 固定语料评测

`scripts/eval_recommendations.mjs` 按 `relevant_ids` 判断相关性，报告 Top-1/Top-3、
Recall@k、MRR@k 与负例空结果率。重复命中同一 ID 不增加 recall；请求失败不能
被当成负例正确返回空结果。只标注 type/tag 的旧样本保留为 metadata proxy，
不会混入相关性指标。报告记录数据集 SHA-256 以便复查。

仓库包含 6 条固定合成 Items、7 条正例查询（含多答案）和 4 条无答案查询。
其中 PostgreSQL 正例与不相关候选共享 database 标签，避免用标签重合冒充命中。
这些是功能回归用例，不能代表个人真实语料上的质量或规模性能。

先准备一个全新的隔离数据库：

```sh
eval_dir=$(mktemp -d)
python3 scripts/seed_search_eval.py --db "$eval_dir/search.sqlite"
REFINE_DB_PATH="$eval_dir/search.sqlite" \
REFINE_SERVER_DB_PATH="$eval_dir/search.sqlite" \
REFINE_DEV_ANON=1 REFINE_SERVER_PORT=24158 \
cargo run --locked -p refine-server
```

在另一个终端对该测试实例评测，并指定输出位置：

```sh
node scripts/eval_recommendations.mjs \
  --base-url http://127.0.0.1:24158 \
  --dataset docs/eval/recommendation_gold.jsonl \
  --out /tmp/refine-recommendation-eval.md
node --test scripts/eval_recommendations.test.mjs
```

Seed 工具拒绝覆盖已存在的文件。不要把测试语料导入个人数据库。
真实质量评测需在稳定语料上单独标注相关 Item IDs 和负例。

## 一套 HTTP 服务实现

独立 `refine-server` 和原生 Tauri 桌面现在共享 `refine-server` library 的 AppState、
路由与任务恢复实现。桌面的原生命令使用同一个 store 和 search engine。
`ingest_only` 返回真实持久化的 captured receipt；普通提炼返回持久化 conversation/job ID，
可使用 `/v1/extraction-jobs/:id` 查询。幂等重放不会创建伪造的 local ID。

配置正数 Items 配额时，配额限制新采集的准入。已持久化的幂等键仍能重放并返回
原 conversation、当前状态及适用的 job ID，即使首次响应丢失后配额已满。
新幂等键继续返回配额错误；已完成的采集不会因重放重新提炼。配额仍沿用当前
单用户、全库 Items 计数合同，不是对尚未完成任务的 Item 数量预留。
幂等键查询、新采集的 Items 计数、conversation 与初始 job 写入在同一个
IMMEDIATE 事务中执行；不把事务外曾查到的旧 key 当作准入许可。其他写入者
删除或修改了该 key 后，新请求须重新满足准入配额。输入校验与现有鉴权不变，
重放不改写已经持久化的原 payload。

### 同 URL 采集结果的发布顺序

服务器在首次持久化采集的同一 SQLite 事务内分配递增接收 revision；修改同一
采集的提炼输入也会获得新 revision。幂等重放及单纯任务状态更新不改变 revision。
worker 领取任务时绑定该源 revision，发布事务重新核对 lease、源 revision 与
Document 输入，再与该 URL **已经成功发布**的 revision 比较。较新任务仅排队、
仍在提炼或失败时，不阻止较旧有效任务发布；较新结果成功发布后，旧任务不能覆盖
Document、Items 或索引。接收顺序不使用客户端 `captured_at` 或模型完成时间。

旧任务仍可通过原 job ID 查询：保留现有 `failed` 状态，`error` /
conversation `last_error` 中的 `capture_superseded` 解释较新结果已发布；
`capture_source_changed` 表示提炼期间原文版本发生变化。自动恢复只处理
pending/running，不会把这些终态重新排入。HTTP 回执字段和状态枚举保持兼容。

升级前历史采集的真实接收 revision 无法可靠恢复，迁移记录为未知的 `0`，不从
rowid、`created_at` 或完成时间猜测。已完成历史与现有 Document 保持可读；
旧任务在该 URL 尚未发布正 revision 时仍可完成，多个历史 `0` 之间保留原行为。
一旦正 revision 成功发布，晚到的历史 `0` 也必须拒绝。防止发布回退的顺序保证
从迁移后分配的正 revision 起生效，不宣称恢复了历史发布先后。较新任务仅排队
或失败不会阻止旧工作。旧 worker 应在升级前停止，再由新版本恢复任务。
此规则不自动重算历史；新版本领取任务后若源内容发生变化，重新提交采集可明确
建立新版输入。

历史 `server.db` 等文件的合并同样使用未知 `0`：导入在事务内暂停正 revision
分配，外部数据库的 job revision 不带入本库时钟。导入不能用文件遍历或源库
revision 猜测跨库接收顺序。已经发布正 revision 的 URL，其 Document 与附属
Items 不被未知顺序的历史快照替换；历史原始文件与回执仍按原迁移合同保留。
尚无正 revision 发布的 URL 继续沿用原历史合并行为。

原生 HTTP 接口沿用独立服务的显式访问配置：`REFINE_API_TOKEN`，或开发时
`REFINE_DEV_ANON=1`。未配置时原生 UI 可使用，HTTP 接口不绑定端口，并记录
配置错误。打包应用从 Finder 启动时不会自动继承终端环境变量；需要通过带配置
的启动环境启动原生程序。默认 local installer 的独立服务配置方式保持适用。
扩展在选项页设置匹配的 token。没有用广泛开放 CORS 解决访问问题。

端口发现先实际 bind，避免探测与绑定之间的竞争。遇到运行中的 Refine 实例时，
只在数据库路径身份、contract、auth 和 runtime profile 一致，且 token 授权探测
通过后复用。runtime profile 包含 LLM provider/model/endpoint identity、检索开关、
配额与 CORS 等运行配置，不包含凭据。

数据库身份来自规范化路径的哈希；将另一个数据库覆盖到相同路径不构成新身份。
有意替换数据库时应停止旧实例再启动，避免旧进程仍持有旧文件。

## 相关合同

- [Session 重算、人工修订与消息证据](session-projections.md)
- [Mirror 缺失证据、来源口径与每日历史](SPEC-mirror-history.md)
- [扩展采集与本地队列边界](../apps/extension/README.md)
