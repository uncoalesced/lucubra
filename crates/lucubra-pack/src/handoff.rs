// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Parser for the `lucubra-handoff v1` block (SPEC section 16.3) that a host
//! without MCP writes at the end of a Pack-mode session. The text comes from
//! chat output, so the parser tolerates surrounding Markdown and prose, never
//! panics, and refuses a block without its `end: lucubra-handoff` line.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// Largest block accepted, in lines and in characters per line.
const MAX_LINES: usize = 5_000;
const MAX_LINE_CHARS: usize = 20_000;
/// Most entries accepted in one list section.
const MAX_ENTRIES: usize = 1_000;

const LISTS: &[&str] = &["steps", "items", "cards", "new_items"];
const SCALARS: &[&str] = &["session", "host", "started", "ended", "summary"];

/// One parsed handoff block.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Handoff {
    pub session: String,
    /// Scalar fields other than `session`: host, started, ended, summary.
    pub fields: BTreeMap<String, String>,
    /// List sections: steps, items, cards, new_items.
    pub lists: BTreeMap<String, Vec<Map<String, Value>>>,
    pub checkin: Map<String, Value>,
    /// Keys the parser did not recognise, kept for the report.
    pub warnings: Vec<String>,
}

impl Handoff {
    /// Entries of a list section, empty when absent.
    pub fn list(&self, key: &str) -> &[Map<String, Value>] {
        self.lists.get(key).map_or(&[], Vec::as_slice)
    }
}

/// Why a block could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The start line was found but not `end: lucubra-handoff`.
    Truncated,
    UnsupportedVersion(String),
    MissingSession,
    Malformed {
        line: usize,
        reason: String,
    },
    TooLarge,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated => write!(
                f,
                "the handoff block is cut off (no 'end: lucubra-handoff' line); copy the whole block again"
            ),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "handoff format '{v}' is not supported; this Lucubra reads v1"
                )
            }
            Self::MissingSession => write!(f, "the handoff block has no 'session:' line"),
            Self::Malformed { line, reason } => {
                write!(f, "handoff block line {line}: {reason}")
            }
            Self::TooLarge => write!(f, "the handoff block is too large to read"),
        }
    }
}

fn start_version(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('`').trim_start();
    let rest = rest.strip_prefix("lucubra-handoff")?;
    Some(rest.trim())
}

fn is_end(line: &str) -> bool {
    line.trim().trim_end_matches('`').trim() == "end: lucubra-handoff"
}

/// Every handoff block in `text`, in order. Text without a block gives an
/// empty list.
pub fn parse_all(text: &str) -> Vec<Result<Handoff, ParseError>> {
    let lines: Vec<&str> = text.lines().collect();
    let is_start = |l: &&str| start_version(l.trim()).is_some();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some(version) = start_version(lines[i].trim()) else {
            i += 1;
            continue;
        };
        let start = i + 1;
        let stop = lines[start..]
            .iter()
            .position(|l| is_end(l) || is_start(l))
            .map(|p| start + p);
        match stop {
            Some(end) if is_end(lines[end]) => {
                out.push(if version == "v1" {
                    parse_body(&lines[start..end], start)
                } else {
                    Err(ParseError::UnsupportedVersion(
                        version.chars().take(20).collect(),
                    ))
                });
                i = end + 1;
            }
            // Cut off: resume at the next start line, if any.
            Some(next) => {
                out.push(Err(ParseError::Truncated));
                i = next;
            }
            None => {
                out.push(Err(ParseError::Truncated));
                i = lines.len();
            }
        }
    }
    out
}

fn malformed(line: usize, reason: impl Into<String>) -> ParseError {
    ParseError::Malformed {
        line,
        reason: reason.into(),
    }
}

