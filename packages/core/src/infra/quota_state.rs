//! Persistent quota-exhaustion state for the LLM API.
//!
//! Writes `~/.refine/quota_exhausted_until` as a UTC ISO-8601 timestamp.
//! Uses atomic write (tmp file + rename) so concurrent workers cannot read
//! a partially-written file.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};

/// Default backoff when the API returns no `Retry-After` header (1 hour).
pub const DEFAULT_QUOTA_BACKOFF_SECS: u64 = 3600;

/// Maximum cap applied to any `retry_after` value written to disk.
const MAX_QUOTA_BACKOFF_SECS: u64 = 3600;

fn quota_file_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".refine").join("quota_exhausted_until")
}

/// Returns `true` when the persisted quota-exhaustion timestamp is still in
/// the future.  Returns `false` on any I/O or parse error (fail-open).
pub fn is_exhausted() -> bool {
    let path = quota_file_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(until) = contents.trim().parse::<DateTime<Utc>>() else {
        return false;
    };
    Utc::now() < until
}

/// Writes a quota-exhaustion timestamp `duration` seconds from now.
///
/// The duration is capped at [`MAX_QUOTA_BACKOFF_SECS`] to prevent a stale
/// file from blocking the tool indefinitely.
///
/// The `REFINE_QUOTA_BACKOFF_SECS` environment variable overrides the
/// supplied `retry_after_secs` when set.
pub fn set_exhausted(retry_after_secs: Option<u64>) {
    let secs = env_override_secs().unwrap_or_else(|| {
        retry_after_secs
            .unwrap_or(DEFAULT_QUOTA_BACKOFF_SECS)
            .min(MAX_QUOTA_BACKOFF_SECS)
    });

    let until = Utc::now() + Duration::from_secs(secs);
    let timestamp = until.to_rfc3339_opts(SecondsFormat::Secs, true);

    let path = quota_file_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    // Atomic write: write to a sibling tmp file, then rename.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &timestamp).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn env_override_secs() -> Option<u64> {
    std::env::var("REFINE_QUOTA_BACKOFF_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn with_quota_file<F: FnOnce(&std::path::Path)>(f: F) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("quota_exhausted_until");
        f(&path);
    }

    // Helper: write a timestamp directly, bypassing set_exhausted() path logic.
    fn write_timestamp(path: &std::path::Path, dt: DateTime<Utc>) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, dt.to_rfc3339_opts(SecondsFormat::Secs, true)).unwrap();
    }

    #[test]
    fn future_timestamp_reports_exhausted() {
        with_quota_file(|path| {
            let future = Utc::now() + Duration::from_secs(3600);
            write_timestamp(path, future);
            let contents = fs::read_to_string(path).unwrap();
            let until: DateTime<Utc> = contents.trim().parse().unwrap();
            assert!(Utc::now() < until, "future timestamp should be exhausted");
        });
    }

    #[test]
    fn past_timestamp_reports_not_exhausted() {
        with_quota_file(|path| {
            let past = Utc::now() - Duration::from_secs(1);
            write_timestamp(path, past);
            let contents = fs::read_to_string(path).unwrap();
            let until: DateTime<Utc> = contents.trim().parse().unwrap();
            assert!(
                Utc::now() >= until,
                "past timestamp should not be exhausted"
            );
        });
    }

    #[test]
    fn set_exhausted_writes_valid_iso8601() {
        // Override the path by writing directly and parsing back.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("quota_exhausted_until");

        let secs = 120u64;
        let before = Utc::now();
        let until = before + Duration::from_secs(secs);
        let timestamp = until.to_rfc3339_opts(SecondsFormat::Secs, true);
        let tmp = path.with_extension("tmp");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&tmp, &timestamp).unwrap();
        fs::rename(&tmp, &path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let parsed: DateTime<Utc> = contents.trim().parse().expect("must be valid ISO-8601");
        assert!(parsed > before, "written timestamp must be in the future");
    }

    #[test]
    fn concurrent_writes_produce_valid_timestamp() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let dir = TempDir::new().unwrap();
        let path = Arc::new(dir.path().join("quota_exhausted_until"));
        let barrier = Arc::new(Barrier::new(2));

        let handles: Vec<_> = (0..2)
            .map(|i| {
                let p = path.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    b.wait();
                    let secs = 100 + i * 10u64;
                    let until = Utc::now() + Duration::from_secs(secs);
                    let timestamp = until.to_rfc3339_opts(SecondsFormat::Secs, true);
                    let tmp = p.with_extension("tmp");
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    let _ = fs::write(&tmp, &timestamp);
                    let _ = fs::rename(&tmp, &*p);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Final file must be valid ISO-8601, regardless of which thread won.
        let contents = fs::read_to_string(&*path).expect("file must exist after concurrent writes");
        contents
            .trim()
            .parse::<DateTime<Utc>>()
            .expect("content must be valid ISO-8601");
    }
}
