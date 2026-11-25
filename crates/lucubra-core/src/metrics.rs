// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Dashboard metrics (SPEC 19).
//!
//! Every metric is a pure function of the replayed state and the ordered event
//! log, so the dashboard reads the same data the planner reads. Metrics that
//! need history (the retention curve, the sleep view) use the events, because
//! folded state keeps only the latest card review.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::planner::{PlannerConfig, days_from_iso, iso_from_days, learned_capacity, unit_mastery};
use crate::replay::Replay;
use crate::state::{State, col};

/// Paired days needed before the sleep and retention view is shown (SPEC 19).
pub const SLEEP_MIN_DAYS: usize = 30;

/// Text shown with the sleep and retention view (SPEC 19).
pub const SLEEP_CAVEAT: &str = "association in your data, not cause";

/// Retention curve buckets: days since the previous review, inclusive.
const BUCKETS: &[(i64, i64, &str)] = &[
    (0, 1, "1 day"),
    (2, 3, "2 to 3 days"),
    (4, 7, "4 to 7 days"),
    (8, 14, "8 to 14 days"),
    (15, 30, "15 to 30 days"),
    (31, i64::MAX, "over 30 days"),
];

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn ts_day(ts: &str) -> Option<i64> {
    ts.get(..10).and_then(days_from_iso)
}

/// Monday of the ISO week holding `day` (day 0, 1970-01-01, was a Thursday).
fn week_start(day: i64) -> i64 {
    day - (day + 3).rem_euclid(7)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        f64::midpoint(values[mid - 1], values[mid])
    } else {
        values[mid]
    })
}

/// Validates a daily check-in and returns the `checkin.logged` payload.
pub fn checkin_payload(date: &str, sleep: f64, quality: u64, energy: u64) -> Result<Value, String> {
    if days_from_iso(date).is_none() {
        return Err("date must be a date like 2026-11-24".into());
    }
    if !(0.0..=24.0).contains(&sleep) || !sleep.is_finite() {
        return Err("sleep must be hours between 0 and 24".into());
    }
    if !(1..=5).contains(&quality) || !(1..=5).contains(&energy) {
        return Err("quality and energy must be whole numbers from 1 to 5".into());
    }
    Ok(json!({"date": date, "sleep": sleep, "quality": quality, "energy": energy}))
}

/// Planned minutes per date from accepted plans; a later acceptance replaces
/// the dates it covers.
fn planned_by_date(state: &State) -> BTreeMap<i64, u64> {
    let mut plans: Vec<&Value> = state
        .collection(col::PLANS)
        .map(|(_, p)| p)
        .filter(|p| text(p, "status") == Some("accepted"))
        .collect();
    plans.sort_by_key(|p| text(p, "accepted_ts").unwrap_or_default().to_owned());
    let mut out = BTreeMap::new();
    for plan in plans {
        let mut days: BTreeMap<i64, u64> = BTreeMap::new();
        for block in plan
            .get("blocks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(day) = text(block, "date").and_then(days_from_iso) {
                *days.entry(day).or_default() +=
                    block.get("minutes").and_then(Value::as_u64).unwrap_or(0);
            }
        }
        out.extend(days);
    }
    out
}

/// Completed study minutes per day, from `step.completed` events.
fn done_by_date(replay: &Replay) -> BTreeMap<i64, u64> {
    let mut out = BTreeMap::new();
    for e in replay.ordered.iter().filter(|e| e.kind == "step.completed") {
        if let Some(day) = ts_day(&e.ts) {
            *out.entry(day).or_default() +=
                e.data.get("minutes").and_then(Value::as_u64).unwrap_or(0);
        }
    }
    out
}

