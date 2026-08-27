use super::{db_error, verify_sqlite};
use crate::error::{InfraError, InfraResult};
use rusqlite::{backup::Backup, backup::StepResult, Connection, OpenFlags};
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BACKUP_STALL_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn create_no_clobber(source_path: &Path, destination: &Path) -> InfraResult<()> {
    let parent = destination.parent().ok_or_else(|| {
        InfraError::Database(format!(
            "backup path has no parent: {}",
            destination.display()
        ))
    })?;
    if !parent.is_dir() {
        return Err(InfraError::Database(format!(
            "backup parent does not exist: {}",
            parent.display()
        )));
    }
    let temporary = reserve_temporary(parent, destination)?;
    let result = (|| {
        // The caller holds BEGIN IMMEDIATE on the source DB. A second read
        // connection therefore sees the exact committed snapshot used by the
        // repair plan while every competing writer remains blocked.
        let source = Connection::open_with_flags(
            source_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        source
            .busy_timeout(BACKUP_STALL_TIMEOUT)
            .map_err(db_error)?;
        let mut backup_conn = Connection::open_with_flags(
            &temporary,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        {
            let backup = Backup::new(&source, &mut backup_conn).map_err(db_error)?;
            run_with_timeout(&backup, BACKUP_STALL_TIMEOUT)?;
        }
        verify_sqlite(&backup_conn)?;
        drop(backup_conn);
        File::open(&temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| {
                InfraError::Database(format!(
                    "failed to fsync backup temporary {}: {error}",
                    temporary.display()
                ))
            })?;

        std::fs::hard_link(&temporary, destination).map_err(|error| {
            InfraError::Database(format!(
                "failed to publish backup without clobbering {}: {error}",
                destination.display()
            ))
        })?;
        sync_directory(parent)?;
        std::fs::remove_file(&temporary).map_err(|error| {
            InfraError::Database(format!(
                "backup published at {}, but temporary cleanup failed for {}: {error}",
                destination.display(),
                temporary.display()
            ))
        })?;
        sync_directory(parent)
    })();
    if result.is_err() {
        // The unique temporary is ours. Never remove destination: a competing
        // creator may have won the atomic hard-link publication race.
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn reserve_temporary(parent: &Path, destination: &Path) -> InfraResult<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("refine-repair-backup");
    for _ in 0..16 {
        let path = parent.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&path) {
            Ok(file) => {
                file.sync_all().map_err(|error| {
                    InfraError::Database(format!(
                        "failed to sync reserved backup temporary {}: {error}",
                        path.display()
                    ))
                })?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(InfraError::Database(format!(
                    "failed to reserve backup temporary in {}: {error}",
                    parent.display()
                )))
            }
        }
    }
    Err(InfraError::Database(format!(
        "failed to reserve a unique backup temporary in {}",
        parent.display()
    )))
}

fn run_with_timeout(backup: &Backup<'_, '_>, timeout: Duration) -> InfraResult<()> {
    let mut last_progress = Instant::now();
    loop {
        match backup.step(256).map_err(db_error)? {
            StepResult::Done => return Ok(()),
            StepResult::More => last_progress = Instant::now(),
            StepResult::Busy | StepResult::Locked => {
                if last_progress.elapsed() >= timeout {
                    return Err(InfraError::Database(format!(
                        "SQLite backup made no progress for {}ms",
                        timeout.as_millis()
                    )));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            _ => {}
        }
    }
}

fn sync_directory(path: &Path) -> InfraResult<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            InfraError::Database(format!(
                "failed to fsync backup directory {}: {error}",
                path.display()
            ))
        })
}
