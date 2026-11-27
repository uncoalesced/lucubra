// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_practice`: FSRS practice queue, engine grading of closed-format
//! answers, and adaptive exam simulation (SPEC sections 13 and 14).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use lucubra_core::grader::{Graded, elo_difficulty, grade, staircase};
use lucubra_core::planner::days_from_iso;
use lucubra_core::srs::{self, Rating};
use lucubra_core::state::{State, col};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, hash_to_path_map, opt_str, opt_u64, req_str,
    require_action,
};

/// JSON Schema arguments for `lucubra_practice`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PracticeArgs {
    /// Action: next, answer, exam_sim, or mock_grade.
    pub action: String,
    /// Optional subject slug filter.
    #[serde(default)]
    pub subject: Option<String>,
    /// Optional unit slug filter.
    #[serde(default)]
    pub unit: Option<String>,
    /// Practice mode: strict (default, your notes only) or extended (adds beyond-notes items).
    #[serde(default)]
    pub mode: Option<String>,
    /// Maximum items in queue (1-20).
    #[serde(default)]
    pub limit: Option<u64>,
    /// Practice item identifier (for answer and exam_sim answers).
    #[serde(default)]
    pub item_id: Option<String>,
    /// User response payload (for answer and exam_sim answers).
    #[serde(default)]
    pub response: Option<Value>,
    /// Optional FSRS self-rating: again, hard, good, easy.
    #[serde(default)]
    pub rating: Option<String>,
    /// Exam id (to start exam_sim).
    #[serde(default)]
    pub exam_id: Option<String>,
    /// Running simulation id (to answer inside exam_sim).
    #[serde(default)]
    pub sim_id: Option<String>,
    /// Timed duration in minutes (5-300, for exam_sim).
    #[serde(default)]
    pub duration_minutes: Option<u64>,
    /// Number of items in exam_sim (1-100).
    #[serde(default)]
    pub count: Option<u64>,
    /// Mock test id from lucubra_export(mock_test) (for mock_grade).
    #[serde(default)]
    pub mock_id: Option<String>,
    /// Answers read back from the photographed sheets (for mock_grade): objects
    /// with `n` (question number), `read_back` (the text as read), `response`
    /// (the answer in the item's format, for closed questions) and `score`
    /// (marks awarded against the rubric, for open questions).
    #[serde(default)]
    pub answers: Option<Vec<Value>>,
    /// Confirm the read-back after the user has checked it (for mock_grade).
    #[serde(default)]
    pub confirm: Option<bool>,
    /// Day to practise as, YYYY-MM-DD (defaults to today, UTC).
    #[serde(default)]
    pub today: Option<String>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn invalid(msg: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorCode::InvalidArguments, msg)
}

fn not_found(msg: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorCode::NotFound, msg)
}