fn parse_body(lines: &[&str], first_line: usize) -> Result<Handoff, ParseError> {
    if lines.len() > MAX_LINES || lines.iter().any(|l| l.chars().count() > MAX_LINE_CHARS) {
        return Err(ParseError::TooLarge);
    }
    let mut h = Handoff::default();
    let mut list: Option<String> = None;
    for (n, raw) in lines.iter().enumerate() {
        let line_no = first_line + n + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(entry) = line
            .strip_prefix("- ")
            .or_else(|| (line == "-").then_some(""))
        {
            let Some(key) = &list else {
                return Err(malformed(
                    line_no,
                    "list entry outside steps, items, cards or new_items",
                ));
            };
            let entries = h.lists.entry(key.clone()).or_default();
            if entries.len() >= MAX_ENTRIES {
                return Err(ParseError::TooLarge);
            }
            entries.push(inline_map(entry).map_err(|r| malformed(line_no, r))?);
            continue;
        }
        let Some((key, value)) = split_pair(line) else {
            return Err(malformed(line_no, "expected 'key: value'"));
        };
        let (key, value) = (key.trim(), value.trim());
        list = None;
        if LISTS.contains(&key) {
            if !value.is_empty() && value != "[]" {
                return Err(malformed(
                    line_no,
                    format!("'{key}:' starts a list; put entries on '- ' lines"),
                ));
            }
            h.lists.entry(key.to_owned()).or_default();
            list = Some(key.to_owned());
        } else if key == "checkin" {
            h.checkin = inline_map(value).map_err(|r| malformed(line_no, r))?;
        } else if SCALARS.contains(&key) {
            let text = match scalar(value) {
                Value::String(s) => s,
                other => other.to_string(),
            };
            if key == "session" {
                h.session = text;
            } else {
                h.fields.insert(key.to_owned(), text);
            }
        } else {
            let shown: String = key.chars().take(40).collect();
            h.warnings.push(format!("unknown key '{shown}' ignored"));
        }
    }
    let session = h.session.trim();
    if session.is_empty() {
        return Err(ParseError::MissingSession);
    }
    if session.chars().count() > 128 || session.chars().any(char::is_control) {
        return Err(malformed(first_line, "session id is not a plain id"));
    }
    h.session = session.to_owned();
    Ok(h)
}

/// Splits `key: value` or `key=value` at the first separator; keys are plain
/// words.
fn split_pair(s: &str) -> Option<(&str, &str)> {
    let idx = s.find([':', '='])?;
    let key = s[..idx].trim();
    let valid = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then(|| (key, &s[idx + 1..]))
}

/// Splits on commas that are not inside quotes or brackets.
fn split_top(s: &str) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut escaped, mut start) = (0usize, false, false, 0usize);
    for (i, c) in s.char_indices() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '[' => depth += 1,
            ']' => depth = depth.checked_sub(1).ok_or("unbalanced ']'")?,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err("unclosed quote".into());
    }
    if depth != 0 {
        return Err("unbalanced '['".into());
    }
    parts.push(&s[start..]);
    Ok(parts)
}

/// `k: v, k: v` (or `k=v`) as a JSON object.
fn inline_map(s: &str) -> Result<Map<String, Value>, String> {
    let mut map = Map::new();
    for part in split_top(s)? {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (k, v) = split_pair(part).ok_or_else(|| {
            let shown: String = part.chars().take(40).collect();
            format!("expected 'key: value', got '{shown}'")
        })?;
        map.insert(k.to_owned(), value(v.trim(), 0)?);
    }
    Ok(map)
}

fn value(s: &str, depth: usize) -> Result<Value, String> {
    if depth > 8 {
        return Err("lists nested too deeply".into());
    }
    if let Some(inner) = s.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or("unclosed '['")?;
        if inner.trim().is_empty() {
            return Ok(Value::Array(Vec::new()));
        }
        return split_top(inner)?
            .into_iter()
            .map(|p| value(p.trim(), depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array);
    }
    Ok(scalar(s))
}

/// A quoted string (with `\"`, `\\` and `\n` escapes), a number, or bare text.
fn scalar(s: &str) -> Value {
    if let Some(inner) = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|_| s.len() >= 2)
    {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return Value::String(out);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::from(n);
    }
    if let Ok(f) = s.parse::<f64>()
        && f.is_finite()
    {
        return Value::from(f);
    }
    Value::String(s.to_owned())
}

/// The SPEC 16.3 example block, wrapped in chat prose, for tests and the
/// fuzz corpus.
pub const EXAMPLE: &str = r#"Thanks for studying! Here is your block:

