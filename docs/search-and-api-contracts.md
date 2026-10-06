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
