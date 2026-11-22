// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! SQLite state cache in the app-data directory (ADR 0010).
//!
//! The cache is derived data: deleting it and refreshing from the event logs
//! gives the same state hash, which is computed from rows read back out of
//! SQLite.

use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;

use crate::error::Result;
use crate::event::{Event, KNOWN_TYPES, OrderKey, RECORD_VERSION};
use crate::hlc::Hlc;
use crate::replay::{Skipped, fold_into, order};
use crate::state::{FOLDED_TYPES, State};

/// Cache schema version.
pub const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS applied (id TEXT PRIMARY KEY);
CREATE TABLE IF NOT EXISTS state (
    collection TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (collection, key)
);
";

/// How a refresh brought the cache up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshMode {
    /// No new events.
    UpToDate,
    /// New events all ordered after the high-water mark and were applied.
    Incremental,
    /// The cache was rebuilt from every event.
    Full,
}

/// Result of a refresh.
#[derive(Debug, Clone)]
pub struct Refresh {
    /// How the cache was updated.
    pub mode: RefreshMode,
    /// Events applied in this refresh.
    pub applied: usize,
    /// Events the fold skipped in this refresh.
    pub skipped: Vec<Skipped>,
}

/// An open state cache.
pub struct Cache {
    conn: Connection,
}

fn key_text(key: &OrderKey) -> String {
    serde_json::json!([key.hlc.to_string(), key.dev, key.seq]).to_string()
}

fn parse_key_text(text: &str) -> Option<OrderKey> {
    let value: Value = serde_json::from_str(text).ok()?;
    Some(OrderKey {
        hlc: Hlc::parse(value.get(0)?.as_str()?)?,
        dev: value.get(1)?.as_str()?.to_owned(),
        seq: value.get(2)?.as_u64()?,
    })
}

type Row = (String, String, String);

fn rows(conn: &Connection) -> Result<Vec<Row>> {
    let mut stmt =
        conn.prepare("SELECT collection, key, value FROM state ORDER BY collection, key")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<Row>>>()?;
    Ok(rows)
}

fn applied_ids(conn: &Connection) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM applied")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()?)
}

fn high_water(conn: &Connection) -> Result<Option<OrderKey>> {
    Ok(meta(conn, "high_water")?
        .as_deref()
        .and_then(parse_key_text))
}

fn load_state(conn: &Connection) -> Result<State> {
    let mut state = State::default();
    for (collection, key, value) in rows(conn)? {
        let value: Value = serde_json::from_str(&value)?;
        state
            .collections
            .entry(collection)
            .or_default()
            .insert(key, value);
    }
    Ok(state)
}

fn store(
    conn: &Connection,
    state: &State,
    new: &[Event],
    high: Option<&OrderKey>,
    full: bool,
) -> Result<()> {
    if full {
        conn.execute("DELETE FROM applied", [])?;
    }
    conn.execute("DELETE FROM state", [])?;
    let mut put = conn.prepare("INSERT INTO state (collection, key, value) VALUES (?1, ?2, ?3)")?;
    for (collection, map) in &state.collections {
        for (key, value) in map {
            put.execute(params![collection, key, value.to_string()])?;
        }
    }
    let mut mark = conn.prepare("INSERT OR IGNORE INTO applied (id) VALUES (?1)")?;
    for event in new {
        mark.execute(params![event.id])?;
    }
    match high {
        Some(key) => conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('high_water', ?1)",
            params![key_text(key)],
        )?,
        None => conn.execute("DELETE FROM meta WHERE key = 'high_water'", [])?,
    };
    Ok(())
}

/// Identifies what the fold does. When an engine upgrade folds more event
/// types or record versions, the fingerprint changes and the next refresh
/// rebuilds from every event, so events skipped earlier are folded.
pub fn fold_fingerprint() -> String {
    format!(
        "v{}:{}:{}",
        RECORD_VERSION,
        FOLDED_TYPES.join(","),
        KNOWN_TYPES.join(",")
    )
}

