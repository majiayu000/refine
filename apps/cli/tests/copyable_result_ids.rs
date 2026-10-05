use std::path::Path;
use std::process::Command;

const IDS: [&str; 2] = [
    "01234567-89ab-4cde-8f01-23456789abcd",
    "01234567-89ab-4cde-8f01-23456789abce",
];

fn run_cli(home: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_refine"))
        .env_clear()
        .env("HOME", home)
        .current_dir(home)
        .arg("--db")
        .arg(home.join("refine.db"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn seed(home: &Path) {
    run_cli(home, &["list"]);
    let conn = rusqlite::Connection::open(home.join("refine.db")).unwrap();
    for id in IDS {
        conn.execute(
            "INSERT INTO documents
             (id, title, raw_content, source, url, captured_at, created_at, updated_at)
             VALUES (?1, 'copyable document', 'synthetic document body', 'test', ?2,
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            rusqlite::params![id, format!("https://example.invalid/{id}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items
             (id, item_type, title, summary, content, tags, created_at, updated_at, document_id)
             VALUES (?1, 'knowledge', 'copyable item', 'synthetic summary', 'synthetic item body',
                     '[]', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', ?1)",
            [id],
        )
        .unwrap();
    }
}

#[test]
fn item_list_and_search_ids_round_trip_through_show() {
    let temp = tempfile::tempdir().unwrap();
    seed(temp.path());
    for args in [&["list"][..], &["search", "copyable"][..]] {
        let output = run_cli(temp.path(), args);
        let ids: Vec<_> = output
            .lines()
            .filter(|line| line.starts_with("[Knowledge] "))
            .map(|line| line.split_whitespace().nth(1).unwrap())
            .collect();
        assert_eq!(ids.len(), IDS.len(), "{output}");
        for id in IDS {
            assert!(ids.contains(&id), "missing complete ID {id}: {output}");
        }
        for id in ids {
            let detail = run_cli(temp.path(), &["show", id]);
            assert!(detail.contains(&format!("ID: {id}\n")), "{detail}");
            assert!(detail.contains("synthetic item body"), "{detail}");
        }
    }
}

#[test]
fn document_list_and_search_ids_round_trip_through_doc_show() {
    let temp = tempfile::tempdir().unwrap();
    seed(temp.path());
    for args in [&["docs"][..], &["doc-search", "copyable"][..]] {
        let output = run_cli(temp.path(), args);
        let ids: Vec<_> = output
            .lines()
            .filter_map(|line| line.split_once(" | ").map(|(id, _)| id.trim()))
            .collect();
        assert_eq!(ids.len(), IDS.len(), "{output}");
        for id in IDS {
            assert!(ids.contains(&id), "missing complete ID {id}: {output}");
        }
        for id in ids {
            let detail = run_cli(temp.path(), &["doc-show", id]);
            assert!(detail.contains(&format!("ID: {id}\n")), "{detail}");
            assert!(detail.contains("synthetic document body"), "{detail}");
        }
    }
}
