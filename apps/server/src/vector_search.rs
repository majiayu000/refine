use async_trait::async_trait;
use refine_core::error::InfraResult;
use refine_core::search::VectorSearch;
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use tokio::sync::RwLock;

const EMBEDDING_DIM: usize = 256;

#[derive(Default)]
pub struct InMemoryVectorSearch {
    vectors: RwLock<HashMap<String, Vec<f32>>>,
}

impl InMemoryVectorSearch {
    pub fn new() -> Self {
        Self::default()
    }

    fn embed(text: &str) -> Vec<f32> {
        let normalized = text.trim().to_lowercase();
        let mut embedding = vec![0.0_f32; EMBEDDING_DIM];
        if normalized.is_empty() {
            return embedding;
        }

        for token in normalized.split(|ch: char| !ch.is_alphanumeric()) {
            if token.len() < 2 {
                continue;
            }
            let idx = hashed_index(token);
            embedding[idx] += 1.0;
        }

        let compact_chars = normalized
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<Vec<_>>();
        if compact_chars.len() >= 3 {
            for window in compact_chars.windows(3) {
                let mut gram = String::with_capacity(3);
                for ch in window {
                    gram.push(*ch);
                }
                let idx = hashed_index(&gram);
                embedding[idx] += 0.35;
            }
        }

        l2_normalize(&mut embedding);
        embedding
    }

    fn score(query_embedding: &[f32], doc_embedding: &[f32]) -> f32 {
        query_embedding
            .iter()
            .zip(doc_embedding.iter())
            .map(|(left, right)| left * right)
            .sum()
    }
}

#[async_trait]
impl VectorSearch for InMemoryVectorSearch {
    async fn search(&self, query: &str, limit: usize) -> InfraResult<Vec<(String, f32)>> {
        let query_embedding = Self::embed(query);
        if query_embedding.iter().all(|value| *value <= 0.0) {
            return Ok(Vec::new());
        }

        let vectors = self.vectors.read().await;
        let mut ranked = vectors
            .iter()
            .map(|(id, embedding)| (id.clone(), Self::score(&query_embedding, embedding)))
            .filter(|(_, score)| score.is_finite() && *score > 0.0)
            .collect::<Vec<_>>();

        ranked.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
        ranked.truncate(limit);
        Ok(ranked)
    }

    async fn index(&self, id: &str, text: &str) -> InfraResult<()> {
        let embedding = Self::embed(text);
        let mut vectors = self.vectors.write().await;
        vectors.insert(id.to_string(), embedding);
        Ok(())
    }

    async fn remove(&self, id: &str) -> InfraResult<()> {
        let mut vectors = self.vectors.write().await;
        vectors.remove(id);
        Ok(())
    }
}

fn hashed_index(value: &str) -> usize {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    (hasher.finish() as usize) % EMBEDDING_DIM
}

fn l2_normalize(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm <= 0.0 {
        return;
    }
    for value in values {
        *value /= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::InMemoryVectorSearch;
    use refine_core::search::VectorSearch;

    #[tokio::test]
    async fn vector_search_prefers_semantically_closer_text() {
        let index = InMemoryVectorSearch::new();
        index
            .index(
                "item-rust",
                "axum middleware authentication pattern in rust service",
            )
            .await
            .unwrap();
        index
            .index(
                "item-python",
                "pandas dataframe cleaning and csv analysis workflow",
            )
            .await
            .unwrap();

        let hits = index
            .search("how to build rust auth middleware with axum", 2)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0, "item-rust");
    }

    #[tokio::test]
    async fn remove_keeps_vector_index_in_sync() {
        let index = InMemoryVectorSearch::new();
        index
            .index("item-delete", "typescript dom observer mutation example")
            .await
            .unwrap();
        index.remove("item-delete").await.unwrap();

        let hits = index.search("dom observer", 5).await.unwrap();
        assert!(hits.is_empty());
    }
}