fn ws_err(e: lucubra_core::Error) -> ToolError {
    ToolError::new(
        ToolErrorCode::WorkspaceError,
        format!("practice update failed: {e}"),
    )
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Today as days since the epoch (UTC).
pub fn today_day() -> i64 {
    now_ms() / 86_400_000
}

fn day_arg(args: &Map<String, Value>) -> std::result::Result<i64, ToolError> {
    match opt_str(args, "today")? {
        Some(s) => days_from_iso(s).ok_or_else(|| invalid("today must be a date like 2026-11-24")),
        None => Ok(today_day()),
    }
}

fn mode_arg(args: &Map<String, Value>) -> std::result::Result<&str, ToolError> {
    match opt_str(args, "mode")?.unwrap_or("strict") {
        m @ ("strict" | "extended") => Ok(m),
        _ => Err(invalid("mode must be 'strict' or 'extended'")),
    }
}

fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

/// The next exam day for the item's subject on or after `today`.
fn exam_day_for(state: &State, unit: &str, today: i64) -> Option<i64> {
    let subject = unit.split('/').next()?;
    state
        .collection(col::EXAMS)
        .filter(|(_, e)| text(e, "subject") == Some(subject))
        .filter_map(|(_, e)| days_from_iso(text(e, "date")?))
        .filter(|d| *d >= today)
        .min()
}

/// Reviews the FSRS card for an item, creating it after a wrong answer or an
/// explicit rating; cards come from items the user got wrong (SPEC 14).
pub fn review_card(
    ctx: &ToolContext<'_>,
    state: &State,
    item_id: &str,
    correct: bool,
    score: f64,
    rating: Option<Rating>,
    today: i64,
) -> std::result::Result<Option<Value>, ToolError> {
    let card_id = format!("c_{item_id}");
    let existing = state.get(col::CARDS, &card_id);
    if existing.is_none() && correct && rating.is_none() {
        return Ok(None);
    }
    let unit = state
        .get(col::ITEMS, item_id)
        .and_then(|i| text(i, "unit"))
        .unwrap_or_default();
    let rating = rating.unwrap_or_else(|| Rating::from_score(correct, score));
    let mut fields = srs::review(
        existing.unwrap_or(&json!({})),
        today,
        rating,
        exam_day_for(state, unit, today),
    )
    .map_err(|e| ToolError::new(ToolErrorCode::WorkspaceError, e))?;
    fields["card_id"] = json!(card_id);
    fields["item_id"] = json!(item_id);
    fields["unit"] = json!(unit);
    ctx.ws
        .append("card.reviewed", fields.clone())
        .map_err(ws_err)?;
    Ok(Some(fields))
}

fn in_scope(item: &Value, subject: Option<&str>, unit: Option<&str>, mode: &str) -> bool {
    let key = text(item, "unit").unwrap_or_default();
    let (s, u) = key.split_once('/').unwrap_or((key, ""));
    subject.is_none_or(|x| x == s)
        && unit.is_none_or(|x| x == u)
        && (mode == "extended" || text(item, "mode") != Some("extended"))
}

/// What the learner sees: never the answer.
fn item_view(state: &State, id: &str, item: &Value, reason: &str) -> Value {
    let paths = hash_to_path_map(state);
    let grounding = text(item, "grounding").unwrap_or("verified");
    let label = match grounding {
        "unverified_transcript" => Some("from an unverified transcript"),
        "beyond_notes" => Some("beyond your notes"),
        _ => None,
    };
    let sources: Vec<Value> = item
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| {
            let hash = text(s, "file_hash").unwrap_or_default();
            json!({"path": paths.get(hash), "location": text(s, "location")})
        })
        .collect();
    let mut view = json!({
        "item_id": id,
        "unit": text(item, "unit"),
        "type": text(item, "type"),
        "prompt": text(item, "prompt"),
        "grounding": grounding,
        "label": label,
        "difficulty": item.get("difficulty"),
        "sources": sources,
        "reason": reason,
    });
    for k in ["options", "rubric"] {
        if let Some(v) = item.get(k) {
            view[k] = v.clone();
        }
    }
    view
}

fn outcome(data: Value, next: &str, limits: BudgetLimits) -> ToolOutcome {
    ToolOutcome {
        data,
        next: Some(next.to_owned()),
        warnings: Vec::new(),
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    }
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(args, &["next", "answer", "exam_sim", "mock_grade"])?;
    let state = ctx.ws.replay().map_err(ws_err)?.state;
    match action {
        "next" => next(args, &state, limits),
        "answer" => answer(args, ctx, &state, limits),
        "exam_sim" => exam_sim(args, ctx, &state, limits),
        "mock_grade" => mock_grade(args, ctx, &state, limits),
        _ => unreachable!(),
    }
}

fn next(
    args: &Map<String, Value>,
    state: &State,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &[
            "action", "subject", "unit", "mode", "limit", "today", "progress",
        ],
        "lucubra_practice(next)",
    )?;
    let subject = opt_str(args, "subject")?;
    let unit = opt_str(args, "unit")?;
    for (field, v) in [("subject", subject), ("unit", unit)] {
        if let Some(v) = v {
            check_slug(field, v)?;
        }
    }
    let mode = mode_arg(args)?;
    let limit = opt_u64(
        args,
        "limit",
        1,
        20,
        "limit must be an integer between 1 and 20",
    )?
    .unwrap_or(5) as usize;
    let today = day_arg(args)?;
    let mut due: Vec<(i64, String)> = state
        .collection(col::CARDS)
        .filter_map(|(_, c)| {
            let day = c.get("due_day").and_then(Value::as_i64)?;
            let id = text(c, "item_id")?;
            let item = state.get(col::ITEMS, id)?;
            (day <= today && in_scope(item, subject, unit, mode)).then(|| (day, id.to_owned()))
        })
        .collect();
    due.sort();
    let fresh: Vec<(&String, &Value)> = state
        .collection(col::ITEMS)
        .filter(|(id, i)| {
            !due.iter().any(|(_, d)| d == *id)
                && num(i, "attempts") == 0
                && in_scope(i, subject, unit, mode)
        })
        .collect();
    let queue: Vec<Value> = due
        .iter()
        .filter_map(|(_, id)| Some(item_view(state, id, state.get(col::ITEMS, id)?, "due")))
        .chain(fresh.iter().map(|(id, i)| item_view(state, id, i, "new")))
        .take(limit)
        .collect();
    let mut data = json!({
        "mode": mode,
        "due_count": due.len(),
        "new_count": fresh.len(),
        "queue": queue,
    });
    if queue.is_empty() {
        data["message"] = json!(
            "Nothing to practise yet. Read a unit with lucubra_read, write items that quote it, and store them with lucubra_submit(action=\"items\")."
        );
        return Ok(outcome(data, "submit_items", limits));
    }
    Ok(outcome(data, "answer", limits))
}

