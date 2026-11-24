// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Grounding check for practice items (SPEC section 13.2, decision D53).
//!
//! A text anchor is accepted only when every one of its tokens appears, in
//! order, inside a window of the cited text short enough for the windowed
//! similarity `2n / (n + w)` to reach 0.85. Extraction noise (page furniture,
//! footnote marks) may add tokens to the source; the anchor may never claim a
//! token the source lacks. Numbers and negations must match exactly; words of
//! five or more letters tolerate one OCR-style edit.

use unicode_normalization::UnicodeNormalization;

/// Minimum windowed similarity between anchor and cited text.
pub const MIN_SIMILARITY: f64 = 0.85;
/// Longest anchor quote accepted, in words.
pub const MAX_ANCHOR_WORDS: usize = 25;

const NEGATIONS: &[&str] = &["not", "no", "never", "without", "except", "cannot", "nor"];

/// NFKC, case fold, rejoin hyphenated line breaks, expand `n't` to ` not`.
pub fn normalise(text: &str) -> String {
    let folded: String = text.nfkc().collect::<String>().to_lowercase();
    let mut joined = String::with_capacity(folded.len());
    let mut chars = folded.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '-' | '\u{00AD}' | '\u{2010}') {
            let mut look = chars.clone();
            let mut saw_newline = false;
            while let Some(&n) = look.peek() {
                if n == '\n' || n == '\r' {
                    saw_newline = true;
                } else if n != ' ' && n != '\t' {
                    break;
                }
                look.next();
            }
            if saw_newline {
                chars = look;
                continue;
            }
        }
        joined.push(c);
    }
    joined
        .replace(['\u{2019}', '\u{2018}'], "'")
        .replace("n't", " not")
}

/// Word tokens of normalised text. Decimal points and thousands separators
/// inside numbers stay part of the number.
pub fn tokens(text: &str) -> Vec<String> {
    let norm: Vec<char> = normalise(text).chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for (i, &c) in norm.iter().enumerate() {
        let inner_sep = matches!(c, '.' | ',')
            && cur.chars().last().is_some_and(|p| p.is_ascii_digit())
            && norm.get(i + 1).is_some_and(char::is_ascii_digit);
        if c.is_alphanumeric() || inner_sep {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_strict(token: &str) -> bool {
    token.starts_with(|c: char| c.is_ascii_digit()) || NEGATIONS.contains(&token)
}

fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

fn token_matches(anchor: &str, source: &str) -> bool {
    anchor == source
        || (!is_strict(anchor)
            && anchor.chars().count() >= 5
            && anchor.chars().all(char::is_alphabetic)
            && within_one_edit(anchor, source))
}

/// Best windowed similarity of `anchor` inside `text`; 0 when some anchor
/// token is missing or out of order everywhere.
pub fn windowed_similarity(anchor: &str, text: &str) -> f64 {
    let a = tokens(anchor);
    let t = tokens(text);
    if a.is_empty() {
        return 0.0;
    }
    let n = a.len() as f64;
    let mut best = 0.0f64;
    for start in 0..t.len() {
        if !token_matches(&a[0], &t[start]) {
            continue;
        }
        let (mut k, mut end) = (1, start);
        for (j, tok) in t.iter().enumerate().skip(start + 1) {
            if k == a.len() {
                break;
            }
            if token_matches(&a[k], tok) {
                k += 1;
                end = j;
            }
        }
        // A negation or number the anchor dropped flips the meaning too.
        let strict_in = |toks: &[String]| toks.iter().filter(|t| is_strict(t)).count();
        if k == a.len() && strict_in(&t[start..=end]) == strict_in(&a) {
            let w = (end - start + 1) as f64;
            best = best.max(2.0 * n / (n + w));
        }
    }
    best
}

/// Why a source failed the grounding check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    AnchorTooLong(usize),
    EmptyAnchor,
    NotFound,
    CellMissing(String),
    CellValue { cell: String, found: String },
    NameMissing(String),
}

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AnchorTooLong(n) => write!(
                f,
                "anchor has {n} words; quote at most {MAX_ANCHOR_WORDS} words"
            ),
            Self::EmptyAnchor => write!(f, "anchor is empty"),
            Self::NotFound => write!(
                f,
                "anchor not found at the cited location or its neighbours; quote the source exactly"
            ),
            Self::CellMissing(c) => write!(f, "cell {c} does not exist in the cited sheet"),
            Self::CellValue { cell, found } => {
                write!(f, "cell {cell} holds '{found}', not the cited value")
            }
            Self::NameMissing(n) => write!(f, "no layer, block or object named '{n}' exists"),
        }
    }
}

/// Checks a text anchor against the cited chunk and its neighbours, joined in
/// document order so a quote may span a chunk boundary.
pub fn verify_text(anchor: &str, chunks: &[&str]) -> Result<f64, Reject> {
    let words = anchor.split_whitespace().count();
    if words == 0 {
        return Err(Reject::EmptyAnchor);
    }
    if words > MAX_ANCHOR_WORDS {
        return Err(Reject::AnchorTooLong(words));
    }
    let score = windowed_similarity(anchor, &chunks.join("\n"));
    if score >= MIN_SIMILARITY {
        Ok(score)
    } else {
        Err(Reject::NotFound)
    }
}

fn same_value(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    a.eq_ignore_ascii_case(b)
        || matches!(
            (a.parse::<f64>(), b.parse::<f64>()),
            (Ok(x), Ok(y)) if (x - y).abs() <= 1e-9 * x.abs().max(1.0)
        )
}

