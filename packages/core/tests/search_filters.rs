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
    assert!(
        result.items[0]
            .item
            .tags()
            .iter()
            .any(|tag| tag.as_str() == "backend")
    );
}
