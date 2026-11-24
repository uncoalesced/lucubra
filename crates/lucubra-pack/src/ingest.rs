// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Ingest of Pack-mode handoff blocks (SPEC section 16.3): from `.md` and
//! `.txt` files in `inbox/` and from pasted text. A session id already
//! ingested is ignored; new items are grounding-checked against the cited
//! file's full text and dropped with a reason when the quote is not there.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use lucubra_core::planner::days_from_iso;
use lucubra_core::srs::{self, Rating};
use lucubra_core::state::{State, col};
use lucubra_core::{Result, Workspace};
use lucubra_ingest::ground::{ground_in_file, resolve_hash};
use lucubra_ingest::index::Index;
use lucubra_ingest::pipeline::open_index;
use serde_json::{Map, Value, json};

use crate::handoff::{Handoff, parse_all};

/// Result of reading one block.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Ingested(Report),
    /// The session id was ingested before; nothing was written.
    Duplicate(String),
    /// The block could not be read; nothing was written.
    Failed(String),
}

/// What one ingested block changed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub session: String,
    pub steps: usize,
    pub answers: usize,
    pub cards: usize,
    pub created: Vec<String>,
    /// New items dropped on ingest: `(prompt, reason)`.
    pub dropped: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

impl Outcome {
    /// JSON form for tool and page responses.
    pub fn to_json(&self) -> Value {
        match self {
            Self::Ingested(r) => json!({
                "status": "ingested",
                "session": r.session,
                "steps": r.steps,
                "answers": r.answers,
                "cards": r.cards,
                "created": r.created,
                "dropped": r.dropped.iter()
                    .map(|(p, why)| json!({"prompt": p, "reason": why}))
                    .collect::<Vec<_>>(),
                "warnings": r.warnings,
            }),
            Self::Duplicate(s) => json!({
                "status": "already_ingested",
                "session": s,
                "message": "This session was already ingested; nothing changed.",
            }),
            Self::Failed(why) => json!({"status": "not_ingested", "reason": why}),
        }
    }
}

const ITEM_TYPES: &[&str] = &[
    "mcq_single",
    "mcq_multi",
    "cloze",
    "true_false",
    "matching",
    "numeric",
    "ordering",
    "short_answer",
    "long_answer",
    "viva_prompt",
    "case_stem",
    "diagram_label",
    "formula_recall",
];

fn today() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64 / 86_400)
}

fn text<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn day_of(stamp: Option<&str>) -> i64 {
    stamp
        .and_then(|s| s.get(..10))
        .and_then(days_from_iso)
        .unwrap_or_else(today)
}

/// Parses `S:<subject>/U:<unit>/F:<hash prefix>[/<locator>]` into subject,
/// unit, hash prefix and locator.
fn parse_source(src: &str) -> Option<(&str, &str, &str, &str)> {
    let mut parts = src.splitn(4, '/');
    let subject = parts.next()?.strip_prefix("S:")?;
    let unit = parts.next()?.strip_prefix("U:")?;
    let hash = parts.next()?.strip_prefix("F:")?;
    Some((subject, unit, hash, parts.next().unwrap_or_default()))
}

fn exam_day(state: &State, unit: &str, day: i64) -> Option<i64> {
    let subject = unit.split('/').next()?;
    state
        .collection(col::EXAMS)
        .filter(|(_, e)| e.get("subject").and_then(Value::as_str) == Some(subject))
        .filter_map(|(_, e)| days_from_iso(e.get("date")?.as_str()?))
        .filter(|d| *d >= day)
        .min()
}

fn new_item(
    state: &State,
    index: Option<&Index>,
    entry: &Map<String, Value>,
) -> std::result::Result<Map<String, Value>, String> {
    let kind = text(entry, "type").unwrap_or_default();
    if !ITEM_TYPES.contains(&kind) {
        return Err(format!("unknown item type '{kind}'"));
    }
    for key in ["prompt", "answer"] {
        if entry.get(key).is_none_or(Value::is_null) {
            return Err(format!("missing '{key}'"));
        }
    }
    let source = text(entry, "source").ok_or("missing 'source'")?;
    let (subject, unit_slug, prefix, location) =
        parse_source(source).ok_or("source is not written as S:<subject>/U:<unit>/F:<file>")?;
    let anchor = text(entry, "anchor").ok_or("missing 'anchor' quote")?;
    if anchor.split_whitespace().count() > 25 {
        return Err("anchor is longer than 25 words".into());
    }
    let hash = resolve_hash(state, prefix)?;
    let index = index.ok_or("the search index is not available on this computer")?;
    ground_in_file(index, &hash, anchor)?;
    let unit = text(entry, "unit")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{subject}/{unit_slug}"));
    let mut item = entry.clone();
    item.remove("source");
    item.remove("anchor");
    item.insert("unit".into(), json!(unit));
    item.insert("mode".into(), json!("strict"));
    item.insert("grounding".into(), json!("verified"));
    item.insert("difficulty".into(), json!(3.0));
    item.insert(
        "sources".into(),
        json!([{"file_hash": hash, "location": location, "anchor": anchor}]),
    );
    Ok(item)
}

