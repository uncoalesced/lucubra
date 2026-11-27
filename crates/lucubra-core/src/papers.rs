// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Past papers (SPEC round 4, A5, D39, S51): topic frequency, the exam
//! blueprint and the bounded planner multiplier.
//!
//! A past-paper question is an item with `past_paper: {year, marks}`. Topic
//! frequency counts the distinct years a unit was asked within the subject's
//! last `WINDOW_YEARS` paper years. It is history, never a prediction.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::state::{State, col};

/// Paper years counted for topic frequency.
pub const WINDOW_YEARS: usize = 5;
/// Upper bound of the planner multiplier from topic frequency.
pub const MAX_MULTIPLIER: f64 = 1.5;

/// `(year, marks)` of a past-paper item.
pub fn past_paper(item: &Value) -> Option<(u32, f64)> {
    let p = item.get("past_paper")?;
    let year = u32::try_from(p.get("year")?.as_u64()?).ok()?;
    let marks = p.get("marks").and_then(Value::as_f64).unwrap_or(1.0);
    Some((year, marks))
}

/// Validates a submitted `past_paper` object.
pub fn validate(v: &Value) -> Result<(), String> {
    let year = v.get("year").and_then(Value::as_u64);
    if !year.is_some_and(|y| (1950..=2100).contains(&y)) {
        return Err("past_paper.year must be a year such as 2024".into());
    }
    if let Some(m) = v.get("marks")
        && !m.as_f64().is_some_and(|m| m > 0.0 && m <= 1000.0)
    {
        return Err("past_paper.marks must be a number above 0".into());
    }
    Ok(())
}

fn subject_of(unit: &str) -> &str {
    unit.split_once('/').map_or(unit, |(s, _)| s)
}

type Question = (String, String, u32, f64);

/// Past-paper items by subject: `(unit, type, year, marks)`.
fn by_subject(state: &State) -> BTreeMap<String, Vec<Question>> {
    let mut out: BTreeMap<String, Vec<Question>> = BTreeMap::new();
    for (_, item) in state.collection(col::ITEMS) {
        let (Some((year, marks)), Some(unit)) =
            (past_paper(item), item.get("unit").and_then(Value::as_str))
        else {
            continue;
        };
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        out.entry(subject_of(unit).to_owned()).or_default().push((
            unit.to_owned(),
            kind,
            year,
            marks,
        ));
    }
    out
}

/// Topic frequency per unit: `(asked, of)`, meaning "asked in `asked` of the
/// last `of` paper years".
pub fn topic_frequency(state: &State) -> BTreeMap<String, (usize, usize)> {
    let mut out = BTreeMap::new();
    for qs in by_subject(state).into_values() {
        let years: Vec<u32> = qs
            .iter()
            .map(|q| q.2)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .rev()
            .take(WINDOW_YEARS)
            .collect();
        let mut per_unit: BTreeMap<&str, BTreeSet<u32>> = BTreeMap::new();
        for (unit, _, year, _) in &qs {
            if years.contains(year) {
                per_unit.entry(unit).or_default().insert(*year);
            }
        }
        for (unit, asked) in per_unit {
            out.insert(unit.to_owned(), (asked.len(), years.len()));
        }
    }
    out
}

/// Planner multiplier for a unit: `1 + 0.5 * asked / of`, in [1.0, 1.5].
pub fn multiplier(freq: &BTreeMap<String, (usize, usize)>, unit: &str) -> f64 {
    match freq.get(unit) {
        Some(&(asked, of)) if of > 0 => 1.0 + (MAX_MULTIPLIER - 1.0) * asked as f64 / of as f64,
        _ => 1.0,
    }
}

/// The blueprint of a subject: each unit's share of past-paper marks, and the
/// question types those papers used. Empty when the subject has no past papers.
pub fn blueprint(state: &State, subject: &str) -> (BTreeMap<String, f64>, BTreeSet<String>) {
    let qs = by_subject(state).remove(subject).unwrap_or_default();
    let total: f64 = qs.iter().map(|q| q.3).sum();
    let mut shares: BTreeMap<String, f64> = BTreeMap::new();
    let mut kinds = BTreeSet::new();
    for (unit, kind, _, marks) in qs {
        *shares.entry(unit).or_default() += marks / total;
        kinds.insert(kind);
    }
    (shares, kinds)
}

