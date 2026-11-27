// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Grades, SGPA and CGPA, the target solver and term close (SPEC round 4, A4,
//! D37, D38).
//!
//! Arithmetic is in integer hundredths: grade points and credits are scaled
//! by 100 and rounded once, so fractional credits such as 1.5 never make a
//! target of 8.5 flip on a float error. Values are rounded only for display.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::state::{State, col};

/// Days a kept card waits at least after a term close (the slow schedule).
pub const SLOW_REVIEW_DAYS: i64 = 30;

/// Upper bound of the planner weight multiplier from a CGPA target.
pub const MAX_WEIGHT_MULTIPLIER: f64 = 1.5;

/// One grade band.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    /// Letter grade, for example `A+`.
    pub grade: String,
    /// Grade points.
    pub points: f64,
    /// Lowest mark (percent) for the band.
    pub min_marks: f64,
}

/// A grading scheme: bands sorted from the highest points down.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheme {
    /// Preset name, or `custom`.
    pub name: String,
    /// Bands, highest points first.
    pub bands: Vec<Band>,
    /// Whether a failed subject's credits count in the CGPA denominator.
    pub fail_counts: bool,
}

fn h(x: f64) -> i64 {
    (x * 100.0).round() as i64
}

/// A weighted sum over counted credits (both in hundredths) as a 2-decimal value.
fn ratio(w: i64, c: i64) -> Option<f64> {
    (c > 0).then(|| (w as f64 / c as f64).round() / 100.0)
}

fn band(grade: &str, points: f64, min_marks: f64) -> Band {
    Band {
        grade: grade.to_owned(),
        points,
        min_marks,
    }
}

/// The 10-point credit-weighted preset. Mark thresholds differ between
/// universities, so these bands are a common default that `grading_scheme.set`
/// with custom bands replaces. A fail counts in the denominator by default.
pub fn ten_point() -> Scheme {
    Scheme {
        name: "ten_point".into(),
        bands: vec![
            band("O", 10.0, 90.0),
            band("A+", 9.0, 80.0),
            band("A", 8.0, 70.0),
            band("B+", 7.0, 60.0),
            band("B", 6.0, 50.0),
            band("C", 5.0, 45.0),
            band("P", 4.0, 40.0),
            band("F", 0.0, 0.0),
        ],
        fail_counts: true,
    }
}

/// Parses `{preset}` or `{bands: [{grade, points, min_marks}], fail_counts?}`.
pub fn parse_scheme(v: &Value) -> Result<Scheme, String> {
    if let Some(p) = v.get("preset").and_then(Value::as_str) {
        return match p {
            "ten_point" => Ok(ten_point()),
            other => Err(format!(
                "unknown grading preset '{other}'; use ten_point or custom bands"
            )),
        };
    }
    let arr = v
        .get("bands")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or("provide {preset: \"ten_point\"} or a non-empty bands array")?;
    let mut bands = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, b) in arr.iter().enumerate() {
        let grade = b
            .get("grade")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .ok_or(format!("bands[{i}].grade must be a non-empty string"))?;
        let points = b
            .get("points")
            .and_then(Value::as_f64)
            .filter(|p| (0.0..=100.0).contains(p))
            .ok_or(format!("bands[{i}].points must be a number from 0 to 100"))?;
        let min_marks = b
            .get("min_marks")
            .and_then(Value::as_f64)
            .filter(|m| (0.0..=100.0).contains(m))
            .ok_or(format!(
                "bands[{i}].min_marks must be a number from 0 to 100"
            ))?;
        if !seen.insert(grade.to_owned()) {
            return Err(format!("grade '{grade}' appears twice"));
        }
        bands.push(band(grade, points, min_marks));
    }
    if !bands.iter().any(|b| b.points > 0.0) {
        return Err("at least one band must have points above 0".into());
    }
    bands.sort_by(|a, b| h(b.points).cmp(&h(a.points)).then(a.grade.cmp(&b.grade)));
    Ok(Scheme {
        name: "custom".into(),
        bands,
        fail_counts: v
            .get("fail_counts")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    })
}

impl Scheme {
    /// Points for a grade.
    pub fn points(&self, grade: &str) -> Option<f64> {
        self.bands
            .iter()
            .find(|b| b.grade == grade)
            .map(|b| b.points)
    }

    fn max_h(&self) -> i64 {
        h(self.bands[0].points)
    }