/// Sessions, minutes and planned minutes for the last `weeks` ISO weeks.
pub fn consistency(replay: &Replay, today: i64, weeks: usize) -> Vec<Value> {
    let first = week_start(today) - 7 * (weeks as i64 - 1);
    let mut sessions: BTreeMap<i64, u64> = BTreeMap::new();
    for e in replay
        .ordered
        .iter()
        .filter(|e| e.kind == "session.started")
    {
        if let Some(day) = ts_day(&e.ts) {
            *sessions.entry(week_start(day)).or_default() += 1;
        }
    }
    let fold = |m: BTreeMap<i64, u64>| {
        let mut w: BTreeMap<i64, u64> = BTreeMap::new();
        for (day, v) in m {
            *w.entry(week_start(day)).or_default() += v;
        }
        w
    };
    let minutes = fold(done_by_date(replay));
    let planned = fold(planned_by_date(&replay.state));
    (0..weeks as i64)
        .map(|i| {
            let w = first + 7 * i;
            json!({
                "week": iso_from_days(w),
                "sessions": sessions.get(&w).copied().unwrap_or(0),
                "minutes": minutes.get(&w).copied().unwrap_or(0),
                "planned_minutes": planned.get(&w).copied().unwrap_or(0),
            })
        })
        .collect()
}

/// Logged minutes per subject, from steps that name a unit.
pub fn time_per_subject(replay: &Replay) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for e in replay.ordered.iter().filter(|e| e.kind == "step.completed") {
        if let Some(subject) = text(&e.data, "unit").and_then(|u| u.split('/').next()) {
            *out.entry(subject.to_owned()).or_default() +=
                e.data.get("minutes").and_then(Value::as_u64).unwrap_or(0);
        }
    }
    out
}

fn all_units(state: &State) -> BTreeSet<String> {
    let mut units: BTreeSet<String> = state
        .collection(col::UNITS)
        .map(|(k, _)| k.clone())
        .collect();
    units.extend(
        state
            .collection(col::ITEMS)
            .filter_map(|(_, i)| text(i, "unit").map(str::to_owned)),
    );
    units
}

/// Mastery per unit (`None` before any practice), as the planner computes it.
pub fn mastery_per_unit(state: &State, today: i64) -> BTreeMap<String, Option<f64>> {
    all_units(state)
        .into_iter()
        .map(|u| {
            let m = unit_mastery(state, &u, today);
            (u, m)
        })
        .collect()
}

/// Readiness per subject: `coverage * mastery`, where coverage is the share of
/// the subject's units with strict practice data and mastery is their mean.
/// Subjects without practice are left out.
pub fn readiness_per_subject(state: &State, today: i64) -> BTreeMap<String, f64> {
    let mut by_subject: BTreeMap<String, (usize, Vec<f64>)> = BTreeMap::new();
    for (unit, m) in mastery_per_unit(state, today) {
        let entry = by_subject
            .entry(unit.split('/').next().unwrap_or_default().to_owned())
            .or_default();
        entry.0 += 1;
        entry.1.extend(m);
    }
    by_subject
        .into_iter()
        .filter(|(_, (_, m))| !m.is_empty())
        .map(|(s, (total, m))| {
            let coverage = m.len() as f64 / total as f64;
            let mastery = m.iter().sum::<f64>() / m.len() as f64;
            (s, coverage * mastery)
        })
        .collect()
}

/// Readiness per exam, from its subject's readiness.
pub fn readiness_per_exam(state: &State, today: i64) -> Vec<Value> {
    let subjects = readiness_per_subject(state, today);
    state
        .collection(col::EXAMS)
        .map(|(id, e)| {
            let subject = text(e, "subject").unwrap_or_default();
            json!({
                "exam": id,
                "subject": subject,
                "date": text(e, "date"),
                "readiness": subjects.get(subject),
            })
        })
        .collect()
}

