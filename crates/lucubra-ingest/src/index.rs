// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Chunk store and full-text index in app-data (SPEC section 5 rule 2,
//! ADR 0003, ADR 0011 section 6).
//!
//! Everything here is rebuildable from the source files, so it lives outside
//! the synced workspace. Two contentless FTS5 tables index the normalised
//! chunk text: a word index for word and prefix queries with BM25, and a
//! trigram index for substrings.

use std::path::Path;
use std::time::Duration;

use lucubra_core::{
    Error, FTS_SUBSTRING_TOKENIZER, FTS_WORD_TOKENIZER, Result, fts_tokenize_option,
    normalize_for_fts,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::native::{Chunk, Extracted};

/// File name inside the workspace's app-data directory.
pub const INDEX_FILE: &str = "ingest.sqlite";
/// Schema version; a mismatch clears the index.
pub const SCHEMA: i64 = 1;

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// File content hash.
    pub hash: String,
    /// Chunk locator.
    pub locator: String,
    /// Chunk text.
    pub text: String,
    /// BM25 score (lower is better); 0 for substring hits.
    pub score: f64,
}

/// The chunk store and indexes for one workspace on one device.
pub struct Index {
    conn: Connection,
}

fn sql(e: rusqlite::Error) -> Error {
    Error::from(e)
}

