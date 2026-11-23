// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Replayed state (ADR 0008 rule 9).
//!
//! State is a set of named collections, each an ordered map from key to a JSON
//! value. Only ordered maps are used, so serialisation is canonical and the
//! state hash is the same on every device.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::event::{Event, RECORD_VERSION, is_known_type};

/// Collection names.
pub mod col {
    /// Workspace identity: key `id`, plus other `workspace.initialized` fields.
    pub const WORKSPACE: &str = "workspace";
    /// Settings by key.
    pub const SETTINGS: &str = "settings";
    /// Profile fields by name.
    pub const PROFILE: &str = "profile";
    /// Exams by id.
    pub const EXAMS: &str = "exams";
    /// Subjects by slug.
    pub const SUBJECTS: &str = "subjects";
    /// Units by `<subject>/<unit>`.
    pub const UNITS: &str = "units";
    /// Preserved user edits by preserved path.
    pub const PRESERVED: &str = "preserved";
    /// Files by BLAKE3 content hash (ADR 0011 section 6).
    pub const FILES: &str = "files";
    /// File locations by workspace-relative path.
    pub const PATHS: &str = "paths";
    /// Vision and handwriting transcripts by `<hash>/p:<n>` or `<hash>/img`.
    pub const TRANSCRIPTS: &str = "transcripts";
    /// Study sessions by session id (and unattached progress under `_unattached`).
    pub const SESSIONS: &str = "sessions";
    /// File moves by move id: hash, from, to, batch, and `undone` once reversed.
    pub const MOVES: &str = "moves";
}

/// Event types the engine folds into state. Other known types are stored in
/// the log and folded by later milestones.
pub const FOLDED_TYPES: &[&str] = &[
    "workspace.initialized",
    "setting.changed",
    "projection.preserved",
    "profile.set",
    "profile.skipped",
    "exam.set",
    "exam.removed",
    "subject.created",
    "subject.updated",
    "unit.created",
    "unit.updated",
    "file.seen",
    "file.extracted",
    "file.failed",
    "file.classified",
    "file.moved",
    "file.move_undone",
    "transcript.saved",
    "transcript.verified",
    "session.started",
    "step.completed",
    "session.ended",
    "handoff.written",
];

/// Why an event did not change state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The record version is newer than this engine supports.
    NewerVersion,
    /// The event type is unknown to this engine.
    UnknownType,
    /// The payload is missing a required field.
    BadPayload(&'static str),
}

/// Replayed workspace state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Collections by name.
    pub collections: BTreeMap<String, BTreeMap<String, Value>>,
}

fn str_field<'a>(data: &'a Value, name: &str) -> Option<&'a str> {
    data.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn transcript_key(hash: &str, data: &Value) -> String {
    match data.get("page").and_then(Value::as_u64) {
        Some(page) => format!("{hash}/p:{page}"),
        None => format!("{hash}/img"),
    }
}

impl State {
    /// Returns a collection, empty if absent.
    pub fn collection(&self, name: &str) -> impl Iterator<Item = (&String, &Value)> {
        self.collections.get(name).into_iter().flatten()
    }

    /// Returns one entry.
    pub fn get(&self, name: &str, key: &str) -> Option<&Value> {
        self.collections.get(name)?.get(key)
    }

    fn put(&mut self, name: &str, key: &str, value: Value) {
        self.collections
            .entry(name.to_owned())
            .or_default()
            .insert(key.to_owned(), value);
    }

    fn remove(&mut self, name: &str, key: &str) {
        if let Some(map) = self.collections.get_mut(name) {
            map.remove(key);
            if map.is_empty() {
                self.collections.remove(name);
            }
        }
    }

    /// Merges the fields of `data` into the object at `name`/`key`.
    fn merge(&mut self, name: &str, key: &str, data: &Map<String, Value>) {
        let mut current = match self.get(name, key) {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        };
        for (field, value) in data {
            if value.is_null() {
                current.remove(field);
            } else {
                current.insert(field.clone(), value.clone());
            }
        }
        self.put(name, key, Value::Object(current));
    }

    /// Records a subject if it is not known yet, and sets its title if given.
    fn touch_subject(&mut self, slug: &str, title: Option<&str>) {
        let mut fields = Map::new();
        fields.insert("slug".into(), Value::String(slug.to_owned()));
        if let Some(title) = title {
            fields.insert("title".into(), Value::String(title.to_owned()));
        }
        self.merge(col::SUBJECTS, slug, &fields);
    }

