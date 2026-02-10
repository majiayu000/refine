use async_trait::async_trait;
use refine_core::error::InfraResult;
use refine_core::infra::SqliteStore;
use refine_core::knowledge::{Item, ItemRepository, ItemType};
use refine_core::search::{SearchEngine, SearchQuery, VectorSearch};
use std::sync::Arc;

struct FixedVectorSearch {
    hits: Vec<(String, f32)>,
}

#[async_trait]
impl VectorSearch for FixedVectorSearch {
    async fn search(&self, _query: &str, limit: usize) -> InfraResult<Vec<(String, f32)>> {
        Ok(self.hits.iter().take(limit).cloned().collect())
    }

    async fn index(&self, _id: &str, _text: &str) -> InfraResult<()> {
        Ok(())
    }

    async fn remove(&self, _id: &str) -> InfraResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn hybrid_search_keeps_keyword_fallback_when_vector_empty() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));

    let mut rust_item = Item::new_knowledge("Rust Middleware", "auth middleware");
    rust_item.set_content("rust axum middleware auth pipeline");
    store.save(&rust_item).await.expect("save failed");

    let mut other = Item::new_knowledge("Python Notebook", "analysis");
    other.set_content("python notebook pandas");
    store.save(&other).await.expect("save failed");

    let engine = SearchEngine::new(store)
        .with_vector_search(Arc::new(FixedVectorSearch { hits: Vec::new() }));

    let result = engine
        .search(SearchQuery::new("rust middleware").with_limit(3))
        .await
        .expect("search failed");

    assert!(!result.items.is_empty());
    assert_eq!(result.items[0].item.title(), "Rust Middleware");
}

#[tokio::test]
async fn hybrid_search_merges_keyword_and_vector_scores() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));

    let mut strong = Item::new_knowledge("Rust Auth", "best match");
    strong.set_content("axum rust middleware auth");
    let strong_id = strong.id().to_string();
    store.save(&strong).await.expect("save failed");

    let mut weak = Item::new_knowledge("Rust Logging", "weaker match");
    weak.set_content("rust tracing logger");
    let weak_id = weak.id().to_string();
    store.save(&weak).await.expect("save failed");

    let engine = SearchEngine::new(store).with_vector_search(Arc::new(FixedVectorSearch {
        hits: vec![(strong_id, 0.95), (weak_id, 0.15)],
    }));

    let result = engine
        .search(SearchQuery::new("rust middleware").with_limit(5))
        .await
        .expect("search failed");

    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].item.title(), "Rust Auth");
    assert!(result.items[0].score > result.items[1].score);
}

#[tokio::test]
async fn hybrid_search_respects_type_filter_for_semantic_candidates() {
    let store = Arc::new(SqliteStore::in_memory().expect("failed to create sqlite store"));

    let mut skill = Item::new_skill("Rust Ops Skill", "skill entry");
    skill.set_content("rust deploy ci pipeline");
    let skill_id = skill.id().to_string();
    store.save(&skill).await.expect("save failed");

    let mut knowledge = Item::new_knowledge("Rust Deployment Notes", "knowledge");
    knowledge.set_content("rust deploy checklist");
    let knowledge_id = knowledge.id().to_string();
    store.save(&knowledge).await.expect("save failed");

    let engine = SearchEngine::new(store).with_vector_search(Arc::new(FixedVectorSearch {
        hits: vec![(skill_id, 0.99), (knowledge_id, 0.60)],
    }));

    let result = engine
        .search(
            SearchQuery::new("rust deploy")
                .with_type(ItemType::Knowledge)
                .with_limit(10),
        )
        .await
        .expect("search failed");

    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].item.item_type(), ItemType::Knowledge);
}
