use chrono::{Duration, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use std::process::{Command, Output};

const STALE_POLICY: &str = "SYNTHETIC_STALE_POLICY";

fn command(home: &Path, arguments: &[&str]) -> Output {
    command_with_language(home, arguments, "en")
}

fn command_with_language(home: &Path, arguments: &[&str], language: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mirror"))
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .current_dir(home)
        .arg("--db")
        .arg(home.join("refine.db"))
        .args(["--lang", language])
        .args(arguments)
        .output()
        .expect("run isolated mirror")
}

fn seed(home: &Path) -> String {
    std::fs::write(home.join(".env"), "").unwrap();
    let dir = home.join(".mirror");
    std::fs::create_dir(&dir).unwrap();
    // Create a real canonical snapshot through the CLI, with no API key in the
    // isolated environment. Fixtures therefore follow the same scope/cache
    // contract as production without duplicating serialized target defaults.
    assert!(command(home, &["score"]).status.success());
    add_session(home);
    let generated = command(home, &["score"]);
    assert!(generated.status.success(), "{:?}", generated);
    let conn = Connection::open(home.join("refine.db")).unwrap();
    conn.execute_batch("DELETE FROM items; DELETE FROM documents;")
        .unwrap();
    let score = std::fs::read_to_string(dir.join("scores.jsonl")).unwrap();
    let path = dir.join("advice.json");
    let mut cached: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    cached["advice"] = STALE_POLICY.into();
    cached["short"] = STALE_POLICY.into();
    std::fs::write(path, cached.to_string()).unwrap();
    std::fs::write(dir.join("statusline.txt"), STALE_POLICY).unwrap();
    score
}

fn add_session(home: &Path) {
    let conn = Connection::open(home.join("refine.db")).unwrap();
    let timestamp = (Utc::now() - Duration::hours(1)).to_rfc3339();
    conn.execute(
        "INSERT INTO documents (id, title, raw_content, source, url, captured_at, created_at, updated_at)
         VALUES ('fixture-session', 'Synthetic session', 'Synthetic evidence', 'codex-session',
                 'codex://synthetic', ?1, ?1, ?1)",
        params![timestamp],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO items (id, item_type, title, summary, content, tags, source, created_at,
                           updated_at, document_id, excerpt)
         VALUES ('fixture-observation', 'observation', 'Synthetic observation', 'Synthetic evidence',
                 '', '[\"synthetic-project\",\"expert\",\"pair_programming\"]', NULL, ?1, ?1,
                 'fixture-session', NULL)",
        params![timestamp],
    )
    .unwrap();
}

#[test]
fn empty_default_score_invalidates_advice_and_statusline_without_inventing_history() {
    for require_advice in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let score = seed(home.path());
        let before = command(home.path(), &["motd"]);
        assert!(before.status.success());
        assert!(String::from_utf8_lossy(&before.stdout).contains(STALE_POLICY));
        let args = if require_advice {
            vec!["score", "--require-advice"]
        } else {
            vec!["score"]
        };
        let result = command(home.path(), &args);
        assert_eq!(result.status.success(), !require_advice);
        let dir = home.path().join(".mirror");
        assert!(!dir.join("advice.json").exists());
        assert!(!dir.join("statusline.txt").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("scores.jsonl")).unwrap(),
            score
        );
        let after = command(home.path(), &["motd"]);
        assert!(after.status.success());
        assert!(!String::from_utf8_lossy(&after.stdout).contains(STALE_POLICY));
    }
}

#[test]
fn empty_custom_window_preserves_the_portfolio_cache() {
    for args in [
        vec!["score", "--since", "2999-01-01"],
        vec!["score", "--all"],
        vec!["dashboard", "--all"],
        vec!["dashboard", "--since", "2999-01-01"],
    ] {
        let home = tempfile::tempdir().unwrap();
        let score = seed(home.path());
        let result = command(home.path(), &args);
        assert!(result.status.success());
        let dir = home.path().join(".mirror");
        assert!(dir.join("advice.json").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("statusline.txt")).unwrap(),
            STALE_POLICY
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("scores.jsonl")).unwrap(),
            score
        );
    }
}

#[test]
fn nonempty_custom_views_do_not_update_history_or_caches() {
    let home = tempfile::tempdir().unwrap();
    let score = seed(home.path());
    add_session(home.path());
    let dir = home.path().join(".mirror");
    let advice = std::fs::read_to_string(dir.join("advice.json")).unwrap();
    for args in [
        vec!["score", "--all"],
        vec!["dashboard", "--all"],
        vec!["score", "--since", "2020-01-01"],
    ] {
        let result = command(home.path(), &args);
        assert!(result.status.success(), "{:?}", result);
        assert!(String::from_utf8_lossy(&result.stdout).contains("View only"));
        assert_eq!(
            std::fs::read_to_string(dir.join("scores.jsonl")).unwrap(),
            score
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("advice.json")).unwrap(),
            advice
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("statusline.txt")).unwrap(),
            STALE_POLICY
        );
    }
}

