use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemRepository};

#[tokio::test]
async fn search_text_handles_special_characters_without_error() {
    let store = SqliteStore::in_memory().expect("failed to create sqlite store");

    let mut item = Item::new_knowledge("C++ note", "language note");
    item.set_content("c++ template and ownership");
    store.save(&item).await.expect("save failed");

    let hits = store.search_text("c++", 0, 10).await;
    assert!(hits.is_ok(), "expected c++ query to avoid fts syntax error");

    let count = store.count_text_hits("\"unterminated").await;
    assert!(
        count.is_ok(),
        "expected malformed quote query to avoid fts parse error"
    );
}
