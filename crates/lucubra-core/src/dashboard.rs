// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Static dashboard (SPEC 19): `dashboard/index.html`, rebuilt at session end,
//! and the same body served by the companion page.
//!
//! The page has no script and no external reference, so opening it makes no
//! request. Every coloured bar carries its value as text, so colour is never
//! the only signal.

use std::path::PathBuf;

use serde_json::Value;

use crate::error::Result;
use crate::metrics;
use crate::planner::PlannerConfig;
use crate::projection::write_atomic;
use crate::replay::Replay;
use crate::workspace::Workspace;

/// Workspace-relative path of the static snapshot.
pub const PATH: &str = "dashboard/index.html";

/// Styles for the dashboard body, in the companion page palette (Graphite,
/// Porcelain, Watermelon for fills only).
const STYLE: &str = ".dash table{border-collapse:collapse;width:100%}\
.dash th,.dash td{text-align:left;padding:.25rem .5rem;border-bottom:1px solid #555}\
.dash .bar{display:inline-block;vertical-align:middle;width:10rem;height:.8rem;background:#555;border-radius:2px;margin-right:.5rem}\
.dash .fill{display:block;height:100%;background:#EF2D56;border-radius:2px}\
.dash .note{color:#ccc;font-size:.9rem}";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A bar for a share in [0, 1], followed by the value as text.
fn bar(share: f64) -> String {
    let pct = (share.clamp(0.0, 1.0) * 100.0).round();
    format!(
        "<span class=\"bar\" aria-hidden=\"true\"><span class=\"fill\" style=\"width:{pct}%\"></span></span>{pct}%"
    )
}

fn section(title: &str, inner: &str) -> String {
    format!(
        "<section class=\"card\"><h2>{}</h2>{inner}</section>",
        esc(title)
    )
}

fn table(head: &[&str], rows: &[Vec<String>]) -> String {
    let th: String = head
        .iter()
        .map(|h| format!("<th scope=\"col\">{}</th>", esc(h)))
        .collect();
    let tr: String = rows
        .iter()
        .map(|r| {
            format!(
                "<tr>{}</tr>",
                r.iter()
                    .map(|c| format!("<td>{c}</td>"))
                    .collect::<String>()
            )
        })
        .collect();
    format!("<table><thead><tr>{th}</tr></thead><tbody>{tr}</tbody></table>")
}

fn none(msg: &str) -> String {
    format!("<p class=\"note\">{}</p>", esc(msg))
}

