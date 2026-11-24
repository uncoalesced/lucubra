// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Engine grading for closed-format items (SPEC section 13.3). Open types
//! and anything the engine cannot decide return [`Graded::Host`], and the
//! host grades them against the stored rubric.

use serde_json::Value;

/// Outcome of grading one response.
#[derive(Debug, Clone, PartialEq)]
pub enum Graded {
    /// Graded by the engine: `score` is 0 to 1.
    Engine {
        correct: bool,
        score: f64,
        detail: String,
    },
    /// The host grades this response against the rubric.
    Host,
}

fn engine(correct: bool, score: f64, detail: impl Into<String>) -> Graded {
    Graded::Engine {
        correct,
        score,
        detail: detail.into(),
    }
}

/// Lowercase, trimmed, inner whitespace collapsed, trailing punctuation off.
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .trim_end_matches(['.', ',', ';', '!'])
        .to_owned()
}

fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(norm(s)),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn texts(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(as_text).collect(),
        other => as_text(other).into_iter().collect(),
    }
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match norm(s).as_str() {
            "true" | "t" | "yes" => Some(true),
            "false" | "f" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn list(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        other => vec![other.clone()],
    }
}

/// Grades `response` against `item` (`type`, `answer`, optional `accepted`,
/// `tolerance {abs, rel}` and `unit`).
pub fn grade(item: &Value, response: &Value) -> Graded {
    let answer = &item["answer"];
    match item["type"].as_str().unwrap_or_default() {
        "mcq_single" => {
            let ok = as_text(answer).is_some() && as_text(answer) == as_text(response);
            engine(ok, f64::from(u8::from(ok)), "single choice")
        }
        "mcq_multi" => {
            let (mut want, mut got) = (texts(answer), texts(response));
            want.sort();
            want.dedup();
            got.sort();
            got.dedup();
            let hit = got.iter().filter(|g| want.contains(g)).count() as f64;
            let union = (want.len() + got.len()) as f64 - hit;
            let score = if union == 0.0 { 0.0 } else { hit / union };
            engine(want == got && !want.is_empty(), score, "multiple choice")
        }
        "cloze" => {
            let (blanks, given) = (list(answer), list(response));
            let accepted = &item["accepted"];
            // `accepted` is a flat list of variants (one blank) or one list per blank.
            let per_blank = accepted
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_array));
            let mut right = 0usize;
            for (i, blank) in blanks.iter().enumerate() {
                let mut ok_set = texts(blank);
                if per_blank {
                    ok_set.extend(accepted.get(i).map(texts).unwrap_or_default());
                } else if blanks.len() == 1 {
                    ok_set.extend(texts(accepted));
                }
                if given
                    .get(i)
                    .and_then(as_text)
                    .is_some_and(|g| ok_set.contains(&g))
                {
                    right += 1;
                }
            }
            let score = right as f64 / blanks.len().max(1) as f64;
            engine(right == blanks.len(), score, "cloze")
        }
        "true_false" => match (as_bool(answer), as_bool(response)) {
            (Some(a), Some(r)) => engine(a == r, f64::from(u8::from(a == r)), "true or false"),
            _ => Graded::Host,
        },
        "matching" => {
            let (Some(want), Some(got)) = (answer.as_object(), response.as_object()) else {
                return Graded::Host;
            };
            let right = want
                .iter()
                .filter(|(k, v)| got.get(*k).and_then(as_text) == as_text(v))
                .count();
            let score = right as f64 / want.len().max(1) as f64;
            engine(right == want.len() && !want.is_empty(), score, "matching")
        }
        "ordering" => {
            let (want, got) = (texts(answer), texts(response));
            let right = want.iter().zip(&got).filter(|(a, b)| a == b).count();
            let score = right as f64 / want.len().max(1) as f64;
            engine(want == got && !want.is_empty(), score, "ordering")
        }
        "numeric" => grade_numeric(item, response),
        "formula_recall" => {
            let f = |v: &Value| v.as_str().map(formula_norm);
            match (f(answer), f(response)) {
                (Some(a), Some(r)) if a == r => engine(true, 1.0, "formula"),
                _ => Graded::Host,
            }
        }
        _ => Graded::Host,
    }
}

/// Whitespace and multiplication signs dropped, `^` and `**` dropped so
/// `c^2`, `c**2` and `c2` compare equal.
fn formula_norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '*' | '^' | '\u{00B7}' | '\u{00D7}'))
        .collect()
}