fn response_arg<'a>(
    args: &'a Map<String, Value>,
    fix: &str,
) -> std::result::Result<&'a Value, ToolError> {
    args.get("response")
        .filter(|v| !v.is_null())
        .ok_or_else(|| invalid(fix))
}

fn answer(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &[
            "action", "item_id", "response", "rating", "today", "progress",
        ],
        "lucubra_practice(answer)",
    )?;
    let fix = "provide both 'item_id' and 'response' for action 'answer'";
    let item_id = req_str(args, "item_id", fix)?;
    let response = response_arg(args, fix)?;
    let rating = match opt_str(args, "rating")? {
        Some(r) => Some(
            Rating::parse(r)
                .ok_or_else(|| invalid("rating must be one of: again, hard, good, easy"))?,
        ),
        None => None,
    };
    let today = day_arg(args)?;
    let item = state.get(col::ITEMS, item_id).ok_or_else(|| {
        not_found(format!(
            "no practice item '{item_id}'; call lucubra_practice(action=\"next\")"
        ))
    })?;
    let Graded::Engine {
        correct,
        score,
        detail,
    } = grade(item, response)
    else {
        ctx.ws
            .append(
                "item.answered",
                json!({"item_id": item_id, "response": response, "graded_by": "pending"}),
            )
            .map_err(ws_err)?;
        return Ok(outcome(
            json!({
                "item_id": item_id,
                "graded_by": "host",
                "rubric": item.get("rubric"),
                "model_answer": item.get("answer"),
                "message": "Grade this answer against each rubric criterion, then call lucubra_submit(action=\"grades\").",
            }),
            "submit_grades",
            limits,
        ));
    };
    let difficulty = item
        .get("difficulty")
        .and_then(Value::as_f64)
        .unwrap_or(3.0);
    ctx.ws
        .append(
            "item.answered",
            json!({"item_id": item_id, "response": response, "correct": correct,
                   "score": score, "graded_by": "engine",
                   "difficulty": elo_difficulty(difficulty, score, num(item, "attempts") + 1)}),
        )
        .map_err(ws_err)?;
    let card = review_card(ctx, state, item_id, correct, score, rating, today)?;
    Ok(outcome(
        json!({
            "item_id": item_id,
            "correct": correct,
            "score": score,
            "graded_by": "engine",
            "check": detail,
            "answer": item.get("answer"),
            "card_due_day": card.as_ref().and_then(|c| c.get("due_day")),
        }),
        "next",
        limits,
    ))
}

const CLOSED: &[&str] = &[
    "mcq_single",
    "mcq_multi",
    "cloze",
    "true_false",
    "matching",
    "numeric",
    "ordering",
    "formula_recall",
];

/// Closed-format items of `subject` for a simulation, not yet served.
fn pool<'a>(
    state: &'a State,
    subject: &str,
    mode: &str,
    served: &[String],
) -> Vec<(&'a String, &'a Value)> {
    state
        .collection(col::ITEMS)
        .filter(|(id, i)| {
            !served.contains(id)
                && in_scope(i, Some(subject), None, mode)
                && text(i, "type").is_some_and(|t| CLOSED.contains(&t))
        })
        .collect()
}

