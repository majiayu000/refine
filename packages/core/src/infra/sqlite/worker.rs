use super::ops;
use super::rows::configure_connection;
use crate::error::{InfraError, InfraResult};
use crate::knowledge::{Item, ItemType};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::mpsc;
use tokio::sync::oneshot;

const WORKER_STOPPED: &str = "sqlite worker stopped";
const WORKER_INIT_FAILED: &str = "sqlite worker init failed";

pub(super) enum OpenMode {
    InMemory,
    File(PathBuf),
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
    let (conn, in_memory) = match mode {
        OpenMode::InMemory => match Connection::open_in_memory() {
            Ok(conn) => (conn, true),
            Err(err) => {
                let _ = init_tx.send(Err(InfraError::Database(err.to_string())));
                return;
            }
        },
        OpenMode::File(path) => match Connection::open(path) {
            Ok(conn) => (conn, false),
            Err(err) => {
                let _ = init_tx.send(Err(InfraError::Database(err.to_string())));
                return;
            }
        },
    };

    if let Err(err) = configure_connection(&conn, in_memory).and_then(|_| ops::init_schema(&conn)) {
        let _ = init_tx.send(Err(err));
        return;
    }

    let _ = init_tx.send(Ok(()));

    for command in rx {
        handle_command(&conn, command);
    }
}

fn handle_command(conn: &Connection, command: SqliteCommand) {
    match command {
        SqliteCommand::FindById { id, resp } => {
            let _ = resp.send(ops::find_by_id(conn, &id));
        }
        SqliteCommand::FindAll(resp) => {
            let _ = resp.send(ops::find_all(conn));
        }
        SqliteCommand::FindByType { item_type, resp } => {
            let _ = resp.send(ops::find_by_type(conn, item_type));
        }
        SqliteCommand::FindRecent {
            item_type,
            offset,
            limit,
            resp,
        } => {
            let _ = resp.send(ops::find_recent(conn, item_type, offset, limit));
        }
        SqliteCommand::CountItems { item_type, resp } => {
            let _ = resp.send(ops::count_items(conn, item_type));
        }
        SqliteCommand::FindByTags { tags, resp } => {
            let _ = resp.send(ops::find_by_tags(conn, &tags));
        }
        SqliteCommand::Save { item, resp } => {
            let _ = resp.send(ops::save(conn, &item));
        }
        SqliteCommand::Delete { id, resp } => {
            let _ = resp.send(ops::delete(conn, &id));
        }
        SqliteCommand::Exists { id, resp } => {
            let _ = resp.send(ops::exists(conn, &id));
        }
        SqliteCommand::SearchText {
            query,
            offset,
            limit,
            resp,
        } => {
            let _ = resp.send(ops::search_text(conn, &query, offset, limit));
        }
        SqliteCommand::CountTextHits { query, resp } => {
            let _ = resp.send(ops::count_text_hits(conn, &query));
        }
    }
}