/// A number and an optional unit from JSON: `12.5`, `"12.5 cm"` or
/// `{"value": 12.5, "unit": "cm"}`.
fn quantity(v: &Value, default_unit: Option<&str>) -> Option<(f64, Option<String>)> {
    let pick = |u: &str| {
        if u.is_empty() {
            default_unit.map(str::to_owned)
        } else {
            Some(u.to_owned())
        }
    };
    match v {
        Value::Number(n) => Some((n.as_f64()?, default_unit.map(str::to_owned))),
        Value::String(s) => {
            let s = s.trim();
            let split = s
                .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
                .unwrap_or(s.len());
            let (num, unit) = s.split_at(split);
            Some((num.trim().parse().ok()?, pick(unit.trim())))
        }
        Value::Object(o) => {
            let value = o.get("value")?.as_f64()?;
            Some((value, pick(o.get("unit").and_then(Value::as_str).unwrap_or(""))))
        }
        _ => None,
    }
}

fn grade_numeric(item: &Value, response: &Value) -> Graded {
    let unit = item["unit"].as_str();
    let (Some((want, wu)), Some((got, gu))) =
        (quantity(&item["answer"], unit), quantity(response, unit))
    else {
        return Graded::Host;
    };
    let (want, got) = match (wu.as_deref(), gu.as_deref()) {
        (Some(a), Some(b)) if a != b => {
            let (Some(ua), Some(ub)) = (parse_unit(a), parse_unit(b)) else {
                return engine(false, 0.0, format!("unknown unit '{b}'"));
            };
            if ua.dim != ub.dim {
                return engine(
                    false,
                    0.0,
                    format!("'{b}' does not measure the same quantity as '{a}'"),
                );
            }
            (ua.to_base(want), ub.to_base(got))
        }
        _ => (want, got),
    };
    let tol = &item["tolerance"];
    let abs = tol["abs"].as_f64().unwrap_or(0.0);
    let rel = tol["rel"].as_f64().unwrap_or(1e-9);
    let ok = (want - got).abs() <= abs.max(rel * want.abs());
    engine(ok, f64::from(u8::from(ok)), "numeric")
}

#[derive(Debug, Clone, Copy)]
struct Unit {
    dim: &'static str,
    factor: f64,
    offset: f64,
}

impl Unit {
    fn to_base(self, v: f64) -> f64 {
        v * self.factor + self.offset
    }
}

/// (symbol, quantity, factor to SI base, offset to SI base).
const BASE: &[(&str, &str, f64, f64)] = &[
    ("m", "length", 1.0, 0.0),
    ("in", "length", 0.0254, 0.0),
    ("ft", "length", 0.3048, 0.0),
    ("mi", "length", 1609.344, 0.0),
    ("g", "mass", 1e-3, 0.0),
    ("lb", "mass", 0.453_592_37, 0.0),
    ("t", "mass", 1000.0, 0.0),
    ("s", "time", 1.0, 0.0),
    ("min", "time", 60.0, 0.0),
    ("h", "time", 3600.0, 0.0),
    ("day", "time", 86_400.0, 0.0),
    ("K", "temperature", 1.0, 0.0),
    ("\u{00B0}C", "temperature", 1.0, 273.15),
    ("degC", "temperature", 1.0, 273.15),
    ("\u{00B0}F", "temperature", 5.0 / 9.0, 255.372_222_222_222_2),
    ("degF", "temperature", 5.0 / 9.0, 255.372_222_222_222_2),
    ("L", "volume", 1e-3, 0.0),
    ("A", "current", 1.0, 0.0),
    ("V", "voltage", 1.0, 0.0),
    ("N", "force", 1.0, 0.0),
    ("J", "energy", 1.0, 0.0),
    ("W", "power", 1.0, 0.0),
    ("Pa", "pressure", 1.0, 0.0),
    ("Hz", "frequency", 1.0, 0.0),
    ("mol", "amount", 1.0, 0.0),
];

/// SI prefixes that may precede a metric base symbol.
const PREFIXES: &[(&str, f64)] = &[
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("c", 1e-2),
    ("m", 1e-3),
    ("\u{00B5}", 1e-6),
    ("u", 1e-6),
    ("n", 1e-9),
];

