use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::Json;
use refine_core::knowledge::ItemRepository;
use refine_core::search::SearchQuery as CoreSearchQuery;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::authorize_user;
use crate::extraction::spawn_extraction;
use crate::models::{
    normalize_timestamp, now_iso, ConversationRecord, ConversationStatus,
    CreateConversationRequest, CreateExtractionJobRequest, ExtractionJobRecord, ExtractionMode,
    ItemDto, JobStatus, ListItemsQuery, SearchQuery,
};
use crate::state::AppState;

pub async fn health() -> impl IntoResponse {
    ok(json!({
        "message": "Refine cloud API (Rust) is running"
    }))
}

pub async fn dashboard_page() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

pub async fn create_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateConversationRequest>,
) -> impl IntoResponse {
    let user_id = match authorize_user(&headers, state.api_token.as_deref()) {
        Ok(user_id) => user_id,
        Err(err) => return err_response(StatusCode::UNAUTHORIZED, &err),
    };

    let content = match payload.content.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: content"),
    };
    let url = match payload.url.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: url"),
    };
    let source = match payload.source.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "Missing required field: source"),
    };
    let idempotency_key = match payload.idempotency_key.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => {
            return err_response(
                StatusCode::BAD_REQUEST,
                "Missing required field: idempotency_key",
            )
        }
    };

    if let Some(conversation_id) = find_conversation_by_idempotency(&state, &idempotency_key).await
    {
        let conversations = state.conversations.read().await;
        if let Some(record) = conversations.get(&conversation_id) {
            return ok(json!({
                "conversation_id": record.id,
                "status": record.status,
                "deduplicated": true
            }));
        }
    }

    let now = now_iso();
    let conversation_id = Uuid::new_v4().to_string();
    let job_id = Uuid::new_v4().to_string();
    let mode = ExtractionMode::Auto;

    let conversation = ConversationRecord {
        id: conversation_id.clone(),
        user_id,
        source,
        url,
        title: payload.title.filter(|v| !v.trim().is_empty()),
        raw_content: content,
        captured_at: normalize_timestamp(payload.captured_at),
        created_at: now.clone(),
        status: ConversationStatus::Queued,
        idempotency_key: idempotency_key.clone(),
        item_ids: Vec::new(),
        last_error: None,
    };

    let job = ExtractionJobRecord {
        id: job_id.clone(),
        conversation_id: conversation_id.clone(),
        mode: mode.clone(),
        status: JobStatus::Pending,
        created_at: now.clone(),
        updated_at: now,
        error: None,
    };

    {
        let mut conversations = state.conversations.write().await;
        conversations.insert(conversation_id.clone(), conversation);
    }
    {
        let mut idempotency = state.idempotency.write().await;
        idempotency.insert(idempotency_key, conversation_id.clone());
    }
    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    spawn_extraction(state, conversation_id.clone(), job_id.clone(), mode);

    ok(json!({
        "conversation_id": conversation_id,
        "status": ConversationStatus::Queued,
        "job_id": job_id
    }))
}

pub async fn create_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateExtractionJobRequest>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let conversation_id = match payload.conversation_id.map(|v| v.trim().to_string()) {
        Some(v) if !v.is_empty() => v,
        _ => return err_response(StatusCode::BAD_REQUEST, "conversation_id is required"),
    };

    {
        let conversations = state.conversations.read().await;
        if !conversations.contains_key(&conversation_id) {
            return err_response(StatusCode::NOT_FOUND, "Conversation not found");
        }
    }

    let mode = ExtractionMode::from_option(payload.mode);
    let now = now_iso();
    let job_id = Uuid::new_v4().to_string();

    let job = ExtractionJobRecord {
        id: job_id.clone(),
        conversation_id: conversation_id.clone(),
        mode: mode.clone(),
        status: JobStatus::Pending,
        created_at: now.clone(),
        updated_at: now,
        error: None,
    };

    {
        let mut jobs = state.jobs.write().await;
        jobs.insert(job_id.clone(), job);
    }

    spawn_extraction(state, conversation_id, job_id.clone(), mode);

    ok(json!({
        "job_id": job_id,
        "status": JobStatus::Pending
    }))
}

pub async fn get_extraction_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let jobs = state.jobs.read().await;
    let Some(job) = jobs.get(&job_id).cloned() else {
        return err_response(StatusCode::NOT_FOUND, "Job not found");
    };

    ok(json!({ "job": job }))
}

