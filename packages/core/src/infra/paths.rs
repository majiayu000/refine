use std::path::{Path, PathBuf};

/// Returns the default database path: `$DATA_LOCAL_DIR/refine/refine.db`.
///
/// All entry points (server, desktop, CLI) share this single default
/// so that knowledge data is never accidentally split across files.
pub fn default_db_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("refine")
        .join("refine.db")
}

/// Resolves the database path by checking environment variables in order:
///
/// 1. `REFINE_DB_PATH` — unified override (highest priority)
/// 2. Each key in `fallback_keys` — entry-specific override
/// 3. [`default_db_path()`] — platform-appropriate default
pub fn resolve_db_path(fallback_keys: &[&str]) -> PathBuf {
    if let Some(path) = env_path("REFINE_DB_PATH") {
        return path;
    }
    for key in fallback_keys {
        if let Some(path) = env_path(key) {
            return path;
        }
    }
    default_db_path()
}

/// Creates parent directories for the given path.
pub fn ensure_db_dir(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create database directory {}: {}",
                parent.display(),
                e
            )
        })?;
    }
    Ok(())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_db_path_ends_with_refine_db() {
        let path = default_db_path();
        assert_eq!(path.file_name().unwrap(), "refine.db");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "refine");
    }

    // env var tests are combined into one function because env vars are
    // process-global state and parallel tests cause race conditions.
    #[test]
    fn resolve_db_path_env_priority() {
        // 1) no env → default
        std::env::remove_var("REFINE_DB_PATH");
        std::env::remove_var("__REFINE_TEST_A");
        assert_eq!(resolve_db_path(&["__REFINE_TEST_A"]), default_db_path());

        // 2) unified env takes priority over fallback
        std::env::set_var("REFINE_DB_PATH", "/tmp/unified.db");
        std::env::set_var("__REFINE_TEST_A", "/tmp/fallback.db");
        assert_eq!(
            resolve_db_path(&["__REFINE_TEST_A"]),
            PathBuf::from("/tmp/unified.db")
        );

        // 3) fallback key used when unified is absent
        std::env::remove_var("REFINE_DB_PATH");
        assert_eq!(
            resolve_db_path(&["__REFINE_TEST_A"]),
            PathBuf::from("/tmp/fallback.db")
        );

        // cleanup
        std::env::remove_var("__REFINE_TEST_A");
    }
}
