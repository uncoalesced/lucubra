// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Engine-first file classification (SPEC section 10.3). Deterministic
//! scoring on course codes, path words and first-chunk keywords; files the
//! engine cannot place confidently are batched for the host.

use std::collections::BTreeSet;

use lucubra_core::state::{State, col};
use lucubra_core::{Result, Workspace};
use serde_json::{Value, json};

use crate::pipeline::open_index;

/// Score a subject must reach for the engine to decide alone.
pub const CONFIDENT: u32 = 6;
/// Lead over the runner-up required for an engine decision.
pub const MARGIN: u32 = 3;
/// Score at or above which an engine signal counts as strong (a course-code
/// match); a host answer that contradicts it goes to the user.
pub const STRONG: u32 = 10;
/// Characters of the first chunk shown to the host and scored by the engine.
pub const HEAD_CHARS: usize = 300;

const STOPWORDS: &[&str] = &[
    "and",
    "the",
    "for",
    "with",
    "from",
    "into",
    "intro",
    "introduction",
    "notes",
    "unit",
    "chapter",
    "lecture",
    "part",
    "pdf",
    "docx",
    "pptx",
    "final",
    "copy",
    "new",
];

/// What the engine knows about one subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectSig {
    /// Subject slug.
    pub slug: String,
    /// Normalised course codes, for example `STAT201`.
    pub codes: BTreeSet<String>,
    /// Lowercase keywords from the slug and title.
    pub keywords: BTreeSet<String>,
}

/// One scored subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Subject slug.
    pub subject: String,
    /// Engine score.
    pub score: u32,
}

/// Engine decision for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Confident; classify with `by: engine`.
    Engine(Candidate),
    /// Not confident; ask the host. Candidates are best first.
    NeedsHost(Vec<Candidate>),
}

/// A file the host is asked to classify.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchItem {
    /// Workspace-relative path.
    pub path: String,
    /// Content hash.
    pub hash: String,
    /// First characters of extracted text (may be empty).
    pub head: String,
    /// Candidate subjects, best first.
    pub candidates: Vec<Candidate>,
}

/// Result of one classification pass.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassifyReport {
    /// Files classified by the engine in this pass: (path, subject).
    pub engine: Vec<(String, String)>,
    /// Files left for the host.
    pub batch: Vec<BatchItem>,
}

