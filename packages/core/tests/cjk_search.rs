use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Document, DocumentRepository, Item, ItemRepository};
use rusqlite::Connection;

async fn assert_hits(store: &SqliteStore, query: &str, expected: &[&str]) {
    let mut ids: Vec<_> = ItemRepository::search_text(store, query, 0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id().to_string())
        .collect();
    ids.sort();
    let mut expected: Vec<_> = expected.iter().map(|id| id.to_string()).collect();
    expected.sort();
    assert_eq!(ids, expected, "query={query}");
    assert_eq!(
        ItemRepository::count_text_hits(store, query).await.unwrap(),
        expected.len()
    );
}

#[tokio::test]
async fn cjk_phrases_match_inside_text_without_losing_english_prefixes_or_and_semantics() {
    let store = SqliteStore::in_memory().unwrap();
    let mut release = Item::new_knowledge("使用灰度发布降低部署风险", "安全上线");
    release.set_content("Rust asynchronous service deployment");
    ItemRepository::save(&store, &release).await.unwrap();
    let other = Item::new_knowledge("学习数据库索引", "Rust storage");
    ItemRepository::save(&store, &other).await.unwrap();
    for query in [
        "灰度发布",
        "部署",
        "度",
        "灰度发布 Rust",
        "部署 asyn",
        "\"灰度发布\"",
        "灰度发布 部署",
    ] {
        assert_hits(&store, query, &[release.id().as_str()]).await;
    }
    for query in [
        "灰度发布 SQLite",
        "部署 存储",
        "不存在的内容",
        "+++ !!!",
        "",
    ] {
        assert_hits(&store, query, &[]).await;
    }
    assert_hits(
        &store,
        "Rust",
        &[release.id().as_str(), other.id().as_str()],
    )
    .await;
    assert_hits(&store, "asyn", &[release.id().as_str()]).await;
    let first = ItemRepository::search_text(&store, "Rust", 0, 1)
        .await
        .unwrap();
    let second = ItemRepository::search_text(&store, "Rust", 1, 1)
        .await
        .unwrap();
    assert_ne!(first[0].id(), second[0].id());
}

#[tokio::test]
async fn chinese_index_tracks_edits_and_deletes() {
    let store = SqliteStore::in_memory().unwrap();
    let mut item = Item::new_knowledge("使用灰度发布", "");
    ItemRepository::save(&store, &item).await.unwrap();
    assert_hits(&store, "灰度发布", &[item.id().as_str()]).await;
    item.set_title("改用蓝绿部署");
    ItemRepository::save(&store, &item).await.unwrap();
    assert_hits(&store, "灰度发布", &[]).await;
    assert_hits(&store, "蓝绿部署", &[item.id().as_str()]).await;
    ItemRepository::delete(&store, item.id()).await.unwrap();
    assert_hits(&store, "蓝绿部署", &[]).await;
}

#[tokio::test]
async fn document_search_indexes_local_content_and_keeps_remem_references_empty() {
    let store = SqliteStore::in_memory().unwrap();
    let mut doc = Document::new("browser", "介绍使用灰度发布降低部署风险");
    doc.set_url("https://example.test/release");
    DocumentRepository::save(&store, &doc).await.unwrap();
    let mut reference = Document::new("codex-session", "");
    reference.set_title("关于数据库的会话");
    reference.set_url("remem://raw-session/v2/reference");
    DocumentRepository::save(&store, &reference).await.unwrap();
    for query in ["灰度发布", "部署"] {
        let hits = DocumentRepository::search_text(&store, query, 0, 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id(), doc.id());
        assert_eq!(
            DocumentRepository::count_text_hits(&store, query)
                .await
                .unwrap(),
            1
        );
    }
    assert!(DocumentRepository::find_by_id(&store, reference.id())
        .await
        .unwrap()
        .unwrap()
        .raw_content()
        .is_empty());
    DocumentRepository::delete(&store, doc.id()).await.unwrap();
    assert_eq!(
        DocumentRepository::count_text_hits(&store, "灰度发布")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn existing_rows_are_backfilled_and_reopening_preserves_trigger_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("../src/infra/schema.sql"))
        .unwrap();
    conn.execute_batch("DROP TRIGGER items_ai; DROP TRIGGER documents_ai;
        INSERT INTO items(id,item_type,title,summary,content,tags,created_at,updated_at)
        VALUES('legacy','knowledge','使用灰度发布','','','[]','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
        INSERT INTO documents(id,title,raw_content,source,url,captured_at,created_at,updated_at)
        VALUES('legacy-doc','旧文档','中文部署技巧','browser','https://example.test/legacy','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');").unwrap();
    drop(conn);
    for _ in 0..2 {
        let store = SqliteStore::open(&path).unwrap();
        assert_hits(&store, "灰度发布", &["legacy"]).await;
        assert_eq!(
            DocumentRepository::count_text_hits(&store, "部署")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            DocumentRepository::count_text_hits(&store, "browser")
                .await
                .unwrap(),
            1
        );
    }
    let conn = Connection::open(&path).unwrap();
    for table in [
        "items_fts",
        "items_substring_fts",
        "documents_fts",
        "documents_substring_fts",
    ] {
        conn.execute(
            &format!("INSERT INTO {table}({table}, rank) VALUES('integrity-check', 1)"),
            [],
        )
        .unwrap();
    }
}
