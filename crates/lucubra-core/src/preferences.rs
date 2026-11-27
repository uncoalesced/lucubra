// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Learning and output preferences (SPEC round 4, A9, D44). Non-diagnostic
//! only: there is no field for a condition or diagnosis. `PREFERENCES.md` is a
//! projection of `preference.set` events, capped at 600 tokens.

use serde_json::Value;

use crate::state::{State, col};

/// Hard cap on `PREFERENCES.md`: 600 tokens at 4 characters per token.
pub const MAX_CHARS: usize = 2_400;

/// Supported keys with their label in `PREFERENCES.md`.
pub const KEYS: &[(&str, &str)] = &[
    ("teaching_style", "Teaching style"),
    ("block_minutes", "Study block length (minutes)"),
    ("break_minutes", "Break length (minutes)"),
    ("one_question_at_a_time", "One question at a time"),
    ("plain_language", "Plain-language explanations"),
    ("read_aloud", "Output that reads well aloud"),
    ("explanation_language", "Explanation language"),
    ("reminders_study_blocks", "Reminders for study blocks"),
    ("reminders_review_due", "Reminders on review days"),
    ("reminders_exam_countdown", "Exam countdown reminders"),
];

fn bad(key: &str, want: &str) -> String {
    format!("preference '{key}' must be {want}")
}

/// Checks one preference value; `null` clears it. Returns the value to store.
pub fn validate(key: &str, value: &Value) -> Result<Value, String> {
    if !KEYS.iter().any(|(k, _)| *k == key) {
        let names: Vec<&str> = KEYS.iter().map(|(k, _)| *k).collect();
        return Err(format!(
            "unknown preference '{key}'; supported: {}",
            names.join(", ")
        ));
    }
    if value.is_null() {
        return Ok(Value::Null);
    }
    match key {
        "teaching_style" => match value.as_str() {
            Some(s) if crate::styles::style(s).is_some() => Ok(value.clone()),
            _ => Err(bad(
                key,
                &format!("one of: {}", crate::styles::ids().join(", ")),
            )),
        },
        "block_minutes" => match value.as_u64() {
            Some(n) if (10..=120).contains(&n) => Ok(value.clone()),
            _ => Err(bad(key, "a whole number from 10 to 120")),
        },
        "break_minutes" => match value.as_u64() {
            Some(n) if n <= 30 => Ok(value.clone()),
            _ => Err(bad(key, "a whole number from 0 to 30")),
        },
        "explanation_language" => match value.as_str().map(str::trim) {
            Some(s) if !s.is_empty() && s.chars().count() <= 40 && !s.contains('\n') => {
                Ok(Value::String(s.to_owned()))
            }
            _ => Err(bad(key, "a language name of at most 40 characters")),
        },
        _ => match value {
            Value::Bool(_) => Ok(value.clone()),
            _ => Err(bad(key, "true or false")),
        },
    }
}

/// A preference value, ignoring cleared ones.
pub fn get<'a>(state: &'a State, key: &str) -> Option<&'a Value> {
    state.get(col::PREFERENCES, key).filter(|v| !v.is_null())
}

/// Whether a true/false preference is on.
pub fn flag(state: &State, key: &str) -> bool {
    get(state, key).and_then(Value::as_bool) == Some(true)
}

fn shown(value: &Value) -> String {
    match value {
        Value::Bool(true) => "yes".into(),
        Value::Bool(false) => "no".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `PREFERENCES.md`, or `None` when no preference has been set (a hand-written
/// file is then left alone).
pub fn render(state: &State) -> Option<String> {
    let set: Vec<(&str, String)> = KEYS
        .iter()
        .filter_map(|(key, label)| get(state, key).map(|v| (*label, shown(v))))
        .collect();
    if set.is_empty() {
        return None;
    }
    let mut out = String::from(
        "# Preferences\n\nRead this before planning or producing output. Change it with \
         lucubra_profile(action=\"preference\") or edit it here; edits are kept.\n\n",
    );
    const MORE: &str = "- (more preferences not shown; the file is capped at 600 tokens)\n";
    for (label, value) in set {
        let line = format!("- {label}: {value}\n");
        if out.len() + line.len() + MORE.len() > MAX_CHARS {
            out.push_str(MORE);
            break;
        }
        out.push_str(&line);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preferences_validate_known_keys_only() {
        assert!(
            validate("diagnosis", &json!("x"))
                .unwrap_err()
                .contains("unknown")
        );
        assert_eq!(
            validate("teaching_style", &json!("tester")).unwrap(),
            json!("tester")
        );
        assert!(validate("teaching_style", &json!("lecture")).is_err());
        assert!(validate("block_minutes", &json!(5)).is_err());
        assert!(validate("block_minutes", &json!(45)).is_ok());
        assert!(validate("break_minutes", &json!(31)).is_err());
        assert!(validate("one_question_at_a_time", &json!("yes")).is_err());
        assert_eq!(
            validate("explanation_language", &json!(" Kannada ")).unwrap(),
            json!("Kannada")
        );
        assert!(validate("explanation_language", &json!("a\nb")).is_err());
        assert_eq!(validate("read_aloud", &Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn preferences_render_from_state_and_stay_under_the_cap() {
        let mut state = State::default();
        assert_eq!(render(&state), None);
        state.put(col::PREFERENCES, "one_question_at_a_time", json!(true));
        state.put(col::PREFERENCES, "teaching_style", json!("socratic"));
        state.put(col::PREFERENCES, "read_aloud", Value::Null);
        let text = render(&state).unwrap();
        assert!(text.contains("- One question at a time: yes"), "{text}");
        assert!(text.contains("- Teaching style: socratic"), "{text}");
        assert!(!text.contains("aloud"), "cleared values are not shown");
        assert!(flag(&state, "one_question_at_a_time"));
        assert!(!flag(&state, "read_aloud"));

        for (key, _) in KEYS {
            state.put(col::PREFERENCES, key, json!("x".repeat(400)));
        }
        let text = render(&state).unwrap();
        assert!(text.len() <= MAX_CHARS, "{}", text.len());
        assert!(text.contains("not shown"));
    }
}
