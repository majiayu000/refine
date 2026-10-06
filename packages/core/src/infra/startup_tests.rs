use super::{configure_sqlite_connection, prepare_sqlite_db};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

pub(super) fn assert_startup_waits_for_writer(
    setup: fn(&Connection),
    prepare: fn(&Connection) -> Result<(), String>,
) {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("synthetic-startup.db");
    let blocker = Connection::open(&path).unwrap();
    let contender = Connection::open(&path).unwrap();
    configure_sqlite_connection(&blocker).unwrap();
    configure_sqlite_connection(&contender).unwrap();
    setup(&blocker);

    let tx = Transaction::new_unchecked(&blocker, TransactionBehavior::Immediate).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        result_tx.send(prepare(&contender)).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let early = result_rx.recv_timeout(Duration::from_millis(100));
    let waited = matches!(&early, Err(RecvTimeoutError::Timeout));
    tx.commit().unwrap();
    let result = match early {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(error) => panic!("startup worker disconnected: {error}"),
    };
    worker.join().unwrap();
    assert!(
        waited,
        "startup should wait for the writer rather than fail its read/write upgrade: {result:?}"
    );
    result.expect("startup should succeed after the writer releases its lock");
    let integrity: String = blocker
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn current_schema_startup_waits_before_reading_schema() {
    assert_startup_waits_for_writer(
        |conn| prepare_sqlite_db(conn).unwrap(),
        |conn| prepare_sqlite_db(conn).map_err(|error| error.to_string()),
    );
}

struct StartupInterleave {
    reached: mpsc::Sender<()>,
    other_finished: mpsc::Receiver<()>,
    paused: bool,
    statement_prefix: &'static str,
}

unsafe extern "C" fn pause_after_first_schema_read(
    _: u32,
    context: *mut std::ffi::c_void,
    statement: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
) -> i32 {
    // SQLite invokes this callback synchronously on the connection's thread.
    // The boxed context stays alive until tracing is disabled below.
    let context = unsafe { &mut *context.cast::<StartupInterleave>() };
    if context.paused {
        return 0;
    }
    // The sqlite3_sql pointer remains valid for this callback's statement.
    let sql = unsafe { rusqlite::ffi::sqlite3_sql(statement.cast()) };
    if sql.is_null() {
        return 0;
    }
    let sql = unsafe { std::ffi::CStr::from_ptr(sql) }.to_string_lossy();
    if sql.trim_start().starts_with(context.statement_prefix) {
        context.paused = true;
        let _ = context.reached.send(());
        // DEFERRED allows the other startup to commit here, making this
        // snapshot stale. IMMEDIATE blocks it until this transaction commits.
        let _ = context.other_finished.recv_timeout(Duration::from_secs(1));
    }
    0
}

pub(super) fn assert_overlapping_initializers(
    setup: fn(&Connection),
    prepare: fn(&Connection) -> Result<(), String>,
    statement_prefix: &'static str,
    verify: fn(&Connection),
) {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("synthetic-overlap.db");
    let first = Connection::open(&path).unwrap();
    let second = Connection::open(&path).unwrap();
    configure_sqlite_connection(&first).unwrap();
    configure_sqlite_connection(&second).unwrap();
    setup(&first);
    let (reached_tx, reached_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut context = Box::new(StartupInterleave {
            reached: reached_tx,
            other_finished: finished_rx,
            paused: false,
            statement_prefix,
        });
        // The connection and context are exclusively owned by this thread.
        // No pointer escapes the registered trace callback's lifetime.
        let enabled = unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                first.handle(),
                rusqlite::ffi::SQLITE_TRACE_STMT as u32,
                Some(pause_after_first_schema_read),
                (&mut *context as *mut StartupInterleave).cast(),
            )
        };
        assert_eq!(enabled, rusqlite::ffi::SQLITE_OK);
        let result = prepare(&first);
        // Disable callbacks before dropping their boxed context.
        unsafe {
            rusqlite::ffi::sqlite3_trace_v2(first.handle(), 0, None, std::ptr::null_mut());
        }
        (result, context.paused)
    });
    reached_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_result = prepare(&second);
    let _ = finished_tx.send(());
    let (first_result, paused) = worker.join().unwrap();
    assert!(
        paused,
        "the intended schema-read interleaving must be reached"
    );
    second_result.expect("second startup should complete");
    first_result.expect("first startup must not fail on a stale read/check decision");
    verify(&second);
    let integrity: String = second
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}

#[test]
fn overlapping_schema_reads_do_not_produce_busy_snapshot() {
    assert_overlapping_initializers(
        |conn| prepare_sqlite_db(conn).unwrap(),
        |conn| prepare_sqlite_db(conn).map_err(|error| error.to_string()),
        "CREATE INDEX IF NOT EXISTS idx_items_type",
        |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name IN ('observations_require_document_insert', 'observations_require_document_update')",
                [], |row| row.get(0)
            ).unwrap();
            assert_eq!(count, 2);
        },
    );
}