    /// Applies one event. Returns why the event was skipped, if it was.
    pub fn apply(&mut self, event: &Event) -> Result<(), Skip> {
        if event.v > RECORD_VERSION {
            return Err(Skip::NewerVersion);
        }
        if !is_known_type(&event.kind) {
            return Err(Skip::UnknownType);
        }
        let data = &event.data;
        let empty = Map::new();
        let fields = data.as_object().unwrap_or(&empty);
        match event.kind.as_str() {
            "workspace.initialized" => {
                let id = str_field(data, "workspace_id").ok_or(Skip::BadPayload("workspace_id"))?;
                // The first initialisation in replay order wins.
                if self.get(col::WORKSPACE, "id").is_none() {
                    self.put(col::WORKSPACE, "id", Value::String(id.to_owned()));
                }
            }
            "setting.changed" => {
                let key = str_field(data, "key").ok_or(Skip::BadPayload("key"))?;
                match data.get("value") {
                    None | Some(Value::Null) => self.remove(col::SETTINGS, key),
                    Some(value) => self.put(col::SETTINGS, key, value.clone()),
                }
            }
            "projection.preserved" => {
                let to = str_field(data, "preserved_to").ok_or(Skip::BadPayload("preserved_to"))?;
                self.put(col::PRESERVED, to, data.clone());
            }
            "profile.set" => {
                for (field, value) in fields {
                    if value.is_null() {
                        self.remove(col::PROFILE, field);
                    } else {
                        self.put(col::PROFILE, field, value.clone());
                    }
                }
            }
            "profile.skipped" => self.put(col::PROFILE, "skipped", Value::Bool(true)),
            "exam.set" => {
                let id = str_field(data, "id").ok_or(Skip::BadPayload("id"))?;
                if let Some(subject) = str_field(data, "subject") {
                    self.touch_subject(subject, str_field(data, "subject_title"));
                }
                self.merge(col::EXAMS, id, fields);
            }
            "exam.removed" => {
                let id = str_field(data, "id").ok_or(Skip::BadPayload("id"))?;
                self.remove(col::EXAMS, id);
            }
            "subject.created" | "subject.updated" => {
                let subject = str_field(data, "subject")
                    .or_else(|| str_field(data, "slug"))
                    .ok_or(Skip::BadPayload("subject"))?;
                let mut subject_fields = fields.clone();
                subject_fields.insert("slug".into(), Value::String(subject.to_owned()));
                if !subject_fields.contains_key("title")
                    && let Some(title) = str_field(data, "subject_title")
                {
                    subject_fields.insert("title".into(), Value::String(title.to_owned()));
                }
                self.merge(col::SUBJECTS, subject, &subject_fields);
            }
            "unit.created" | "unit.updated" => {
                let subject = str_field(data, "subject").ok_or(Skip::BadPayload("subject"))?;
                let unit = str_field(data, "unit").ok_or(Skip::BadPayload("unit"))?;
                self.touch_subject(subject, str_field(data, "subject_title"));
                let mut unit_fields = fields.clone();
                unit_fields.remove("subject_title");
                self.merge(col::UNITS, &format!("{subject}/{unit}"), &unit_fields);
            }
            "file.seen" => {
                let path = str_field(data, "path").ok_or(Skip::BadPayload("path"))?;
                let hash = str_field(data, "hash").ok_or(Skip::BadPayload("hash"))?;
                let mut location = fields.clone();
                location.remove("path");
                self.put(col::PATHS, path, Value::Object(location));
                let mut file = Map::new();
                file.insert("hash".into(), Value::String(hash.to_owned()));
                if let Some(bytes) = data.get("bytes") {
                    file.insert("bytes".into(), bytes.clone());
                }
                self.merge(col::FILES, hash, &file);
            }
            "file.extracted" | "file.failed" => {
                let hash = str_field(data, "hash").ok_or(Skip::BadPayload("hash"))?;
                let mut file = fields.clone();
                file.remove("path");
                if event.kind == "file.extracted" {
                    // A later success clears an earlier failure.
                    file.insert("status".into(), Value::String("extracted".into()));
                    for stale in ["code", "detail", "tool"] {
                        file.entry(stale).or_insert(Value::Null);
                    }
                } else {
                    let outcome = str_field(data, "outcome").unwrap_or("failed");
                    file.remove("outcome");
                    file.insert("status".into(), Value::String(outcome.to_owned()));
                }
                self.merge(col::FILES, hash, &file);
            }
            "file.classified" => {
                let hash = str_field(data, "hash").ok_or(Skip::BadPayload("hash"))?;
                let subject = str_field(data, "subject").ok_or(Skip::BadPayload("subject"))?;
                self.touch_subject(subject, None);
                let mut file = Map::new();
                file.insert("subject".into(), Value::String(subject.to_owned()));
                file.insert(
                    "unit".into(),
                    str_field(data, "unit").map_or(Value::Null, |u| Value::String(u.to_owned())),
                );
                file.insert(
                    "classified_by".into(),
                    Value::String(str_field(data, "by").unwrap_or("engine").to_owned()),
                );
                self.merge(col::FILES, hash, &file);
            }
            "file.moved" | "file.move_undone" => {
                let id = str_field(data, "move_id").ok_or(Skip::BadPayload("move_id"))?;
                let from = str_field(data, "from").ok_or(Skip::BadPayload("from"))?;
                let to = str_field(data, "to").ok_or(Skip::BadPayload("to"))?;
                let location = match self.get(col::PATHS, from) {
                    Some(Value::Object(map)) => map.clone(),
                    _ => Map::new(),
                };
                self.remove(col::PATHS, from);
                self.put(col::PATHS, to, Value::Object(location));
                if event.kind == "file.moved" {
                    let mut entry = fields.clone();
                    entry.remove("move_id");
                    entry.insert("undone".into(), Value::Bool(false));
                    self.put(col::MOVES, id, Value::Object(entry));
                } else {
                    let mut entry = Map::new();
                    entry.insert("undone".into(), Value::Bool(true));
                    self.merge(col::MOVES, id, &entry);
                }
            }
            "transcript.saved" => {
                let hash = str_field(data, "hash").ok_or(Skip::BadPayload("hash"))?;
                let key = transcript_key(hash, data);
                let mut entry = fields.clone();
                entry.entry("verified").or_insert(Value::Bool(false));
                self.merge(col::TRANSCRIPTS, &key, &entry);
            }
            "transcript.verified" => {
                let hash = str_field(data, "hash").ok_or(Skip::BadPayload("hash"))?;
                let key = transcript_key(hash, data);
                let mut entry = fields.clone();
                entry.insert("verified".into(), Value::Bool(true));
                self.merge(col::TRANSCRIPTS, &key, &entry);
            }
            "session.started" => {
                let sid = str_field(data, "session_id").ok_or(Skip::BadPayload("session_id"))?;
                let mut entry = fields.clone();
                entry.insert("session_id".into(), Value::String(sid.to_owned()));
                entry.insert("status".into(), Value::String("active".into()));
                entry.insert("started_hlc".into(), Value::String(event.hlc.clone()));
                if !event.ts.is_empty() {
                    entry.insert("started_ts".into(), Value::String(event.ts.clone()));
                }
                if self
                    .get(col::SESSIONS, sid)
                    .and_then(|v| v.get("logged_minutes"))
                    .is_none()
                {
                    entry.insert("logged_minutes".into(), Value::from(0u64));
                }
                if self
                    .get(col::SESSIONS, sid)
                    .and_then(|v| v.get("steps_count"))
                    .is_none()
                {
                    entry.insert("steps_count".into(), Value::from(0u64));
                }
                self.merge(col::SESSIONS, sid, &entry);
            }
            "step.completed" => {
                let sid = str_field(data, "session_id").unwrap_or("_unattached");
                let minutes = data.get("minutes").and_then(Value::as_u64).unwrap_or(0);
                let existing = self.get(col::SESSIONS, sid);
                let prev_minutes = existing
                    .and_then(|v| v.get("logged_minutes"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let prev_steps = existing
                    .and_then(|v| v.get("steps_count"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let mut entry = Map::new();
                entry.insert("logged_minutes".into(), Value::from(prev_minutes + minutes));
                entry.insert("steps_count".into(), Value::from(prev_steps + 1));
                if let Some(unit) = str_field(data, "unit") {
                    entry.insert("last_unit".into(), Value::String(unit.to_owned()));
                }
                if let Some(style) = str_field(data, "style") {
                    entry.insert("last_style".into(), Value::String(style.to_owned()));
                    entry.insert("style".into(), Value::String(style.to_owned()));
                }
                if let Some(kind) = str_field(data, "kind") {
                    entry.insert("last_kind".into(), Value::String(kind.to_owned()));
                }
                self.merge(col::SESSIONS, sid, &entry);
            }
            "session.ended" => {
                let sid = str_field(data, "session_id").ok_or(Skip::BadPayload("session_id"))?;
                let mut entry = fields.clone();
                entry.insert("status".into(), Value::String("ended".into()));
                entry.insert("ended_hlc".into(), Value::String(event.hlc.clone()));
                if !event.ts.is_empty() {
                    entry.insert("ended_ts".into(), Value::String(event.ts.clone()));
                }
                self.merge(col::SESSIONS, sid, &entry);
            }
            "handoff.written" => {
                let sid = str_field(data, "session_id").ok_or(Skip::BadPayload("session_id"))?;
                let mut entry = Map::new();
                if let Some(summary) = str_field(data, "summary") {
                    entry.insert("summary".into(), Value::String(summary.to_owned()));
                }
                if let Some(path) = str_field(data, "path") {
                    entry.insert("handoff_path".into(), Value::String(path.to_owned()));
                }
                if !entry.is_empty() {
                    self.merge(col::SESSIONS, sid, &entry);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Canonical serialisation: one line per entry, `collection\tkey\tjson`,
    /// in key order.
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (name, map) in &self.collections {
            for (key, value) in map {
                out.push_str(name);
                out.push('\t');
                out.push_str(key);
                out.push('\t');
                out.push_str(&value.to_string());
                out.push('\n');
            }
        }
        out
    }

    /// BLAKE3 hash of the canonical serialisation, as hex.
    pub fn hash(&self) -> String {
        blake3::hash(self.canonical().as_bytes())
            .to_hex()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(kind: &str, data: Value) -> Event {
        Event {
            v: 1,
            id: "x".into(),
            dev: "d-a".into(),
            seq: 1,
            hlc: "1762872000000-0000-d-a".into(),
            ts: String::new(),
            kind: kind.into(),
            data,
        }
    }

    #[test]
    fn units_create_subjects_and_merge_fields() {
        let mut s = State::default();
        s.apply(&event(
            "unit.created",
            json!({"subject": "maths", "unit": "algebra", "title": "Algebra", "subject_title": "Mathematics"}),
        ))
        .unwrap();
        s.apply(&event(
            "unit.updated",
            json!({"subject": "maths", "unit": "algebra", "summary": "Linear equations"}),
        ))
        .unwrap();
        let unit = s.get(col::UNITS, "maths/algebra").unwrap();
        assert_eq!(unit["title"], "Algebra");
        assert_eq!(unit["summary"], "Linear equations");
        assert_eq!(
            s.get(col::SUBJECTS, "maths").unwrap()["title"],
            "Mathematics"
        );
    }

    #[test]
    fn skips_unknown_and_newer_events() {
        let mut s = State::default();
        assert_eq!(
            s.apply(&event("no.such", json!({}))),
            Err(Skip::UnknownType)
        );
        let mut newer = event("setting.changed", json!({"key": "a", "value": 1}));
        newer.v = 2;
        assert_eq!(s.apply(&newer), Err(Skip::NewerVersion));
        assert_eq!(s, State::default());
        assert!(s.apply(&event("item.answered", json!({}))).is_ok());
        assert_eq!(s, State::default());
    }

    #[test]
    fn file_events_track_paths_and_status() {
        let mut s = State::default();
        s.apply(&event(
            "file.seen",
            json!({"path": "inbox/a.pdf", "hash": "h1", "bytes": 10, "mtime_ms": 5, "online_only": false}),
        ))
        .unwrap();
        s.apply(&event(
            "file.seen",
            json!({"path": "subjects/x/a.pdf", "hash": "h1", "bytes": 10, "mtime_ms": 6, "online_only": false}),
        ))
        .unwrap();
        s.apply(&event(
            "file.failed",
            json!({"hash": "h1", "path": "inbox/a.pdf", "format": "pdf", "outcome": "failed", "code": "timeout", "detail": "slow"}),
        ))
        .unwrap();
        assert_eq!(s.get(col::FILES, "h1").unwrap()["status"], "failed");
        assert_eq!(s.get(col::FILES, "h1").unwrap()["code"], "timeout");
        s.apply(&event(
            "file.extracted",
            json!({"hash": "h1", "format": "pdf", "chunks": 3, "needs_vision": []}),
        ))
        .unwrap();
        let file = s.get(col::FILES, "h1").unwrap();
        assert_eq!(file["status"], "extracted");
        assert!(file.get("code").is_none());
        assert_eq!(file["bytes"], 10);
        assert_eq!(s.get(col::PATHS, "inbox/a.pdf").unwrap()["hash"], "h1");
        assert_eq!(s.get(col::PATHS, "subjects/x/a.pdf").unwrap()["hash"], "h1");
    }

    #[test]
    fn exam_removed_deletes_exam() {
        let mut s = State::default();
        s.apply(&event(
            "exam.set",
            json!({"id": "e1", "subject": "maths", "date": "2026-12-01"}),
        ))
        .unwrap();
        s.apply(&event("exam.removed", json!({"id": "e1"})))
            .unwrap();
        assert!(s.get(col::EXAMS, "e1").is_none());
        assert!(s.get(col::SUBJECTS, "maths").is_some());
    }

    #[test]
    fn transcript_events_track_verification_and_file_metadata() {
        let mut s = State::default();
        s.apply(&event(
            "file.extracted",
            json!({
                "hash": "h1",
                "format": "pdf",
                "chunks": 2,
                "needs_vision": [1],
                "title": "STAT201 Lecture 1",
                "subject": "Statistics"
            }),
        ))
        .unwrap();
        let file = s.get(col::FILES, "h1").unwrap();
        assert_eq!(file["title"], "STAT201 Lecture 1");
        assert_eq!(file["subject"], "Statistics");

        s.apply(&event(
            "transcript.saved",
            json!({"hash": "h1", "page": 1, "path": "notes/lec.pdf.p1.transcript.md"}),
        ))
        .unwrap();
        s.apply(&event(
            "transcript.saved",
            json!({"hash": "h2", "path": "inbox/photo.png.transcript.md", "verified": false}),
        ))
        .unwrap();
        assert_eq!(
            s.get(col::TRANSCRIPTS, "h1/p:1").unwrap()["verified"],
            false
        );
        assert_eq!(
            s.get(col::TRANSCRIPTS, "h2/img").unwrap()["verified"],
            false
        );

        s.apply(&event(
            "transcript.verified",
            json!({"hash": "h1", "page": 1}),
        ))
        .unwrap();
        assert_eq!(s.get(col::TRANSCRIPTS, "h1/p:1").unwrap()["verified"], true);
        assert_eq!(
            s.get(col::TRANSCRIPTS, "h1/p:1").unwrap()["path"],
            "notes/lec.pdf.p1.transcript.md"
        );
    }

    #[test]
    fn subject_and_session_events_fold_deterministically() {
        let mut s = State::default();
        s.apply(&event(
            "subject.created",
            json!({"subject": "stats", "title": "Statistics"}),
        ))
        .unwrap();
        s.apply(&event(
            "subject.updated",
            json!({"subject": "stats", "course_code": "STAT201"}),
        ))
        .unwrap();
        let subj = s.get(col::SUBJECTS, "stats").unwrap();
        assert_eq!(subj["title"], "Statistics");
        assert_eq!(subj["course_code"], "STAT201");

        s.apply(&event(
            "session.started",
            json!({"session_id": "S1", "units": ["stats/u1"], "style": "tutor"}),
        ))
        .unwrap();
        s.apply(&event(
            "step.completed",
            json!({"session_id": "S1", "kind": "step", "unit": "stats/u1", "minutes": 25}),
        ))
        .unwrap();
        s.apply(&event(
            "handoff.written",
            json!({"session_id": "S1", "summary": "Reviewed variance."}),
        ))
        .unwrap();
        s.apply(&event(
            "session.ended",
            json!({"session_id": "S1", "reason": "user"}),
        ))
        .unwrap();
        let sess = s.get(col::SESSIONS, "S1").unwrap();
        assert_eq!(sess["status"], "ended");
        assert_eq!(sess["logged_minutes"], 25);
        assert_eq!(sess["steps_count"], 1);
        assert_eq!(sess["summary"], "Reviewed variance.");
    }
}