/// Retention curve: share of reviews recalled (not rated `again`) by days
/// since the card's previous review. First reviews have no interval and are
/// left out.
pub fn retention_curve(replay: &Replay) -> Vec<Value> {
    let mut last: BTreeMap<String, i64> = BTreeMap::new();
    let mut counts = vec![(0u64, 0u64); BUCKETS.len()];
    for e in replay.ordered.iter().filter(|e| e.kind == "card.reviewed") {
        let (Some(card), Some(day)) = (
            text(&e.data, "card_id"),
            e.data.get("last_day").and_then(Value::as_i64),
        ) else {
            continue;
        };
        if let Some(prev) = last.insert(card.to_owned(), day) {
            let gap = day - prev;
            if let Some(i) = BUCKETS
                .iter()
                .position(|(lo, hi, _)| (*lo..=*hi).contains(&gap))
            {
                counts[i].0 += 1;
                counts[i].1 += u64::from(text(&e.data, "last_rating") != Some("again"));
            }
        }
    }
    BUCKETS
        .iter()
        .zip(counts)
        .map(|((_, _, label), (n, ok))| {
            json!({
                "interval": label,
                "reviews": n,
                "recalled": (n > 0).then(|| ok as f64 / n as f64),
            })
        })
        .collect()
}

/// Plan adherence over past planned days: completed minutes (capped at the
/// plan per day) divided by planned minutes. `None` before any past plan day.
pub fn plan_adherence(replay: &Replay, today: i64) -> Option<(f64, usize)> {
    let done = done_by_date(replay);
    let (mut planned, mut kept, mut days) = (0u64, 0u64, 0usize);
    for (day, minutes) in planned_by_date(&replay.state) {
        if day >= today || minutes == 0 {
            continue;
        }
        planned += minutes;
        kept += done.get(&day).copied().unwrap_or(0).min(minutes);
        days += 1;
    }
    (planned > 0).then(|| (kept as f64 / planned as f64, days))
}

/// Learned daily capacity from ended sessions (SPEC 12.2).
pub fn capacity(cfg: &PlannerConfig, state: &State) -> u32 {
    let done: Vec<u32> = state
        .collection(col::SESSIONS)
        .filter(|(_, s)| text(s, "status") == Some("ended"))
        .filter_map(|(_, s)| s.get("logged_minutes").and_then(Value::as_u64))
        .map(|m| m as u32)
        .collect();
    learned_capacity(cfg, &done)
}

/// Learned focus span (SPEC 12.3): the median length of the last 20 steps,
/// or the configured span before three steps exist.
pub fn focus_span(cfg: &PlannerConfig, replay: &Replay) -> f64 {
    let steps: Vec<f64> = replay
        .ordered
        .iter()
        .rev()
        .filter(|e| e.kind == "step.completed")
        .filter_map(|e| e.data.get("minutes").and_then(Value::as_u64))
        .filter(|m| *m > 0)
        .take(20)
        .map(|m| m as f64)
        .collect();
    if steps.len() < 3 {
        return cfg.focus_span;
    }
    median(steps).unwrap_or(cfg.focus_span)
}

