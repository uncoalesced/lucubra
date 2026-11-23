// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Onboarding through MCP form elicitation (SPEC section 9, S35). When the
//! client declares form elicitation, `lucubra_profile(next)` first asks for the
//! missing primitive required fields in one form; accepted answers go through
//! the same `answer` path as the conversational flow, so the events match.

use std::time::Duration;

use rmcp::model::{ElicitRequestParams, ElicitationAction};
use rmcp::service::{Peer, RoleServer};
use serde_json::{Map, Value, json};

/// Required profile fields a flat elicitation form can carry. `exams` is a
/// list of objects, so it stays conversational.
pub const FORM_FIELDS: &[&str] = &[
    "level",
    "programme",
    "term_system",
    "current_term",
    "start_year",
    "completion_year",
    "subjects",
];

const FORM_TIMEOUT: Duration = Duration::from_secs(600);

fn property(field: &str) -> Value {
    match field {
        "level" => json!({"type": "string", "title": "Academic level",
            "description": "For example school, diploma, bachelor's, master's, doctoral, professional certification, self-learner"}),
        "programme" => json!({"type": "string", "title": "Programme or course",
            "description": "Degree, department or stream, for example B.Com Banking"}),
        "term_system" => json!({"type": "string", "title": "Term system",
            "enum": ["semester", "trimester", "quarter", "annual", "other"]}),
        "current_term" => {
            json!({"type": "string", "title": "Current term", "description": "For example Semester 5"})
        }
        "start_year" => {
            json!({"type": "integer", "title": "Start year", "minimum": 1950, "maximum": 2100})
        }
        "completion_year" => {
            json!({"type": "integer", "title": "Planned completion year", "minimum": 1950, "maximum": 2100})
        }
        _ => json!({"type": "string", "title": "Subjects this term",
            "description": "Comma-separated, for example Statistics, Income Tax, Banking"}),
    }
}

/// The form request for `missing` fields, or `None` when none fit a form.
pub fn form_request(missing: &[&str]) -> Option<ElicitRequestParams> {
    let fields: Vec<&str> = FORM_FIELDS
        .iter()
        .copied()
        .filter(|f| missing.contains(f))
        .collect();
    if fields.is_empty() {
        return None;
    }
    let properties: Map<String, Value> = fields
        .iter()
        .map(|f| ((*f).to_owned(), property(f)))
        .collect();
    serde_json::from_value(json!({
        "mode": "form",
        "message": "Lucubra setup: a few details about your studies. Everything stays in your study folder and can be changed later.",
        "requestedSchema": {"type": "object", "properties": properties, "required": fields},
    }))
    .ok()
}

/// Turns a comma-separated subject list into `{slug, title}` objects.
pub fn subjects_value(raw: &str) -> Value {
    let items: Vec<Value> = raw
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .filter_map(|title| {
            let mut slug = String::new();
            for c in title.chars().flat_map(char::to_lowercase) {
                if c.is_ascii_alphanumeric() {
                    slug.push(c);
                } else if !slug.ends_with('-') {
                    slug.push('-');
                }
            }
            let slug: String = slug.trim_matches('-').chars().take(64).collect();
            (!slug.is_empty()).then(|| json!({"slug": slug, "title": title}))
        })
        .collect();
    Value::Array(items)
}

/// Accepted form content as `(field, value)` answers in form order.
pub fn answers(content: &Map<String, Value>) -> Vec<(String, Value)> {
    FORM_FIELDS
        .iter()
        .filter_map(|field| {
            let value = content.get(*field)?;
            let value = match (*field, value) {
                ("subjects", Value::String(s)) => subjects_value(s),
                (_, Value::String(s)) if s.trim().is_empty() => return None,
                _ => value.clone(),
            };
            Some(((*field).to_owned(), value))
        })
        .collect()
}

/// Sends the form and returns accepted answers; empty on decline, cancel,
/// timeout or a client error, so the caller falls back to conversation.
pub async fn ask(peer: &Peer<RoleServer>, missing: &[&str]) -> Vec<(String, Value)> {
    let Some(request) = form_request(missing) else {
        return Vec::new();
    };
    match peer
        .create_elicitation_with_timeout(request, Some(FORM_TIMEOUT))
        .await
    {
        Ok(result) if result.action == ElicitationAction::Accept => result
            .content
            .as_ref()
            .and_then(Value::as_object)
            .map(answers)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_covers_only_flat_missing_fields() {
        let req = form_request(&["subjects", "exams", "start_year"]).unwrap();
        let wire = serde_json::to_value(&req).unwrap();
        let props = wire["requestedSchema"]["properties"].as_object().unwrap();
        assert_eq!(props.len(), 2);
        assert!(props.contains_key("subjects") && props.contains_key("start_year"));
        assert!(form_request(&["exams"]).is_none());
    }

    #[test]
    fn subjects_become_slugs_with_titles() {
        assert_eq!(
            subjects_value("Income Tax, GST; Banking & Insurance,"),
            json!([
                {"slug": "income-tax", "title": "Income Tax"},
                {"slug": "gst", "title": "GST"},
                {"slug": "banking-insurance", "title": "Banking & Insurance"}
            ])
        );
    }
}
