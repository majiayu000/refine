//! SQLite 存储实现
//!
//! ItemRepository 的 SQLite 实现（worker 线程模型）

use crate::conversation::{
    ConversationRecord, ConversationRepository, EventRecord, EventRepository, ExtractionJobRecord,
    JobRepository,
};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{
    Document, DocumentId, DocumentRepository, Item, ItemId, ItemRepository, ItemType,
    ObservationWindowSnapshot, Tag,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use tokio::sync::oneshot;

mod conversation_ops;
mod doc_ops;
mod insights_snapshot;
mod ops;
mod rows;
mod worker;
mod worker_support;

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

    /// Open an existing database without running migrations or allowing writes.
    /// Used by preview commands whose read-only promise must include startup.
    pub fn open_read_only(path: impl AsRef<Path>) -> InfraResult<Self> {
        let mode = OpenMode::ReadOnlyFile(PathBuf::from(path.as_ref()));
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
    async fn find_by_id(&self, id: &ItemId) -> InfraResult<Option<Item>> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::FindById { id, resp })
            .await
    }

    async fn find_all(&self) -> InfraResult<Vec<Item>> {
        self.request(SqliteCommand::FindAll).await
    }

    async fn find_by_type(&self, item_type: ItemType) -> InfraResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindByType { item_type, resp })
            .await
    }

    async fn find_recent(
        &self,
        item_type: Option<ItemType>,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindRecent {
            item_type,
            offset,
            limit,
            resp,
        })
        .await
    }

    async fn count_items(&self, item_type: Option<ItemType>) -> InfraResult<usize> {
        self.request(|resp| SqliteCommand::CountItems { item_type, resp })
            .await
    }

    async fn find_by_tags(&self, tags: &[Tag]) -> InfraResult<Vec<Item>> {
        let tags = tags.iter().map(|tag| tag.as_str().to_string()).collect();
        self.request(|resp| SqliteCommand::FindByTags { tags, resp })
            .await
    }

    async fn save(&self, item: &Item) -> InfraResult<()> {
        let item = item.clone();
        self.request(|resp| SqliteCommand::Save { item, resp })
            .await
    }

    async fn delete(&self, id: &ItemId) -> InfraResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::Delete { id, resp })
            .await
    }

    async fn exists(&self, id: &ItemId) -> InfraResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::Exists { id, resp })
            .await
    }

    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Item>> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::SearchText {
            query,
            offset,
            limit,
            resp,
        })
        .await
    }

    async fn count_text_hits(&self, query: &str) -> InfraResult<usize> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::CountTextHits { query, resp })
            .await
    }

    async fn find_since(&self, since: DateTime<Utc>) -> InfraResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindSince { since, resp })
            .await
    }

    async fn find_by_date_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> InfraResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindByDateRange { start, end, resp })
            .await
    }

    async fn find_observations_by_event_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> InfraResult<Vec<Item>> {
        self.request(|resp| SqliteCommand::FindObservationsByEventRange { start, end, resp })
            .await
    }

    async fn load_observation_window_snapshot(
        &self,
        cutoff: DateTime<Utc>,
        period_days: Option<usize>,
    ) -> InfraResult<ObservationWindowSnapshot> {
        self.request(|resp| SqliteCommand::LoadObservationWindowSnapshot {
            cutoff,
            period_days,
            resp,
        })
        .await
    }

    async fn find_by_document_id(&self, doc_id: &DocumentId) -> InfraResult<Vec<Item>> {
        let id = doc_id.as_str().to_string();
        self.request(|resp| SqliteCommand::FindByDocumentId {
            document_id: id,
            resp,
        })
        .await
    }
}

