//! SQLite 存储实现
//!
//! ItemRepository 的 SQLite 实现（worker 线程模型）

use crate::error::RepoResult;
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{
    Document, DocumentId, DocumentRepository, Item, ItemId, ItemRepository, ItemType, Tag,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

mod doc_ops;
mod ops;
mod rows;
mod worker;

use worker::{start_worker, OpenMode, SqliteCommand, WorkerHandle};

pub(crate) use rows::configure_connection;

const WORKER_CLOSED: &str = "sqlite worker closed";

/// SQLite 存储
pub struct SqliteStore {
    worker: WorkerHandle,
}

impl SqliteStore {
    /// 打开或创建数据库
    pub fn open(path: impl AsRef<Path>) -> InfraResult<Self> {
        let mode = OpenMode::File(PathBuf::from(path.as_ref()));
        let worker = start_worker(mode)?;
        Ok(Self { worker })
    }

    /// 内存数据库（测试用）
    pub fn in_memory() -> InfraResult<Self> {
        let worker = start_worker(OpenMode::InMemory)?;
        Ok(Self { worker })
    }

    async fn request<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<InfraResult<T>>) -> SqliteCommand,
    ) -> InfraResult<T> {
        let (tx, rx) = oneshot::channel();
        self.worker.send(build(tx))?;
        rx.await
            .map_err(|_| InfraError::Database(WORKER_CLOSED.to_string()))?
    }
}

#[async_trait]
impl ItemRepository for SqliteStore {
    async fn find_by_id(&self, id: &ItemId) -> RepoResult<Option<Item>> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::FindById { id, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_all(&self) -> RepoResult<Vec<Item>> {
        self.request(SqliteCommand::FindAll)
            .await
            .map_err(Into::into)
    }

    async fn find_by_type(&self, item_type: ItemType) -> RepoResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindByType { item_type, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_recent(
        &self,
        item_type: Option<ItemType>,
        offset: usize,
        limit: usize,
    ) -> RepoResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindRecent {
            item_type,
            offset,
            limit,
            resp,
        })
        .await
        .map_err(Into::into)
    }

    async fn count_items(&self, item_type: Option<ItemType>) -> RepoResult<usize> {
        self.request(|resp| SqliteCommand::CountItems { item_type, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_by_tags(&self, tags: &[Tag]) -> RepoResult<Vec<Item>> {
        let tags = tags.iter().map(|tag| tag.as_str().to_string()).collect();
        self.request(|resp| SqliteCommand::FindByTags { tags, resp })
            .await
            .map_err(Into::into)
    }

    async fn save(&self, item: &Item) -> RepoResult<()> {
        let item = item.clone();
        self.request(|resp| SqliteCommand::Save { item, resp })
            .await
            .map_err(Into::into)
    }

    async fn delete(&self, id: &ItemId) -> RepoResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::Delete { id, resp })
            .await
            .map_err(Into::into)
    }

    async fn exists(&self, id: &ItemId) -> RepoResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::Exists { id, resp })
            .await
            .map_err(Into::into)
    }

    async fn search_text(&self, query: &str, offset: usize, limit: usize) -> RepoResult<Vec<Item>> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::SearchText {
            query,
            offset,
            limit,
            resp,
        })
        .await
        .map_err(Into::into)
    }

    async fn count_text_hits(&self, query: &str) -> RepoResult<usize> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::CountTextHits { query, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_since(&self, since: DateTime<Utc>) -> RepoResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindSince { since, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_by_date_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> RepoResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindByDateRange { start, end, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_observations_by_event_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> RepoResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindObservationsByEventRange { start, end, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_by_document_id(&self, doc_id: &DocumentId) -> RepoResult<Vec<Item>> {
        let id = doc_id.as_str().to_string();
        self.request(|resp| SqliteCommand::FindByDocumentId {
            document_id: id,
            resp,
        })
        .await
        .map_err(Into::into)
    }
}

#[async_trait]
impl DocumentRepository for SqliteStore {
    async fn find_by_id(&self, id: &DocumentId) -> RepoResult<Option<Document>> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::DocFindById { id, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_by_url(&self, url: &str) -> RepoResult<Option<Document>> {
        let url = url.to_string();
        self.request(|resp| SqliteCommand::DocFindByUrl { url, resp })
            .await
            .map_err(Into::into)
    }

    async fn find_recent(&self, offset: usize, limit: usize) -> RepoResult<Vec<Document>> {
        self.request(|resp| SqliteCommand::DocFindRecent {
            offset,
            limit,
            resp,
        })
        .await
        .map_err(Into::into)
    }

    async fn count(&self) -> RepoResult<usize> {
        self.request(|resp| SqliteCommand::DocCount { resp })
            .await
            .map_err(Into::into)
    }

    async fn save(&self, doc: &Document) -> RepoResult<()> {
        let doc = doc.clone();
        self.request(|resp| SqliteCommand::DocSave { doc, resp })
            .await
            .map_err(Into::into)
    }

    async fn delete(&self, id: &DocumentId) -> RepoResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::DocDelete { id, resp })
            .await
            .map_err(Into::into)
    }

    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> RepoResult<Vec<Document>> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::DocSearchText {
            query,
            offset,
            limit,
            resp,
        })
        .await
        .map_err(Into::into)
    }

    async fn count_text_hits(&self, query: &str) -> RepoResult<usize> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::DocCountTextHits { query, resp })
            .await
            .map_err(Into::into)
    }
}