/// Checks that `cell` (for example `B4`) exists in a sheet chunk and holds
/// the cited value or formula. Chunk lines look like `B4: 12.5 (=SUM(B1:B3))`
/// with cells separated by ` | `.
pub fn verify_cell(cell: &str, cited: &str, chunks: &[&str]) -> Result<(), Reject> {
    let cell = cell.trim().to_ascii_uppercase();
    let prefix = format!("{cell}: ");
    let found = chunks
        .iter()
        .flat_map(|c| c.lines())
        .flat_map(|l| l.split(" | "))
        .find_map(|part| part.trim().strip_prefix(prefix.as_str()));
    let Some(found) = found else {
        return Err(Reject::CellMissing(cell));
    };
    let (value, formula) = match found.rfind("(=") {
        Some(i) if found.ends_with(')') => {
            (found[..i].trim(), Some(&found[i + 1..found.len() - 1]))
        }
        _ => (found, None),
    };
    let want = cited.trim();
    let want = want.strip_prefix(prefix.as_str()).unwrap_or(want);
    let formula_hit = formula.is_some_and(|f| {
        same_value(f, want) || same_value(f.trim_start_matches('='), want.trim_start_matches('='))
    });
    if same_value(value, want) || formula_hit || same_value(found, want) {
        Ok(())
    } else {
        Err(Reject::CellValue {
            cell,
            found: found.to_owned(),
        })
    }
}

/// Checks that a CAD or 3D name (`l:<layer>` or `o:<object>`) exists exactly
/// among a file's chunk locators; split parts `l:<name>.<n>` count.
pub fn verify_name(location: &str, locators: &[String]) -> Result<(), Reject> {
    let hit = locators.iter().any(|l| {
        l == location
            || l.strip_prefix(location)
                .and_then(|rest| rest.strip_prefix('.'))
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    });
    if hit {
        Ok(())
    } else {
        let name = location.get(2..).unwrap_or(location);
        Err(Reject::NameMissing(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "The Krebs cycle takes place in the mitochondrial matrix. \
Each turn of the cycle produces 3 NADH, 1 FADH2 and 1 GTP, and releases \
two molecules of carbon dioxide. Citrate synthase is not regulated by \
ATP in plants, and the cycle never runs without oxygen in aerobic cells.";

    #[test]
    fn grounding_accepts_clean_variants() {
        let clean = [
            "Each turn of the cycle produces 3 NADH, 1 FADH2 and 1 GTP",
            "EACH TURN   of the\ncycle produces 3 nadh",
            "takes place in the mito-\nchondrial matrix",
            "releases two molecules of carbon dioxide",
            "Citrate synthase is not regulated by ATP in plants",
            "the cycle never runs without oxygen",
        ];
        for a in clean {
            assert!(verify_text(a, &[PAGE]).is_ok(), "should accept: {a}");
        }
        assert!(verify_text("the \u{FB01}nal step", &["Then the final step follows."]).is_ok());
        assert!(verify_text("It isn't stable", &["It is not stable here."]).is_ok());
        assert!(verify_text("mitochondrial matrix", &["the mitochondria1 matrix"]).is_ok());
    }

    #[test]
    fn grounding_rejects_perturbed_anchors() {
        let bad = [
            "Each turn of the cycle produces 2 NADH, 1 FADH2 and 1 GTP",
            "Citrate synthase is regulated by ATP in plants",
            "Citrate synthase is not regulated by ADP in plants",
            "The Krebs cycle takes place in the cytoplasm",
            "matrix mitochondrial the in place takes cycle Krebs The",
            "the cycle never runs with oxygen in anaerobic cells",
            "Glycolysis splits glucose into two pyruvate molecules",
        ];
        for a in bad {
            assert!(verify_text(a, &[PAGE]).is_err(), "should reject: {a}");
        }
    }

    #[test]
    fn grounding_adjacent_chunk_and_length_limit() {
        let a = "ends here and the next chunk starts";
        assert!(verify_text(a, &["text that ends here and", "the next chunk starts now"]).is_ok());
        let long = "word ".repeat(26);
        assert_eq!(verify_text(&long, &[&long]), Err(Reject::AnchorTooLong(26)));
    }

    #[test]
    fn grounding_cell_checks_value_and_formula() {
        let sheet = "A1: Units | B1: Price\nA4: Total | B4: 12.5 (=SUM(B1:B3))";
        assert!(verify_cell("B4", "12.5", &[sheet]).is_ok());
        assert!(verify_cell("b4", "12.50", &[sheet]).is_ok());
        assert!(verify_cell("B4", "=SUM(B1:B3)", &[sheet]).is_ok());
        assert!(matches!(
            verify_cell("B4", "13", &[sheet]),
            Err(Reject::CellValue { .. })
        ));
        assert!(matches!(
            verify_cell("C9", "1", &[sheet]),
            Err(Reject::CellMissing(_))
        ));
    }

    #[test]
    fn grounding_name_is_exact() {
        let locs = vec![
            "l:summary".to_owned(),
            "l:Walls".to_owned(),
            "l:Walls.1".to_owned(),
        ];
        assert!(verify_name("l:Walls", &locs).is_ok());
        assert!(verify_name("l:walls", &locs).is_err());
        assert!(verify_name("l:Wall", &locs).is_err());
    }
}
