// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! `lucubra_export`: session brief, Anki deck and calendar exports, plus
//! host-aware stubs for later milestones (`A6`).

use lucubra_core::anki;
use lucubra_core::state::col;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::budget::BudgetLimits;
use crate::envelope::{ProgressEntry, ToolError, ToolErrorCode};
use crate::tools::{
    ToolContext, ToolOutcome, check_keys, check_slug, opt_str, req_str, require_action,
};

/// JSON Schema arguments for `lucubra_export`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    /// Action: brief, pack, anki, ics, dashboard, or mock_test.
    pub action: String,
    /// Subject slug (required for mock_test; optional for others).
    #[serde(default)]
    pub subject: Option<String>,
    /// Session id filter (for brief).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Exam id filter (for mock_test).
    #[serde(default)]
    pub exam_id: Option<String>,
    /// Output format: apkg or tsv (for anki); md (for pack); md or pdf (for mock_test).
    #[serde(default)]
    pub format: Option<String>,
    /// Number of questions (for mock_test, 1 to 60, default 10).
    #[serde(default)]
    pub count: Option<u64>,
    /// Time allowed in minutes (for mock_test, 10 to 480, default 180).
    #[serde(default)]
    pub duration_minutes: Option<u64>,
    /// Optional completed-step checkpoints to log alongside this call.
    #[serde(default)]
    pub progress: Option<Vec<ProgressEntry>>,
}

fn as_card_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_owned))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Every stored practice item of `subject` (or all subjects) as a card.
fn anki_cards(
    state: &lucubra_core::state::State,
    paths: &std::collections::BTreeMap<String, String>,
    subject: Option<&str>,
) -> Vec<anki::AnkiCard> {
    state
        .collection(col::ITEMS)
        .filter(|(_, i)| {
            let unit = i.get("unit").and_then(Value::as_str).unwrap_or_default();
            subject.is_none_or(|s| unit.split('/').next() == Some(s))
        })
        .map(|(id, i)| {
            let mut front = as_card_text(i.get("prompt"));
            if let Some(Value::Array(opts)) = i.get("options") {
                for o in opts {
                    front.push_str(&format!(
                        "
- {}",
                        as_card_text(Some(o))
                    ));
                }
            }
            let source = i
                .get("sources")
                .and_then(Value::as_array)
                .and_then(|s| s.first())
                .map(|s| {
                    let hash = s
                        .get("file_hash")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let loc = s
                        .get("location")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    format!("{} {loc}", paths.get(hash).map_or("", String::as_str))
                })
                .unwrap_or_else(|| "beyond your notes".into());
            let unit = i.get("unit").and_then(Value::as_str).unwrap_or_default();
            anki::AnkiCard {
                id: id.clone(),
                front,
                back: as_card_text(i.get("answer")),
                source: source.trim().to_owned(),
                tags: unit.split('/').map(str::to_owned).collect(),
            }
        })
        .collect()
}

