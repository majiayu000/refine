# 检索与服务合同

## 中文查询与索引迁移

Items 和本地 Documents 同时维护词项 FTS5 索引与 trigram 外部内容索引。
英文保留前缀查询；含中日韩文字的词条按字段内连续子串匹配；多个词条仍为 AND。
因此 `灰度发布` 可以命中 `使用灰度发布降低部署风险`，`部署 Rust` 可以跨标题与正文匹配。
所有查询参数绑定，标点不会作为 FTS 操作符执行，相同排名以完整 ID 稳定排序。

启动迁移在同一个 IMMEDIATE 事务内创建触发器、回填旧记录，并补齐旧版 Documents
全文索引的回填。后续 insert/update/delete 在业务事务内维护索引。
trigram 使用 external content，不另存一份正文。

Items 的默认 keyword 检索把 type 和全部 tags 的 AND 条件放在 SQL 中，
同一读快照内用相同谓词获取精确 total 和目标页。空查询的最近条目也使用同一路径。
每页固定为一次 count 和一次分页查询，只反序列化目标页的完整 Item，
不再以 128 条为一批反复 OFFSET 读取所有文本命中。相同排名或创建时间用完整 ID
稳定排序。标签保留 Rust `Tag` 的 Unicode 大小写与旧数据规范化行为，
不依赖只处理 ASCII 的 SQLite `LOWER`。

精确 count 仍可能扫描所有匹配行；这项变更不承诺与库大小无关的查询时间。
`packages/core/tests/search_filters.rs` 覆盖中英文、Unicode 标签、类型组合、
空查询、空页，以及目标页之外的匹配行不会被完整反序列化。可用以下可重复探针
比较 1k/10k/100k 合成库的旧扫描路径和当前第一页。每个规模覆盖英文、
中文短词、中英混合、trigram 和最近条目，以及 type/tag 组合；每种查询记录
21 次热查询与 5 次新连接查询的 p50/p95、单条写入后延迟、第一页 ID 与总数
一致性、物化条目数及 Linux 进程 RSS。新连接只清空 SQLite 连接缓存，
不清操作系统页缓存；输出明确记录该边界。延迟探针与 SQL trace 分开运行，
避免记录完整 SQL 的开销污染计时。SQL trace 在真实 `ops::search_page` 上记录
两条 SELECT、BEGIN/COMMIT 和 SQLite FTS 内部语句；三个规模均检查没有重复
OFFSET 翻页。小规模语句数回归进入普通测试，三规模探针按需运行。
探针没有依赖机器速度的通过阈值：

```sh
cargo test --locked -p refine-core --test search_filters filtered_search_scale_probe -- --ignored --nocapture
cargo test --locked -p refine-core --lib filtered_search_sql_scale_probe -- --ignored --nocapture
```

2026-10-07 的实际合成库结果保存在
[`docs/eval/search-scale-20261007.json`](eval/search-scale-20261007.json)。
该次共享 Linux 主机的 debug 构建中，100k 英文首屏热查询 p50/p95 为
742.6/828.9 ms；旧 128 条分页扫描为 455.2 秒、782 次调用。
15 种规模/查询组合均保留完整 total 和首屏 ID。SQL trace 实测始终为
两条顶层 SELECT 加 BEGIN/COMMIT；FTS 内部语句仍随命中增长，精确 count、
排序和中文短词扫描仍有成本。该结果不等同于生产延迟承诺或真实任务语义召回率。

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

### 采集接收顺序、发布与回执

每个首次持久化的 conversation 由同一 SQLite 数据库分配递增接收序号。
正文、来源、URL、标题或采集时间真正改变时，使用同一个接收序列取得新版本；
状态更新和幂等重放保持原版本。任务领取时固定源版本，提交结果时在事务内
重新核对版本和实际输入。运行期间修改后再改回也不能让旧任务提交；这种任务
以 `capture_source_changed` 失败并保留终态回执，不会被恢复流程自动重跑。
相同 URL 的普通 HTTP capture 以**已经成功发布的最大接收序号**作为当前版本：
较新任务尚在运行或失败时，保留此前有效结果；较新任务一旦发布，较早采集的
任务即使随后完成，也不能覆盖 Document 或 Items。判定、正文与 Items 替换、
job/conversation 终态位于同一个 IMMEDIATE 事务中。该顺序不比较客户端
`captured_at`：离线采集稍后送达会获得新的接收序号，相同或不准确的客户端时间
不会改变接收顺序。

被较新采集替代的任务仍是 `succeeded` / `processed`，表示已经处理完毕。
创建回执、job 查询和 conversation 列表可包含 `superseded_by`，其值是替代它的
conversation ID；再次替代时可以沿此关系追到当前发布者。被替代的 conversation
清空 `item_ids`，不会把已删除的旧 Items 描述为当前结果。会话原文与终态保留。
这套规则仅作用于普通 capture 的提炼发布；Session observation 的人工覆盖、
tombstone 和历史投影继续遵循自己的合同。

未记录接收序号的历史 conversation 保持未知版本 0；升级不会用 rowid 或客户端
时间推测旧顺序，也不会重发或改写已有文档。已经记录的正版本与序列高水位保留。
历史任务可以在尚无正版本成功发布时完成；正版本发布后，晚到的版本 0 结果
只形成 `superseded_by` 回执，不会替换当前文档。

旧库导入在事务内使用未知历史模式，不把另一数据库的任务版本当成本地顺序。
已在本地取得正版本的采集和任务不被旧库的同 key、同 ID 或更换父级的记录覆盖；
已由正版本发布的 Document 和 Items 也受保护。未知历史记录仍按原有的版本、
单向状态迁移和身份映射规则合并；导入完成或回滚后恢复正常接收计数。
升级时应重启所有使用该数据库的旧版本提炼进程，使所有发布者执行同一规则。

### 幂等重放与准入配额

准入在同一写事务内先查幂等键。已有请求须匹配原 owner、source、URL、title、
content 和 metadata，否则返回 400，并要求新的请求使用新的键。创建时间与
capture 时间不参与身份比较；回放保留原记录的时间，避免未传时间的同一请求无法重放。
只有真正创建 conversation 的分支才检查启用的 Item 配额。原请求的 captured、
pending 或 processed 回执可以在之后达到配额时继续重放；processed 提炼重放
保留可查询的终态 job ID，且不重新运行该任务。

配额仍是新 capture 的准入阈值，作用于不在 premium 集合中的用户；它不是为
尚未完成的提炼预留未知数量 Items 的硬上限。新请求被拒绝时不创建 conversation、
job 或接收序号。

计数沿用单用户、全库 Items 合同。IMMEDIATE 事务保护幂等键查询、配额计数、
conversation 和初始 job 写入；事务外曾经读到的 key 不能作为后续写入的准入许可。
其他写入者删除或修改该 key 后，请求必须重新满足准入配额。

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
