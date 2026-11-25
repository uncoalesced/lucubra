// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Deterministic study planner (SPEC section 12): effort, priority, greedy
//! scheduling with interleaving, triage, overcommit guard and Pomodoro-Flow
//! block lengths. Every constant lives in [`PlannerConfig`] so M10 can tune it.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::state::{State, col};

/// Planner constants (initial values from SPEC section 12, tuned in M10).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannerConfig {
    /// Study minutes per 1,000 tokens of source text.
    pub minutes_per_k_tokens: f64,
    /// Cap on exam time pressure `tau`.
    pub tau_cap: f64,
    /// Most consecutive blocks of one subject while another has work.
    pub max_run: usize,
    /// Accepted daily minutes above `capacity * ratio` draw one warning.
    pub overcommit_ratio: f64,
    /// Daily capacity before three sessions have been completed.
    pub default_capacity: u32,
    /// Initial focus span `F` in minutes.
    pub focus_span: f64,
    /// Shortest block, used when the user sets no bounds.
    pub block_min: u32,
    /// Longest block, used when the user sets no bounds.
    pub block_max: u32,
    /// Long break after every fourth block.
    pub long_break: u32,
    /// Idle time after which an open session is closed as recovered.
    pub recovery_idle_ms: u64,
    /// Priority multiplier for units with no exam (spare capacity only).
    pub no_exam_weight: f64,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        PlannerConfig {
            minutes_per_k_tokens: 6.0,
            tau_cap: 3.0,
            max_run: 2,
            overcommit_ratio: 1.3,
            default_capacity: 90,
            focus_span: 25.0,
            block_min: 15,
            block_max: 50,
            long_break: 20,
            recovery_idle_ms: 2 * 60 * 60 * 1000,
            no_exam_weight: 0.05,
        }
    }
}

/// Days since 1970-01-01 for an ISO `YYYY-MM-DD` date.
pub fn days_from_iso(date: &str) -> Option<i64> {
    let mut parts = date.get(..10)?.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days-from-civil algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// ISO `YYYY-MM-DD` for days since 1970-01-01.
pub fn iso_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Domain factor for effort (SPEC 12.1), from the matching domain pack's
/// `effort_factor` (1.0 when no pack matches).
pub fn domain_factor(domain: &str) -> f64 {
    crate::domain::pack_for(domain).map_or(1.0, |p| p.meta.effort_factor)
}

/// Estimated study minutes for `tokens` of source text.
pub fn effort_minutes(cfg: &PlannerConfig, tokens: u64, factor: f64, difficulty: f64) -> u32 {
    let minutes =
        tokens as f64 / 1000.0 * cfg.minutes_per_k_tokens * factor * difficulty.clamp(0.7, 1.5);
    minutes.round().max(1.0) as u32
}

/// Pomodoro-Flow block length for difficulty `d` in [0, 1], within bounds.
pub fn block_length(focus: f64, d: f64, min: u32, max: u32) -> u32 {
    let raw = (focus * (1.15 - 0.3 * d.clamp(0.0, 1.0))).round() as u32;
    raw.clamp(min, max.max(min))
}

/// Learned daily capacity: median of the last 10 completed session lengths,
/// or the default before three sessions exist.
pub fn learned_capacity(cfg: &PlannerConfig, completed_minutes: &[u32]) -> u32 {
    let mut recent: Vec<u32> = completed_minutes
        .iter()
        .rev()
        .take(10)
        .copied()
        .filter(|m| *m > 0)
        .collect();
    if recent.len() < 3 {
        return cfg.default_capacity;
    }
    recent.sort_unstable();
    recent[recent.len() / 2]
}

/// One unit of work to schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkItem {
    /// Subject slug.
    pub subject: String,
    /// Unit slug.
    pub unit: String,
    /// Exam day as an index from today (0 = today); `None` if no exam.
    pub exam_day: Option<u32>,
    /// Exam weight `w_e`.
    pub weight: f64,
    /// Remaining minutes of work.
    pub remaining: u32,
    /// Mastery `m_u` in [0, 1].
    pub mastery: f64,
    /// Difficulty normalised to [0, 1].
    pub difficulty: f64,
}