#[async_trait]
impl DocumentRepository for SqliteStore {
    async fn find_by_id(&self, id: &DocumentId) -> InfraResult<Option<Document>> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::DocFindById { id, resp })
            .await
    }

    async fn find_by_url(&self, url: &str) -> InfraResult<Option<Document>> {
        let url = url.to_string();
        self.request(|resp| SqliteCommand::DocFindByUrl { url, resp })
            .await
    }

    async fn find_recent(&self, offset: usize, limit: usize) -> InfraResult<Vec<Document>> {
        self.request(|resp| SqliteCommand::DocFindRecent {
            offset,
            limit,
            resp,
        })
        .await
    }

    async fn find_items_by_document_id(&self, id: &DocumentId) -> InfraResult<Vec<Item>> {
        let document_id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::FindByDocumentId { document_id, resp })
            .await
    }

    async fn count(&self) -> InfraResult<usize> {
        self.request(|resp| SqliteCommand::DocCount { resp }).await
    }

    async fn save(&self, doc: &Document) -> InfraResult<()> {
        let doc = doc.clone();
        self.request(|resp| SqliteCommand::DocSave { doc, resp })
            .await
    }

    async fn save_with_replaced_items(&self, doc: &Document, items: &[Item]) -> InfraResult<()> {
        let doc = doc.clone();
        let items = items.to_vec();
        self.request(|resp| SqliteCommand::DocSaveWithReplacedItems { doc, items, resp })
            .await
    }

    async fn save_with_replaced_items_and_delete_documents(
        &self,
        doc: &Document,
        items: &[Item],
        obsolete_document_ids: &[DocumentId],
    ) -> InfraResult<()> {
        let doc = doc.clone();
        let items = items.to_vec();
        let obsolete_document_ids = obsolete_document_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect();
        self.request(
            |resp| SqliteCommand::DocSaveWithReplacedItemsAndDeleteDocuments {
                doc,
                items,
                obsolete_document_ids,
                resp,
            },
        )
        .await
    }

    async fn delete_documents_with_items(&self, document_ids: &[DocumentId]) -> InfraResult<()> {
        let ids = document_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect();
        self.request(|resp| SqliteCommand::DocDeleteWithItems { ids, resp })
            .await
    }

    async fn delete(&self, id: &DocumentId) -> InfraResult<bool> {
        let id = id.as_str().to_string();
        self.request(|resp| SqliteCommand::DocDelete { id, resp })
            .await
    }

    async fn search_text(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<Document>> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::DocSearchText {
            query,
            offset,
            limit,
            resp,
        })
        .await
    }

    async fn count_text_hits(&self, query: &str) -> InfraResult<usize> {
        let query = query.to_string();
        self.request(|resp| SqliteCommand::DocCountTextHits { query, resp })
            .await
    }
}

#[async_trait]
impl ConversationRepository for SqliteStore {
    async fn find_conversation_by_id(&self, id: &str) -> InfraResult<Option<ConversationRecord>> {
        let id = id.to_string();
        self.request(|resp| SqliteCommand::ConversationFindById { id, resp })
            .await
    }

    async fn list_conversations(
        &self,
        status: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> InfraResult<Vec<ConversationRecord>> {
        let status = status.map(|s| s.to_string());
        self.request(|resp| SqliteCommand::ConversationList {
            status,
            offset,
            limit,
            resp,
        })
        .await
    }

    async fn count_conversations(&self, status: Option<&str>) -> InfraResult<usize> {
        let status = status.map(|s| s.to_string());
        self.request(|resp| SqliteCommand::ConversationCount { status, resp })
            .await
    }

    async fn upsert_conversation(&self, record: &ConversationRecord) -> InfraResult<()> {
        let record = record.clone();
        self.request(|resp| SqliteCommand::ConversationUpsert { record, resp })
            .await
    }

    async fn insert_or_fetch_conversation_by_idempotency(
        &self,
        record: &ConversationRecord,
    ) -> InfraResult<ConversationRecord> {
        let record = record.clone();
        self.request(|resp| SqliteCommand::ConversationInsertOrFetchByIdempotency { record, resp })
            .await
    }

    async fn insert_or_fetch_conversation_with_job(
        &self,
        record: &ConversationRecord,
        job: &ExtractionJobRecord,
    ) -> InfraResult<(ConversationRecord, Option<ExtractionJobRecord>)> {
        let record = record.clone();
        let job = job.clone();
        self.request(|resp| SqliteCommand::ConversationInsertOrFetchWithJob { record, job, resp })
            .await
    }
}

#[async_trait]
impl JobRepository for SqliteStore {
    async fn find_job_by_id(&self, id: &str) -> InfraResult<Option<ExtractionJobRecord>> {
        let id = id.to_string();
        self.request(|resp| SqliteCommand::JobFindById { id, resp })
            .await
    }

