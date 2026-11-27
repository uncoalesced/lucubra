// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Event record envelope and event type names (SPEC section 6, ADR 0008).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hlc::Hlc;

/// Record schema version written by this engine.
pub const RECORD_VERSION: u32 = 1;

/// Every event type in SPEC section 6.2 and addendum A4 and A6.
pub const KNOWN_TYPES: &[&str] = &[
    // Workspace
    "workspace.initialized",
    "setting.changed",
    "projection.preserved",
    // Profile
    "profile.set",
    "profile.skipped",
    "commitment.set",
    "exam.set",
    "exam.removed",
    // Files
    "file.seen",
    "file.extracted",
    "file.failed",
    "file.classified",
    "file.moved",
    "file.move_undone",
    "file.tagged",
    // Content
    "subject.created",
    "subject.updated",
    "transcript.saved",
    "transcript.verified",
    "unit.created",
    "unit.updated",
    "concept.linked",
    // Planning
    "plan.proposed",
    "plan.edited",
    "plan.accepted",
    "plan.triaged",
    // Sessions
    "session.started",
    "step.completed",
    "session.ended",
    "handoff.written",
    "handoff.ingested",
    "pack.built",
    // Practice
    "item.created",
    "item.rejected",
    "item.answered",
    "item.graded",
    "card.reviewed",
    "exam_sim.started",
    "exam_sim.completed",
    // Wellbeing
    "checkin.logged",
    "preference.set",
    // Addendum A4 and A6
    "term.closed",
    "result.recorded",
    "grading_scheme.set",
    "target.set",
    "subject.prerequisite",
    // Addendum A5
    "source.fetched",
    "mock_test.exported",
    "mock_test.graded",
];

/// Whether `kind` is an event type this engine knows.
pub fn is_known_type(kind: &str) -> bool {
    KNOWN_TYPES.contains(&kind)
}

/// One event record, as stored on one line of a device log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Record schema version.
    pub v: u32,
    /// ULID of the event.
    pub id: String,
    /// Device that wrote the event.
    pub dev: String,
    /// Per-device sequence number, starting at 1.
    pub seq: u64,
    /// Hybrid logical clock value, as text.
    pub hlc: String,
    /// UTC wall time, informational only.
    pub ts: String,
    /// Event type name.
    #[serde(rename = "type")]
    pub kind: String,
    /// Type-specific payload.
    #[serde(default)]
    pub data: Value,
}

/// Total-order key for replay: `(hlc, dev, seq)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrderKey {
    /// Parsed clock value.
    pub hlc: Hlc,
    /// Device identifier.
    pub dev: String,
    /// Per-device sequence number.
    pub seq: u64,
}

impl Event {
    /// The replay order key, or `None` if the clock value does not parse.
    pub fn order_key(&self) -> Option<OrderKey> {
        Some(OrderKey {
            hlc: Hlc::parse(&self.hlc)?,
            dev: self.dev.clone(),
            seq: self.seq,
        })
    }

    /// Serialises the record as one line, without the newline.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("event serialises")
    }
}

/// Parses one log line and upcasts it to the current record version.
///
/// Returns `None` for a line that is not a valid record. Records from a newer
/// engine are returned unchanged; the fold skips them.
pub fn parse_line(line: &str) -> Option<Event> {
    let value: Value = serde_json::from_str(line).ok()?;
    let value = upcast(value);
    let event: Event = serde_json::from_value(value).ok()?;
    if event.id.is_empty() || event.dev.is_empty() || event.kind.is_empty() {
        return None;
    }
    event.order_key()?;
    Some(event)
}

/// Upcasts a raw record to [`RECORD_VERSION`]. Version 1 is current, so this
/// is the identity; later versions add their steps here (A8).
fn upcast(value: Value) -> Value {
    value
}

/// Formats milliseconds since the Unix epoch as RFC 3339 UTC with milliseconds.
pub fn format_ts(ms: u64) -> String {
    let (y, mo, d, h, mi, s, milli) = civil(ms);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{milli:03}Z")
}

/// Formats milliseconds as a compact UTC stamp for file names.
pub fn format_stamp(ms: u64) -> String {
    let (y, mo, d, h, mi, s, milli) = civil(ms);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}{milli:03}Z")
}

/// Converts milliseconds since the epoch to UTC calendar fields.
fn civil(ms: u64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let milli = (ms % 1000) as u32;
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    // Howard Hinnant's days-to-civil algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(mo <= 2);
    (y, mo, d, rem / 3600, (rem / 60) % 60, rem % 60, milli)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_ts(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_ts(1_762_872_000_123), "2025-11-11T14:40:00.123Z");
        assert_eq!(format_stamp(951_782_400_000), "20000229T000000000Z");
    }

    #[test]
    fn parses_the_spec_example_shape() {
        let line = r#"{"v":1,"id":"01JD3Q8","dev":"d-7f2c","seq":1042,"hlc":"1762872000123-0003-d-7f2c","ts":"2026-11-11T17:00:00Z","type":"item.answered","data":{}}"#;
        let event = parse_line(line).unwrap();
        assert_eq!(event.seq, 1042);
        assert_eq!(event.kind, "item.answered");
        assert!(parse_line(&line[..line.len() - 5]).is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn known_types_are_unique() {
        let mut names = KNOWN_TYPES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KNOWN_TYPES.len());
    }
}
