use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemRepository, ItemType, Tag};
use refine_core::search::{SearchEngine, SearchQuery};
use std::sync::Arc;

#[tokio::test]
async fn repository_find_by_tags_requires_all_tags() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    let mut a = Item::new_knowledge("A", "first");
    a.set_tags(vec![
        Tag::new("rust").expect("invalid tag"),
        Tag::new("async").expect("invalid tag"),
    ])
    .expect("set tags failed");

    let mut b = Item::new_knowledge("B", "second");
    b.set_tags(vec![Tag::new("rust").expect("invalid tag")])
        .expect("set tags failed");

    store.save(&a).await.expect("save failed");
    store.save(&b).await.expect("save failed");

    let found = store
        .find_by_tags(&[
            Tag::new("rust").expect("invalid tag"),
            Tag::new("async").expect("invalid tag"),
        ])
        .await
        .expect("find_by_tags failed");

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title(), "A");
}

#[tokio::test]
async fn search_engine_applies_type_and_tag_filters_for_keyword_search() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));
    let engine = SearchEngine::new(store.clone());

    let mut a = Item::new_knowledge("Rust Memory", "knowledge");
    a.set_content("rust ownership and memory model");
    a.set_tags(vec![
        Tag::new("rust").expect("invalid tag"),
        Tag::new("backend").expect("invalid tag"),
    ])
    .expect("set tags failed");

    let mut b = Item::new_skill("Rust Skill", "skill");
    b.set_content("rust ownership and memory model");
    b.set_tags(vec![
        Tag::new("rust").expect("invalid tag"),
        Tag::new("backend").expect("invalid tag"),
    ])
    .expect("set tags failed");

    let mut c = Item::new_knowledge("Rust UI", "knowledge ui");
    c.set_content("rust frontend rendering");
    c.set_tags(vec![
        Tag::new("rust").expect("invalid tag"),
        Tag::new("frontend").expect("invalid tag"),
    ])
    .expect("set tags failed");

    store.save(&a).await.expect("save failed");
    store.save(&b).await.expect("save failed");
    store.save(&c).await.expect("save failed");

    let result = engine
        .search(
            SearchQuery::new("rust")
                .with_type(ItemType::Knowledge)
                .with_tags(vec!["backend".to_string()])
                .with_limit(10),
        )
        .await
        .expect("search failed");

    assert_eq!(result.total, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].item.title(), "Rust Memory");
}

#[tokio::test]
async fn search_engine_applies_offset_and_limit_for_recent_results() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));
    let engine = SearchEngine::new(store.clone());

    for title in ["One", "Two", "Three"] {
        store
            .save(&Item::new_knowledge(title, "summary"))
            .await
            .expect("save failed");
    }

    let result = engine
        .search(SearchQuery::new("").with_offset(1).with_limit(1))
        .await
        .expect("search failed");

    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 1);
}

#[tokio::test]
async fn search_engine_applies_offset_and_limit_for_keyword_results() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));
    let engine = SearchEngine::new(store.clone());

    for title in ["Rust One", "Rust Two", "Rust Three"] {
        let mut item = Item::new_knowledge(title, "summary");
        item.set_content("rust memory model");
        store.save(&item).await.expect("save failed");
    }

    let result = engine
        .search(SearchQuery::new("rust").with_offset(1).with_limit(1))
        .await
        .expect("search failed");

    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 1);
}

#[tokio::test]
async fn search_engine_paginates_filtered_keyword_results_without_loading_all() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));
    let engine = SearchEngine::new(store.clone());

    for title in ["A", "B", "C"] {
        let mut item = Item::new_knowledge(title, "knowledge");
        item.set_content("rust async backend");
        item.set_tags(vec![
            Tag::new("rust").expect("invalid tag"),
            Tag::new("backend").expect("invalid tag"),
        ])
        .expect("set tags failed");
        store.save(&item).await.expect("save failed");
    }

    let mut filtered_out_type = Item::new_skill("D", "skill");
    filtered_out_type.set_content("rust async backend");
    filtered_out_type
        .set_tags(vec![Tag::new("backend").expect("invalid tag")])
        .expect("set tags failed");
    store.save(&filtered_out_type).await.expect("save failed");

    let mut filtered_out_tag = Item::new_knowledge("E", "knowledge");
    filtered_out_tag.set_content("rust async backend");
    filtered_out_tag
        .set_tags(vec![Tag::new("frontend").expect("invalid tag")])
        .expect("set tags failed");
    store.save(&filtered_out_tag).await.expect("save failed");

    let result = engine
        .search(
            SearchQuery::new("rust")
                .with_type(ItemType::Knowledge)
                .with_tags(vec!["backend".to_string()])
                .with_offset(1)
                .with_limit(1),
        )
        .await
        .expect("search failed");

    assert_eq!(result.total, 3);
    assert_eq!(result.items.len(), 1);
    assert!(matches!(
        result.items[0].item.item_type(),
        ItemType::Knowledge
    ));
    assert!(result.items[0]
        .item
        .tags()
        .iter()
        .any(|tag| tag.as_str() == "backend"));
}