/// Quotes each term so user input cannot inject FTS5 syntax.
fn match_expr(query: &str, prefix: bool) -> Option<String> {
    let terms: Vec<String> = normalize_for_fts(query)
        .split_whitespace()
        .map(|t| {
            let quoted = format!("\"{}\"", t.replace('"', "\"\""));
            if prefix { format!("{quoted}*") } else { quoted }
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

const WORD_SELECT: &str = "SELECT c.hash, c.locator, c.text, bm25(fts_word) FROM fts_word
     JOIN chunks c ON c.id = fts_word.rowid
     WHERE fts_word MATCH ?1 ORDER BY bm25(fts_word), c.id LIMIT ?2";

impl Index {
    /// Opens or creates the index at `path`.
    pub fn open(path: &Path) -> Result<Index> {
        let conn = Connection::open(path).map_err(sql)?;
        conn.busy_timeout(Duration::from_secs(10)).map_err(sql)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        if version != SCHEMA {
            conn.execute_batch(
                "DROP TABLE IF EXISTS fts_word; DROP TABLE IF EXISTS fts_tri;
                 DROP TABLE IF EXISTS chunks; DROP TABLE IF EXISTS extractions;",
            )
            .map_err(sql)?;
        }
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS extractions (
                 hash TEXT PRIMARY KEY, extractor TEXT NOT NULL, chunks INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS chunks (
                 id INTEGER PRIMARY KEY, hash TEXT NOT NULL, ord INTEGER NOT NULL,
                 locator TEXT NOT NULL, text TEXT NOT NULL, UNIQUE (hash, ord));
             CREATE VIRTUAL TABLE IF NOT EXISTS fts_word USING fts5(
                 body, content='', contentless_delete=1, {word});
             CREATE VIRTUAL TABLE IF NOT EXISTS fts_tri USING fts5(
                 body, content='', contentless_delete=1, {tri});
             PRAGMA user_version = {SCHEMA};",
            word = fts_tokenize_option(FTS_WORD_TOKENIZER),
            tri = fts_tokenize_option(FTS_SUBSTRING_TOKENIZER),
        ))
        .map_err(sql)?;
        Ok(Index { conn })
    }

    /// The extractor that produced the stored chunks for `hash`, if any.
    pub fn extractor_for(&self, hash: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT extractor FROM extractions WHERE hash = ?1",
                [hash],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Replaces the stored chunks for `hash` in one transaction.
    pub fn store(&mut self, hash: &str, extracted: &Extracted) -> Result<()> {
        let tx = self.conn.transaction().map_err(sql)?;
        for table in ["fts_word", "fts_tri"] {
            tx.execute(
                &format!(
                    "DELETE FROM {table} WHERE rowid IN (SELECT id FROM chunks WHERE hash = ?1)"
                ),
                [hash],
            )
            .map_err(sql)?;
        }
        tx.execute("DELETE FROM chunks WHERE hash = ?1", [hash])
            .map_err(sql)?;
        for (ord, Chunk { locator, text }) in extracted.chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO chunks (hash, ord, locator, text) VALUES (?1, ?2, ?3, ?4)",
                params![hash, ord as i64, locator, text],
            )
            .map_err(sql)?;
            let id = tx.last_insert_rowid();
            let body = normalize_for_fts(text);
            for table in ["fts_word", "fts_tri"] {
                tx.execute(
                    &format!("INSERT INTO {table} (rowid, body) VALUES (?1, ?2)"),
                    params![id, body],
                )
                .map_err(sql)?;
            }
        }
        tx.execute(
            "INSERT INTO extractions (hash, extractor, chunks) VALUES (?1, ?2, ?3)
             ON CONFLICT (hash) DO UPDATE
             SET extractor = excluded.extractor, chunks = excluded.chunks",
            params![hash, extracted.extractor, extracted.chunks.len() as i64],
        )
        .map_err(sql)?;
        tx.commit().map_err(sql)
    }

    /// Returns one chunk's text.
    pub fn chunk(&self, hash: &str, locator: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT text FROM chunks WHERE hash = ?1 AND locator = ?2",
                [hash, locator],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Word search with BM25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        match match_expr(query, false) {
            Some(expr) => self.query(WORD_SELECT, &expr, limit),
            None => Ok(Vec::new()),
        }
    }

    /// Prefix search: every term matches the start of a word.
    pub fn search_prefix(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        match match_expr(query, true) {
            Some(expr) => self.query(WORD_SELECT, &expr, limit),
            None => Ok(Vec::new()),
        }
    }

    /// Substring search through the trigram index. Queries with a term shorter
    /// than three characters fall back to word search (ADR 0003).
    pub fn search_substring(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let normalized = normalize_for_fts(query);
        if normalized.split_whitespace().any(|t| t.chars().count() < 3) {
            return self.search(query, limit);
        }
        match match_expr(query, false) {
            Some(expr) => self.query(
                "SELECT c.hash, c.locator, c.text, 0.0 FROM fts_tri
                 JOIN chunks c ON c.id = fts_tri.rowid
                 WHERE fts_tri MATCH ?1 ORDER BY c.id LIMIT ?2",
                &expr,
                limit,
            ),
            None => Ok(Vec::new()),
        }
    }

    fn query(&self, sql_text: &str, expr: &str, limit: usize) -> Result<Vec<Hit>> {
        let mut stmt = self.conn.prepare(sql_text).map_err(sql)?;
        let rows = stmt
            .query_map(params![expr, limit as i64], |r| {
                Ok(Hit {
                    hash: r.get(0)?,
                    locator: r.get(1)?,
                    text: r.get(2)?,
                    score: r.get(3)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extracted(chunks: &[(&str, &str)]) -> Extracted {
        Extracted {
            extractor: "test@1".into(),
            chunks: chunks
                .iter()
                .map(|(l, t)| Chunk {
                    locator: (*l).into(),
                    text: (*t).into(),
                })
                .collect(),
            ..Extracted::default()
        }
    }

    #[test]
    fn index_store_search_and_replace() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index::open(&dir.path().join(INDEX_FILE)).unwrap();
        index
            .store(
                "h1",
                &extracted(&[
                    ("h:0", "Photosynthesis makes glucose."),
                    ("h:1", "Mitochondria"),
                ]),
            )
            .unwrap();
        index
            .store("h2", &extracted(&[("h:0", "प्रकाश संश्लेषण पत्तियों में")]))
            .unwrap();
        assert_eq!(
            index.extractor_for("h1").unwrap().as_deref(),
            Some("test@1")
        );
        assert_eq!(index.extractor_for("zz").unwrap(), None);

        let hits = index.search("glucose", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].hash.as_str(), hits[0].locator.as_str()),
            ("h1", "h:0")
        );
        assert_eq!(index.search_prefix("mitoch", 10).unwrap()[0].locator, "h:1");
        assert_eq!(index.search("संश्लेषण", 10).unwrap()[0].hash, "h2");
        assert_eq!(index.search_substring("श्लेष", 10).unwrap()[0].hash, "h2");
        assert_eq!(index.search_substring("ynthes", 10).unwrap()[0].hash, "h1");

        // Replacing a file's chunks removes the old ones from both indexes.
        index
            .store("h1", &extracted(&[("h:0", "Respiration releases energy.")]))
            .unwrap();
        assert!(index.search("glucose", 10).unwrap().is_empty());
        assert!(index.search_substring("ynthes", 10).unwrap().is_empty());
        assert_eq!(
            index.chunk("h1", "h:0").unwrap().as_deref(),
            Some("Respiration releases energy.")
        );
        assert_eq!(index.chunk("h1", "h:1").unwrap(), None);
    }

    #[test]
    fn index_query_syntax_is_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = Index::open(&dir.path().join(INDEX_FILE)).unwrap();
        index
            .store("h1", &extracted(&[("h:0", "AND OR NOT near quotes")]))
            .unwrap();
        for hostile in ["\"", "AND", "body:x", "a OR", "NEAR(a b)", "*", ""] {
            assert!(index.search(hostile, 5).is_ok(), "query {hostile:?}");
        }
        assert_eq!(index.search("AND", 5).unwrap().len(), 1);
    }

    #[test]
    fn index_survives_reopen_and_schema_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INDEX_FILE);
        Index::open(&path)
            .unwrap()
            .store("h1", &extracted(&[("h:0", "kept")]))
            .unwrap();
        assert_eq!(
            Index::open(&path).unwrap().search("kept", 5).unwrap().len(),
            1
        );
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 99")
            .unwrap();
        assert!(
            Index::open(&path)
                .unwrap()
                .search("kept", 5)
                .unwrap()
                .is_empty()
        );
    }
}
