//! SQLite 存储实现
//!
//! ItemRepository 的 SQLite 实现

use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemId, ItemRepository, ItemType, Source, Tag};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

/// SQLite 存储
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// 打开或创建数据库
    pub fn open(path: impl AsRef<Path>) -> InfraResult<Self> {
        let conn = Connection::open(path).map_err(|e| InfraError::Database(e.to_string()))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// 内存数据库（测试用）
    pub fn in_memory() -> InfraResult<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| InfraError::Database(e.to_string()))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> InfraResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(include_str!("schema.sql"))
            .map_err(|e| InfraError::Database(e.to_string()))?;
        Ok(())
    }

    fn row_to_item(&self, row: &rusqlite::Row) -> InfraResult<Item> {
        let id: String = row.get(0).map_err(|e| InfraError::Database(e.to_string()))?;
        let type_str: String = row.get(1).map_err(|e| InfraError::Database(e.to_string()))?;
        let title: String = row.get(2).map_err(|e| InfraError::Database(e.to_string()))?;
        let summary: String = row.get(3).map_err(|e| InfraError::Database(e.to_string()))?;
        let content: String = row.get(4).map_err(|e| InfraError::Database(e.to_string()))?;
        let tags_json: String = row.get(5).map_err(|e| InfraError::Database(e.to_string()))?;
        let source_json: Option<String> =
            row.get(6).map_err(|e| InfraError::Database(e.to_string()))?;

        let item_type = match type_str.as_str() {
            "skill" => ItemType::Skill,
            "snippet" => ItemType::Snippet,
            _ => ItemType::Knowledge,
        };

        let tags: Vec<String> = serde_json::from_str(&tags_json)
            .map_err(|e| InfraError::Serialization(e.to_string()))?;
        let tags: Vec<Tag> = tags.iter().filter_map(|t| Tag::try_new(t)).collect();

        let source: Option<Source> = source_json
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| InfraError::Serialization(e.to_string()))?;

        let mut item = match item_type {
            ItemType::Knowledge => Item::new_knowledge(&title, &summary),
            ItemType::Skill => Item::new_skill(&title, &summary),
            ItemType::Snippet => Item::new_snippet(&title, &summary),
        };

        item = item.with_id(ItemId::from_str(&id)).with_tags(tags);

        if let Some(src) = source {
            item = item.with_source(src);
        }

        item.set_content(&content);

        Ok(item)
    }
}

#[async_trait]
impl ItemRepository for SqliteStore {
    async fn find_by_id(&self, id: &ItemId) -> InfraResult<Option<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, item_type, title, summary, content, tags, source FROM items WHERE id = ?1",
            )
            .map_err(|e| InfraError::Database(e.to_string()))?;

        let result = stmt
            .query_row([id.as_str()], |row| {
                self.row_to_item(row)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
            })
            .optional()
            .map_err(|e| InfraError::Database(e.to_string()))?;

        Ok(result)
    }

    async fn find_all(&self) -> InfraResult<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, item_type, title, summary, content, tags, source FROM items ORDER BY created_at DESC")
            .map_err(|e| InfraError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| Ok(self.row_to_item(row)))
            .map_err(|e| InfraError::Database(e.to_string()))?;

        rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string()))?)
            .collect()
    }

    async fn find_by_type(&self, item_type: ItemType) -> InfraResult<Vec<Item>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, item_type, title, summary, content, tags, source FROM items WHERE item_type = ?1 ORDER BY created_at DESC")
            .map_err(|e| InfraError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([item_type.as_str()], |row| Ok(self.row_to_item(row)))
            .map_err(|e| InfraError::Database(e.to_string()))?;

        rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string()))?)
            .collect()
    }

    async fn find_by_tags(&self, _tags: &[Tag]) -> InfraResult<Vec<Item>> {
        // TODO: 实现标签过滤
        self.find_all().await
    }

    async fn save(&self, item: &Item) -> InfraResult<()> {
        let conn = self.conn.lock().unwrap();
        let tags_json =
            serde_json::to_string(item.tags()).map_err(|e| InfraError::Serialization(e.to_string()))?;
        let source_json = item
            .source()
            .map(|s| serde_json::to_string(s))
            .transpose()
            .map_err(|e| InfraError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT OR REPLACE INTO items (id, item_type, title, summary, content, tags, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                item.id().as_str(),
                item.item_type().as_str(),
                item.title(),
                item.summary(),
                item.content(),
                tags_json,
                source_json,
                item.created_at().to_rfc3339(),
                item.updated_at().to_rfc3339(),
            ],
        ).map_err(|e| InfraError::Database(e.to_string()))?;

        Ok(())
    }

    async fn delete(&self, id: &ItemId) -> InfraResult<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM items WHERE id = ?1", [id.as_str()])
            .map_err(|e| InfraError::Database(e.to_string()))?;
        Ok(rows > 0)
    }

    async fn exists(&self, id: &ItemId) -> InfraResult<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM items WHERE id = ?1", [id.as_str()], |row| {
                row.get(0)
            })
            .map_err(|e| InfraError::Database(e.to_string()))?;
        Ok(count > 0)
    }

    async fn search_text(&self, query: &str, limit: usize) -> InfraResult<Vec<Item>> {
        let conn = self.conn.lock().unwrap();

        // 为 FTS5 添加通配符支持前缀匹配
        let fts_query = if query.contains('*') || query.contains('"') {
            query.to_string()
        } else {
            // 对每个词添加 * 后缀实现前缀匹配
            query
                .split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        };

        let mut stmt = conn
            .prepare(
                "SELECT i.id, i.item_type, i.title, i.summary, i.content, i.tags, i.source
                 FROM items i
                 JOIN items_fts fts ON i.rowid = fts.rowid
                 WHERE items_fts MATCH ?1
                 ORDER BY fts.rank
                 LIMIT ?2",
            )
            .map_err(|e| InfraError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![fts_query, limit as i64], |row| {
                Ok(self.row_to_item(row))
            })
            .map_err(|e| InfraError::Database(e.to_string()))?;

        rows.map(|r| r.map_err(|e| InfraError::Database(e.to_string()))?)
            .collect()
    }
}