fn pearson(pairs: &[(f64, f64)]) -> Option<f64> {
    let n = pairs.len() as f64;
    let (mx, my) = pairs
        .iter()
        .fold((0.0, 0.0), |(a, b), (x, y)| (a + x / n, b + y / n));
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pairs {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// Sleep and retention view (SPEC 19). Hidden until at least 30 days have both
/// a check-in and review data; when shown it states the sample size and that
/// it is an association, not a cause.
pub fn sleep_view(replay: &Replay) -> Value {
    let mut recall: BTreeMap<i64, (u64, u64)> = BTreeMap::new();
    for e in replay.ordered.iter().filter(|e| e.kind == "card.reviewed") {
        if let Some(day) = e.data.get("last_day").and_then(Value::as_i64) {
            let r = recall.entry(day).or_default();
            r.0 += 1;
            r.1 += u64::from(text(&e.data, "last_rating") != Some("again"));
        }
    }
    let pairs: Vec<(f64, f64)> = replay
        .state
        .collection(col::CHECKINS)
        .filter_map(|(date, c)| {
            let (n, ok) = recall.get(&days_from_iso(date)?)?;
            let sleep = c.get("sleep").and_then(Value::as_f64)?;
            Some((sleep, *ok as f64 / *n as f64))
        })
        .collect();
    if pairs.len() < SLEEP_MIN_DAYS {
        return json!({
            "available": false,
            "paired_days": pairs.len(),
            "required_days": SLEEP_MIN_DAYS,
            "message": "not enough data yet",
        });
    }
    json!({
        "available": true,
        "paired_days": pairs.len(),
        "sample_size": pairs.len(),
        "correlation_sleep_recall": pearson(&pairs),
        "message": SLEEP_CAVEAT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::replay::replay;

    fn ev(n: u64, kind: &str, ts: &str, data: Value) -> Event {
        Event {
            v: 1,
            id: format!("e{n}"),
            dev: "d-a".into(),
            seq: n,
            hlc: format!("{:013}-0000-d-a", 1_762_000_000_000u64 + n),
            ts: ts.into(),
            kind: kind.into(),
            data,
        }
    }

    fn day(iso: &str) -> i64 {
        days_from_iso(iso).unwrap()
    }

    #[test]
    fn metrics_checkin_payload_validates() {
        assert!(checkin_payload("2026-11-24", 6.5, 3, 2).is_ok());
        assert!(checkin_payload("yesterday", 6.5, 3, 2).is_err());
        assert!(checkin_payload("2026-11-24", 25.0, 3, 2).is_err());
        assert!(checkin_payload("2026-11-24", 7.0, 0, 2).is_err());
        assert!(checkin_payload("2026-11-24", 7.0, 3, 6).is_err());
    }

    #[test]
    fn metrics_consistency_time_and_adherence() {
        // 2026-11-23 is a Monday.
        let events = vec![
            ev(
                1,
                "plan.accepted",
                "2026-11-23T08:00:00Z",
                json!({"plan_id": "p1",
                "blocks": [{"date": "2026-11-23", "subject": "ma", "unit": "u1", "minutes": 60},
                           {"date": "2026-11-24", "subject": "ph", "unit": "u1", "minutes": 40}]}),
            ),
            ev(
                2,
                "session.started",
                "2026-11-23T09:00:00Z",
                json!({"session_id": "s1"}),
            ),
            ev(
                3,
                "step.completed",
                "2026-11-23T09:30:00Z",
                json!({"session_id": "s1", "unit": "ma/u1", "minutes": 30}),
            ),
            ev(
                4,
                "step.completed",
                "2026-11-23T10:00:00Z",
                json!({"session_id": "s1", "unit": "ph/u1", "minutes": 50}),
            ),
            ev(
                5,
                "session.ended",
                "2026-11-23T10:00:00Z",
                json!({"session_id": "s1"}),
            ),
        ];
        let r = replay(events);
        let weeks = consistency(&r, day("2026-11-25"), 2);
        assert_eq!(weeks[1]["week"], "2026-11-23");
        assert_eq!(weeks[1]["sessions"], 1);
        assert_eq!(weeks[1]["minutes"], 80);
        assert_eq!(weeks[1]["planned_minutes"], 100);
        assert_eq!(weeks[0]["minutes"], 0);

        let t = time_per_subject(&r);
        assert_eq!(t["ma"], 30);
        assert_eq!(t["ph"], 50);

        // Day 1: 60 planned, 80 done (capped at 60). Day 2: 40 planned, none done.
        let (a, days) = plan_adherence(&r, day("2026-11-25")).unwrap();
        assert_eq!(days, 2);
        assert!((a - 0.6).abs() < 1e-9);
        // Today and later days are not judged yet.
        assert_eq!(plan_adherence(&r, day("2026-11-23")), None);
    }

    #[test]
    fn metrics_retention_curve_uses_review_history() {
        let review = |n, card: &str, d: i64, rating: &str| {
            ev(
                n,
                "card.reviewed",
                "",
                json!({"card_id": card, "last_day": d, "last_rating": rating}),
            )
        };
        let r = replay(vec![
            review(1, "c1", 100, "again"),
            review(2, "c1", 101, "good"),
            review(3, "c1", 106, "again"),
            review(4, "c2", 100, "good"),
            review(5, "c2", 105, "good"),
        ]);
        let curve = retention_curve(&r);
        assert_eq!(curve[0]["reviews"], 1);
        assert_eq!(curve[0]["recalled"], 1.0);
        assert_eq!(curve[2]["interval"], "4 to 7 days");
        assert_eq!(curve[2]["reviews"], 2);
        assert_eq!(curve[2]["recalled"], 0.5);
        assert_eq!(curve[5]["recalled"], Value::Null);
    }

    #[test]
    fn metrics_readiness_counts_coverage_and_strict_items_only() {
        let r = replay(vec![
            ev(
                1,
                "unit.created",
                "",
                json!({"subject": "ma", "unit": "u1"}),
            ),
            ev(
                2,
                "unit.created",
                "",
                json!({"subject": "ma", "unit": "u2"}),
            ),
            ev(
                3,
                "item.created",
                "",
                json!({"item_id": "i1", "unit": "ma/u1", "mode": "strict"}),
            ),
            ev(
                4,
                "item.answered",
                "",
                json!({"item_id": "i1", "score": 0.8, "correct": true}),
            ),
            ev(
                5,
                "item.created",
                "",
                json!({"item_id": "i2", "unit": "ma/u2", "mode": "extended"}),
            ),
            ev(
                6,
                "item.answered",
                "",
                json!({"item_id": "i2", "score": 1.0, "correct": true}),
            ),
            ev(
                7,
                "exam.set",
                "",
                json!({"id": "x1", "subject": "ma", "date": "2026-12-10"}),
            ),
        ]);
        let m = mastery_per_unit(&r.state, 0);
        assert_eq!(m["ma/u2"], None, "extended items do not count");
        let ready = readiness_per_subject(&r.state, 0);
        let mastery = m["ma/u1"].unwrap();
        assert!(
            (ready["ma"] - 0.5 * mastery).abs() < 1e-9,
            "coverage is one unit of two"
        );
        let exams = readiness_per_exam(&r.state, 0);
        assert_eq!(exams[0]["exam"], "x1");
        assert!((exams[0]["readiness"].as_f64().unwrap() - ready["ma"]).abs() < 1e-9);
    }

    #[test]
    fn metrics_capacity_and_focus_span() {
        let cfg = PlannerConfig::default();
        let step = |n, m: u64| ev(n, "step.completed", "", json!({"minutes": m}));
        let few = replay(vec![step(1, 40), step(2, 10)]);
        assert_eq!(focus_span(&cfg, &few), cfg.focus_span);
        let many = replay(vec![step(1, 40), step(2, 10), step(3, 30), step(4, 20)]);
        assert_eq!(focus_span(&cfg, &many), 25.0);
        assert_eq!(capacity(&cfg, &many.state), cfg.default_capacity);
    }

    fn sleep_log(checkins: i64, reviewed: i64) -> Replay {
        let base = day("2026-10-01");
        let mut events = Vec::new();
        let mut n = 0;
        for i in 0..checkins {
            n += 1;
            events.push(ev(n, "checkin.logged", "", json!({
                "date": iso_from_days(base + i), "sleep": 5.0 + (i % 4) as f64, "quality": 3, "energy": 3})));
            if i < reviewed {
                n += 1;
                let rating = if i % 4 >= 2 { "good" } else { "again" };
                events.push(ev(
                    n,
                    "card.reviewed",
                    "",
                    json!({
                    "card_id": format!("c{i}"), "last_day": base + i, "last_rating": rating}),
                ));
            }
        }
        replay(events)
    }

    #[test]
    fn sleep_view_hidden_until_thirty_paired_days() {
        let twelve = sleep_view(&sleep_log(12, 12));
        assert_eq!(twelve["available"], false);
        assert_eq!(twelve["message"], "not enough data yet");
        assert_eq!(twelve["paired_days"], 12);

        let unpaired = sleep_view(&sleep_log(30, 29));
        assert_eq!(unpaired["available"], false, "29 paired days stay hidden");

        let shown = sleep_view(&sleep_log(30, 30));
        assert_eq!(shown["available"], true);
        assert_eq!(shown["sample_size"], 30);
        assert_eq!(shown["message"], SLEEP_CAVEAT);
        assert!(shown["correlation_sleep_recall"].as_f64().unwrap() > 0.0);
    }
}
