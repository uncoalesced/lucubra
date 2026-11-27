// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Teaching styles as data (SPEC round 4, A2, D35). The engine prescribes the
//! step sequence for each style; the host supplies the language. Styles are
//! embedded TOML files, so a new style is a data file, not code.

use std::sync::OnceLock;

use serde::Deserialize;

/// Step kinds a style may use.
pub const STEP_KINDS: &[&str] = &[
    "explain",
    "check",
    "advance",
    "recap",
    "question",
    "feedback",
    "explain_missed",
    "light_check",
    "guide",
    "hint",
    "reveal",
    "grade",
    "probe",
    "timed_question",
    "review_card",
    "weak_concepts",
    "quick_check",
];

const DEPTHS: &[&str] = &["brief", "standard", "deep"];
const ADVANCE: &[&str] = &["answer_or_skip", "answer", "time", "always"];

/// The style used when none is chosen.
pub const DEFAULT_STYLE: &str = "tutor";

/// One teaching style.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Step kinds in order; the sequence repeats per concept.
    pub step_order: Vec<String>,
    /// Concepts between questions; 0 means questions only.
    pub question_every: u32,
    /// Explanation depth: brief, standard or deep.
    pub depth: String,
    /// When to move on: answer_or_skip, answer, time or always.
    pub advance_on: String,
    /// Whether hints are allowed.
    pub hints: bool,
}

macro_rules! embed {
    ($($id:literal),* $(,)?) => {
        &[$(($id, include_str!(concat!("../styles/", $id, ".toml")))),*]
    };
}

/// Embedded style files, in display order.
pub const EMBEDDED: &[(&str, &str)] = embed!(
    "tutor",
    "tester",
    "explainer",
    "socratic",
    "drill",
    "viva",
    "exam_sim",
    "revision",
);

/// Parses and checks one style file.
pub fn parse(id: &str, text: &str) -> Result<Style, String> {
    let style: Style = toml::from_str(text).map_err(|e| format!("style {id}: {e}"))?;
    if style.id != id {
        return Err(format!("style {id}: id field is '{}'", style.id));
    }
    if style.step_order.is_empty() {
        return Err(format!("style {id}: step_order is empty"));
    }
    if let Some(bad) = style
        .step_order
        .iter()
        .find(|s| !STEP_KINDS.contains(&s.as_str()))
    {
        return Err(format!("style {id}: unknown step kind '{bad}'"));
    }
    if !DEPTHS.contains(&style.depth.as_str()) {
        return Err(format!("style {id}: unknown depth '{}'", style.depth));
    }
    if !ADVANCE.contains(&style.advance_on.as_str()) {
        return Err(format!(
            "style {id}: unknown advance_on '{}'",
            style.advance_on
        ));
    }
    Ok(style)
}

/// Every embedded style that parses (checked by `styles_load_and_validate`).
pub fn styles() -> &'static [Style] {
    static STYLES: OnceLock<Vec<Style>> = OnceLock::new();
    STYLES.get_or_init(|| {
        EMBEDDED
            .iter()
            .filter_map(|(id, text)| parse(id, text).ok())
            .collect()
    })
}

/// A style by id.
pub fn style(id: &str) -> Option<&'static Style> {
    styles().iter().find(|s| s.id == id)
}

/// Style ids in display order.
pub fn ids() -> Vec<&'static str> {
    styles().iter().map(|s| s.id.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_load_and_validate() {
        for (id, text) in EMBEDDED {
            parse(id, text).unwrap();
            assert!(
                !text.contains("SPDX") && !text.contains("Engineered by"),
                "{id}"
            );
            assert!(!text.contains('\u{2014}'), "{id} has an em dash");
        }
        assert_eq!(styles().len(), EMBEDDED.len());
        assert_eq!(
            ids(),
            [
                "tutor",
                "tester",
                "explainer",
                "socratic",
                "drill",
                "viva",
                "exam_sim",
                "revision"
            ]
        );
        assert!(style(DEFAULT_STYLE).is_some());
        assert_eq!(style("tester").unwrap().step_order[0], "question");
        assert!(!style("exam_sim").unwrap().hints);
    }

    #[test]
    fn bad_style_files_are_refused() {
        let good = EMBEDDED[0].1;
        assert!(parse("other", good).unwrap_err().contains("id field"));
        let bad_step = good.replace("\"explain\"", "\"dance\"");
        assert!(parse("tutor", &bad_step).unwrap_err().contains("dance"));
        let bad_depth = good.replace("standard", "bottomless");
        assert!(parse("tutor", &bad_depth).unwrap_err().contains("depth"));
        assert!(parse("tutor", &format!("{good}install = \"x\"\n")).is_err());
    }
}
