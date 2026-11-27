// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Round-4 practice (SPEC round 4, A5, A6): past papers and topic frequency
//! (S51), the SOURCES.md download allow-list (S52) and printable mock tests
//! (S53). The fetch test uses a server on 127.0.0.1 and needs no internet.

mod support;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

use serde_json::json;
use support::{Fx, ok, open, refused};

async fn past_paper(fx: &Fx, unit: &str, kind: &str, year: u64, marks: f64, answer: &str) {
    let mut item = json!({"type": kind, "prompt": format!("{unit} question from {year}"),
        "answer": answer, "difficulty": 3,
        "past_paper": {"year": year, "marks": marks}});
    if kind == "mcq_single" {
        item["options"] = json!(["yes", "no"]);
    } else {
        item["rubric"] = json!([{"criterion": "Covers the rule", "marks": marks}]);
    }
    ok(
        fx,
        "lucubra_submit",
        json!({"action": "items", "unit": unit, "mode": "extended", "items": [item]}),
    )
    .await;
}

#[tokio::test]
async fn s51_past_papers_give_topic_frequency_and_drive_the_simulation() {
    let fx = open("claude-ai").await;
    for year in 2020..=2024 {
        past_paper(&fx, "torts/negligence", "mcq_single", year, 10.0, "yes").await;
    }
    past_paper(&fx, "torts/duty", "mcq_single", 2024, 2.0, "yes").await;
    refused(
        &fx,
        "lucubra_submit",
        json!({"action": "items", "unit": "torts/duty", "mode": "extended",
               "items": [{"type": "true_false", "prompt": "p", "answer": "true",
                          "past_paper": {"year": 24}}]}),
        "invalid_arguments",
    )
    .await;

    let status = ok(&fx, "lucubra_status", json!({})).await;
    let pp = &status["past_papers"];
    assert_eq!(
        pp["note"], "history of past papers, not a prediction",
        "{pp}"
    );
    assert_eq!(pp["units"][0]["unit"], "torts/negligence", "{pp}");
    assert_eq!(pp["units"][0]["text"], "asked in 5 of the last 5 years");
    assert_eq!(pp["units"][1]["text"], "asked in 1 of the last 5 years");

    // Negligence holds 50 of the 52 past-paper marks, so the simulation opens on it.
    let sim = ok(
        &fx,
        "lucubra_practice",
        json!({"action": "exam_sim", "subject": "torts", "mode": "extended", "count": 3}),
    )
    .await;
    assert_eq!(sim["item"]["unit"], "torts/negligence", "{sim}");
}

/// Serves `responses` in order, one per connection, on 127.0.0.1. Returns the
/// base URL and a channel with each request line, plus a `LEAK` line for any
/// cookie or referrer header received.
fn server(responses: Vec<String>) -> (String, std::sync::mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                let lower = line.to_ascii_lowercase();
                if lower.starts_with("cookie") || lower.starts_with("referer") {
                    tx.send(format!("LEAK {line}")).ok();
                }
                line.clear();
            }
            tx.send(first.trim().to_owned()).ok();
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (base, rx)
}