impl Cache {
    /// Opens or creates the cache file.
    pub fn open(path: &Path) -> Result<Cache> {
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(SCHEMA)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let fingerprint = fold_fingerprint();
        if meta(&tx, "schema")?.as_deref() != Some(SCHEMA_VERSION)
            || meta(&tx, "fold")?.as_deref() != Some(fingerprint.as_str())
        {
            tx.execute_batch("DELETE FROM applied; DELETE FROM state; DELETE FROM meta;")?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema', ?1), ('fold', ?2)",
                params![SCHEMA_VERSION, fingerprint],
            )?;
        }
        tx.commit()?;
        Ok(Cache { conn })
    }

    /// Loads state from the cache tables.
    pub fn load_state(&self) -> Result<State> {
        load_state(&self.conn)
    }

    /// Brings the cache up to date with the given events (all logs, any order).
    ///
    /// The whole refresh runs in one `BEGIN IMMEDIATE` transaction, so two
    /// processes refreshing at once cannot overwrite each other's work.
    pub fn refresh(&mut self, events: Vec<Event>) -> Result<Refresh> {
        let ordered = order(events);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied = applied_ids(&tx)?;
        let known: BTreeSet<&str> = ordered.iter().map(|e| e.id.as_str()).collect();
        let lost = applied.iter().any(|id| !known.contains(id.as_str()));
        let new: Vec<Event> = ordered
            .iter()
            .filter(|e| !applied.contains(&e.id))
            .cloned()
            .collect();
        let mut skipped = Vec::new();

        if !lost && new.is_empty() {
            tx.commit()?;
            return Ok(Refresh {
                mode: RefreshMode::UpToDate,
                applied: 0,
                skipped,
            });
        }
        let high = high_water(&tx)?;
        let after_high = new.iter().all(|e| match (&high, e.order_key()) {
            (Some(h), Some(k)) => k > *h,
            (None, _) => applied.is_empty(),
            (Some(_), None) => false,
        });
        let new_high = ordered.last().and_then(Event::order_key);
        let refresh = if !lost && after_high {
            let mut state = load_state(&tx)?;
            fold_into(&mut state, &new, &mut skipped);
            store(&tx, &state, &new, new_high.as_ref(), false)?;
            Refresh {
                mode: RefreshMode::Incremental,
                applied: new.len(),
                skipped,
            }
        } else {
            let mut state = State::default();
            fold_into(&mut state, &ordered, &mut skipped);
            store(&tx, &state, &ordered, new_high.as_ref(), true)?;
            Refresh {
                mode: RefreshMode::Full,
                applied: ordered.len(),
                skipped,
            }
        };
        tx.commit()?;
        Ok(refresh)
    }

    /// Canonical dump of the state table, read from SQLite in key order.
    pub fn canonical(&self) -> Result<String> {
        let mut out = String::new();
        for (collection, key, value) in rows(&self.conn)? {
            out.push_str(&collection);
            out.push('\t');
            out.push_str(&key);
            out.push('\t');
            out.push_str(&value);
            out.push('\n');
        }
        Ok(out)
    }

    /// BLAKE3 hash of [`Cache::canonical`], as hex.
    pub fn state_hash(&self) -> Result<String> {
        Ok(blake3::hash(self.canonical()?.as_bytes())
            .to_hex()
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::replay;
    use serde_json::json;

    fn ev(id: &str, ms: u64, dev: &str, key: &str) -> Event {
        Event {
            v: 1,
            id: id.into(),
            dev: dev.into(),
            seq: 1,
            hlc: format!("{ms:013}-0000-{dev}"),
            ts: String::new(),
            kind: "setting.changed".into(),
            data: json!({"key": key, "value": id}),
        }
    }

    #[test]
    fn incremental_full_and_memory_hashes_agree() {
        let dir = tempfile::tempdir().unwrap();
        let mut cache = Cache::open(&dir.path().join("cache.sqlite")).unwrap();
        let a = ev("a", 10, "d-a", "k");
        let b = ev("b", 20, "d-b", "k");
        let c = ev("c", 15, "d-c", "j");

        let r = cache.refresh(vec![a.clone()]).unwrap();
        assert_eq!(r.mode, RefreshMode::Incremental);
        let r = cache.refresh(vec![a.clone(), b.clone()]).unwrap();
        assert_eq!(r.mode, RefreshMode::Incremental);
        // `c` sorts before the high-water mark: full rebuild.
        let r = cache
            .refresh(vec![a.clone(), b.clone(), c.clone()])
            .unwrap();
        assert_eq!(r.mode, RefreshMode::Full);
        let r = cache
            .refresh(vec![c.clone(), b.clone(), a.clone()])
            .unwrap();
        assert_eq!(r.mode, RefreshMode::UpToDate);

        let expected = replay(vec![a, b, c]).state.hash();
        assert_eq!(cache.state_hash().unwrap(), expected);
        assert_eq!(cache.load_state().unwrap().hash(), expected);
    }
}