    async fn upsert_job(&self, job: &ExtractionJobRecord) -> InfraResult<()> {
        let job = job.clone();
        self.request(|resp| SqliteCommand::JobUpsert { job, resp })
            .await
    }

    async fn enqueue_job(&self, job: &ExtractionJobRecord) -> InfraResult<ExtractionJobRecord> {
        let job = job.clone();
        self.request(|resp| SqliteCommand::JobEnqueue { job, resp })
            .await
    }

    async fn list_recoverable_jobs(
        &self,
        now: &str,
        limit: usize,
    ) -> InfraResult<Vec<ExtractionJobRecord>> {
        let now = now.to_string();
        self.request(|resp| SqliteCommand::JobListRecoverable { now, limit, resp })
            .await
    }

    async fn reconcile_processed_jobs(&self, now: &str) -> InfraResult<usize> {
        let now = now.to_string();
        self.request(|resp| SqliteCommand::JobReconcileProcessed { now, resp })
            .await
    }

    async fn claim_job(
        &self,
        id: &str,
        owner: &str,
        now: &str,
        lease_expires_at: &str,
    ) -> InfraResult<Option<ExtractionJobRecord>> {
        let id = id.to_string();
        let owner = owner.to_string();
        let now = now.to_string();
        let lease_expires_at = lease_expires_at.to_string();
        self.request(|resp| SqliteCommand::JobClaim {
            id,
            owner,
            now,
            lease_expires_at,
            resp,
        })
        .await
    }

    async fn renew_job_lease(
        &self,
        id: &str,
        owner: &str,
        now: &str,
        lease_expires_at: &str,
    ) -> InfraResult<bool> {
        let id = id.to_string();
        let owner = owner.to_string();
        let now = now.to_string();
        let lease_expires_at = lease_expires_at.to_string();
        self.request(|resp| SqliteCommand::JobRenewLease {
            id,
            owner,
            now,
            lease_expires_at,
            resp,
        })
        .await
    }

    async fn finish_job_claim(
        &self,
        id: &str,
        owner: &str,
        status: crate::conversation::JobStatus,
        item_ids: &[String],
        error: Option<&str>,
        now: &str,
    ) -> InfraResult<bool> {
        let id = id.to_string();
        let owner = owner.to_string();
        let item_ids = item_ids.to_vec();
        let error = error.map(ToString::to_string);
        let now = now.to_string();
        self.request(|resp| SqliteCommand::JobFinishClaim {
            id,
            owner,
            status,
            item_ids,
            error,
            now,
            resp,
        })
        .await
    }

    async fn finish_job_claim_with_results(
        &self,
        id: &str,
        owner: &str,
        document: &Document,
        items: &[Item],
        now: &str,
    ) -> InfraResult<bool> {
        let id = id.to_string();
        let owner = owner.to_string();
        let document = document.clone();
        let items = items.to_vec();
        let now = now.to_string();
        self.request(|resp| SqliteCommand::JobFinishClaimWithResults {
            id,
            owner,
            document,
            items,
            now,
            resp,
        })
        .await
    }
}

#[async_trait]
impl EventRepository for SqliteStore {
    async fn insert_event(&self, event: &EventRecord) -> InfraResult<()> {
        let event = event.clone();
        self.request(|resp| SqliteCommand::EventInsert { event, resp })
            .await
    }

    async fn event_counts_since(&self, since: Option<&str>) -> InfraResult<Vec<(String, usize)>> {
        let since = since.map(|s| s.to_string());
        self.request(|resp| SqliteCommand::EventCountsSince { since, resp })
            .await
    }
}
