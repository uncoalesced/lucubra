// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Replay: merge every device log, de-duplicate, order and fold (ADR 0008).

use std::collections::BTreeSet;

use crate::event::{Event, OrderKey};
use crate::hlc::Hlc;
use crate::state::{Skip, State};

/// An event the fold skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Event id.
    pub id: String,
    /// Event type.
    pub kind: String,
    /// Reason.
    pub reason: Skip,
}

/// Result of a replay.
#[derive(Debug, Clone, Default)]
pub struct Replay {
    /// Folded state.
    pub state: State,
    /// Unique events in replay order.
    pub ordered: Vec<Event>,
    /// Events that did not change state.
    pub skipped: Vec<Skipped>,
    /// Greatest clock value seen.
    pub max_hlc: Option<Hlc>,
}

/// Sorts events into replay order and drops duplicate ids.
///
/// Order is `(hlc, dev, seq)`, then id and the serialised line, so the result
/// does not depend on input order even when one id appears with two payloads.
pub fn order(events: impl IntoIterator<Item = Event>) -> Vec<Event> {
    let mut keyed: Vec<(OrderKey, String, String, Event)> = events
        .into_iter()
        .filter_map(|e| Some((e.order_key()?, e.id.clone(), e.to_line(), e)))
        .collect();
    keyed.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    let mut seen = BTreeSet::new();
    keyed
        .into_iter()
        .filter(|(_, id, _, _)| seen.insert(id.clone()))
        .map(|(_, _, _, e)| e)
        .collect()
}

/// Folds ordered events onto `state`, collecting skipped events.
pub fn fold_into(state: &mut State, ordered: &[Event], skipped: &mut Vec<Skipped>) {
    for event in ordered {
        if let Err(reason) = state.apply(event) {
            skipped.push(Skipped {
                id: event.id.clone(),
                kind: event.kind.clone(),
                reason,
            });
        }
    }
}

/// Replays a set of events from scratch.
pub fn replay(events: impl IntoIterator<Item = Event>) -> Replay {
    let ordered = order(events);
    let mut out = Replay {
        max_hlc: ordered.last().and_then(|e| Hlc::parse(&e.hlc)),
        ..Replay::default()
    };
    fold_into(&mut out.state, &ordered, &mut out.skipped);
    out.ordered = ordered;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn logged() -> (tempfile::TempDir, tempfile::TempDir, Vec<Event>) {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = crate::Workspace::init(root.path(), data.path()).unwrap();
        for unit in ["a", "b", "c"] {
            ws.append("unit.created", json!({"subject": "s", "unit": unit}))
                .unwrap();
        }
        ws.append("file.extracted", json!({"no_hash": true}))
            .unwrap();
        let events = ws.replay().unwrap().ordered;
        (root, data, events)
    }

    #[test]
    fn order_ignores_input_order_and_drops_duplicate_ids() {
        let (_r, _d, events) = logged();
        let mut shuffled: Vec<Event> = events.iter().rev().cloned().collect();
        shuffled.push(events[1].clone());
        shuffled.push(events[0].clone());
        let ordered = order(shuffled);
        let ids: Vec<&str> = ordered.iter().map(|e| e.id.as_str()).collect();
        let want: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, want);
    }

    #[test]
    fn events_without_a_clock_are_dropped() {
        let (_r, _d, mut events) = logged();
        events[0].hlc = "not a clock".into();
        assert_eq!(order(events.clone()).len(), events.len() - 1);
    }

    #[test]
    fn replay_folds_and_reports_skipped_payloads() {
        let (_r, _d, events) = logged();
        let out = replay(events.clone());
        assert_eq!(out.ordered.len(), events.len());
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].kind, "file.extracted");
        assert_eq!(out.skipped[0].reason, Skip::BadPayload("hash"));
        assert_eq!(out.max_hlc, Hlc::parse(&events.last().unwrap().hlc));
        assert_eq!(replay(Vec::new()).max_hlc, None);
    }
}
