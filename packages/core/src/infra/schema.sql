-- Refine 数据库 Schema

CREATE TABLE IF NOT EXISTS items (
    id TEXT PRIMARY KEY,
    item_type TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    content TEXT NOT NULL,
    tags TEXT NOT NULL,
    source TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    document_id TEXT,
    excerpt TEXT,
    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_items_type ON items(item_type);
CREATE INDEX IF NOT EXISTS idx_items_created ON items(created_at);

-- 全文搜索
CREATE VIRTUAL TABLE IF NOT EXISTS items_fts USING fts5(
    title,
    summary,
    content,
    tags,
    content=items,
    content_rowid=rowid
);

-- 同步触发器
CREATE TRIGGER IF NOT EXISTS items_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, title, summary, content, tags)
    VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
END;

CREATE TRIGGER IF NOT EXISTS items_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
    VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
END;

CREATE TRIGGER IF NOT EXISTS items_au AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, summary, content, tags)
    VALUES('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
    INSERT INTO items_fts(rowid, title, summary, content, tags)
    VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
END;

-- Documents 表
CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    title TEXT,
    raw_content TEXT NOT NULL,
    source TEXT NOT NULL,
    url TEXT NOT NULL,
    source_version TEXT,
    captured_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_documents_created ON documents(created_at);
CREATE INDEX IF NOT EXISTS idx_documents_captured ON documents(captured_at);

-- Documents 全文搜索
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
    title,
    raw_content,
    source,
    content=documents,
    content_rowid=rowid
);

CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents BEGIN
    INSERT INTO documents_fts(rowid, title, raw_content, source)
    VALUES (NEW.rowid, NEW.title, NEW.raw_content, NEW.source);
END;

CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, raw_content, source)
    VALUES('delete', OLD.rowid, OLD.title, OLD.raw_content, OLD.source);
END;

CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents BEGIN
    INSERT INTO documents_fts(documents_fts, rowid, title, raw_content, source)
    VALUES('delete', OLD.rowid, OLD.title, OLD.raw_content, OLD.source);
    INSERT INTO documents_fts(rowid, title, raw_content, source)
    VALUES (NEW.rowid, NEW.title, NEW.raw_content, NEW.source);
END;

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    source TEXT NOT NULL,
    url TEXT NOT NULL,
    title TEXT,
    raw_content TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    captured_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    status TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    item_ids TEXT NOT NULL,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_conversations_status_created
ON conversations(status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_conversations_captured_at
ON conversations(captured_at DESC);

CREATE TABLE IF NOT EXISTS extraction_jobs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    lease_owner TEXT,
    lease_expires_at TEXT,
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_extraction_jobs_conversation
ON extraction_jobs(conversation_id, created_at DESC);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    event_name TEXT NOT NULL,
    source TEXT NOT NULL,
    properties_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_created_at
ON events(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_events_event_name_created_at
ON events(event_name, created_at DESC);