/// Ingests one parsed block.
pub fn ingest_block(ws: &Workspace, h: &Handoff, source: &str) -> Result<Outcome> {
    let state = ws.replay()?.state;
    if state.get(col::HANDOFFS, &h.session).is_some() {
        return Ok(Outcome::Duplicate(h.session.clone()));
    }
    let sid = h.session.as_str();
    let host = h.fields.get("host").map_or("pack", String::as_str);
    let mut r = Report {
        session: h.session.clone(),
        warnings: h.warnings.clone(),
        ..Report::default()
    };
    ws.append(
        "session.started",
        json!({"session_id": sid, "host": host, "mode": "pack",
               "started": h.fields.get("started")}),
    )?;
    for step in h.list("steps") {
        let Some(unit) = text(step, "unit").filter(|u| u.contains('/')) else {
            r.warnings
                .push("a step without 'unit: <subject>/<unit>' was skipped".into());
            continue;
        };
        let minutes = step.get("minutes").and_then(Value::as_u64).unwrap_or(0);
        ws.append(
            "step.completed",
            json!({"session_id": sid, "unit": unit, "minutes": minutes,
                   "kind": text(step, "activity").unwrap_or("study"), "via": "pack"}),
        )?;
        r.steps += 1;
    }
    for answer in h.list("items") {
        let Some(id) = text(answer, "id").filter(|i| state.get(col::ITEMS, i).is_some()) else {
            let shown = text(answer, "id").unwrap_or("?");
            r.warnings
                .push(format!("answer for unknown item '{shown}' skipped"));
            continue;
        };
        let correct = text(answer, "result").is_some_and(|v| v.eq_ignore_ascii_case("correct"));
        ws.append(
            "item.answered",
            json!({"item_id": id, "response": answer.get("response"), "correct": correct,
                   "score": f64::from(u8::from(correct)), "graded_by": "pack",
                   "session_id": sid}),
        )?;
        r.answers += 1;
    }
    for review in h.list("cards") {
        let card = text(review, "id").and_then(|i| state.get(col::CARDS, i).map(|c| (i, c)));
        let rating = text(review, "rating").and_then(Rating::parse);
        let (Some((id, card)), Some(rating)) = (card, rating) else {
            let shown = text(review, "id").unwrap_or("?");
            r.warnings.push(format!(
                "card review '{shown}' skipped (unknown card or rating)"
            ));
            continue;
        };
        let day = day_of(text(review, "at"));
        let unit = card.get("unit").and_then(Value::as_str).unwrap_or_default();
        let mut fields = srs::review(card, day, rating, exam_day(&state, unit, day))
            .map_err(lucubra_core::Error::Invalid)?;
        fields["card_id"] = json!(id);
        fields["via"] = json!("pack");
        ws.append("card.reviewed", fields)?;
        r.cards += 1;
    }
    let index = open_index(ws).ok();
    for entry in h.list("new_items") {
        let prompt = text(entry, "prompt").unwrap_or("(no prompt)").to_owned();
        match new_item(&state, index.as_ref(), entry) {
            Ok(mut item) => {
                let id = format!("it_{}", ulid::Ulid::new().to_string().to_lowercase());
                item.insert("item_id".into(), json!(id));
                item.insert("host".into(), json!(host));
                ws.append("item.created", Value::Object(item))?;
                r.created.push(id);
            }
            Err(reason) => {
                ws.append(
                    "item.rejected",
                    json!({"item_key": format!("pack:{sid}:{}", r.dropped.len()),
                           "reason": reason, "final": true, "via": "pack"}),
                )?;
                r.dropped.push((prompt, reason));
            }
        }
    }
    if !h.checkin.is_empty() {
        let mut data = h.checkin.clone();
        data.insert("session_id".into(), json!(sid));
        ws.append("checkin.logged", Value::Object(data))?;
    }
    ws.append(
        "session.ended",
        json!({"session_id": sid, "reason": "pack", "ended": h.fields.get("ended"),
               "summary": h.fields.get("summary")}),
    )?;
    ws.append(
        "handoff.ingested",
        json!({"session_id": sid, "host": host, "source": source, "steps": r.steps,
               "answers": r.answers, "cards": r.cards, "created": r.created.len(),
               "dropped": r.dropped.len()}),
    )?;
    Ok(Outcome::Ingested(r))
}

/// Ingests every block in `text`; `source` names where it came from.
pub fn ingest_text(ws: &Workspace, text: &str, source: &str) -> Result<Vec<Outcome>> {
    parse_all(text)
        .into_iter()
        .map(|parsed| match parsed {
            Ok(h) => ingest_block(ws, &h, source),
            Err(e) => Ok(Outcome::Failed(e.to_string())),
        })
        .collect()
}

/// Ingests handoff blocks from `.md` and `.txt` files directly in `inbox/`.
/// Returns `(path, outcomes)` for each file that holds a block.
pub fn ingest_inbox(ws: &Workspace) -> Result<Vec<(String, Vec<Outcome>)>> {
    let dir = ws.root().join("inbox");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            let lower = n.to_lowercase();
            lower.ends_with(".md") || lower.ends_with(".txt")
        })
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let path = dir.join(&name);
        // shortcut: whole-file read capped at 2 MB; stream if handoff files grow.
        if fs::metadata(&path).map_or(true, |m| m.len() > 2_000_000) {
            continue;
        }
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        if !body.contains("lucubra-handoff") {
            continue;
        }
        let rel = format!("inbox/{name}");
        let outcomes = ingest_text(ws, &body, &rel)?;
        if !outcomes.is_empty() {
            out.push((rel, outcomes));
        }
    }
    Ok(out)
}