/// Splits `count` questions across units by share, largest remainder first
/// (ties by unit name), so the counts always sum to `count`.
pub fn allocate(shares: &BTreeMap<String, f64>, count: usize) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    let mut rest: Vec<(f64, &String)> = Vec::new();
    let mut given = 0;
    for (unit, share) in shares {
        let exact = share * count as f64;
        let floor = exact.floor() as usize;
        out.insert(unit.clone(), floor);
        given += floor;
        rest.push((exact - floor as f64, unit));
    }
    rest.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(b.1)));
    for (_, unit) in rest.into_iter().take(count.saturating_sub(given)) {
        *out.get_mut(unit).expect("allocated unit") += 1;
    }
    out
}

/// The `past_papers` history block for status: units asked at least once,
/// most frequent first, at most `limit` entries.
pub fn history(state: &State, limit: usize) -> Option<Value> {
    let freq = topic_frequency(state);
    if freq.is_empty() {
        return None;
    }
    let mut rows: Vec<(&String, &(usize, usize))> = freq.iter().collect();
    rows.sort_by(|a, b| (b.1.0 * a.1.1).cmp(&(a.1.0 * b.1.1)).then(a.0.cmp(b.0)));
    let rows: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|(unit, (asked, of))| {
            json!({"unit": unit, "asked": asked, "of_years": of,
                   "text": format!("asked in {asked} of the last {of} years")})
        })
        .collect();
    Some(json!({"note": "history of past papers, not a prediction", "units": rows}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn state(items: &[(&str, &str, u64, f64)]) -> State {
        let mut s = State::default();
        for (i, (unit, kind, year, marks)) in items.iter().enumerate() {
            s.apply(&Event {
                v: 1,
                id: format!("e{i}"),
                dev: "d-a".into(),
                seq: i as u64 + 1,
                hlc: "1762872000000-0000-d-a".into(),
                ts: String::new(),
                kind: "item.created".into(),
                data: json!({"item_id": format!("i{i}"), "unit": unit, "type": kind,
                             "prompt": "q", "answer": "a",
                             "past_paper": {"year": year, "marks": marks}}),
            })
            .unwrap();
        }
        s
    }

    #[test]
    fn frequency_counts_distinct_years_in_the_window() {
        let mut items = Vec::new();
        for y in 2019..=2024 {
            items.push(("law/torts", "short_answer", y, 10.0));
        }
        items.push(("law/torts", "mcq_single", 2024, 1.0));
        items.push(("law/contract", "short_answer", 2023, 10.0));
        items.push(("law/contract", "short_answer", 2019, 10.0));
        let s = state(&items);
        let f = topic_frequency(&s);
        // The window is 2020 to 2024; 2019 falls outside.
        assert_eq!(f["law/torts"], (5, 5));
        assert_eq!(f["law/contract"], (1, 5));
        assert_eq!(multiplier(&f, "law/torts"), 1.5);
        assert_eq!(multiplier(&f, "law/contract"), 1.1);
        assert_eq!(multiplier(&f, "law/other"), 1.0);
        let h = history(&s, 10).unwrap();
        assert_eq!(h["units"][0]["text"], "asked in 5 of the last 5 years");
    }

    #[test]
    fn blueprint_and_allocation_follow_marks() {
        let s = state(&[
            ("law/torts", "short_answer", 2024, 30.0),
            ("law/contract", "mcq_single", 2024, 10.0),
        ]);
        let (shares, kinds) = blueprint(&s, "law");
        assert_eq!(shares["law/torts"], 0.75);
        assert!(kinds.contains("mcq_single"));
        let a = allocate(&shares, 4);
        assert_eq!((a["law/torts"], a["law/contract"]), (3, 1));
        for n in 0..12 {
            assert_eq!(allocate(&shares, n).values().sum::<usize>(), n);
        }
        assert!(blueprint(&s, "maths").0.is_empty());
    }

    #[test]
    fn past_paper_validation() {
        assert!(validate(&json!({"year": 2024, "marks": 5})).is_ok());
        assert!(validate(&json!({"year": 24})).is_err());
        assert!(validate(&json!({"year": 2024, "marks": 0})).is_err());
    }
}