#[tokio::test]
async fn sql_filtered_pages_match_reference_for_cjk_unicode_tags_and_pagination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filters.sqlite");
    let store = Arc::new(SqliteStore::open(&path).unwrap());
    let engine = SearchEngine::new(store.clone());
    for index in 0..24 {
        let mut item = if index % 3 == 0 {
            Item::new_skill(&format!("Rust 灰度发布 {index}"), "部署")
        } else {
            Item::new_knowledge(&format!("Rust 灰度发布 {index}"), "部署")
        };
        let mut tags = vec![Tag::new("BÄCKEND").unwrap()];
        if index % 2 == 0 {
            tags.push(Tag::new("数据库").unwrap());
        }
        item.set_tags(tags).unwrap();
        store.save(&item).await.unwrap();
    }
    // Existing database rows may predate Tag's canonical serializer. SQL must
    // preserve the Rust reader's Unicode lowercasing and whitespace trimming.
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE items SET tags = REPLACE(tags, 'bäckend', ' BÄCKEND ')",
        [],
    )
    .unwrap();
    for text in ["rust", "部署", "灰度发布", "部署 rust", "", "+++ !!!"] {
        let candidates = if text.is_empty() {
            store.find_all().await.unwrap()
        } else {
            store.search_text(text, 0, 1000).await.unwrap()
        };
        for item_type in [None, Some(ItemType::Knowledge), Some(ItemType::Skill)] {
            for tags in [
                vec![],
                vec!["BÄCKEND".to_string()],
                vec!["bäckend".to_string(), "数据库".to_string()],
                vec!["%') OR 1=1 --".to_string()],
                vec![" bäckend ".to_string()],
            ] {
                let expected: Vec<_> = candidates
                    .iter()
                    .filter(|item| {
                        item_type.is_none_or(|kind| kind == item.item_type())
                            && tags.iter().all(|tag| {
                                item.tags().iter().any(|item_tag| {
                                    item_tag.as_str().to_lowercase() == tag.to_lowercase()
                                })
                            })
                    })
                    .map(|item| item.id().to_string())
                    .collect();
                for (offset, limit) in [(0, 3), (3, 5), (0, 0), (99, 3)] {
                    let mut query = SearchQuery::new(text)
                        .with_tags(tags.clone())
                        .with_offset(offset)
                        .with_limit(limit);
                    query.filter.item_type = item_type;
                    let result = engine.search(query).await.unwrap();
                    assert_eq!(
                        result.total,
                        expected.len(),
                        "query={text:?}, type={item_type:?}, tags={tags:?}"
                    );
                    assert_eq!(
                        result
                            .items
                            .iter()
                            .map(|hit| hit.item.id().to_string())
                            .collect::<Vec<_>>(),
                        expected
                            .iter()
                            .skip(offset)
                            .take(limit)
                            .cloned()
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn filtered_page_does_not_deserialize_matching_rows_outside_the_requested_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bounded-page.sqlite");
    let store = Arc::new(SqliteStore::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    seed_rows(&conn, 512);
    // All rows still match the same text/type/tags. Reading their source JSON
    // would fail, so success proves only the requested full Items were decoded.
    conn.execute(
        "UPDATE items SET source = '{invalid-source-json' WHERE id != '000001'",
        [],
    )
    .unwrap();
    let engine = SearchEngine::new(store.clone());
    for text in ["rust", ""] {
        let result = engine
            .search(
                SearchQuery::new(text)
                    .with_tags(vec!["backend".into()])
                    .with_type(ItemType::Knowledge)
                    .with_limit(1),
            )
            .await
            .unwrap();
        assert_eq!(result.total, 512);
        assert_eq!(result.items[0].item.id().as_str(), "000001");
    }
    assert!(store.search_text("rust", 0, 128).await.is_err());
}

fn seed_rows(conn: &rusqlite::Connection, rows: usize) {
    conn.execute(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < ?1)
         INSERT INTO items(id, item_type, title, summary, content, tags, created_at, updated_at)
         SELECT printf('%06d', n), 'knowledge', 'rust backend 灰度发布', '部署', 'rust async',
                '[\"backend\"]', '2026-10-06T00:00:00Z', '2026-10-06T00:00:00Z' FROM seq",
        [rows],
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "informational scale probe; no machine-dependent latency gate"]
async fn filtered_search_scale_probe() {
    for size in [1_000, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scale.sqlite");
        let store = Arc::new(SqliteStore::open(&path).unwrap());
        let conn = rusqlite::Connection::open(&path).unwrap();
        seed_rows(&conn, size);
        let started = std::time::Instant::now();
        let mut baseline = Vec::new();
        let mut calls = 0;
        loop {
            let batch = store
                .search_text("rust", baseline.len(), 128)
                .await
                .unwrap();
            calls += 1;
            let len = batch.len();
            baseline.extend(batch);
            if len < 128 {
                break;
            }
        }
        let baseline_time = started.elapsed();
        let engine = SearchEngine::new(store);
        let started = std::time::Instant::now();
        let result = engine
            .search(
                SearchQuery::new("rust")
                    .with_type(ItemType::Knowledge)
                    .with_tags(vec!["backend".into()])
                    .with_limit(20),
            )
            .await
            .unwrap();
        let filtered_time = started.elapsed();
        assert_eq!(result.total, baseline.len());
        assert_eq!(
            result
                .items
                .iter()
                .map(|hit| hit.item.id())
                .collect::<Vec<_>>(),
            baseline
                .iter()
                .take(20)
                .map(|item| item.id())
                .collect::<Vec<_>>()
        );
        eprintln!("rows={size} baseline_full_items={} baseline_page_calls={calls} baseline_ms={:.3} filtered_full_items={} filtered_ms={:.3}", baseline.len(), baseline_time.as_secs_f64() * 1000.0, result.items.len(), filtered_time.as_secs_f64() * 1000.0);
    }
}
