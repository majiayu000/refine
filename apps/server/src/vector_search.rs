use async_trait::async_trait;
use refine_core::error::{InfraError, InfraResult};
use refine_core::search::VectorSearch;
use rusqlite::{Connection, OpenFlags};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

const EMBEDDING_DIM: usize = 256;

#[derive(Default)]
pub struct FeatureHashSearch {
    vectors: RwLock<HashMap<String, Vec<f32>>>,
    database: Option<Arc<DatabaseSnapshot>>,
}

impl FeatureHashSearch {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh only after committed item mutations, including writes by the CLI or desktop.
    /// The generation and rows are read from one SQLite snapshot before replacing the index.
    pub(crate) async fn refresh_from_database(&self) -> InfraResult<usize> {
        let Some(database) = &self.database else {
            return Ok(self.vectors.read().await.len());
        };
        let _refresh_guard = database.refresh.lock().await;
        let known_generation = database.generation.load(AtomicOrdering::Acquire);
        let reader = database.clone();
        let snapshot = tokio::task::spawn_blocking(move || -> InfraResult<_> {
            let mut conn = reader
                .connection
                .lock()
                .map_err(|_| InfraError::Database("search snapshot lock poisoned".into()))?;
            let tx = conn.transaction().map_err(database_error)?;
            let generation: i64 = tx
                .query_row(
                    "SELECT generation FROM search_index_state WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .map_err(database_error)?;
            if generation == known_generation {
                return Ok(None);
            }
            let vectors = {
                let mut stmt = tx
                    .prepare("SELECT id, title, summary, content FROM items ORDER BY id")
                    .map_err(database_error)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(database_error)?;
                let mut vectors = HashMap::new();
                for row in rows {
                    let (id, title, summary, content) = row.map_err(database_error)?;
                    vectors.insert(id, Self::embed(&format!("{title} {summary} {content}")));
                }
                vectors
            };
            tx.commit().map_err(database_error)?;
            Ok(Some((generation, vectors)))
        })
        .await
        .map_err(|error| InfraError::Database(format!("search snapshot task failed: {error}")))??;
        if let Some((generation, vectors)) = snapshot {
            *self.vectors.write().await = vectors;
            database
                .generation
                .store(generation, AtomicOrdering::Release);
        }
        Ok(self.vectors.read().await.len())
    }

    pub fn with_database(path: &Path) -> InfraResult<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(database_error)?;
        refine_core::infra::configure_sqlite_connection(&conn)?;
        conn.pragma_update(None, "query_only", true)
            .map_err(database_error)?;
        Ok(Self {
            vectors: RwLock::new(HashMap::new()),
            database: Some(Arc::new(DatabaseSnapshot {
                connection: Mutex::new(conn),
                generation: AtomicI64::new(-1),
                refresh: tokio::sync::Mutex::new(()),
            })),
        })
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
impl VectorSearch for FeatureHashSearch {
    async fn search(&self, query: &str, limit: usize) -> InfraResult<Vec<(String, f32)>> {
        self.refresh_from_database().await?;
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

        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        ranked.truncate(limit);
        Ok(ranked)
    }

    async fn index(&self, id: &str, text: &str) -> InfraResult<()> {
        // A persistent index is derived only from committed rows. Late extraction
        // callbacks must not overwrite a newer database snapshot with stale text.
        if self.database.is_some() {
            return Ok(());
        }
        let embedding = Self::embed(text);
        let mut vectors = self.vectors.write().await;
        vectors.insert(id.to_string(), embedding);
        Ok(())
    }

    async fn remove(&self, id: &str) -> InfraResult<()> {
        // A delayed delete callback can arrive after another writer restored the
        // same ID. The database generation, checked by search, is authoritative.
        if self.database.is_some() {
            return Ok(());
        }
        let mut vectors = self.vectors.write().await;
        vectors.remove(id);
        Ok(())
    }
}

struct DatabaseSnapshot {
    connection: Mutex<Connection>,
    generation: AtomicI64,
    refresh: tokio::sync::Mutex<()>,
}

fn database_error(error: rusqlite::Error) -> InfraError {
    InfraError::Database(error.to_string())
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
    use super::FeatureHashSearch;
    use refine_core::search::VectorSearch;

    #[tokio::test]
    async fn feature_hash_search_prefers_lexical_overlap() {
        let index = FeatureHashSearch::new();
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
    async fn database_index_observes_external_insert_edit_and_delete_without_restart() {
        use refine_core::infra::SqliteStore;
        use refine_core::knowledge::{Item, ItemRepository};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.sqlite");
        let writer = SqliteStore::open(&path).unwrap();
        let index = FeatureHashSearch::with_database(&path).unwrap();
        assert!(index.search("quartz", 10).await.unwrap().is_empty());
        let mut item = Item::new_knowledge("quartz", "");
        writer.save(&item).await.unwrap();
        let hits = index.search("quartz", 10).await.unwrap();
        assert_eq!(hits[0].0, item.id().as_str());
        assert!((hits[0].1 - 1.0).abs() < 0.0001);
        item.set_title("zirconium");
        writer.save(&item).await.unwrap();
        let updated = index.search("zirconium", 10).await.unwrap();
        assert!((updated[0].1 - 1.0).abs() < 0.0001);
        writer.delete(item.id()).await.unwrap();
        assert!(index.search("zirconium", 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn late_callbacks_cannot_corrupt_a_newer_database_snapshot() {
        use refine_core::infra::SqliteStore;
        use refine_core::knowledge::{Item, ItemRepository};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restored.sqlite");
        let writer = SqliteStore::open(&path).unwrap();
        let index = FeatureHashSearch::with_database(&path).unwrap();
        let mut item = Item::new_knowledge("quartz", "");
        writer.save(&item).await.unwrap();
        writer.delete(item.id()).await.unwrap();
        item.set_title("zirconium");
        writer.save(&item).await.unwrap();
        assert_eq!(index.search("zirconium", 10).await.unwrap().len(), 1);
        // These callbacks belong to operations that completed before the restore.
        index.remove(item.id().as_str()).await.unwrap();
        index.index(item.id().as_str(), "quartz").await.unwrap();
        let hits = index.search("zirconium", 10).await.unwrap();
        assert_eq!(hits[0].0, item.id().as_str());
        assert!((hits[0].1 - 1.0).abs() < 0.0001);
    }

    #[tokio::test]
    async fn remove_keeps_vector_index_in_sync() {
        let index = FeatureHashSearch::new();
        index
            .index("item-delete", "typescript dom observer mutation example")
            .await
            .unwrap();
        index.remove("item-delete").await.unwrap();

        let hits = index.search("dom observer", 5).await.unwrap();
        assert!(hits.is_empty());
    }
}