pub async fn list_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListItemsQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let cursor = query.cursor.unwrap_or(0);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let total = match state.store.count_items(None).await {
        Ok(total) => total,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };
    let items = match state.store.find_recent(None, cursor, limit).await {
        Ok(items) => items,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let next_cursor = if cursor + items.len() < total {
        Some(cursor + items.len())
    } else {
        None
    };

    let data = items.iter().map(ItemDto::from).collect::<Vec<_>>();
    ok(json!({
        "items": data,
        "next_cursor": next_cursor
    }))
}

pub async fn search_items(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> impl IntoResponse {
    if let Err(err) = authorize_user(&headers, state.api_token.as_deref()) {
        return err_response(StatusCode::UNAUTHORIZED, &err);
    }

    let keyword = query.q.unwrap_or_default().trim().to_string();
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    if keyword.is_empty() {
        return ok(json!({ "items": [] }));
    }

    let result = match state
        .engine
        .search(CoreSearchQuery::new(&keyword).with_limit(limit))
        .await
    {
        Ok(result) => result,
        Err(err) => return err_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let data = result
        .items
        .iter()
        .map(|hit| ItemDto::from(&hit.item))
        .collect::<Vec<_>>();

    ok(json!({ "items": data }))
}

async fn find_conversation_by_idempotency(state: &Arc<AppState>, key: &str) -> Option<String> {
    let index = state.idempotency.read().await;
    index.get(key).cloned()
}

fn ok(payload: serde_json::Value) -> (StatusCode, Json<serde_json::Value>) {
    let mut body = serde_json::Map::new();
    body.insert("success".to_string(), serde_json::Value::Bool(true));
    if let serde_json::Value::Object(map) = payload {
        for (k, v) in map {
            body.insert(k, v);
        }
    }
    (StatusCode::OK, Json(serde_json::Value::Object(body)))
}

fn err_response(status: StatusCode, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "success": false,
            "message": message
        })),
    )
}