/// A scheduled study block.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// Day index from today.
    pub day: u32,
    /// Subject slug.
    pub subject: String,
    /// Unit slug.
    pub unit: String,
    /// Study minutes.
    pub minutes: u32,
    /// Break after the block, in minutes.
    pub break_after: u32,
}

/// An exam whose required time exceeds the time available before it.
#[derive(Debug, Clone, PartialEq)]
pub struct Triage {
    /// Subject slug.
    pub subject: String,
    /// Exam day index.
    pub exam_day: u32,
    /// Minutes of work required.
    pub required: u32,
    /// Study minutes available before the exam.
    pub available: u32,
    /// Units left with work after scheduling.
    pub uncovered: Vec<String>,
}

/// A proposed schedule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Schedule {
    /// Blocks in day order.
    pub blocks: Vec<Block>,
    /// Exams the plan cannot cover in full.
    pub triage: Vec<Triage>,
}

type ExamKey = (String, u32);

fn exam_key(item: &WorkItem) -> Option<ExamKey> {
    item.exam_day.map(|d| (item.subject.clone(), d))
}

/// Priority `w_e * tau_e * (1 - m_u) * remaining_u / remaining_e` (SPEC 12.2).
fn priority(
    cfg: &PlannerConfig,
    items: &[WorkItem],
    remaining: &[u32],
    tau: &BTreeMap<ExamKey, f64>,
    i: usize,
) -> f64 {
    let item = &items[i];
    let Some(key) = exam_key(item) else {
        return cfg.no_exam_weight * (1.0 - item.mastery);
    };
    let rem_e: u32 = items
        .iter()
        .zip(remaining)
        .filter(|(it, _)| exam_key(it).as_ref() == Some(&key))
        .map(|(_, r)| *r)
        .sum();
    item.weight * tau[&key] * (1.0 - item.mastery) * f64::from(remaining[i])
        / f64::from(rem_e.max(1))
}

