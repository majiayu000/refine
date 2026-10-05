use chrono::Utc;
use serde_json::json;
use std::path::Path;
use std::process::{Command, Output};

const STALE_POLICY: &str = "SYNTHETIC_STALE_POLICY";

fn command(home: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mirror"))
        .env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .current_dir(home)
        .arg("--db")
        .arg(home.join("refine.db"))
        .args(["--lang", "en"])
        .args(arguments)
        .output()
        .expect("run isolated mirror")
}

fn seed(home: &Path) -> String {
    std::fs::write(home.join(".env"), "").unwrap();
    let dir = home.join(".mirror");
    std::fs::create_dir(&dir).unwrap();
    let now = Utc::now().to_rfc3339();
    let score = json!({
        "score_schema_version": 5, "timestamp": now, "tension": null,
        "layers": [
            {"name":"depth","signal":"Green","indicators":[]},
            {"name":"breadth","signal":"Green","indicators":[]},
            {"name":"collaboration","signal":"Green","indicators":[]}
        ]
    })
    .to_string()
        + "\n";
    std::fs::write(dir.join("scores.jsonl"), &score).unwrap();
    std::fs::write(
        dir.join("advice.json"),
        json!({
            "cache_version":"advice-score-v5", "cache_key":"fixture", "model_identity":"fixture",
            "generated_at":now, "score_timestamp":now, "advice":STALE_POLICY, "short":STALE_POLICY
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("statusline.txt"), STALE_POLICY).unwrap();
    score
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
    let home = tempfile::tempdir().unwrap();
    let score = seed(home.path());
    let result = command(home.path(), &["score", "--since", "2999-01-01"]);
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