const NO_PREFIX: &[&str] = &["in", "ft", "mi", "lb", "min", "h", "day", "t"];

fn parse_unit(s: &str) -> Option<Unit> {
    let lookup = |sym: &str| {
        BASE.iter()
            .find(|(b, ..)| *b == sym)
            .map(|&(_, dim, factor, offset)| Unit {
                dim,
                factor,
                offset,
            })
    };
    let s = s.trim();
    lookup(s).or_else(|| {
        PREFIXES.iter().find_map(|(p, mult)| {
            let rest = s.strip_prefix(p)?;
            let u = lookup(rest)?;
            (u.offset == 0.0 && !NO_PREFIX.contains(&rest)).then_some(Unit {
                factor: u.factor * mult,
                ..u
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok(item: &Value, resp: Value) -> bool {
        matches!(grade(item, &resp), Graded::Engine { correct: true, .. })
    }

    #[test]
    fn grader_closed_types() {
        let mcq = json!({"type": "mcq_single", "answer": "B"});
        assert!(ok(&mcq, json!("b")));
        assert!(!ok(&mcq, json!("C")));

        let multi = json!({"type": "mcq_multi", "answer": ["A", "C"]});
        assert!(ok(&multi, json!(["c", "a"])));
        match grade(&multi, &json!(["A"])) {
            Graded::Engine { correct, score, .. } => {
                assert!(!correct && (score - 0.5).abs() < 1e-9);
            }
            Graded::Host => panic!("multi is engine-graded"),
        }

        let cloze = json!({"type": "cloze", "answer": "mitochondria", "accepted": ["mitochondrion"]});
        assert!(ok(&cloze, json!("  Mitochondrion. ")));
        assert!(!ok(&cloze, json!("nucleus")));
        let two = json!({"type": "cloze", "answer": ["ATP", "NADPH"],
            "accepted": [["adenosine triphosphate"], []]});
        assert!(ok(&two, json!(["adenosine triphosphate", "nadph"])));
        assert!(!ok(&two, json!(["ATP", "NADH"])));

        let tf = json!({"type": "true_false", "answer": false});
        assert!(ok(&tf, json!("False")));
        assert!(!ok(&tf, json!(true)));

        let m = json!({"type": "matching", "answer": {"H2O": "water", "NaCl": "salt"}});
        assert!(ok(&m, json!({"NaCl": "Salt", "H2O": "water"})));
        assert!(!ok(&m, json!({"NaCl": "water", "H2O": "salt"})));

        let ord = json!({"type": "ordering", "answer": ["prophase", "metaphase", "anaphase"]});
        assert!(ok(&ord, json!(["Prophase", "Metaphase", "Anaphase"])));
        assert!(!ok(&ord, json!(["metaphase", "prophase", "anaphase"])));

        let f = json!({"type": "formula_recall", "answer": "E = m c^2"});
        assert!(ok(&f, json!("E=m*c2")));
        assert_eq!(grade(&f, &json!("E = 1/2 m v^2")), Graded::Host);

        let open = json!({"type": "long_answer", "answer": "x"});
        assert_eq!(grade(&open, &json!("y")), Graded::Host);
    }

    #[test]
    fn grader_numeric_tolerance_and_units() {
        let abs = json!({"type": "numeric", "answer": 9.81, "unit": "m", "tolerance": {"abs": 0.05}});
        assert!(ok(&abs, json!(9.8)));
        assert!(!ok(&abs, json!(9.7)));

        let rel = json!({"type": "numeric", "answer": 1000, "tolerance": {"rel": 0.01}});
        assert!(ok(&rel, json!("1009")));
        assert!(!ok(&rel, json!(1020)));

        let conv = json!({"type": "numeric", "answer": {"value": 2.5, "unit": "km"},
            "tolerance": {"rel": 1e-6}});
        assert!(ok(&conv, json!("2500 m")));
        assert!(ok(&conv, json!({"value": 250000, "unit": "cm"})));
        assert!(!ok(&conv, json!("2500 s")));
        assert!(!ok(&conv, json!("2500 furlong")));

        let temp = json!({"type": "numeric", "answer": "100 \u{00B0}C", "tolerance": {"abs": 0.01}});
        assert!(ok(&temp, json!("373.15 K")));
        assert!(ok(&temp, json!("212 \u{00B0}F")));
    }
}
