// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Printable mock tests (SPEC round 4, A6, D41, S53).
//!
//! A paper is built from the past-paper blueprint, or from an even spread over
//! the subject's units when there are no past papers. It is written as
//! Markdown and as an HTML page with print styles. The HTML has no script and
//! no external reference, so it opens offline and prints to PDF from any
//! browser. The answer key never appears on the paper.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::papers;
use crate::state::{State, col};

/// Engine-gradable item types (one mark by default).
pub const CLOSED: &[&str] = &[
    "mcq_single",
    "mcq_multi",
    "cloze",
    "true_false",
    "matching",
    "numeric",
    "ordering",
    "formula_recall",
];

/// Default marks for an open question without past-paper marks.
pub const OPEN_MARKS: f64 = 5.0;

/// One question on the paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    /// 1-based question number.
    pub n: usize,
    /// Item id.
    pub item_id: String,
    /// `<subject>/<unit>`.
    pub unit: String,
    /// Item type.
    pub kind: String,
    /// Question text.
    pub prompt: String,
    /// Answer options, if the item has any.
    pub options: Vec<String>,
    /// Marks.
    pub marks: f64,
}

/// A built paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    /// Subject slug.
    pub subject: String,
    /// Questions in order.
    pub questions: Vec<Question>,
    /// Sum of marks.
    pub total_marks: f64,
    /// Time allowed in minutes.
    pub duration_minutes: u32,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

/// Marks for an item: past-paper marks, else 1 for closed and 5 for open types.
pub fn marks(item: &Value) -> f64 {
    match papers::past_paper(item) {
        Some((_, m)) => m,
        None if s(item, "type").is_some_and(|t| CLOSED.contains(&t.as_str())) => 1.0,
        None => OPEN_MARKS,
    }
}

/// Builds a paper of up to `count` questions for `subject`.
pub fn build(
    state: &State,
    subject: &str,
    count: usize,
    duration_minutes: u32,
) -> Result<Paper, String> {
    let prefix = format!("{subject}/");
    let mut by_unit: BTreeMap<String, Vec<(&String, &Value)>> = BTreeMap::new();
    for (id, item) in state.collection(col::ITEMS) {
        let Some(unit) = s(item, "unit").filter(|u| u.starts_with(&prefix)) else {
            continue;
        };
        let solution = s(item, "type").as_deref() == Some("solution")
            || item.get("solution").and_then(Value::as_bool) == Some(true);
        if solution || s(item, "prompt").is_none_or(|p| p.trim().is_empty()) {
            continue;
        }
        by_unit.entry(unit).or_default().push((id, item));
    }
    if by_unit.is_empty() {
        return Err(format!(
            "subject '{subject}' has no practice items yet; submit items first"
        ));
    }
    // Past-paper questions first (latest year), then easier first, then by id.
    let year = |v: &Value| papers::past_paper(v).map_or(0, |(y, _)| y);
    let diff = |v: &Value| v.get("difficulty").and_then(Value::as_f64).unwrap_or(3.0);
    for list in by_unit.values_mut() {
        list.sort_by(|(a, x), (b, y)| {
            year(y)
                .cmp(&year(x))
                .then(diff(x).total_cmp(&diff(y)))
                .then(a.cmp(b))
        });
    }
    let (mut shares, _) = papers::blueprint(state, subject);
    shares.retain(|u, _| by_unit.contains_key(u));
    if shares.is_empty() {
        let even = 1.0 / by_unit.len() as f64;
        shares = by_unit.keys().map(|u| (u.clone(), even)).collect();
    }
    let available: usize = by_unit.values().map(Vec::len).sum();
    let count = count.min(available);
    let mut chosen: Vec<(&String, &Value)> = Vec::new();
    let mut used = BTreeSet::new();
    for (unit, n) in papers::allocate(&shares, count) {
        for (id, item) in by_unit[&unit].iter().take(n) {
            used.insert(*id);
            chosen.push((id, item));
        }
    }
    // A unit with too few items leaves room; fill it from the rest in unit order.
    for list in by_unit.values() {
        for (id, item) in list {
            if chosen.len() >= count {
                break;
            }
            if used.insert(*id) {
                chosen.push((id, item));
            }
        }
    }
    let questions: Vec<Question> = chosen
        .into_iter()
        .enumerate()
        .map(|(i, (id, item))| Question {
            n: i + 1,
            item_id: id.clone(),
            unit: s(item, "unit").unwrap_or_default(),
            kind: s(item, "type").unwrap_or_default(),
            prompt: s(item, "prompt").unwrap_or_default(),
            options: ["options", "choices"]
                .iter()
                .find_map(|k| item.get(*k).and_then(Value::as_array))
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            marks: marks(item),
        })
        .collect();
    let total_marks = questions.iter().map(|q| q.marks).sum();
    Ok(Paper {
        subject: subject.to_owned(),
        questions,
        total_marks,
        duration_minutes,
    })
}

fn fmt_marks(m: f64) -> String {
    if m.fract() == 0.0 {
        format!("{m:.0}")
    } else {
        format!("{m}")
    }
}

fn unit_word(m: f64) -> &'static str {
    if m == 1.0 { "mark" } else { "marks" }
}

const INSTRUCTION: &str = "Answer on paper. Write the question number beside each answer, then photograph the sheets into";