fn ok_pdf() -> String {
    let body = "%PDF-1.4 fake paper";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn events(fx: &Fx) -> String {
    std::fs::read_dir(fx.root.path().join(".lucubra/events"))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn s52_fetch_needs_sources_md_and_refuses_unlisted_domains() {
    let fx = open("claude-ai").await;
    let off = refused(
        &fx,
        "lucubra_ingest",
        json!({"action": "fetch", "url": "https://portal.univ.edu/paper.pdf"}),
        "untrusted_domain",
    )
    .await;
    assert!(off.to_string().contains("SOURCES.md"), "{off}");

    std::fs::write(
        fx.root.path().join("SOURCES.md"),
        "# Trusted sources\n\n- https://univ.edu (university portal, past papers)\n- 127.0.0.1 (local test server)\n",
    )
    .unwrap();
    let unlisted = refused(
        &fx,
        "lucubra_ingest",
        json!({"action": "fetch", "url": "https://questions.example.org/x.pdf"}),
        "untrusted_domain",
    )
    .await;
    assert!(
        unlisted
            .to_string()
            .contains("add the domain to SOURCES.md"),
        "{unlisted}"
    );
    refused(
        &fx,
        "lucubra_ingest",
        json!({"action": "fetch", "url": "http://univ.edu/x.pdf"}),
        "invalid_arguments",
    )
    .await;

    let redirect = "HTTP/1.1 302 Found\r\nLocation: https://evil.example/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned();
    let (base, rx) = server(vec![ok_pdf(), redirect]);
    let saved = ok(
        &fx,
        "lucubra_ingest",
        json!({"action": "fetch", "url": format!("{base}/papers/2024%20paper.pdf?token=s3cret")}),
    )
    .await;
    assert_eq!(saved["saved"], "inbox/2024-20paper.pdf", "{saved}");
    let file = fx.root.path().join("inbox/2024-20paper.pdf");
    assert_eq!(std::fs::read(&file).unwrap(), b"%PDF-1.4 fake paper");
    let request = rx.recv().unwrap();
    assert!(request.starts_with("GET /papers/"), "{request}");
    let log = events(&fx);
    assert!(log.contains("source.fetched"), "{log}");
    assert!(!log.contains("s3cret"), "the query string is never logged");

    let bounced = refused(
        &fx,
        "lucubra_ingest",
        json!({"action": "fetch", "url": format!("{base}/next")}),
        "untrusted_domain",
    )
    .await;
    assert!(bounced.to_string().contains("evil.example"), "{bounced}");
    assert!(rx.try_iter().all(|l| !l.starts_with("LEAK")));
    let leftovers: Vec<String> = std::fs::read_dir(fx.root.path().join("inbox"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// The question number printed with `marks` on the Markdown paper.
fn question_with(md: &str, marks: &str) -> u64 {
    md.lines()
        .find(|l| l.starts_with("## Question ") && l.contains(&format!("({marks} ")))
        .and_then(|l| l["## Question ".len()..].split(' ').next()?.parse().ok())
        .unwrap_or_else(|| panic!("no question with {marks} marks in {md}"))
}

#[tokio::test]
async fn s53_mock_test_exports_a_paper_and_grades_the_read_back() {
    let fx = open("claude-ai").await;
    past_paper(
        &fx,
        "torts/negligence",
        "long_answer",
        2024,
        10.0,
        "Duty, breach, damage.",
    )
    .await;
    past_paper(&fx, "torts/negligence", "mcq_single", 2023, 2.0, "yes").await;

    let mock = ok(
        &fx,
        "lucubra_export",
        json!({"action": "mock_test", "subject": "torts", "count": 3, "duration_minutes": 90}),
    )
    .await;
    let mock_id = mock["mock_id"].as_str().unwrap().to_owned();
    let md_path = mock["files"][0].as_str().unwrap();
    let html_path = mock["files"][1].as_str().unwrap();
    let md = std::fs::read_to_string(fx.root.path().join(md_path)).unwrap();
    let html = std::fs::read_to_string(fx.root.path().join(html_path)).unwrap();
    assert!(md.contains("Time allowed: 90 minutes"), "{md}");
    assert!(
        !md.contains("Duty, breach, damage."),
        "no answer key on the paper"
    );
    assert!(html.contains("@media print") && !html.contains("<script"));
    assert!(!html.contains("http://") && !html.contains("https://"));
    let log = events(&fx);
    let exported = log
        .lines()
        .find(|l| l.contains("mock_test.exported"))
        .unwrap();
    assert!(
        !exported.contains("question from"),
        "no prompts in the event: {exported}"
    );

    let (open_q, closed_q) = (question_with(&md, "10"), question_with(&md, "2"));
    refused(
        &fx,
        "lucubra_practice",
        json!({"action": "mock_grade", "mock_id": mock_id,
               "answers": [{"n": open_q, "read_back": "Duty and breach."}]}),
        "invalid_arguments",
    )
    .await;
    let graded = ok(
        &fx,
        "lucubra_practice",
        json!({"action": "mock_grade", "mock_id": mock_id, "answers": [
            {"n": open_q, "read_back": "Duty and breach.", "score": 6},
            {"n": closed_q, "read_back": "yes", "response": "yes"}
        ]}),
    )
    .await;
    assert_eq!(graded["verified"], false, "{graded}");
    assert_eq!(
        graded["awarded"], 8.0,
        "6 by the rubric and 2 by the engine: {graded}"
    );
    let by: Vec<&str> = graded["per_question"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["graded_by"].as_str().unwrap())
        .collect();
    assert!(by.contains(&"rubric") && by.contains(&"engine"), "{graded}");
    let done = ok(
        &fx,
        "lucubra_practice",
        json!({"action": "mock_grade", "mock_id": mock_id, "confirm": true}),
    )
    .await;
    assert_eq!(done["verified"], true, "{done}");
    refused(
        &fx,
        "lucubra_practice",
        json!({"action": "mock_grade", "mock_id": "mt_unknown", "confirm": true}),
        "invalid_arguments",
    )
    .await;
}