/// The next simulation item. With past papers, the unit furthest below its
/// share of past-paper marks goes first, and the types those papers used are
/// preferred (S51); then the difficulty nearest the staircase step wins.
fn pick<'a>(
    state: &State,
    subject: &str,
    items: &[(&'a String, &'a Value)],
    served: &[String],
    step: u8,
) -> Option<(&'a String, &'a Value)> {
    let (shares, kinds) = lucubra_core::papers::blueprint(state, subject);
    if shares.is_empty() {
        return nearest(items, step);
    }
    let unit_of = |v: &Value| text(v, "unit").unwrap_or_default().to_owned();
    let mut done: BTreeMap<String, f64> = BTreeMap::new();
    for id in served {
        if let Some(i) = state.get(col::ITEMS, id) {
            *done.entry(unit_of(i)).or_default() += 1.0;
        }
    }
    let total = served.len() as f64 + 1.0;
    let deficit = |u: &str| {
        shares.get(u).copied().unwrap_or(0.0) * total - done.get(u).copied().unwrap_or(0.0)
    };
    let unit = items
        .iter()
        .map(|(_, v)| unit_of(v))
        .max_by(|a, b| deficit(a).total_cmp(&deficit(b)).then_with(|| b.cmp(a)))?;
    let in_unit: Vec<_> = items
        .iter()
        .copied()
        .filter(|(_, v)| unit_of(v) == unit)
        .collect();
    let typed: Vec<_> = in_unit
        .iter()
        .copied()
        .filter(|(_, v)| text(v, "type").is_some_and(|t| kinds.contains(t)))
        .collect();
    nearest(if typed.is_empty() { &in_unit } else { &typed }, step)
}

/// The item whose difficulty is nearest the staircase step.
fn nearest<'a>(items: &[(&'a String, &'a Value)], step: u8) -> Option<(&'a String, &'a Value)> {
    let gap = |v: &Value| {
        (v.get("difficulty").and_then(Value::as_f64).unwrap_or(3.0) - f64::from(step)).abs()
    };
    items
        .iter()
        .copied()
        .min_by(|(a, x), (b, y)| gap(x).total_cmp(&gap(y)).then_with(|| a.cmp(b)))
}

fn exam_sim(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &[
            "action",
            "subject",
            "exam_id",
            "mode",
            "duration_minutes",
            "count",
            "sim_id",
            "item_id",
            "response",
            "today",
            "progress",
        ],
        "lucubra_practice(exam_sim)",
    )?;
    let mode = mode_arg(args)?;
    let today = day_arg(args)?;
    match opt_str(args, "sim_id")? {
        None => start_sim(args, ctx, state, mode, limits),
        Some(sim_id) => answer_sim(args, ctx, state, sim_id, today, limits),
    }
}

fn start_sim(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    mode: &str,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let exam_id = opt_str(args, "exam_id")?;
    let exam = match exam_id {
        Some(id) => Some(
            state
                .get(col::EXAMS, id)
                .ok_or_else(|| not_found(format!("no exam '{id}'")))?,
        ),
        None => None,
    };
    let subject = opt_str(args, "subject")?
        .or_else(|| exam.and_then(|e| text(e, "subject")))
        .ok_or_else(|| invalid("provide 'subject' or 'exam_id' to start an exam simulation"))?;
    check_slug("subject", subject)?;
    let duration = opt_u64(
        args,
        "duration_minutes",
        5,
        300,
        "duration_minutes must be an integer between 5 and 300",
    )?
    .unwrap_or(30);
    let wanted = opt_u64(
        args,
        "count",
        1,
        100,
        "count must be an integer between 1 and 100",
    )?
    .unwrap_or(10);
    let items = pool(state, subject, mode, &[]);
    let (first_id, first) = pick(state, subject, &items, &[], 3).ok_or_else(|| {
        not_found(format!(
            "no closed-format practice items for '{subject}' yet; store some with lucubra_submit(action=\"items\")"
        ))
    })?;
    let count = wanted.min(items.len() as u64);
    let sim_id = format!("sim_{}", ulid::Ulid::new().to_string().to_lowercase());
    ctx.ws
        .append(
            "exam_sim.started",
            json!({"sim_id": sim_id, "subject": subject, "exam_id": exam_id, "mode": mode,
                   "count": count, "duration_minutes": duration,
                   "deadline_ms": now_ms() + duration as i64 * 60_000,
                   "step": 3, "answered": 0, "correct_count": 0, "served": [first_id]}),
        )
        .map_err(ws_err)?;
    Ok(outcome(
        json!({
            "sim_id": sim_id,
            "count": count,
            "duration_minutes": duration,
            "rules": "Timed, no hints. Answer each item with lucubra_practice(action=\"exam_sim\", sim_id, item_id, response).",
            "item": item_view(state, first_id, first, "exam_sim"),
            "position": 1,
        }),
        "exam_sim",
        limits,
    ))
}