```lucubra-handoff v1
session: 7b1e9c2a-55aa
host: chatgpt-web
started: 2026-11-12T19:05+05:30
ended: 2026-11-12T19:52+05:30
steps:
  - unit: stats/u3, minutes: 25, activity: recall
items:
  - id: q_01JD, response: "B", result: correct
cards:
  - id: c_01JD, rating: good, at: 2026-11-12T19:40+05:30
new_items:
  - type: mcq_single, unit: stats/u3, prompt: "Mean, median, \"mode\"?", options: ["a, b", c], answer: "a, b", source: "S:stats/U:u3/F:9a1c22e0/p:14", anchor: "the mean"
checkin: sleep=6.5, quality=3, energy=2
summary: "Covered the u3 basics."
end: lucubra-handoff
```

Save this in your inbox folder."#;

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn handoff_parses_the_spec_example() {
        let all = parse_all(EXAMPLE);
        assert_eq!(all.len(), 1);
        let h = all[0].as_ref().unwrap();
        assert_eq!(h.session, "7b1e9c2a-55aa");
        assert_eq!(h.fields["host"], "chatgpt-web");
        assert_eq!(h.fields["ended"], "2026-11-12T19:52+05:30");
        assert_eq!(h.list("steps")[0]["minutes"], 25);
        assert_eq!(h.list("items")[0]["response"], "B");
        assert_eq!(h.list("cards")[0]["at"], "2026-11-12T19:40+05:30");
        let item = &h.list("new_items")[0];
        assert_eq!(item["prompt"], "Mean, median, \"mode\"?");
        assert_eq!(item["options"], serde_json::json!(["a, b", "c"]));
        assert_eq!(item["source"], "S:stats/U:u3/F:9a1c22e0/p:14");
        assert_eq!(h.checkin["sleep"], 6.5);
        assert_eq!(h.fields["summary"], "Covered the u3 basics.");
        assert!(h.warnings.is_empty());
    }

    #[test]
    fn handoff_truncated_and_bad_blocks() {
        let cut = EXAMPLE.replace("end: lucubra-handoff", "");
        assert_eq!(parse_all(&cut), vec![Err(ParseError::Truncated)]);
        assert!(parse_all("no block here").is_empty());
        let v2 = EXAMPLE.replace("lucubra-handoff v1", "lucubra-handoff v2");
        assert!(matches!(
            parse_all(&v2)[0],
            Err(ParseError::UnsupportedVersion(_))
        ));
        let no_session = EXAMPLE.replace("session: 7b1e9c2a-55aa\n", "");
        assert_eq!(
            parse_all(&no_session),
            vec![Err(ParseError::MissingSession)]
        );
        let unknown = EXAMPLE.replace("host:", "mood: calm\nhost:");
        assert_eq!(parse_all(&unknown)[0].as_ref().unwrap().warnings.len(), 1);
        let two = format!("{EXAMPLE}\n{}", EXAMPLE.replace("7b1e9c2a-55aa", "second"));
        let sessions: Vec<String> = parse_all(&two)
            .into_iter()
            .map(|r| r.unwrap().session)
            .collect();
        assert_eq!(sessions, ["7b1e9c2a-55aa", "second"]);
        let mixed = format!("{cut}\n{EXAMPLE}");
        let r = parse_all(&mixed);
        assert_eq!(r[0], Err(ParseError::Truncated));
        assert!(r[1].is_ok(), "{r:?}");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4096))]
        #[test]
        fn handoff_never_panics(s in "\\PC{0,400}") {
            let _ = parse_all(&s);
            let _ = parse_all(&format!("```lucubra-handoff v1\n{s}\nend: lucubra-handoff\n```"));
        }

        #[test]
        fn handoff_never_panics_on_mutations(cut in 0usize..2000, junk in "\\PC{0,8}") {
            let mut chars: Vec<char> = EXAMPLE.chars().collect();
            let at = cut % (chars.len() + 1);
            chars.splice(at..at, junk.chars());
            let mutated: String = chars.into_iter().collect();
            let _ = parse_all(&mutated);
        }
    }
}
