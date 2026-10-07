#!/usr/bin/env python3
"""Seed a new, isolated evaluation database. Refuses to replace any existing path."""
import argparse
import json
import os
from pathlib import Path
import sqlite3


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    corpus = json.loads((repo / "docs/eval/search_corpus.json").read_text())
    # Exclusive creation is the guard against touching a real user database.
    fd = os.open(args.db, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    os.close(fd)
    with sqlite3.connect(args.db) as conn:
        conn.executescript((repo / "packages/core/src/infra/schema.sql").read_text())
        for item in corpus:
            conn.execute(
                "INSERT INTO items(id,item_type,title,summary,content,tags,created_at,updated_at) "
                "VALUES(?,?,?,?,?,?,?,?)",
                (item["id"], item["item_type"], item["title"], item["summary"], item["content"],
                 json.dumps(item["tags"]), "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"),
            )
    print(f"Seeded {len(corpus)} synthetic items in {args.db}")


if __name__ == "__main__":
    main()
