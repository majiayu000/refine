use anyhow::{Context, Result};
use chrono::Utc;
use fs2::FileExt;
use refine_core::session::SessionSource;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) const INGEST_CURSOR_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CursorPurpose {
    Ingest,
    Metadata,
}

impl CursorPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ingest => "ingest",
            Self::Metadata => "metadata",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct IngestCursorFailure {
    pub(super) path_sha256: String,
    pub(super) modified_at_secs: u64,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct IngestCursorState {
    pub(super) version: u8,
    pub(super) watermark_secs: u64,
    pub(super) failures: Vec<IngestCursorFailure>,
}

pub(super) fn incremental_cursor_path(
    home: &Path,
    source: Option<&SessionSource>,
    db_path: &Path,
    purpose: CursorPurpose,
) -> PathBuf {
    let source_key = match source {
        Some(SessionSource::ClaudeCode) => "claude-code",
        Some(SessionSource::Codex) => "codex",
        Some(SessionSource::RememRaw) => "remem-raw",
        None => "all",
    };
    let db_key = encode_path_for_filename(db_path);
    home.join(".refine").join("ingest-cursors").join(format!(
        "last-{}-mtime-{source_key}-{db_key}",
        purpose.as_str()
    ))
}

fn encode_path_for_filename(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .to_string_lossy()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn read_last_ingest_mtime(
    source: Option<&SessionSource>,
    db_path: &Path,
    purpose: CursorPurpose,
) -> Option<SystemTime> {
    let home = dirs::home_dir()?;
    let path = incremental_cursor_path(&home, source, db_path, purpose);
    let secs = parse_ingest_cursor(&std::fs::read_to_string(path).ok()?)?;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
}

pub(super) fn parse_ingest_cursor(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    trimmed.parse::<u64>().ok().or_else(|| {
        serde_json::from_str::<IngestCursorState>(trimmed)
            .ok()
            .filter(|state| state.version == INGEST_CURSOR_VERSION)
            .map(|state| state.watermark_secs)
    })
}

pub(super) fn write_last_ingest_mtime(
    source: Option<&SessionSource>,
    db_path: &Path,
    purpose: CursorPurpose,
    t: SystemTime,
    failures: Vec<IngestCursorFailure>,
) -> Result<()> {
    let home = dirs::home_dir().context("home directory is unavailable for ingest cursor")?;
    let path = incremental_cursor_path(&home, source, db_path, purpose);
    write_ingest_cursor_at(
        &path,
        &IngestCursorState {
            version: INGEST_CURSOR_VERSION,
            watermark_secs: unix_seconds(t),
            failures,
        },
    )
}

pub(super) fn safe_cursor_watermark(
    scan_start: SystemTime,
    failed_mtimes: &[SystemTime],
) -> SystemTime {
    failed_mtimes
        .iter()
        .copied()
        .min()
        .map(|failed| {
            failed
                .checked_sub(Duration::from_secs(1))
                .unwrap_or(UNIX_EPOCH)
        })
        .map_or(scan_start, |safe| safe.min(scan_start))
}

pub(super) fn write_ingest_cursor_at(path: &Path, state: &IngestCursorState) -> Result<()> {
    let dir = path
        .parent()
        .context("ingest cursor has no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let nonce = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let temp_path = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let payload = serde_json::to_vec(state).context("failed to serialize ingest cursor")?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut temp = options
        .open(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    temp.write_all(&payload)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    temp.sync_all()
        .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    drop(temp);
    std::fs::rename(&temp_path, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    std::fs::File::open(dir)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("failed to sync {}", dir.display()))?;
    Ok(())
}

pub(super) fn lock_incremental_cursor(
    source: Option<&SessionSource>,
    db_path: &Path,
    purpose: CursorPurpose,
) -> Result<std::fs::File> {
    let home = dirs::home_dir().context("home directory is unavailable for ingest cursor")?;
    let cursor_path = incremental_cursor_path(&home, source, db_path, purpose);
    let lock_path = cursor_path.with_extension("lock");
    lock_file(&lock_path)
}

/// Serialize every session-document mutation for one database. Metadata
/// reconciliation and normal ingestion use different cursor locks, but must
/// not concurrently replace the same document's observations.
pub(super) fn lock_session_mutations(db_path: &Path) -> Result<std::fs::File> {
    let home =
        dirs::home_dir().context("home directory is unavailable for session mutation lock")?;
    let lock_path = home.join(".refine").join("ingest-cursors").join(format!(
        "session-mutations-{}.lock",
        encode_path_for_filename(db_path)
    ));
    lock_file(&lock_path)
}

fn lock_file(lock_path: &Path) -> Result<std::fs::File> {
    let parent = lock_path
        .parent()
        .context("session ingest lock has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    options.mode(0o600);
    let lock = options
        .open(lock_path)
        .with_context(|| format!("failed to open {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("failed to lock {}", lock_path.display()))?;
    Ok(lock)
}

pub(super) fn unix_seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub(super) fn cursor_failure(
    path: &Path,
    modified_at: SystemTime,
    reason: &str,
) -> IngestCursorFailure {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    IngestCursorFailure {
        path_sha256: format!("{digest:x}"),
        modified_at_secs: unix_seconds(modified_at),
        reason: reason.to_string(),
    }
}