const DASHBOARD_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Refine Dashboard</title>
  <style>
    :root {
      color-scheme: light;
      --bg: #f6f7fb;
      --card: #ffffff;
      --text: #0f172a;
      --muted: #475569;
      --primary: #2563eb;
      --border: #dbe1ea;
      --danger: #dc2626;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font: 14px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }
    .wrap { max-width: 980px; margin: 0 auto; padding: 20px; }
    .head { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-bottom: 14px; }
    .title { font-size: 22px; font-weight: 700; margin: 0; }
    .sub { color: var(--muted); font-size: 12px; margin-top: 2px; }
    .tools {
      display: grid;
      grid-template-columns: 1fr auto auto;
      gap: 8px;
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 10px;
      margin-bottom: 10px;
    }
    .token {
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 8px;
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 10px;
      margin-bottom: 12px;
    }
    input {
      width: 100%;
      border: 1px solid var(--border);
      border-radius: 10px;
      padding: 10px 12px;
      outline: none;
      background: #fff;
    }
    button {
      border: 0;
      border-radius: 10px;
      padding: 10px 14px;
      cursor: pointer;
      background: var(--primary);
      color: #fff;
      font-weight: 600;
    }
    button.secondary { background: #334155; }
    button[disabled] { opacity: .55; cursor: not-allowed; }
    .notice {
      border: 1px solid var(--border);
      background: #fff;
      border-radius: 10px;
      padding: 10px 12px;
      margin-bottom: 12px;
      color: var(--muted);
    }
    .notice.error {
      border-color: #fecaca;
      background: #fff5f5;
      color: var(--danger);
    }
    .list { display: grid; gap: 10px; }
    .item {
      background: var(--card);
      border: 1px solid var(--border);
      border-radius: 12px;
      padding: 12px;
    }
    .item h3 { margin: 0 0 6px; font-size: 16px; }
    .meta { color: var(--muted); font-size: 12px; margin-bottom: 8px; }
    .summary { margin: 0 0 8px; }
    .content {
      margin: 0;
      white-space: pre-wrap;
      background: #f8fafc;
      border: 1px solid #e2e8f0;
      border-radius: 8px;
      padding: 10px;
      max-height: 220px;
      overflow: auto;
    }
    .footer { margin-top: 12px; display: flex; justify-content: center; }
  </style>
</head>
<body>
  <main class="wrap">
    <div class="head">
      <div>
        <h1 class="title">Refine Dashboard</h1>
        <div class="sub">查看插件采集并提炼后的内容</div>
      </div>
      <button class="secondary" id="refreshBtn">刷新</button>
    </div>

    <section class="token">
      <input id="tokenInput" type="password" placeholder="可选：REFINE_API_TOKEN（如果服务端开启鉴权）" />
      <button id="saveTokenBtn" class="secondary">保存 Token</button>
    </section>

    <section class="tools">
      <input id="searchInput" type="text" placeholder="搜索标题/内容，例如 rust、提示词..." />
      <button id="searchBtn">搜索</button>
      <button id="clearBtn" class="secondary">清空</button>
    </section>

    <section id="notice" class="notice">正在加载...</section>
    <section id="list" class="list"></section>
    <div class="footer">
      <button id="moreBtn" class="secondary" style="display:none;">加载更多</button>
    </div>
  </main>

  <script>
    const state = {
      cursor: 0,
      nextCursor: null,
      query: "",
      loading: false,
      items: [],
      token: localStorage.getItem("refine_api_token") || ""
    }

    const noticeEl = document.getElementById("notice")
    const listEl = document.getElementById("list")
    const moreBtn = document.getElementById("moreBtn")
    const searchInput = document.getElementById("searchInput")
    const tokenInput = document.getElementById("tokenInput")

    tokenInput.value = state.token

    function authHeaders() {
      const headers = { "Content-Type": "application/json" }
      if (state.token) headers["Authorization"] = `Bearer ${state.token}`
      return headers
    }

    function esc(input) {
      const div = document.createElement("div")
      div.textContent = input ?? ""
      return div.innerHTML
    }

    function setNotice(text, isError = false) {
      noticeEl.textContent = text
      noticeEl.className = isError ? "notice error" : "notice"
    }

    function render() {
      if (!state.items.length) {
        listEl.innerHTML = ""
        setNotice("暂无数据。请在插件里点“提取当前对话”。")
      } else {
        setNotice(`已加载 ${state.items.length} 条`)
        listEl.innerHTML = state.items.map(item => `
          <article class="item">
            <h3>${esc(item.title || "(无标题)")}</h3>
            <div class="meta">${esc(item.item_type || "unknown")} · ${esc(item.created_at || "")}</div>
            <p class="summary">${esc(item.summary || "")}</p>
            <pre class="content">${esc(item.content || "")}</pre>
          </article>
        `).join("")
      }

      moreBtn.style.display = state.nextCursor == null || state.query ? "none" : "inline-block"
      moreBtn.disabled = state.loading
    }

    async function requestJson(path) {
      const res = await fetch(path, { headers: authHeaders() })
      let data = {}
      try {
        data = await res.json()
      } catch (_) {}
      return { ok: res.ok, status: res.status, data }
    }

    async function load(reset = true) {
      if (state.loading) return
      state.loading = true
      render()

      if (reset) {
        state.cursor = 0
        state.nextCursor = null
        state.items = []
      }

      try {
        let resp
        if (state.query) {
          const q = encodeURIComponent(state.query)
          resp = await requestJson(`/v1/search?q=${q}&limit=100`)
          if (!resp.ok) throw resp
          state.items = resp.data.items || []
          state.nextCursor = null
        } else {
          const respPath = `/v1/items?cursor=${state.cursor}&limit=30`
          resp = await requestJson(respPath)
          if (!resp.ok) throw resp
          const loaded = resp.data.items || []
          state.items = reset ? loaded : state.items.concat(loaded)
          state.nextCursor = resp.data.next_cursor ?? null
        }
      } catch (err) {
        if (err && err.status === 401) {
          setNotice("鉴权失败：请填写正确 Token 后重试。", true)
        } else {
          const msg = err?.data?.message || "加载失败，请检查服务是否在线。"
          setNotice(msg, true)
        }
      } finally {
        state.loading = false
        render()
      }
    }

    document.getElementById("refreshBtn").addEventListener("click", () => {
      state.query = searchInput.value.trim()
      void load(true)
    })
    document.getElementById("searchBtn").addEventListener("click", () => {
      state.query = searchInput.value.trim()
      void load(true)
    })
    document.getElementById("clearBtn").addEventListener("click", () => {
      searchInput.value = ""
      state.query = ""
      void load(true)
    })
    document.getElementById("saveTokenBtn").addEventListener("click", () => {
      state.token = tokenInput.value.trim()
      localStorage.setItem("refine_api_token", state.token)
      setNotice("Token 已保存")
    })
    moreBtn.addEventListener("click", () => {
      if (state.nextCursor == null) return
      state.cursor = state.nextCursor
      void load(false)
    })

    void load(true)
  </script>
</body>
</html>
"#;