/// The dashboard body: a `<style>` block and the metric sections.
pub fn body(replay: &Replay, today: i64) -> String {
    let cfg = PlannerConfig::default();
    let state = &replay.state;
    let mut out = format!("<style>{STYLE}</style><div class=\"dash\"><h1>Dashboard</h1>");

    let exams = metrics::readiness_per_exam(state, today);
    let rows: Vec<Vec<String>> = exams
        .iter()
        .map(|e| {
            let text = |k: &str| esc(e.get(k).and_then(Value::as_str).unwrap_or("-"));
            vec![
                text("exam"),
                text("subject"),
                text("date"),
                e.get("readiness")
                    .and_then(Value::as_f64)
                    .map_or_else(|| "no practice yet".to_owned(), bar),
            ]
        })
        .collect();
    out += &section(
        "Readiness per exam",
        &if rows.is_empty() {
            none("No exams set yet.")
        } else {
            table(&["Exam", "Subject", "Date", "Readiness"], &rows)
                + &none("Readiness is coverage times mastery, from strict items only.")
        },
    );

    let weeks = metrics::consistency(replay, today, 4);
    let rows: Vec<Vec<String>> = weeks
        .iter()
        .map(|w| {
            let n = |k: &str| w.get(k).and_then(Value::as_u64).unwrap_or(0).to_string();
            vec![
                esc(w.get("week").and_then(Value::as_str).unwrap_or("")),
                n("sessions"),
                n("minutes"),
                n("planned_minutes"),
            ]
        })
        .collect();
    let adherence = metrics::plan_adherence(replay, today).map_or_else(
        || "Plan adherence: no past planned days yet.".to_owned(),
        |(share, days)| {
            format!(
                "Plan adherence: {}% over {days} planned days.",
                (share * 100.0).round()
            )
        },
    );
    out += &section(
        "Study consistency",
        &(table(
            &["Week of", "Sessions", "Minutes", "Planned minutes"],
            &rows,
        ) + &none(&adherence)),
    );

    let minutes = metrics::time_per_subject(replay);
    let total: u64 = minutes.values().sum();
    let rows: Vec<Vec<String>> = minutes
        .iter()
        .map(|(s, m)| {
            let share = if total > 0 {
                *m as f64 / total as f64
            } else {
                0.0
            };
            vec![esc(s), format!("{m} min"), bar(share)]
        })
        .collect();
    out += &section(
        "Time per subject",
        &if rows.is_empty() {
            none("No study time logged yet.")
        } else {
            table(&["Subject", "Minutes", "Share"], &rows)
        },
    );

    let rows: Vec<Vec<String>> = metrics::mastery_per_unit(state, today)
        .into_iter()
        .map(|(u, m)| vec![esc(&u), m.map_or_else(|| "no practice yet".to_owned(), bar)])
        .collect();
    out += &section(
        "Mastery per unit",
        &if rows.is_empty() {
            none("No units yet.")
        } else {
            table(&["Unit", "Mastery"], &rows)
        },
    );

    let rows: Vec<Vec<String>> = metrics::retention_curve(replay)
        .iter()
        .filter(|b| b.get("reviews").and_then(Value::as_u64).unwrap_or(0) > 0)
        .map(|b| {
            vec![
                esc(b.get("interval").and_then(Value::as_str).unwrap_or("")),
                b.get("reviews")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .to_string(),
                bar(b.get("recalled").and_then(Value::as_f64).unwrap_or(0.0)),
            ]
        })
        .collect();
    out += &section(
        "Retention curve",
        &if rows.is_empty() {
            none("Not enough repeat reviews yet.")
        } else {
            table(&["Days since last review", "Reviews", "Recalled"], &rows)
        },
    );

    out += &section(
        "Capacity and focus",
        &format!(
            "<p>Learned daily capacity: {} minutes.</p><p>Focus span: {} minutes.</p>",
            metrics::capacity(&cfg, state),
            metrics::focus_span(&cfg, replay).round()
        ),
    );

    let sleep = metrics::sleep_view(replay);
    let n = sleep
        .get("paired_days")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let sleep_html = if sleep.get("available").and_then(Value::as_bool) == Some(true) {
        let r = sleep
            .get("correlation_sleep_recall")
            .and_then(Value::as_f64)
            .map_or_else(
                || "not defined (no variation)".to_owned(),
                |r| format!("{r:.2}"),
            );
        format!(
            "<p>Correlation between hours slept and the share of reviews recalled that day: {r}, over {n} days.</p><p><strong>This is an {}.</strong></p>",
            metrics::SLEEP_CAVEAT
        )
    } else {
        none(&format!(
            "Not enough data yet: {n} of {} days have both a check-in and reviews.",
            metrics::SLEEP_MIN_DAYS
        ))
    };
    out += &section("Sleep and retention", &sleep_html);
    out += "</div>";
    out
}

/// The standalone page written to `dashboard/index.html`.
pub fn document(replay: &Replay, today: i64) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Lucubra dashboard</title><style>body{{background:#333333;color:#FDFFFC;font-family:system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem;line-height:1.5}}.card{{border:1px solid #555;border-radius:6px;padding:1rem;margin:1rem 0}}</style></head><body><main>{}</main></body></html>\n",
        body(replay, today)
    )
}

/// Rebuilds `dashboard/index.html` from the workspace and returns its path.
pub fn write(ws: &Workspace, today: i64) -> Result<PathBuf> {
    let replay = ws.replay()?;
    let path = ws.root().join(PATH);
    write_atomic(&path, document(&replay, today).as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::replay::replay;
    use serde_json::json;

    fn ev(n: u64, kind: &str, data: Value) -> Event {
        Event {
            v: 1,
            id: format!("e{n}"),
            dev: "d-a".into(),
            seq: n,
            hlc: format!("{:013}-0000-d-a", 1_762_000_000_000u64 + n),
            ts: "2026-11-23T09:00:00Z".into(),
            kind: kind.into(),
            data,
        }
    }

    #[test]
    fn dashboard_is_static_labelled_and_escaped() {
        let r = replay(vec![
            ev(1, "unit.created", json!({"subject": "ma", "unit": "<u1>"})),
            ev(
                2,
                "item.created",
                json!({"item_id": "i1", "unit": "ma/<u1>", "mode": "strict"}),
            ),
            ev(3, "item.answered", json!({"item_id": "i1", "score": 0.5})),
            ev(
                4,
                "exam.set",
                json!({"id": "x1", "subject": "ma", "date": "2026-12-10"}),
            ),
            ev(
                5,
                "step.completed",
                json!({"unit": "ma/<u1>", "minutes": 30}),
            ),
        ]);
        let html = document(&r, 20_416);
        assert!(!html.contains("<script"));
        assert!(!html.contains("http:") && !html.contains("https:"));
        assert!(!html.contains("<u1>"), "unit names are escaped");
        // Every bar is followed by its value as text.
        assert_eq!(
            html.matches("class=\"bar\"").count(),
            html.matches("%</td>").count()
        );
        assert!(html.contains("Not enough data yet: 0 of 30 days"));
        assert!(!html.to_lowercase().contains("generated by"));
        assert!(!html.to_lowercase().contains("engineered by"));
    }
}