fn answer_sim(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    sim_id: &str,
    today: i64,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let sim = state
        .get(col::EXAM_SIMS, sim_id)
        .ok_or_else(|| not_found(format!("no exam simulation '{sim_id}'")))?;
    if text(sim, "status") != Some("active") {
        return Err(invalid(format!(
            "exam simulation '{sim_id}' has already finished"
        )));
    }
    let fix = "provide 'item_id' and 'response' to answer inside the simulation";
    let item_id = req_str(args, "item_id", fix)?;
    let response = response_arg(args, fix)?;
    let served: Vec<String> = sim
        .get("served")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let current = served.last().map_or("", String::as_str);
    if current != item_id {
        return Err(invalid(format!(
            "answer the current item '{current}' first"
        )));
    }
    let item = state
        .get(col::ITEMS, item_id)
        .ok_or_else(|| not_found(format!("no practice item '{item_id}'")))?;
    let (correct, score) = match grade(item, response) {
        Graded::Engine { correct, score, .. } => (correct, score),
        Graded::Host => (false, 0.0),
    };
    let step = staircase(num(sim, "step").clamp(1, 5) as u8, correct);
    let answered = num(sim, "answered") + 1;
    let correct_count = num(sim, "correct_count") + u64::from(correct);
    let timed_out = sim
        .get("deadline_ms")
        .and_then(Value::as_i64)
        .is_some_and(|d| now_ms() > d);
    let upcoming = if answered >= num(sim, "count") || timed_out {
        None
    } else {
        let subject = text(sim, "subject").unwrap_or_default();
        let sim_mode = text(sim, "mode").unwrap_or("strict");
        pick(
            state,
            subject,
            &pool(state, subject, sim_mode, &served),
            &served,
            step,
        )
    };
    let mut served_after = served.clone();
    if let Some((id, _)) = upcoming {
        served_after.push(id.clone());
    }
    let difficulty = item
        .get("difficulty")
        .and_then(Value::as_f64)
        .unwrap_or(3.0);
    ctx.ws
        .append(
            "item.answered",
            json!({"item_id": item_id, "response": response, "correct": correct, "score": score,
                   "graded_by": "engine",
                   "difficulty": elo_difficulty(difficulty, score, num(item, "attempts") + 1),
                   "sim_id": sim_id, "sim_step": step, "sim_answered": answered,
                   "sim_correct_count": correct_count, "sim_served": served_after}),
        )
        .map_err(ws_err)?;
    review_card(ctx, state, item_id, correct, score, None, today)?;
    if let Some((id, it)) = upcoming {
        return Ok(outcome(
            json!({"sim_id": sim_id, "item": item_view(state, id, it, "exam_sim"),
                   "position": answered + 1, "count": num(sim, "count")}),
            "exam_sim",
            limits,
        ));
    }
    let result = correct_count as f64 / answered.max(1) as f64;
    let summary = json!({"sim_id": sim_id, "answered": answered,
                         "correct_count": correct_count, "score": result,
                         "final_step": step, "timed_out": timed_out});
    ctx.ws
        .append("exam_sim.completed", summary.clone())
        .map_err(ws_err)?;
    let mut data = summary;
    data["completed"] = json!(true);
    Ok(outcome(data, "review_results", limits))
}

/// Longest read-back kept per question.
const READ_BACK_MAX_CHARS: usize = 4_000;

