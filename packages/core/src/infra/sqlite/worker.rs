use super::rows::{configure_connection, configure_read_only_connection};
use super::{conversation_ops, doc_ops, ops};
use crate::conversation::{ConversationRecord, EventRecord, ExtractionJobRecord};
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Document, Item, ItemType};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use std::path::PathBuf;
use std::sync::mpsc;
use tokio::sync::oneshot;

const WORKER_STOPPED: &str = "sqlite worker stopped";
const WORKER_INIT_FAILED: &str = "sqlite worker init failed";

pub(super) enum OpenMode {
    InMemory,
    File(PathBuf),
    ReadOnlyFile(PathBuf),
}

pub(super) enum SqliteCommand {
    FindById {
        id: String,
        resp: oneshot::Sender<InfraResult<Option<Item>>>,
    },
    FindAll(oneshot::Sender<InfraResult<Vec<Item>>>),
    FindByType {
        item_type: ItemType,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    FindRecent {
        item_type: Option<ItemType>,
        offset: usize,
        limit: usize,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    CountItems {
        item_type: Option<ItemType>,
        resp: oneshot::Sender<InfraResult<usize>>,
    },
    FindByTags {
        tags: Vec<String>,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    Save {
        item: Item,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    Delete {
        id: String,
        resp: oneshot::Sender<InfraResult<bool>>,
    },
    Exists {
        id: String,
        resp: oneshot::Sender<InfraResult<bool>>,
    },
    SearchText {
        query: String,
        offset: usize,
        limit: usize,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    CountTextHits {
        query: String,
        resp: oneshot::Sender<InfraResult<usize>>,
    },
    FindByDocumentId {
        document_id: String,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    FindSince {
        since: DateTime<Utc>,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    FindByDateRange {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    FindObservationsByEventRange {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        resp: oneshot::Sender<InfraResult<Vec<Item>>>,
    },
    // Document 操作
    DocFindByUrl {
        url: String,
        resp: oneshot::Sender<InfraResult<Option<Document>>>,
    },
    DocSave {
        doc: Document,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    DocSaveWithReplacedItems {
        doc: Document,
        items: Vec<Item>,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    DocSaveWithReplacedItemsAndDeleteDocuments {
        doc: Document,
        items: Vec<Item>,
        obsolete_document_ids: Vec<String>,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    DocFindById {
        id: String,
        resp: oneshot::Sender<InfraResult<Option<Document>>>,
    },
    DocFindRecent {
        offset: usize,
        limit: usize,
        resp: oneshot::Sender<InfraResult<Vec<Document>>>,
    },
    DocCount {
        resp: oneshot::Sender<InfraResult<usize>>,
    },
    DocDelete {
        id: String,
        resp: oneshot::Sender<InfraResult<bool>>,
    },
    DocDeleteWithItems {
        ids: Vec<String>,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    DocSearchText {
        query: String,
        offset: usize,
        limit: usize,
        resp: oneshot::Sender<InfraResult<Vec<Document>>>,
    },
    DocCountTextHits {
        query: String,
        resp: oneshot::Sender<InfraResult<usize>>,
    },
    // Conversation 操作
    ConversationFindById {
        id: String,
        resp: oneshot::Sender<InfraResult<Option<ConversationRecord>>>,
    },
    ConversationList {
        status: Option<String>,
        offset: usize,
        limit: usize,
        resp: oneshot::Sender<InfraResult<Vec<ConversationRecord>>>,
    },
    ConversationCount {
        status: Option<String>,
        resp: oneshot::Sender<InfraResult<usize>>,
    },
    ConversationUpsert {
        record: ConversationRecord,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    ConversationInsertOrFetchByIdempotency {
        record: ConversationRecord,
        resp: oneshot::Sender<InfraResult<ConversationRecord>>,
    },
    // Extraction job 操作
    JobFindById {
        id: String,
        resp: oneshot::Sender<InfraResult<Option<ExtractionJobRecord>>>,
    },
    JobUpsert {
        job: ExtractionJobRecord,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    // Event 操作
    EventInsert {
        event: EventRecord,
        resp: oneshot::Sender<InfraResult<()>>,
    },
    EventCountsSince {
        since: Option<String>,
        resp: oneshot::Sender<InfraResult<Vec<(String, usize)>>>,
    },
}

#[derive(Clone)]
pub(super) struct WorkerHandle {
    tx: mpsc::Sender<SqliteCommand>,
}

impl WorkerHandle {
    pub(super) fn send(&self, command: SqliteCommand) -> InfraResult<()> {
        self.tx
            .send(command)
            .map_err(|_| InfraError::Database(WORKER_STOPPED.to_string()))
    }
}

pub(super) fn start_worker(mode: OpenMode) -> InfraResult<WorkerHandle> {
    let (tx, rx) = mpsc::channel();
    let (init_tx, init_rx) = mpsc::sync_channel(1);

    std::thread::spawn(move || run_worker(mode, rx, init_tx));

    match init_rx.recv() {
        Ok(Ok(())) => Ok(WorkerHandle { tx }),
        Ok(Err(err)) => Err(err),
        Err(_) => Err(InfraError::Database(WORKER_INIT_FAILED.to_string())),
    }
}

fn run_worker(
    mode: OpenMode,
    rx: mpsc::Receiver<SqliteCommand>,
    init_tx: mpsc::SyncSender<InfraResult<()>>,
) {
    let (conn, in_memory, read_only) = match mode {
        OpenMode::InMemory => match Connection::open_in_memory() {
            Ok(conn) => (conn, true, false),
            Err(err) => {
                send_init_result(
                    &init_tx,
                    Err(InfraError::Database(err.to_string())),
                    "open in-memory connection",
                );
                return;
            }
        },
        OpenMode::File(path) => match Connection::open(path) {
            Ok(conn) => (conn, false, false),
            Err(err) => {
                send_init_result(
                    &init_tx,
                    Err(InfraError::Database(err.to_string())),
                    "open file connection",
                );
                return;
            }
        },
        OpenMode::ReadOnlyFile(path) => match Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(conn) => (conn, false, true),
            Err(err) => {
                send_init_result(
                    &init_tx,
                    Err(InfraError::Database(err.to_string())),
                    "open read-only file connection",
                );
                return;
            }
        },
    };

    let configured = if read_only {
        configure_read_only_connection(&conn)
    } else {
        configure_connection(&conn, in_memory).and_then(|_| ops::init_schema(&conn))
    };
    if let Err(err) = configured {
        send_init_result(&init_tx, Err(err), "configure schema");
        return;
    }

    send_init_result(&init_tx, Ok(()), "ready");

    for command in rx {
        handle_command(&conn, command);
    }
}

fn send_init_result(
    init_tx: &mpsc::SyncSender<InfraResult<()>>,
    result: InfraResult<()>,
    context: &'static str,
) {
    if let Err(err) = &result {
        tracing::error!(context = context, error = %err, "sqlite worker init failed");
    }
    if init_tx.send(result).is_err() {
        tracing::warn!(context = context, "sqlite worker init receiver dropped");
    }
}

fn send_response<T>(
    command: &'static str,
    resp: oneshot::Sender<InfraResult<T>>,
    result: InfraResult<T>,
) {
    if resp.send(result).is_err() {
        tracing::warn!(command = command, "sqlite response receiver dropped");
    }
}

fn handle_command(conn: &Connection, command: SqliteCommand) {
    match command {
        SqliteCommand::FindById { id, resp } => {
            send_response("FindById", resp, ops::find_by_id(conn, &id));
        }
        SqliteCommand::FindAll(resp) => {
            send_response("FindAll", resp, ops::find_all(conn));
        }
        SqliteCommand::FindByType { item_type, resp } => {
            send_response("FindByType", resp, ops::find_by_type(conn, item_type));
        }
        SqliteCommand::FindRecent {
            item_type,
            offset,
            limit,
            resp,
        } => {
            send_response(
                "FindRecent",
                resp,
                ops::find_recent(conn, item_type, offset, limit),
            );
        }
        SqliteCommand::CountItems { item_type, resp } => {
            send_response("CountItems", resp, ops::count_items(conn, item_type));
        }
        SqliteCommand::FindByTags { tags, resp } => {
            send_response("FindByTags", resp, ops::find_by_tags(conn, &tags));
        }
        SqliteCommand::Save { item, resp } => {
            send_response("Save", resp, ops::save(conn, &item));
        }
        SqliteCommand::Delete { id, resp } => {
            send_response("Delete", resp, ops::delete(conn, &id));
        }
        SqliteCommand::Exists { id, resp } => {
            send_response("Exists", resp, ops::exists(conn, &id));
        }
        SqliteCommand::SearchText {
            query,
            offset,
            limit,
            resp,
        } => {
            send_response(
                "SearchText",
                resp,
                ops::search_text(conn, &query, offset, limit),
            );
        }
        SqliteCommand::CountTextHits { query, resp } => {
            send_response("CountTextHits", resp, ops::count_text_hits(conn, &query));
        }
        SqliteCommand::FindByDocumentId { document_id, resp } => {
            send_response(
                "FindByDocumentId",
                resp,
                ops::find_by_document_id(conn, &document_id),
            );
        }
        SqliteCommand::FindSince { since, resp } => {
            send_response("FindSince", resp, ops::find_since(conn, since));
        }
        SqliteCommand::FindByDateRange { start, end, resp } => {
            send_response(
                "FindByDateRange",
                resp,
                ops::find_by_date_range(conn, start, end),
            );
        }
        SqliteCommand::FindObservationsByEventRange { start, end, resp } => {
            send_response(
                "FindObservationsByEventRange",
                resp,
                ops::find_observations_by_event_range(conn, start, end),
            );
        }
        SqliteCommand::DocFindByUrl { url, resp } => {
            send_response("DocFindByUrl", resp, doc_ops::find_by_url(conn, &url));
        }
        SqliteCommand::DocSave { doc, resp } => {
            send_response("DocSave", resp, doc_ops::save(conn, &doc));
        }
        SqliteCommand::DocSaveWithReplacedItems { doc, items, resp } => {
            send_response(
                "DocSaveWithReplacedItems",
                resp,
                save_document_with_replaced_items(conn, &doc, &items),
            );
        }
        SqliteCommand::DocSaveWithReplacedItemsAndDeleteDocuments {
            doc,
            items,
            obsolete_document_ids,
            resp,
        } => {
            send_response(
                "DocSaveWithReplacedItemsAndDeleteDocuments",
                resp,
                save_document_with_replaced_items_and_delete_documents(
                    conn,
                    &doc,
                    &items,
                    &obsolete_document_ids,
                ),
            );
        }
        SqliteCommand::DocFindById { id, resp } => {
            send_response("DocFindById", resp, doc_ops::find_by_id(conn, &id));
        }
        SqliteCommand::DocFindRecent {
            offset,
            limit,
            resp,
        } => {
            send_response(
                "DocFindRecent",
                resp,
                doc_ops::find_recent(conn, offset, limit),
            );
        }
        SqliteCommand::DocCount { resp } => {
            send_response("DocCount", resp, doc_ops::count(conn));
        }
        SqliteCommand::DocDelete { id, resp } => {
            send_response("DocDelete", resp, doc_ops::delete(conn, &id));
        }
        SqliteCommand::DocDeleteWithItems { ids, resp } => {
            send_response(
                "DocDeleteWithItems",
                resp,
                delete_documents_with_items(conn, &ids),
            );
        }
        SqliteCommand::DocSearchText {
            query,
            offset,
            limit,
            resp,
        } => {
            send_response(
                "DocSearchText",
                resp,
                doc_ops::search_text(conn, &query, offset, limit),
            );
        }
        SqliteCommand::DocCountTextHits { query, resp } => {
            send_response(
                "DocCountTextHits",
                resp,
                doc_ops::count_text_hits(conn, &query),
            );
        }
        SqliteCommand::ConversationFindById { id, resp } => {
            send_response(
                "ConversationFindById",
                resp,
                conversation_ops::find_conversation_by_id(conn, &id),
            );
        }
        SqliteCommand::ConversationList {
            status,
            offset,
            limit,
            resp,
        } => {
            send_response(
                "ConversationList",
                resp,
                conversation_ops::list_conversations(conn, status.as_deref(), offset, limit),
            );
        }
        SqliteCommand::ConversationCount { status, resp } => {
            send_response(
                "ConversationCount",
                resp,
                conversation_ops::count_conversations(conn, status.as_deref()),
            );
        }
        SqliteCommand::ConversationUpsert { record, resp } => {
            send_response(
                "ConversationUpsert",
                resp,
                conversation_ops::upsert_conversation(conn, &record),
            );
        }
        SqliteCommand::ConversationInsertOrFetchByIdempotency { record, resp } => {
            send_response(
                "ConversationInsertOrFetchByIdempotency",
                resp,
                conversation_ops::insert_or_fetch_conversation_by_idempotency(conn, &record),
            );
        }
        SqliteCommand::JobFindById { id, resp } => {
            send_response(
                "JobFindById",
                resp,
                conversation_ops::find_job_by_id(conn, &id),
            );
        }
        SqliteCommand::JobUpsert { job, resp } => {
            send_response("JobUpsert", resp, conversation_ops::upsert_job(conn, &job));
        }
        SqliteCommand::EventInsert { event, resp } => {
            send_response(
                "EventInsert",
                resp,
                conversation_ops::insert_event(conn, &event),
            );
        }
        SqliteCommand::EventCountsSince { since, resp } => {
            send_response(
                "EventCountsSince",
                resp,
                conversation_ops::event_counts_since(conn, since.as_deref()),
            );
        }
    }
}

fn save_document_with_replaced_items(
    conn: &Connection,
    doc: &Document,
    items: &[Item],
) -> InfraResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    doc_ops::save(&tx, doc)?;
    ops::delete_by_document_id(&tx, doc.id().as_str())?;
    for item in items {
        ops::save(&tx, item)?;
    }
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn save_document_with_replaced_items_and_delete_documents(
    conn: &Connection,
    doc: &Document,
    items: &[Item],
    obsolete_document_ids: &[String],
) -> InfraResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    doc_ops::save(&tx, doc)?;
    ops::delete_by_document_id(&tx, doc.id().as_str())?;
    for item in items {
        ops::save(&tx, item)?;
    }
    delete_documents_with_items_in_transaction(&tx, obsolete_document_ids)?;
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn delete_documents_with_items(conn: &Connection, document_ids: &[String]) -> InfraResult<()> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    delete_documents_with_items_in_transaction(&tx, document_ids)?;
    tx.commit()
        .map_err(|e| InfraError::Database(e.to_string()))?;
    Ok(())
}

fn delete_documents_with_items_in_transaction(
    conn: &Connection,
    document_ids: &[String],
) -> InfraResult<()> {
    for document_id in document_ids {
        ops::delete_by_document_id(conn, document_id)?;
        if !doc_ops::delete(conn, document_id)? {
            return Err(InfraError::Database(format!(
                "obsolete document {document_id} does not exist"
            )));
        }
    }
    Ok(())
}
