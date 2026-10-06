-- Substring indexes keep Chinese phrases searchable inside unsegmented text.
-- External content avoids storing a second copy of the source text.

CREATE VIRTUAL TABLE IF NOT EXISTS items_substring_fts USING fts5(
    title, summary, content, tags, content='items', content_rowid='rowid', tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS items_substring_ai AFTER INSERT ON items BEGIN
    INSERT INTO items_substring_fts(rowid, title, summary, content, tags) VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
END;
CREATE TRIGGER IF NOT EXISTS items_substring_ad AFTER DELETE ON items BEGIN
    INSERT INTO items_substring_fts(items_substring_fts, rowid, title, summary, content, tags) VALUES ('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
END;
CREATE TRIGGER IF NOT EXISTS items_substring_au AFTER UPDATE ON items BEGIN
    INSERT INTO items_substring_fts(items_substring_fts, rowid, title, summary, content, tags) VALUES ('delete', OLD.rowid, OLD.title, OLD.summary, OLD.content, OLD.tags);
    INSERT INTO items_substring_fts(rowid, title, summary, content, tags) VALUES (NEW.rowid, NEW.title, NEW.summary, NEW.content, NEW.tags);
END;

CREATE VIRTUAL TABLE IF NOT EXISTS documents_substring_fts USING fts5(
    title, raw_content, source, content='documents', content_rowid='rowid', tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS documents_substring_ai AFTER INSERT ON documents BEGIN
    INSERT INTO documents_substring_fts(rowid, title, raw_content, source) VALUES (NEW.rowid, NEW.title, NEW.raw_content, NEW.source);
END;
CREATE TRIGGER IF NOT EXISTS documents_substring_ad AFTER DELETE ON documents BEGIN
    INSERT INTO documents_substring_fts(documents_substring_fts, rowid, title, raw_content, source) VALUES ('delete', OLD.rowid, OLD.title, OLD.raw_content, OLD.source);
END;
CREATE TRIGGER IF NOT EXISTS documents_substring_au AFTER UPDATE ON documents BEGIN
    INSERT INTO documents_substring_fts(documents_substring_fts, rowid, title, raw_content, source) VALUES ('delete', OLD.rowid, OLD.title, OLD.raw_content, OLD.source);
    INSERT INTO documents_substring_fts(rowid, title, raw_content, source) VALUES (NEW.rowid, NEW.title, NEW.raw_content, NEW.source);
END;

CREATE TABLE IF NOT EXISTS search_index_versions (
    name TEXT PRIMARY KEY,
    version INTEGER NOT NULL
);
-- A cheap cross-process invalidation stamp for optional in-memory search indexes.
CREATE TABLE IF NOT EXISTS search_index_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    generation INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO search_index_state(id, generation) VALUES (1, 0);
CREATE TRIGGER IF NOT EXISTS items_search_generation_ai AFTER INSERT ON items BEGIN
    UPDATE search_index_state SET generation = generation + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS items_search_generation_ad AFTER DELETE ON items BEGIN
    UPDATE search_index_state SET generation = generation + 1 WHERE id = 1;
END;
CREATE TRIGGER IF NOT EXISTS items_search_generation_au AFTER UPDATE ON items BEGIN
    UPDATE search_index_state SET generation = generation + 1 WHERE id = 1;
END;