/// Grades a sat mock test (S53). Closed questions are graded by the engine
/// from `response`; open questions take the host's rubric `score`. The
/// read-back stays unverified until a later call with `confirm: true`.
fn mock_grade(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    state: &State,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    check_keys(
        args,
        &["action", "mock_id", "answers", "confirm", "progress"],
        "lucubra_practice(mock_grade)",
    )?;
    let bad = |m: String| ToolError::new(ToolErrorCode::InvalidArguments, m);
    let mock_id = req_str(
        args,
        "mock_id",
        "provide 'mock_id' from lucubra_export(action=\"mock_test\")",
    )?;
    let mock = state.get(col::MOCK_TESTS, mock_id).ok_or_else(|| {
        bad(format!(
            "mock test '{mock_id}' was not exported in this workspace"
        ))
    })?;
    let confirm =
        crate::tools::opt_bool(args, "confirm", "confirm must be true or false")?.unwrap_or(false);
    let answers = args.get("answers");
    if answers.is_none() {
        let Some(graded) = mock.get("graded").filter(|_| confirm) else {
            return Err(bad(
                "provide 'answers', or 'confirm: true' once the user has checked the read-back"
                    .into(),
            ));
        };
        let mut done = graded.clone();
        done["mock_id"] = json!(mock_id);
        done["verified"] = json!(true);
        ctx.ws.append("mock_test.graded", done).map_err(ws_err)?;
        return Ok(outcome(
            json!({"mock_id": mock_id, "verified": true}),
            "review_weak_units",
            limits,
        ));
    }
    let answers = answers.and_then(Value::as_array).ok_or_else(|| {
        bad("answers must be an array of {n, read_back, response?, score?}".into())
    })?;
    let mut by_n: BTreeMap<u64, &Map<String, Value>> = BTreeMap::new();
    for (i, a) in answers.iter().enumerate() {
        let obj = a
            .as_object()
            .ok_or_else(|| bad(format!("answers[{i}] must be an object")))?;
        let n = obj
            .get("n")
            .and_then(Value::as_u64)
            .ok_or_else(|| bad(format!("answers[{i}].n must be the question number")))?;
        if by_n.insert(n, obj).is_some() {
            return Err(bad(format!("question {n} is answered twice")));
        }
    }
    let questions = mock
        .get("questions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for n in by_n.keys() {
        if !questions
            .iter()
            .any(|q| q.get("n").and_then(Value::as_u64) == Some(*n))
        {
            return Err(bad(format!("question {n} is not on paper {mock_id}")));
        }
    }
    let (mut breakdown, mut awarded_total, mut max_total) = (Vec::new(), 0.0, 0.0);
    for q in &questions {
        let n = q.get("n").and_then(Value::as_u64).unwrap_or(0);
        let marks = q.get("marks").and_then(Value::as_f64).unwrap_or(0.0);
        let item_id = text(q, "item_id").unwrap_or_default();
        let item = state
            .get(col::ITEMS, item_id)
            .cloned()
            .unwrap_or(Value::Null);
        let answer = by_n.get(&n);
        let read_back: Option<String> = answer
            .and_then(|a| a.get("read_back"))
            .and_then(Value::as_str)
            .map(|s| s.chars().take(READ_BACK_MAX_CHARS).collect());
        let (awarded, graded_by) = match answer {
            None => (0.0, "unanswered"),
            Some(a) => match a.get("response").map(|r| grade(&item, r)) {
                Some(Graded::Engine { score, .. }) => {
                    ((score * marks * 100.0).round() / 100.0, "engine")
                }
                _ => {
                    let score = a.get("score").and_then(Value::as_f64).ok_or_else(|| {
                        bad(format!("question {n} is an open question; give 'score' (0 to {marks}) from its rubric"))
                    })?;
                    if !(0.0..=marks).contains(&score) {
                        return Err(bad(format!("question {n} score must be from 0 to {marks}")));
                    }
                    (score, "rubric")
                }
            },
        };
        awarded_total += awarded;
        max_total += marks;
        breakdown.push(json!({
            "n": n, "item_id": item_id, "marks": marks, "awarded": awarded,
            "graded_by": graded_by, "read_back": read_back,
        }));
    }
    let round = |x: f64| (x * 100.0).round() / 100.0;
    ctx.ws
        .append(
            "mock_test.graded",
            json!({
                "mock_id": mock_id,
                "verified": confirm,
                "breakdown": breakdown,
                "awarded": round(awarded_total),
                "total_marks": round(max_total),
            }),
        )
        .map_err(ws_err)?;
    let summary: Vec<Value> = breakdown
        .iter()
        .map(|b| json!({"n": b["n"], "awarded": b["awarded"], "marks": b["marks"], "graded_by": b["graded_by"]}))
        .collect();
    Ok(outcome(
        json!({
            "mock_id": mock_id,
            "awarded": round(awarded_total),
            "total_marks": round(max_total),
            "verified": confirm,
            "per_question": summary,
            "note": if confirm { "read-back confirmed" } else { "the read-back is unverified; show it to the user, then call again with confirm: true" },
        }),
        if confirm {
            "review_weak_units"
        } else {
            "confirm_read_back"
        },
        limits,
    ))
}