/// Fills days greedily by priority (SPEC 12.2). `day_minutes[d]` is the study
/// time free on day `d`; each day is also capped at `capacity`.
pub fn schedule(
    cfg: &PlannerConfig,
    items: &[WorkItem],
    day_minutes: &[u32],
    capacity: u32,
) -> Schedule {
    let mut remaining: Vec<u32> = items.iter().map(|i| i.remaining).collect();
    let day_cap: Vec<u32> = day_minutes.iter().map(|m| (*m).min(capacity)).collect();
    let available = |exam_day: u32| -> u32 { day_cap.iter().take(exam_day as usize).sum() };
    let mut required: BTreeMap<ExamKey, u32> = BTreeMap::new();
    for item in items {
        if let Some(key) = exam_key(item) {
            *required.entry(key).or_insert(0) += item.remaining;
        }
    }
    let tau: BTreeMap<ExamKey, f64> = required
        .iter()
        .map(|(k, req)| {
            let avail = available(k.1);
            let t = if avail == 0 {
                cfg.tau_cap
            } else {
                (f64::from(*req) / f64::from(avail)).min(cfg.tau_cap)
            };
            (k.clone(), t)
        })
        .collect();

    let mut out = Schedule::default();
    for (day, budget) in day_cap.iter().enumerate() {
        let day = day as u32;
        let mut left = *budget;
        let mut run: Vec<String> = Vec::new();
        let mut blocks_today = 0u32;
        while left > 0 {
            let open: Vec<usize> = (0..items.len())
                .filter(|&i| remaining[i] > 0 && items[i].exam_day.is_none_or(|e| day < e))
                .collect();
            // Interleaving: after `max_run` blocks of one subject, pick
            // another subject if one still has work today.
            let streak = run
                .last()
                .filter(|last| {
                    run.len() >= cfg.max_run
                        && run.iter().rev().take(cfg.max_run).all(|s| s == *last)
                })
                .cloned();
            let blocked = streak
                .filter(|s| open.iter().any(|&i| &items[i].subject != s))
                .unwrap_or_default();
            let best = open
                .iter()
                .copied()
                .filter(|&i| items[i].subject != blocked)
                .max_by(|&a, &b| {
                    priority(cfg, items, &remaining, &tau, a)
                        .total_cmp(&priority(cfg, items, &remaining, &tau, b))
                        .then(b.cmp(&a))
                });
            let Some(i) = best else { break };
            let length = block_length(
                cfg.focus_span,
                items[i].difficulty,
                cfg.block_min,
                cfg.block_max,
            );
            let minutes = length.min(remaining[i]).min(left);
            remaining[i] -= minutes;
            left -= minutes;
            blocks_today += 1;
            let break_after = if blocks_today.is_multiple_of(4) {
                cfg.long_break
            } else {
                (f64::from(minutes) / 5.0).round() as u32
            };
            run.push(items[i].subject.clone());
            out.blocks.push(Block {
                day,
                subject: items[i].subject.clone(),
                unit: items[i].unit.clone(),
                minutes,
                break_after,
            });
        }
    }
    for ((subject, exam_day), req) in &required {
        let avail = available(*exam_day);
        if *req > avail {
            let key = (subject.clone(), *exam_day);
            let uncovered = items
                .iter()
                .zip(&remaining)
                .filter(|(it, r)| exam_key(it).as_ref() == Some(&key) && **r > 0)
                .map(|(it, _)| it.unit.clone())
                .collect();
            out.triage.push(Triage {
                subject: subject.clone(),
                exam_day: *exam_day,
                required: *req,
                available: avail,
                uncovered,
            });
        }
    }
    out
}

/// One warning when a day's accepted minutes exceed `ratio * capacity`.
pub fn overcommit(cfg: &PlannerConfig, daily_minutes: &[u32], capacity: u32) -> Option<String> {
    let peak = daily_minutes.iter().copied().max()?;
    (f64::from(peak) > cfg.overcommit_ratio * f64::from(capacity)).then(|| {
        format!(
            "This plan has up to {peak} minutes of study in a day; your recent sessions average {capacity} minutes. The plan is saved as accepted."
        )
    })
}

/// Estimated tokens of source text for a file record.
///
/// shortcut: page, slide and chunk counts stand in for exact token counts;
/// replace with indexed character totals if M10 shows effort estimates drift.
fn file_tokens(file: &Value) -> u64 {
    let get = |k: &str| file.get(k).and_then(Value::as_u64);
    get("pages")
        .map(|p| p * 500)
        .or_else(|| get("slides").map(|s| s * 120))
        .or_else(|| get("chunks").map(|c| c * 400))
        .unwrap_or(400)
}

/// Unit mastery from practice: `0.6 * R + 0.4 * A`, where R is the mean
/// recall probability of the unit's cards on `today` and A the mean last score
/// of its strict items. Extended (`beyond_notes`) items never count. `None`
/// without practice data.
pub fn unit_mastery(state: &State, unit: &str, today: i64) -> Option<f64> {
    let mean = |v: Vec<f64>| (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64);
    let in_unit = |v: &Value| v.get("unit").and_then(Value::as_str) == Some(unit);
    let r = mean(
        state
            .collection(col::CARDS)
            .filter(|(_, c)| in_unit(c) && c.get("last_day").is_some())
            .map(|(_, c)| crate::srs::retrievability(c, today))
            .collect(),
    );
    let a = mean(
        state
            .collection(col::ITEMS)
            .filter(|(_, i)| {
                in_unit(i) && i.get("mode").and_then(Value::as_str) != Some("extended")
            })
            .filter_map(|(_, i)| i.get("last_score").and_then(Value::as_f64))
            .collect(),
    );
    match (r, a) {
        (Some(r), Some(a)) => Some(0.6 * r + 0.4 * a),
        (r, a) => r.or(a),
    }
}