/// Normalised course codes in `text`: 2 to 5 letters, an optional space,
/// hyphen or underscore, 3 or 4 digits and an optional letter (`STAT 201`,
/// `cs-101a`, `MA2001`).
pub fn codes_in(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        let starts_word = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if !(starts_word && chars[i].is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        let letters = j - i;
        let mut k = j;
        if k < chars.len() && matches!(chars[k], ' ' | '-' | '_') {
            k += 1;
        }
        let d0 = k;
        while k < chars.len() && chars[k].is_ascii_digit() {
            k += 1;
        }
        let digits = k - d0;
        if digits > 0
            && k < chars.len()
            && chars[k].is_ascii_alphabetic()
            && (k + 1 == chars.len() || !chars[k + 1].is_ascii_alphanumeric())
        {
            k += 1;
        }
        let ends_word = k == chars.len() || !chars[k].is_ascii_alphanumeric();
        if (2..=5).contains(&letters) && (3..=4).contains(&digits) && ends_word {
            let code: String = chars[i..j]
                .iter()
                .chain(&chars[d0..k])
                .map(char::to_ascii_uppercase)
                .collect();
            out.insert(code);
            i = k;
        } else {
            i = j.max(i + 1);
        }
    }
    out
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !w.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_lowercase)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Builds subject signatures from replayed state.
pub fn subjects_from_state(state: &State) -> Vec<SubjectSig> {
    state
        .collection(col::SUBJECTS)
        .map(|(slug, data)| {
            let title = data.get("title").and_then(Value::as_str).unwrap_or("");
            let mut codes = codes_in(title);
            codes.extend(codes_in(&slug.replace('-', " ")));
            let listed = data
                .get("codes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .chain(data.get("code"))
                .filter_map(Value::as_str);
            for code in listed {
                codes.extend(codes_in(code));
            }
            let mut keywords = words(&slug.replace(['-', '_'], " "));
            keywords.extend(words(title));
            SubjectSig {
                slug: slug.clone(),
                codes,
                keywords,
            }
        })
        .collect()
}

/// Scores one file against every subject.
///
/// shortcut: keyword overlap stands in for BM25 against confirmed files;
/// upgrade when M10 measurements show misclassification on real material.
pub fn classify(path: &str, head: &str, subjects: &[SubjectSig]) -> Verdict {
    if let Some(slug) = path
        .strip_prefix("subjects/")
        .and_then(|rest| rest.split('/').next())
        .filter(|slug| subjects.iter().any(|s| s.slug == *slug))
    {
        return Verdict::Engine(Candidate {
            subject: slug.to_owned(),
            score: 100,
        });
    }
    let path_codes = codes_in(path);
    let head_codes = codes_in(head);
    let path_words = words(path);
    let head_words = words(head);
    let mut scored: Vec<Candidate> = subjects
        .iter()
        .map(|s| {
            let mut score = 0;
            if !s.codes.is_disjoint(&path_codes) {
                score += 10;
            }
            if !s.codes.is_disjoint(&head_codes) {
                score += 6;
            }
            score += 4 * (s.keywords.intersection(&path_words).count().min(2) as u32);
            score += s.keywords.intersection(&head_words).count().min(4) as u32;
            Candidate {
                subject: s.slug.clone(),
                score,
            }
        })
        .collect();
    scored.sort_by(|a, b| b.score.cmp(&a.score).then(a.subject.cmp(&b.subject)));
    let top = scored.first().map_or(0, |c| c.score);
    let second = scored.get(1).map_or(0, |c| c.score);
    if top >= CONFIDENT && top - second >= MARGIN {
        return Verdict::Engine(scored.swap_remove(0));
    }
    let mut candidates: Vec<Candidate> = scored.iter().filter(|c| c.score > 0).cloned().collect();
    if candidates.is_empty() {
        candidates = scored;
    }
    candidates.truncate(3);
    Verdict::NeedsHost(candidates)
}

fn head_text(index: Option<&crate::index::Index>, hash: &str) -> String {
    let Some(index) = index else {
        return String::new();
    };
    index
        .locators_for_hash(hash, 1)
        .ok()
        .and_then(|locs| locs.into_iter().next())
        .and_then(|loc| index.chunk(hash, &loc).ok().flatten())
        .map(|text| text.chars().take(HEAD_CHARS).collect())
        .unwrap_or_default()
}

/// Engine candidates for one file, best first, for checking a host answer.
pub fn candidates_for(ws: &Workspace, state: &State, path: &str, hash: &str) -> Vec<Candidate> {
    let index = open_index(ws).ok();
    match classify(
        path,
        &head_text(index.as_ref(), hash),
        &subjects_from_state(state),
    ) {
        Verdict::Engine(c) => vec![c],
        Verdict::NeedsHost(c) => c,
    }
}

/// Words in a file name or first heading that mark graded coursework
/// (SPEC tutor mode, S26). The user confirms before tutor mode applies.
const ASSESSMENT_WORDS: &[&str] = &[
    "assignment",
    "homework",
    "coursework",
    "assessment",
    "take home",
    "lab report",
    "problem set",
];

/// Whether a path or heading looks like a graded assessment.
pub fn looks_like_assessment(path: &str, head: &str) -> bool {
    let flat = |s: &str| {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    };
    let file = path.rsplit('/').next().unwrap_or(path);
    let first_line = head.lines().next().unwrap_or_default();
    let text = format!(" {} {} ", flat(file), flat(first_line));
    ASSESSMENT_WORDS
        .iter()
        .any(|w| text.contains(&format!(" {w} ")))
}

/// Files that look like assessments and have no confirmed tag yet, as
/// `(path, hash)`, for the user to confirm or decline.
pub fn assessment_suggestions(ws: &Workspace, state: &State) -> Vec<(String, String)> {
    let index = open_index(ws).ok();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (path, location) in state.collection(col::PATHS) {
        let Some(hash) = location.get("hash").and_then(Value::as_str) else {
            continue;
        };
        let confirmed = state
            .get(col::FILES, hash)
            .and_then(|f| f.get("assessment_confirmed"))
            .is_some();
        if confirmed || !seen.insert(hash.to_owned()) {
            continue;
        }
        if looks_like_assessment(path, &head_text(index.as_ref(), hash)) {
            out.push((path.clone(), hash.to_owned()));
        }
    }
    out
}

/// Classifies every scanned file that has no subject yet. With `write`, the
/// engine's confident decisions are logged as `file.classified` events;
/// either way the rest come back as one host batch.
pub fn classify_pending(ws: &Workspace, write: bool) -> Result<ClassifyReport> {
    let state = ws.replay()?.state;
    let subjects = subjects_from_state(&state);
    let mut report = ClassifyReport::default();
    if subjects.is_empty() {
        return Ok(report);
    }
    let index = open_index(ws).ok();
    let mut seen = BTreeSet::new();
    for (path, location) in state.collection(col::PATHS) {
        let Some(hash) = location.get("hash").and_then(Value::as_str) else {
            continue;
        };
        let classified = state
            .get(col::FILES, hash)
            .and_then(|f| f.get("subject"))
            .is_some();
        if classified || !seen.insert(hash.to_owned()) {
            continue;
        }
        let head = head_text(index.as_ref(), hash);
        match classify(path, &head, &subjects) {
            Verdict::Engine(c) => {
                if write {
                    ws.append(
                        "file.classified",
                        json!({"hash": hash, "subject": c.subject, "by": "engine", "score": c.score}),
                    )?;
                }
                report.engine.push((path.clone(), c.subject));
            }
            Verdict::NeedsHost(candidates) => report.batch.push(BatchItem {
                path: path.clone(),
                hash: hash.to_owned(),
                head,
                candidates,
            }),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(slug: &str, codes: &[&str], title: &str) -> SubjectSig {
        SubjectSig {
            slug: slug.into(),
            codes: codes.iter().map(|c| (*c).into()).collect(),
            keywords: words(&format!("{} {title}", slug.replace('-', " "))),
        }
    }

    #[test]
    fn classify_finds_course_codes_in_common_spellings() {
        let found = codes_in("STAT 201 midterm, cs-101a lab, MA2001_notes, ISBN 9780131103627");
        let want: BTreeSet<String> = ["CS101A", "MA2001", "STAT201"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(found, want);
        assert!(codes_in("COVID19 2026 page12").is_empty());
    }

    #[test]
    fn classify_engine_decides_on_code_and_defers_on_ties() {
        let subjects = [
            sig("stats", &["STAT201"], "Statistics"),
            sig("gst", &["COM305"], "Goods and Services Tax"),
        ];
        assert_eq!(
            classify("inbox/STAT201_week3.pdf", "", &subjects),
            Verdict::Engine(Candidate {
                subject: "stats".into(),
                score: 10
            })
        );
        assert!(matches!(
            classify("inbox/scan_0042.jpg", "", &subjects),
            Verdict::NeedsHost(c) if c.len() == 2
        ));
        assert!(matches!(
            classify("subjects/gst/u1/sources/a.pdf", "", &subjects),
            Verdict::Engine(c) if c.subject == "gst"
        ));
    }

    #[test]
    fn classify_uses_folder_words_without_codes() {
        let subjects = [
            sig("income-tax", &[], "Income Tax"),
            sig("banking", &[], "Law and Practice of Banking"),
        ];
        assert!(matches!(
            classify("Income Tax/IT NOTES.pdf", "", &subjects),
            Verdict::Engine(c) if c.subject == "income-tax"
        ));
    }

    #[test]
    fn classify_assessment_from_name_or_heading() {
        assert!(looks_like_assessment("inbox/assignment 3.pdf", ""));
        assert!(looks_like_assessment("inbox/CS101_Homework-2.docx", ""));
        assert!(looks_like_assessment(
            "inbox/x.pdf",
            "Take-home exam, due Friday
Q1"
        ));
        assert!(looks_like_assessment("subjects/bio/Lab_Report_4.docx", ""));
        assert!(!looks_like_assessment(
            "inbox/lecture 3.pdf",
            "Assignments are due on Moodle"
        ));
        assert!(!looks_like_assessment("inbox/reassignment-notes.pdf", ""));
    }
}