/// The paper as Markdown.
pub fn markdown(p: &Paper, title: &str, mock_id: &str) -> String {
    let mut out = format!(
        "# Mock test: {title}\n\nTime allowed: {} minutes. Total: {} marks. Paper {mock_id}.\n\n{INSTRUCTION} `inbox/`.\n",
        p.duration_minutes,
        fmt_marks(p.total_marks)
    );
    for q in &p.questions {
        out.push_str(&format!(
            "\n## Question {} ({} {})\n\n{}\n",
            q.n,
            fmt_marks(q.marks),
            unit_word(q.marks),
            q.prompt.trim()
        ));
        for (i, o) in q.options.iter().enumerate() {
            out.push_str(&format!("\n{}. {o}", (b'a' + (i % 26) as u8) as char));
        }
        if !q.options.is_empty() {
            out.push('\n');
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = "body{font-family:Georgia,'Times New Roman',serif;max-width:46rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#000;background:#fff}header{border-bottom:2px solid #000;margin-bottom:1rem}h1{font-size:1.5rem}h2{font-size:1.05rem;margin:1.2rem 0 .3rem}h2 span{font-weight:normal}section{break-inside:avoid}@media print{body{margin:0;max-width:none}@page{margin:18mm}}";

/// The paper as a self-contained HTML page styled for printing.
pub fn html(p: &Paper, title: &str, mock_id: &str) -> String {
    let mut body = String::new();
    for q in &p.questions {
        body.push_str(&format!(
            "<section><h2>Question {} <span>({} {})</span></h2><p>{}</p>",
            q.n,
            fmt_marks(q.marks),
            unit_word(q.marks),
            esc(q.prompt.trim()).replace('\n', "<br>")
        ));
        if !q.options.is_empty() {
            body.push_str("<ol type=\"a\">");
            for o in &q.options {
                body.push_str(&format!("<li>{}</li>", esc(o)));
            }
            body.push_str("</ol>");
        }
        body.push_str("</section>\n");
    }
    let t = esc(title);
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><title>Mock test: {t}</title>\n<style>{STYLE}</style></head>\n<body><header><h1>Mock test: {t}</h1><p>Time allowed: {} minutes. Total: {} marks. Paper {}.</p><p>{INSTRUCTION} <code>inbox/</code>.</p></header>\n{body}</body></html>\n",
        p.duration_minutes,
        fmt_marks(p.total_marks),
        esc(mock_id),
    )
}

/// The `mock_test.exported` payload: ids and marks only, no prompts or answers.
pub fn exported_payload(p: &Paper, mock_id: &str, date: &str, files: &[String]) -> Value {
    let questions: Vec<Value> = p
        .questions
        .iter()
        .map(|q| json!({"n": q.n, "item_id": q.item_id, "marks": q.marks}))
        .collect();
    json!({
        "mock_id": mock_id,
        "subject": p.subject,
        "date": date,
        "duration_minutes": p.duration_minutes,
        "total_marks": p.total_marks,
        "questions": questions,
        "files": files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;

    fn state() -> State {
        let mut st = State::default();
        let items = [
            json!({"item_id": "a", "unit": "law/torts", "type": "long_answer", "prompt": "Explain <duty> & care.", "answer": "x", "past_paper": {"year": 2024, "marks": 15}}),
            json!({"item_id": "b", "unit": "law/torts", "type": "mcq_single", "prompt": "Pick one.", "answer": "x", "options": ["Yes", "No"]}),
            json!({"item_id": "c", "unit": "law/contract", "type": "short_answer", "prompt": "Define offer.", "answer": "x", "past_paper": {"year": 2023, "marks": 5}}),
            json!({"item_id": "d", "unit": "law/contract", "type": "solution", "prompt": "Worked answer.", "answer": "x"}),
        ];
        for (i, data) in items.into_iter().enumerate() {
            st.apply(&Event {
                v: 1,
                id: format!("e{i}"),
                dev: "d-a".into(),
                seq: i as u64 + 1,
                hlc: "1762872000000-0000-d-a".into(),
                ts: String::new(),
                kind: "item.created".into(),
                data,
            })
            .unwrap();
        }
        st
    }

    #[test]
    fn paper_follows_the_blueprint_and_hides_answers() {
        let st = state();
        let p = build(&st, "law", 2, 90).unwrap();
        // torts holds 15 of the 20 past-paper marks: one question each at
        // count 2, and the 2024 past-paper question leads its unit.
        let ids: Vec<&str> = p.questions.iter().map(|q| q.item_id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a"]);
        assert_eq!(p.total_marks, 20.0);
        let all = build(&st, "law", 10, 90).unwrap();
        assert_eq!(
            all.questions.len(),
            3,
            "a solution item is never on a paper"
        );
        assert_eq!(marks(st.get(col::ITEMS, "b").unwrap()), 1.0);

        let md = markdown(&all, "Law", "mt_1");
        assert!(md.contains("## Question 1 (5 marks)"), "{md}");
        assert!(md.contains("a. Yes"));
        let page = html(&all, "Law", "mt_1");
        assert!(page.contains("Explain &lt;duty&gt; &amp; care."));
        assert!(!page.contains("<script") && !page.contains("http"));
        assert!(page.contains("@media print"));
        let ev = exported_payload(&all, "mt_1", "2026-11-27", &[]);
        assert!(
            !ev.to_string().contains("Explain"),
            "no prompts in the event"
        );
        assert!(build(&st, "maths", 5, 60).is_err());
    }
}
