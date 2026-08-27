use std::process::Command;

#[test]
fn migration_failure_is_a_hard_cli_gate() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("refine.db");
    std::fs::write(temp.path().join("server.db"), b"not a sqlite database").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_refine"))
        .args(["--db", target.to_str().unwrap(), "list", "--limit", "1"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to backup"), "{stderr}");

    let conn = rusqlite::Connection::open(&target).unwrap();
    let item_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        item_count, 0,
        "the CLI must not continue with an empty target"
    );
}