#[test]
fn repeated_default_views_keep_one_canonical_snapshot_for_today() {
    let home = tempfile::tempdir().unwrap();
    seed(home.path());
    add_session(home.path());
    for args in [vec!["score", "--require-advice"], vec!["dashboard"]] {
        let result = command(home.path(), &args);
        assert!(result.status.success(), "{:?}", result);
    }
    let history = std::fs::read_to_string(home.path().join(".mirror/scores.jsonl")).unwrap();
    assert_eq!(history.lines().count(), 1);
    let score: serde_json::Value = serde_json::from_str(history.trim()).unwrap();
    assert_eq!(score["scope"]["window"], "rolling-90d");
    assert_eq!(score["scope"]["window_end"], score["timestamp"]);
    let cache: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.path().join(".mirror/advice.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cache["score_timestamp"], score["timestamp"]);
    let motd = command(home.path(), &["motd"]);
    assert!(motd.status.success());
    assert!(String::from_utf8_lossy(&motd.stdout).contains(cache["advice"].as_str().unwrap()));
    let statusline = std::fs::read_to_string(home.path().join(".mirror/statusline.txt")).unwrap();
    assert!(statusline.contains(cache["short"].as_str().unwrap()));
}

#[test]
fn mirror_excludes_non_session_document_sources_before_scoring() {
    for args in [vec!["score"], vec!["dashboard"], vec!["weekly"]] {
        let home = tempfile::tempdir().unwrap();
        let score = seed(home.path());
        add_session(home.path());
        let conn = Connection::open(home.path().join("refine.db")).unwrap();
        conn.execute("UPDATE documents SET source = 'grok-knowledge'", [])
            .unwrap();
        let result = command(home.path(), &args);
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("来源排除: 1"), "{error}");
        assert_eq!(
            std::fs::read_to_string(home.path().join(".mirror/scores.jsonl")).unwrap(),
            score
        );
        if args[0] != "weekly" {
            assert!(!home.path().join(".mirror/advice.json").exists());
            assert!(!home.path().join(".mirror/statusline.txt").exists());
        }
    }
}

#[test]
fn empty_default_dashboard_invalidates_derived_output_without_inventing_history() {
    let home = tempfile::tempdir().unwrap();
    let score = seed(home.path());
    let result = command(home.path(), &["dashboard"]);
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(
        std::fs::read_to_string(home.path().join(".mirror/scores.jsonl")).unwrap(),
        score
    );
    assert!(!home.path().join(".mirror/advice.json").exists());
    assert!(!home.path().join(".mirror/statusline.txt").exists());
}

#[test]
fn motd_never_uses_another_database_or_target_configuration() {
    let home = tempfile::tempdir().unwrap();
    let history = seed(home.path());
    let result = Command::new(env!("CARGO_BIN_EXE_mirror"))
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .current_dir(home.path())
        .arg("--db")
        .arg(home.path().join("other.db"))
        .args(["--lang", "en", "motd"])
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    assert!(!String::from_utf8_lossy(&result.stdout).contains(STALE_POLICY));

    std::fs::write(
        home.path().join(".mirror/config.toml"),
        "[targets]\ndreyfus_green = 4.2\n",
    )
    .unwrap();
    let changed_targets = command(home.path(), &["motd"]);
    assert!(changed_targets.status.success());
    assert!(!String::from_utf8_lossy(&changed_targets.stdout).contains(STALE_POLICY));
    assert_eq!(
        std::fs::read_to_string(home.path().join(".mirror/scores.jsonl")).unwrap(),
        history
    );
}

#[test]
fn motd_rejects_other_language_and_untagged_renderings() {
    let home = tempfile::tempdir().unwrap();
    seed(home.path());
    let english = command(home.path(), &["motd"]);
    assert!(english.status.success());
    assert!(String::from_utf8_lossy(&english.stdout).contains(STALE_POLICY));

    let chinese = command_with_language(home.path(), &["motd"], "zh");
    assert!(chinese.status.success());
    assert!(!String::from_utf8_lossy(&chinese.stdout).contains(STALE_POLICY));
    let english_again = command(home.path(), &["motd"]);
    assert!(String::from_utf8_lossy(&english_again.stdout).contains(STALE_POLICY));

    let path = home.path().join(".mirror/advice.json");
    let mut legacy: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("render_language");
    std::fs::write(path, legacy.to_string()).unwrap();
    let untagged = command(home.path(), &["motd"]);
    assert!(untagged.status.success());
    assert!(!String::from_utf8_lossy(&untagged.stdout).contains(STALE_POLICY));
}