pub fn execute(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    let action = require_action(
        args,
        &["brief", "pack", "anki", "ics", "dashboard", "mock_test"],
    )?;
    check_keys(
        args,
        &[
            "action",
            "subject",
            "session_id",
            "exam_id",
            "format",
            "count",
            "duration_minutes",
            "progress",
        ],
        "lucubra_export",
    )?;
    let format = opt_str(args, "format")?;

    if action == "mock_test" {
        let subj = req_str(
            args,
            "subject",
            "provide 'subject' slug for action 'mock_test'",
        )?;
        check_slug("subject", subj)?;
        if let Some(fmt) = format
            && !matches!(fmt, "md" | "pdf")
        {
            return Err(ToolError::new(
                ToolErrorCode::InvalidArguments,
                "format for action 'mock_test' must be 'md' or 'pdf'",
            ));
        }
        return mock_test(args, ctx, subj, limits);
    }

    if let Some(s) = opt_str(args, "subject")? {
        check_slug("subject", s)?;
    }

    match action {
        "brief" => {
            let state = ctx
                .ws
                .replay()
                .map_err(|e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("failed reading workspace state: {e}"),
                    )
                })?
                .state;
            let requested_sid = opt_str(args, "session_id")?;
            let chosen = if let Some(sid) = requested_sid {
                state
                    .get(col::SESSIONS, sid)
                    .map(|v| (sid.to_owned(), v.clone()))
            } else {
                state
                    .collection(col::SESSIONS)
                    .filter(|(k, _)| k.as_str() != "_unattached")
                    .last()
                    .map(|(k, v)| (k.clone(), v.clone()))
            };
            let markdown = match &chosen {
                Some((sid, s)) => {
                    let mins = s.get("logged_minutes").and_then(Value::as_u64).unwrap_or(0);
                    let steps = s.get("steps_count").and_then(Value::as_u64).unwrap_or(0);
                    let sum = s
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("No summary recorded.");
                    format!(
                        "# Session Brief ({sid})\n\n- Logged minutes: {mins}\n- Steps completed: {steps}\n\n{sum}\n"
                    )
                }
                None => "# Workspace Brief\n\nNo study sessions recorded yet.\n".to_owned(),
            };
            Ok(ToolOutcome {
                data: json!({
                    "session_id": chosen.as_ref().map(|(k, _)| k),
                    "host": ctx.host.profile.id,
                    "export_recipe": ctx.host.profile.export_recipe,
                    "markdown": markdown,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "anki" => {
            let fmt = format.unwrap_or("apkg");
            if !matches!(fmt, "apkg" | "tsv") {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "format for action 'anki' must be 'apkg' or 'tsv'",
                ));
            }
            let subject = opt_str(args, "subject")?;
            if let Some(s) = subject {
                check_slug("subject", s)?;
            }
            let state = ctx
                .ws
                .replay()
                .map_err(|e| ToolError::new(ToolErrorCode::WorkspaceError, e.to_string()))?
                .state;
            let paths = crate::tools::hash_to_path_map(&state);
            let cards = anki_cards(&state, &paths, subject);
            if cards.is_empty() {
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    "no practice items to export yet; store some with lucubra_submit(action=\"items\")",
                ));
            }
            let day = lucubra_core::planner::iso_from_days(crate::tools::practice::today_day());
            let stem = format!("{}_{day}", subject.unwrap_or("all"));
            let rel = format!("exports/anki/{stem}.{fmt}");
            let abs = ctx.ws.root().join(&rel);
            let deck = format!("Lucubra::{}", subject.unwrap_or("All subjects"));
            let written = if fmt == "apkg" {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as i64);
                anki::write_apkg(&abs, &deck, &cards, now).map_err(|e| e.to_string())
            } else {
                std::fs::create_dir_all(abs.parent().unwrap_or(ctx.ws.root()))
                    .and_then(|()| std::fs::write(&abs, anki::tsv(&cards)))
                    .map_err(|e| e.to_string())
            };
            written.map_err(|e| {
                ToolError::new(
                    ToolErrorCode::WorkspaceError,
                    format!("Anki export failed: {e}"),
                )
            })?;
            Ok(ToolOutcome {
                data: json!({
                    "path": rel,
                    "format": fmt,
                    "deck": deck,
                    "cards": cards.len(),
                    "notice": anki::NOTICE,
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "ics" => {
            let rel = "schedule/lucubra-plan.ics";
            if !ctx.ws.root().join(rel).is_file() {
                return Err(ToolError::new(
                    ToolErrorCode::NotFound,
                    "no calendar yet; accept a plan or set an exam date first",
                ));
            }
            Ok(ToolOutcome {
                data: json!({"path": rel, "format": "ics"}),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "pack" => {
            if let Some(fmt) = format
                && fmt != "md"
            {
                return Err(ToolError::new(
                    ToolErrorCode::InvalidArguments,
                    "format for action 'pack' must be 'md'",
                ));
            }
            let ws_fail = |e: lucubra_core::Error| {
                ToolError::new(
                    ToolErrorCode::WorkspaceError,
                    format!("pack build failed: {e}"),
                )
            };
            let state = ctx.ws.replay().map_err(ws_fail)?.state;
            let budget = limits
                .pack_limit
                .unwrap_or(lucubra_pack::build::FREE_BUDGET);
            let previous = lucubra_pack::build::last_fingerprint(&state);
            let pack = lucubra_pack::build::build(
                &state,
                lucubra_pack::build::Options {
                    today: crate::tools::practice::today_day(),
                    budget,
                },
            );
            let fingerprint = lucubra_pack::build::write(ctx.ws, &pack).map_err(ws_fail)?;
            let changed = previous.as_deref() != Some(fingerprint.as_str());
            Ok(ToolOutcome {
                data: json!({
                    "folder": "packs",
                    "files": pack.files.keys().collect::<Vec<_>>(),
                    "tokens": pack.tokens(),
                    "budget": budget,
                    "fingerprint": fingerprint,
                    "changed_since_last_build": changed,
                    "delivery": "Upload the files in packs/ to a project, Gem or skill (at most four files), or let a Drive connector read the folder directly. Re-upload when changed_since_last_build is true.",
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        "dashboard" => {
            lucubra_core::dashboard::write(ctx.ws, crate::tools::practice::today_day()).map_err(
                |e| {
                    ToolError::new(
                        ToolErrorCode::WorkspaceError,
                        format!("dashboard build failed: {e}"),
                    )
                },
            )?;
            Ok(ToolOutcome {
                data: json!({
                    "file": lucubra_core::dashboard::PATH,
                    "self_contained": true,
                    "delivery": "Open dashboard/index.html in a browser. It makes no network request. The same page is at /dashboard on the companion page from lucubra serve.",
                }),
                next: None,
                warnings: Vec::new(),
                per_call_limit: limits.response_limit,
                suppress_wrap_up: false,
                active_session_override: None,
            })
        }
        _ => unreachable!(),
    }
}

fn mock_test(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
    subject: &str,
    limits: BudgetLimits,
) -> std::result::Result<ToolOutcome, ToolError> {
    use lucubra_core::mock;
    let bad = |m: String| ToolError::new(ToolErrorCode::InvalidArguments, m);
    let ws_fail = |e: lucubra_core::Error| {
        ToolError::new(
            ToolErrorCode::WorkspaceError,
            format!("workspace update failed: {e}"),
        )
    };
    let count = crate::tools::opt_u64(
        args,
        "count",
        1,
        60,
        "count must be an integer from 1 to 60",
    )?
    .unwrap_or(10);
    let minutes = crate::tools::opt_u64(
        args,
        "duration_minutes",
        10,
        480,
        "duration_minutes must be an integer from 10 to 480",
    )?
    .unwrap_or(180);
    let state = ctx.ws.replay().map_err(ws_fail)?.state;
    let title = match opt_str(args, "exam_id")? {
        Some(id) => {
            let exam = state
                .get(col::EXAMS, id)
                .ok_or_else(|| bad(format!("exam '{id}' is not in the profile")))?;
            exam.get("title")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_owned()
        }
        None => state
            .get(col::SUBJECTS, subject)
            .and_then(|s| s.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(subject)
            .to_owned(),
    };
    let paper = mock::build(&state, subject, count as usize, minutes as u32).map_err(bad)?;
    let mock_id = format!("mt_{}", ulid::Ulid::new().to_string().to_lowercase());
    let days = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        / 86_400) as i64;
    let date = lucubra_core::planner::iso_from_days(days);
    let root = ctx.ws.root();
    let mut stem = format!("MOCK_{date}_{subject}");
    let mut n = 2;
    while root.join(format!("exports/{stem}.md")).exists() {
        stem = format!("MOCK_{date}_{subject}-{n}");
        n += 1;
    }
    let files = vec![format!("exports/{stem}.md"), format!("exports/{stem}.html")];
    for (rel, body) in [
        (&files[0], mock::markdown(&paper, &title, &mock_id)),
        (&files[1], mock::html(&paper, &title, &mock_id)),
    ] {
        lucubra_core::projection::write_atomic(&root.join(rel), body.as_bytes())
            .map_err(ws_fail)?;
    }
    ctx.ws
        .append(
            "mock_test.exported",
            mock::exported_payload(&paper, &mock_id, &date, &files),
        )
        .map_err(ws_fail)?;
    Ok(ToolOutcome {
        data: json!({
            "mock_id": mock_id,
            "files": files,
            "questions": paper.questions.len(),
            "total_marks": paper.total_marks,
            "duration_minutes": minutes,
            "print": "open the .html file in a browser and print it, or save it as PDF",
            "after": "photograph the answer sheets into inbox/, read them back, then call lucubra_practice(action=\"mock_grade\")",
        }),
        next: Some("print_and_sit".into()),
        warnings: Vec::new(),
        per_call_limit: limits.response_limit,
        suppress_wrap_up: false,
        active_session_override: None,
    })
}