    /// Index of the lowest passing band.
    fn pass_index(&self) -> usize {
        self.bands.iter().rposition(|b| b.points > 0.0).unwrap_or(0)
    }
}

/// The workspace scheme, or the 10-point preset.
pub fn scheme(state: &State) -> Scheme {
    state
        .get(col::GRADING, "scheme")
        .and_then(|v| parse_scheme(v).ok())
        .unwrap_or_else(ten_point)
}

/// The target CGPA, if set.
pub fn target(state: &State) -> Option<f64> {
    state
        .get(col::GRADING, "target")
        .and_then(|t| t.get("cgpa"))
        .and_then(Value::as_f64)
}

/// One recorded result.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Subject slug.
    pub subject: String,
    /// Term label.
    pub term: String,
    /// Grade.
    pub grade: String,
    /// Credits.
    pub credits: f64,
    /// Fold order; a higher order supersedes a lower one for the same subject.
    pub order: u64,
}

/// Recorded results, in fold order.
pub fn records(state: &State) -> Vec<Record> {
    let mut out: Vec<Record> = state
        .collection(col::RESULTS)
        .filter_map(|(_, r)| {
            Some(Record {
                subject: r.get("subject")?.as_str()?.to_owned(),
                term: r.get("term")?.as_str()?.to_owned(),
                grade: r.get("grade")?.as_str()?.to_owned(),
                credits: r.get("credits")?.as_f64()?,
                order: r.get("order").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    out.sort_by_key(|r| r.order);
    out
}

/// The latest record per subject (records must be in fold order).
pub fn latest<'a>(records: impl IntoIterator<Item = &'a Record>) -> BTreeMap<String, &'a Record> {
    let mut out = BTreeMap::new();
    for r in records {
        out.insert(r.subject.clone(), r);
    }
    out
}

/// Weighted sum and counted credits, both in hundredths.
fn totals<'a>(scheme: &Scheme, recs: impl Iterator<Item = &'a Record>) -> (i64, i64) {
    let (mut w, mut c) = (0, 0);
    for r in recs {
        let Some(p) = scheme.points(&r.grade) else {
            continue;
        };
        if p <= 0.0 && !scheme.fail_counts {
            continue;
        }
        w += h(p) * h(r.credits);
        c += h(r.credits);
    }
    (w, c)
}

/// SGPA for one term.
pub fn sgpa(scheme: &Scheme, records: &[Record], term: &str) -> Option<f64> {
    let (w, c) = totals(
        scheme,
        latest(records.iter().filter(|r| r.term == term)).into_values(),
    );
    ratio(w, c)
}

/// CGPA over the latest result of each subject.
pub fn cgpa(scheme: &Scheme, records: &[Record]) -> Option<f64> {
    let (w, c) = totals(scheme, latest(records).into_values());
    ratio(w, c)
}

/// The grade a pending subject needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Need {
    /// Subject slug.
    pub subject: String,
    /// Grade.
    pub grade: String,
    /// Grade points.
    pub points: f64,
    /// Lowest mark for that grade.
    pub min_marks: f64,
}

/// Solver output.
#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    /// Whether the target can be reached.
    pub reachable: bool,
    /// Average grade points needed over the pending credits.
    pub needed_average: Option<f64>,
    /// CGPA with every pending subject at the top band.
    pub best_reachable: Option<f64>,
    /// CGPA if every pending subject gets exactly its planned grade.
    pub projected: Option<f64>,
    /// Planned grade per pending subject.
    pub plan: Vec<Need>,
}

