# Refine - 发布检查清单

> 目标：保证小规模公开测试可回滚、可观测、可排障

---

## 1. 发布前必查

1. 代码：`main` 分支 CI 绿灯，关键功能均有自动化测试记录。
2. 配置：生产环境使用 `apps/server/.env.production.example`，已替换 `REFINE_API_TOKEN`。
3. 配额：`REFINE_FREE_QUOTA_ITEMS` 已配置（默认 `100`），并验证超限提示可见。
4. 鉴权：`REFINE_ENV=production` 时未配置 token 会启动失败（已验证）。
5. 兼容：扩展在 ChatGPT / Claude / Gemini 页面注入正常，无明显卡顿。

---

## 2. 可观测性清单

1. 漏斗事件可见：`conversation_extracted -> conversation_synced -> recommendation_exposed -> recommendation_clicked -> knowledge_reused`。
2. Dashboard 可访问：`GET /dashboard`。
3. 错误可见：`syncStatus.lastError`、服务端 `warn/error` 日志可追踪。
4. 配额可见：`GET /v1/quota` 返回 `used/limit/remaining/exceeded`。

---

## 3. 基础告警建议

1. `5xx` 比例告警：5 分钟窗口内错误率 > 3%。
2. 同步失败告警：`sync failed` 连续增长且 10 分钟未恢复。
3. 鉴权失败告警：`401` 异常突增（疑似 token 失配或泄漏）。
4. 配额告警：`exceeded=true` 用户占比持续上升（用于评估升级引导）。

---

## 4. 回滚脚本与开关

### 4.1 服务端快速回滚

```bash
# 关闭语义混排，回退到关键词
export REFINE_ENABLE_SEMANTIC_SEARCH=false

# 临时放开配额限制（0 = 不限）
export REFINE_FREE_QUOTA_ITEMS=0

# 开发/应急窗口（仅受控场景）
unset REFINE_ENV
```

### 4.2 扩展侧快速降级

1. 关闭输入态推荐（每站点开关，用户可在页面右下角切换）。
2. 保留“提取当前对话 + 同步队列”主链路，确保基础功能不中断。
3. 必要时回滚到上一个扩展包版本。

---

## 5. FAQ（上线值班）

### Q1: 用户提示 Unauthorized 怎么处理？

确认服务端 `REFINE_API_TOKEN` 与扩展侧 Bearer token 一致；检查网关是否剥离了 `Authorization` 头。

### Q2: 用户提示 Free quota exceeded 怎么处理？

确认 `GET /v1/quota` 返回；可临时提高 `REFINE_FREE_QUOTA_ITEMS` 或开启升级白名单。

### Q3: 推荐结果突然变差怎么办？

先将 `REFINE_ENABLE_SEMANTIC_SEARCH=false` 回退到关键词模式，再分析评测脚本输出：

```bash
node scripts/eval_recommendations.mjs --base-url http://127.0.0.1:21567
```

### Q4: 同步队列一直失败怎么办？

先看 popup 的 `lastError`，再排查 `/health`、鉴权、网络连通性；必要时重试队列并观察 10 分钟趋势。