/// Work items from replayed state: one per (subject, unit) with classified
/// files or a unit card, tied to the subject's next exam on or after `today`.
pub fn work_items(cfg: &PlannerConfig, state: &State, today: i64, domain: &str) -> Vec<WorkItem> {
    let mut tokens: BTreeMap<(String, String), u64> = BTreeMap::new();
    for (_, file) in state.collection(col::FILES) {
        let Some(subject) = file.get("subject").and_then(Value::as_str) else {
            continue;
        };
        let unit = file
            .get("unit")
            .and_then(Value::as_str)
            .unwrap_or("general");
        *tokens
            .entry((subject.to_owned(), unit.to_owned()))
            .or_default() += file_tokens(file);
    }
    for (key, _) in state.collection(col::UNITS) {
        if let Some((s, u)) = key.split_once('/') {
            tokens.entry((s.to_owned(), u.to_owned())).or_insert(4000);
        }
    }
    let factor = domain_factor(domain);
    tokens
        .into_iter()
        .map(|((subject, unit), tok)| {
            let unit_data = state.get(col::UNITS, &format!("{subject}/{unit}"));
            let num = |k: &str| unit_data.and_then(|u| u.get(k)).and_then(Value::as_f64);
            let difficulty = num("difficulty").unwrap_or(1.0);
            let mastery = unit_mastery(state, &format!("{subject}/{unit}"), today)
                .or_else(|| num("mastery"))
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            let exam = state
                .collection(col::EXAMS)
                .filter(|(_, e)| e.get("subject").and_then(Value::as_str) == Some(subject.as_str()))
                .filter_map(|(_, e)| {
                    let day = days_from_iso(e.get("date")?.as_str()?)?;
                    let weight = e.get("weight").and_then(Value::as_f64).unwrap_or(1.0);
                    (day >= today).then_some((day, weight))
                })
                .min_by_key(|(d, _)| *d);
            let effort = effort_minutes(cfg, tok, factor, difficulty);
            WorkItem {
                subject,
                unit,
                exam_day: exam.map(|(d, _)| (d - today) as u32),
                weight: exam.map_or(1.0, |(_, w)| w),
                remaining: ((1.0 - mastery) * f64::from(effort)).round() as u32,
                mastery,
                difficulty: ((difficulty - 0.7) / 0.8).clamp(0.0, 1.0),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(subject: &str, unit: &str, exam_day: Option<u32>, remaining: u32) -> WorkItem {
        WorkItem {
            subject: subject.into(),
            unit: unit.into(),
            exam_day,
            weight: 1.0,
            remaining,
            mastery: 0.0,
            difficulty: 0.5,
        }
    }

    #[test]
    fn planner_mastery_from_practice() {
        let mut state = State::default();
        let ev = |kind: &str, data: Value| crate::event::Event {
            v: 1,
            id: "e".into(),
            dev: "d".into(),
            seq: 1,
            hlc: "h".into(),
            ts: String::new(),
            kind: kind.into(),
            data,
        };
        assert_eq!(unit_mastery(&state, "bio/u1", 10), None);
        let strict = serde_json::json!({"item_id": "a", "unit": "bio/u1", "mode": "strict"});
        let extended = serde_json::json!({"item_id": "b", "unit": "bio/u1", "mode": "extended"});
        state.apply(&ev("item.created", strict)).unwrap();
        state.apply(&ev("item.created", extended)).unwrap();
        let graded = |id: &str, score: f64| serde_json::json!({"item_id": id, "score": score});
        state.apply(&ev("item.answered", graded("a", 0.5))).unwrap();
        state.apply(&ev("item.answered", graded("b", 1.0))).unwrap();
        assert_eq!(unit_mastery(&state, "bio/u1", 10), Some(0.5));
        let card =
            crate::srs::review(&serde_json::json!({}), 10, crate::srs::Rating::Good, None).unwrap();
        let r = crate::srs::retrievability(&card, 12);
        let mut data = card.as_object().unwrap().clone();
        data.insert("card_id".into(), "c".into());
        data.insert("unit".into(), "bio/u1".into());
        state
            .apply(&ev("card.reviewed", Value::Object(data)))
            .unwrap();
        let m = unit_mastery(&state, "bio/u1", 12).unwrap();
        assert!((m - (0.6 * r + 0.4 * 0.5)).abs() < 1e-9);
    }

    #[test]
    fn planner_dates_round_trip() {
        for iso in ["1970-01-01", "2026-11-23", "2028-02-29", "2000-12-31"] {
            assert_eq!(iso_from_days(days_from_iso(iso).unwrap()), iso);
        }
        let a = days_from_iso("2026-11-24").unwrap();
        assert_eq!(a - days_from_iso("2026-11-23").unwrap(), 1);
        assert!(days_from_iso("2026-13-01").is_none());
    }

    #[test]
    fn planner_effort_and_block_lengths_follow_spec() {
        let cfg = PlannerConfig::default();
        assert_eq!(effort_minutes(&cfg, 10_000, 1.5, 1.0), 90);
        assert_eq!(
            effort_minutes(&cfg, 10_000, 1.0, 9.0),
            90,
            "difficulty clamped"
        );
        assert_eq!(block_length(25.0, 0.0, 15, 50), 29);
        assert_eq!(block_length(25.0, 1.0, 15, 50), 21);
        assert_eq!(block_length(25.0, 1.0, 30, 50), 30, "user bounds win");
        assert_eq!(learned_capacity(&cfg, &[60, 120]), 90);
        assert_eq!(learned_capacity(&cfg, &[40, 60, 200, 80, 90]), 80);
    }

    #[test]
    fn planner_respects_capacity_interleaving_and_exam_days() {
        let cfg = PlannerConfig::default();
        let items = [item("a", "u1", Some(3), 600), item("b", "u1", Some(5), 600)];
        let plan = schedule(&cfg, &items, &[300; 6], 120);
        for day in 0..6 {
            let total: u32 = plan
                .blocks
                .iter()
                .filter(|b| b.day == day)
                .map(|b| b.minutes)
                .sum();
            assert!(total <= 120, "day {day} has {total}");
        }
        assert!(plan.blocks.iter().all(|b| b.subject != "a" || b.day < 3));
        for w in plan.blocks.windows(3) {
            let same_day = w[0].day == w[2].day;
            let run = w[0].subject == w[1].subject && w[1].subject == w[2].subject;
            let other_left = w[0].day < 3;
            assert!(!(same_day && run && other_left), "{w:?}");
        }
        assert!(plan.blocks.iter().any(|b| b.subject == "b" && b.day >= 3));
    }

    #[test]
    fn planner_s13_triage_reports_shortfall_with_numbers() {
        let cfg = PlannerConfig::default();
        let items: Vec<WorkItem> = (0..8)
            .map(|i| item("stats", &format!("u{i}"), Some(3), 300))
            .collect();
        let plan = schedule(&cfg, &items, &[180, 180, 180, 180], 180);
        assert_eq!(plan.triage.len(), 1);
        let t = &plan.triage[0];
        assert_eq!((t.required, t.available), (2400, 540));
        assert!(!t.uncovered.is_empty());
        let scheduled: u32 = plan.blocks.iter().map(|b| b.minutes).sum();
        assert!(scheduled <= 540);
    }

    #[test]
    fn planner_overcommit_warns_once_with_numbers() {
        let cfg = PlannerConfig::default();
        let w = overcommit(&cfg, &[240, 240], 90).unwrap();
        assert!(w.contains("240") && w.contains("90"));
        assert!(overcommit(&cfg, &[110], 90).is_none());
    }
}