/// Deterministic target solver. `fixed` is `(credits, points)` of results that
/// stay; `pending` is `(subject, credits)` of subjects still to be graded.
/// Every pending subject starts at the pass band; then, in order of credits
/// (largest first, ties by slug), each is raised one band per round until the
/// target is met. No combination is enumerated.
pub fn solve(
    scheme: &Scheme,
    fixed: &[(f64, f64)],
    pending: &[(String, f64)],
    target: f64,
) -> Solution {
    let w_fixed: i64 = fixed.iter().map(|(c, p)| h(*c) * h(*p)).sum();
    let c_fixed: i64 = fixed.iter().map(|(c, _)| h(*c)).sum();
    let c_pend: i64 = pending.iter().map(|(_, c)| h(*c)).sum();
    let c_all = c_fixed + c_pend;
    let t = h(target);
    let need = t * c_all - w_fixed;
    let best = ratio(w_fixed + scheme.max_h() * c_pend, c_all);
    if c_pend == 0 {
        return Solution {
            reachable: c_all > 0 && w_fixed >= t * c_all,
            needed_average: None,
            best_reachable: best,
            projected: best,
            plan: Vec::new(),
        };
    }
    let needed_average = ratio(need.max(0), c_pend);
    if need > scheme.max_h() * c_pend {
        return Solution {
            reachable: false,
            needed_average,
            best_reachable: best,
            projected: best,
            plan: Vec::new(),
        };
    }
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by(|&a, &b| {
        h(pending[b].1)
            .cmp(&h(pending[a].1))
            .then(pending[a].0.cmp(&pending[b].0))
    });
    let mut idx = vec![scheme.pass_index(); pending.len()];
    let sum = |idx: &[usize]| -> i64 {
        pending
            .iter()
            .zip(idx)
            .map(|((_, c), i)| h(*c) * h(scheme.bands[*i].points))
            .sum()
    };
    'raise: while sum(&idx) < need {
        let mut moved = false;
        for &k in &order {
            if idx[k] > 0 {
                idx[k] -= 1;
                moved = true;
                if sum(&idx) >= need {
                    break 'raise;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let plan = pending
        .iter()
        .zip(&idx)
        .map(|((s, _), i)| {
            let b = &scheme.bands[*i];
            Need {
                subject: s.clone(),
                grade: b.grade.clone(),
                points: b.points,
                min_marks: b.min_marks,
            }
        })
        .collect();
    Solution {
        reachable: true,
        needed_average,
        best_reachable: best,
        projected: ratio(w_fixed + sum(&idx), c_all),
        plan,
    }
}

/// Whether a subject is still active (not archived by a term close).
pub fn is_active(subject: &Value) -> bool {
    subject.get("status").and_then(Value::as_str) != Some("archived")
}

/// Pending subjects: active, with credits, and no result in the current term.
/// With no current term set, a subject is pending until its latest result is a
/// pass, so a carried-over fail stays pending.
pub fn pending(state: &State) -> Vec<(String, f64)> {
    let term = state
        .get(col::PROFILE, "current_term")
        .and_then(Value::as_str);
    let recs = records(state);
    let scheme = scheme(state);
    let last = latest(&recs);
    state
        .collection(col::SUBJECTS)
        .filter(|(_, s)| is_active(s))
        .filter_map(|(slug, s)| {
            let credits = s
                .get("credits")
                .and_then(Value::as_f64)
                .filter(|c| *c > 0.0)?;
            let graded = match term {
                Some(t) => recs.iter().any(|r| &r.subject == slug && r.term == t),
                None => last
                    .get(slug)
                    .and_then(|r| scheme.points(&r.grade))
                    .is_some_and(|p| p > 0.0),
            };
            (!graded).then(|| (slug.clone(), credits))
        })
        .collect()
}

/// Solves the workspace target, if one is set. A pending subject's earlier
/// result (a backlog fail) is left out, because its new result supersedes it.
pub fn solve_state(state: &State) -> Option<Solution> {
    let target = target(state)?;
    let scheme = scheme(state);
    let pend = pending(state);
    let recs = records(state);
    let fixed: Vec<(f64, f64)> = latest(&recs)
        .into_values()
        .filter(|r| !pend.iter().any(|(s, _)| s == &r.subject))
        .filter_map(|r| {
            let p = scheme.points(&r.grade)?;
            (p > 0.0 || scheme.fail_counts).then_some((r.credits, p))
        })
        .collect();
    Some(solve(&scheme, &fixed, &pend, target))
}

/// Planner weight multipliers per pending subject when a target is set: one
/// band of improvement moves the CGPA in proportion to the subject's credits,
/// so the multiplier is `1 + 0.5 * credits / max_credits`, in [1.0, 1.5].
pub fn weight_multipliers(state: &State) -> BTreeMap<String, f64> {
    if target(state).is_none() {
        return BTreeMap::new();
    }
    let pend = pending(state);
    let max = pend.iter().map(|(_, c)| *c).fold(0.0, f64::max);
    if max <= 0.0 {
        return BTreeMap::new();
    }
    pend.into_iter()
        .map(|(s, c)| {
            let m = 1.0 + (MAX_WEIGHT_MULTIPLIER - 1.0) * c / max;
            (s, (m * 100.0).round() / 100.0)
        })
        .collect()
}

/// The `grades` block for `lucubra_status`, or `None` before any grade data.
pub fn summary(state: &State) -> Option<Value> {
    let recs = records(state);
    let target = target(state);
    if recs.is_empty() && target.is_none() && state.get(col::GRADING, "scheme").is_none() {
        return None;
    }
    let scheme = scheme(state);
    let terms: BTreeSet<&str> = recs.iter().map(|r| r.term.as_str()).collect();
    let sgpa: BTreeMap<&str, Option<f64>> = terms
        .iter()
        .map(|t| (*t, sgpa(&scheme, &recs, t)))
        .collect();
    let mut out = json!({
        "scheme": scheme.name,
        "cgpa": cgpa(&scheme, &recs),
        "sgpa": sgpa,
    });
    if let (Some(t), Some(sol)) = (target, solve_state(state)) {
        let plan: Vec<Value> = sol
            .plan
            .iter()
            .map(|n| json!({"subject": n.subject, "grade": n.grade, "min_marks": n.min_marks}))
            .collect();
        out["target"] = json!({
            "cgpa": t,
            "reachable": sol.reachable,
            "needed_average": sol.needed_average,
            "best_reachable": sol.best_reachable,
            "projected": sol.projected,
            "plan": plan,
            "weights": weight_multipliers(state),
        });
    }
    Some(out)
}

/// Validates a result against the workspace scheme.
pub fn validate_result(state: &State, grade: &str, credits: f64) -> Result<(), String> {
    let s = scheme(state);
    if s.points(grade).is_none() {
        let grades: Vec<&str> = s.bands.iter().map(|b| b.grade.as_str()).collect();
        return Err(format!(
            "grade '{grade}' is not in the scheme; use one of {}",
            grades.join(", ")
        ));
    }
    if !(credits > 0.0 && credits <= 100.0) {
        return Err("credits must be a number above 0 and at most 100".into());
    }
    Ok(())
}

/// Validates a target CGPA against the workspace scheme.
pub fn validate_target(state: &State, cgpa: f64) -> Result<(), String> {
    let max = scheme(state).bands[0].points;
    if !(cgpa > 0.0 && cgpa <= max) {
        return Err(format!("target must be above 0 and at most {max}"));
    }
    Ok(())
}

/// Builds the `term.closed` payload: subjects whose latest result is a fail
/// carry over; `keep` subjects keep their cards on the slow schedule; the rest
/// retire.
pub fn close_payload(
    state: &State,
    term: &str,
    keep: &[String],
    day: i64,
) -> Result<Value, String> {
    if state.get(col::TERMS, term).is_some() {
        return Err(format!("term '{term}' is already closed"));
    }
    let scheme = scheme(state);
    let recs = records(state);
    let last = latest(&recs);
    let active: Vec<&String> = state
        .collection(col::SUBJECTS)
        .filter(|(_, s)| is_active(s))
        .map(|(k, _)| k)
        .collect();
    for k in keep {
        if !active.contains(&k) {
            return Err(format!("--keep '{k}' is not an active subject"));
        }
    }
    let failed = |s: &str| {
        last.get(s)
            .and_then(|r| scheme.points(&r.grade))
            .is_some_and(|p| p <= 0.0)
    };
    let (mut carried, mut kept, mut retired) = (Vec::new(), Vec::new(), Vec::new());
    for s in active {
        if failed(s) {
            carried.push(s.clone());
        } else if keep.contains(s) {
            kept.push(s.clone());
        } else {
            retired.push(s.clone());
        }
    }
    Ok(json!({
        "term": term, "day": day, "carried": carried, "kept": kept, "retired": retired,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(subject: &str, term: &str, grade: &str, credits: f64, order: u64) -> Record {
        Record {
            subject: subject.into(),
            term: term.into(),
            grade: grade.into(),
            credits,
            order,
        }
    }

    #[test]
    fn sgpa_and_cgpa_use_credit_weights_and_the_latest_attempt() {
        let s = ten_point();
        let r = vec![
            rec("maths", "s1", "A", 4.0, 1),
            rec("lab", "s1", "O", 1.5, 2),
            rec("physics", "s1", "F", 3.0, 3),
            rec("physics", "s2", "B", 3.0, 4),
        ];
        // s1: (8*4 + 10*1.5 + 0*3) / 8.5 = 5.53
        assert_eq!(sgpa(&s, &r, "s1"), Some(5.53));
        // the latest physics result is B: (32 + 15 + 18) / 8.5 = 7.65
        assert_eq!(cgpa(&s, &r), Some(7.65));
        let mut excl = s.clone();
        excl.fail_counts = false;
        assert_eq!(sgpa(&excl, &r[..3], "s1"), Some(8.55));
    }

    #[test]
    fn custom_bands_parse_and_bad_ones_are_refused() {
        let s = parse_scheme(&json!({"bands": [
            {"grade": "F", "points": 0, "min_marks": 0},
            {"grade": "A", "points": 4, "min_marks": 70}
        ], "fail_counts": false}))
        .unwrap();
        assert_eq!(s.bands[0].grade, "A");
        assert!(!s.fail_counts);
        assert!(parse_scheme(&json!({"bands": []})).is_err());
        assert!(
            parse_scheme(&json!({"bands": [{"grade": "F", "points": 0, "min_marks": 0}]})).is_err()
        );
        assert!(parse_scheme(&json!({"preset": "gpa4"})).is_err());
        assert!(
            parse_scheme(&json!({"bands": [
                {"grade": "A", "points": 4, "min_marks": 70},
                {"grade": "A", "points": 3, "min_marks": 60}
            ]}))
            .is_err()
        );
    }

    #[test]
    fn target_on_the_boundary_is_reachable_without_float_drift() {
        let s = ten_point();
        // 8 points over 1.5 credits fixed, 1.5 credits pending: 8.5 needs exactly 9.
        let sol = solve(&s, &[(1.5, 8.0)], &[("x".into(), 1.5)], 8.5);
        assert!(sol.reachable);
        assert_eq!(sol.plan[0].grade, "A+");
        assert_eq!(sol.projected, Some(8.5));
    }

    #[test]
    fn unreachable_target_reports_the_best_value() {
        let s = ten_point();
        let sol = solve(&s, &[(20.0, 6.0)], &[("x".into(), 4.0)], 9.0);
        assert!(!sol.reachable);
        // (120 + 40) / 24 = 6.67
        assert_eq!(sol.best_reachable, Some(6.67));
        assert!(sol.plan.is_empty());
    }

    #[test]
    fn solver_raises_high_credit_subjects_first() {
        let s = ten_point();
        let sol = solve(&s, &[], &[("a".into(), 1.0), ("b".into(), 4.0)], 4.6);
        assert!(sol.reachable);
        let b = sol.plan.iter().find(|n| n.subject == "b").unwrap();
        let a = sol.plan.iter().find(|n| n.subject == "a").unwrap();
        assert_eq!((b.grade.as_str(), a.grade.as_str()), ("C", "P"), "{sol:?}");
    }

    /// Seeded property loop: reachable means achieved >= target and <= the
    /// all-maximum value; unreachable means the all-maximum value is below it.
    #[test]
    fn solver_property_holds_over_random_inputs() {
        let mut seed: u64 = 0x5eed;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let s = ten_point();
        for _ in 0..2_000 {
            let nf = next(6);
            let mut fixed = Vec::new();
            for _ in 0..nf {
                let c = (1 + next(8)) as f64 * 0.5;
                let p = s.bands[next(s.bands.len() as u64) as usize].points;
                fixed.push((c, p));
            }
            let np = next(5);
            let mut pending = Vec::new();
            for i in 0..np {
                pending.push((format!("s{i}"), (1 + next(8)) as f64 * 0.5));
            }
            let target = (1 + next(1_000)) as f64 / 100.0;
            let sol = solve(&s, &fixed, &pending, target);
            let c_pend: i64 = pending.iter().map(|(_, c)| h(*c)).sum();
            let c_all: i64 = fixed.iter().map(|(c, _)| h(*c)).sum::<i64>() + c_pend;
            if c_all == 0 {
                assert!(!sol.reachable);
                continue;
            }
            let w_fixed: i64 = fixed.iter().map(|(c, p)| h(*c) * h(*p)).sum();
            let best_w = w_fixed + 1000 * c_pend;
            if sol.reachable {
                let got: i64 = w_fixed
                    + sol
                        .plan
                        .iter()
                        .zip(&pending)
                        .map(|(n, (_, c))| h(n.points) * h(*c))
                        .sum::<i64>();
                assert!(got >= h(target) * c_all, "{target} {fixed:?} {pending:?}");
                assert!(got <= best_w);
            } else {
                assert!(
                    best_w < h(target) * c_all,
                    "{target} {fixed:?} {pending:?} {sol:?}"
                );
            }
        }
    }
}
