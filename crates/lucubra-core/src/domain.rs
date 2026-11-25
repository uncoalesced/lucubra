// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Domain packs (SPEC 20): per-discipline data for item templates, exam type
//! to session method overrides, just-in-time onboarding questions and the MCP
//! servers to recommend for operating domain software.
//!
//! The nine v1 packs are TOML files under `crates/lucubra-core/domain_packs/`,
//! embedded at build time so they ship inside the wheel. Lucubra only
//! recommends companion servers; it never installs or starts them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::state::{State, col};

/// Item types the engine stores (SPEC 13.1).
pub const ITEM_TYPES: &[&str] = &[
    "mcq_single",
    "mcq_multi",
    "cloze",
    "true_false",
    "matching",
    "numeric",
    "ordering",
    "short_answer",
    "long_answer",
    "viva_prompt",
    "case_stem",
    "diagram_label",
    "formula_recall",
];

/// Open item types the host grades against a stored rubric (SPEC 13.3).
pub const RUBRIC_TYPES: &[&str] = &["short_answer", "long_answer", "viva_prompt", "case_stem"];

/// Exam type to session method, the base table of SPEC 12.4. A pack's
/// `methods.toml` overrides entries by exam type.
pub const BASE_METHODS: &[(&str, &str)] = &[
    (
        "theory",
        "Active recall, then long-answer practice graded against rubrics",
    ),
    (
        "numericals",
        "Worked problems with step checking; numeric items graded with tolerance and units",
    ),
    (
        "practical",
        "Procedure walkthroughs, ordering items, lab record checks",
    ),
    (
        "viva",
        "Spoken-style question and answer simulation with follow-ups",
    ),
    ("objective", "Timed drills"),
    ("osce", "Case stems with stepwise questions"),
    (
        "open_book",
        "Locating and applying material: search drills, application questions",
    ),
    ("project", "Milestone planning and rehearsal prompts"),
];

/// Prefix of profile fields that answer a pack's onboarding question.
pub const QUESTION_PREFIX: &str = "pack_";

/// `pack.toml`: identity, matching keywords and onboarding questions.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meta {
    /// Stable id, equal to the directory name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Pack version.
    pub version: String,
    /// Lowercase substrings matched against the profile programme.
    pub keywords: Vec<String>,
    /// Effort factor for the planner (SPEC 12.1).
    pub effort_factor: f64,
    /// File formats common in the domain (extensions without the dot).
    pub formats: Vec<String>,
    /// Exam types the domain uses, from the SPEC 12.4 table.
    pub exam_types: Vec<String>,
    /// Just-in-time onboarding questions; never required.
    #[serde(default)]
    pub questions: Vec<Question>,
}

/// One onboarding question, answered through `lucubra_profile` as a field.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Profile field name, starting with `pack_`.
    pub id: String,
    /// The question to ask.
    pub prompt: String,
    /// An example answer.
    pub example: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Methods {
    #[serde(default)]
    methods: BTreeMap<String, String>,
}

/// One item or rubric template (`items/*.toml`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    /// Template id, unique within the pack.
    pub id: String,
    /// Item type from SPEC 13.1.
    #[serde(rename = "type")]
    pub kind: String,
    /// Exam type the template serves.
    pub exam_type: String,
    /// Prompt pattern for the host; `{placeholders}` are filled from material.
    pub prompt: String,
    /// Rubric for open types.
    #[serde(default)]
    pub rubric: Vec<Criterion>,
}

/// One rubric criterion.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    /// What is assessed.
    pub criterion: String,
    /// Points available.
    pub max_points: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Templates {
    templates: Vec<Template>,
}

/// An MCP server to recommend for operating domain software. There is no
/// install or run field by design (`deny_unknown_fields` refuses one).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    /// Display name.
    pub name: String,
    /// What it lets the assistant do.
    pub purpose: String,
    /// File extensions (with the dot) whose presence triggers the suggestion.
    pub detect: Vec<String>,
    /// Project page, when one is well known.
    #[serde(default)]
    pub url: Option<String>,
    /// Words to search for in the host's connector or MCP directory.
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Companions {
    #[serde(default)]
    companions: Vec<Companion>,
}

/// A loaded domain pack.
#[derive(Debug, Clone)]
pub struct Pack {
    /// `pack.toml`.
    pub meta: Meta,
    /// `methods.toml` overrides by exam type.
    pub methods: BTreeMap<String, String>,
    /// `items/templates.toml`.
    pub templates: Vec<Template>,
    /// `companions.toml`.
    pub companions: Vec<Companion>,
}

macro_rules! embed {
    ($($id:literal),+ $(,)?) => {
        &[$((
            $id,
            [
                include_str!(concat!("../domain_packs/", $id, "/pack.toml")),
                include_str!(concat!("../domain_packs/", $id, "/methods.toml")),
                include_str!(concat!("../domain_packs/", $id, "/items/templates.toml")),
                include_str!(concat!("../domain_packs/", $id, "/companions.toml")),
            ],
        )),+]
    };
}

/// Embedded pack files in matching order: more specific programmes first, so
/// "Computer Science Engineering" selects computer science before engineering.
pub const EMBEDDED: &[(&str, [&str; 4])] = embed!(
    "medicine",
    "computer-science",
    "electronics",
    "architecture",
    "mechanical-civil",
    "commerce-finance",
    "sciences-mathematics",
    "humanities-law",
    "languages",
);

/// Parses and checks one pack from its four files.
pub fn parse(id: &str, files: [&str; 4]) -> Result<Pack, String> {
    let ctx = |file: &str, e: toml::de::Error| format!("{id}/{file}: {e}");
    let meta: Meta = toml::from_str(files[0]).map_err(|e| ctx("pack.toml", e))?;
    let methods: Methods = toml::from_str(files[1]).map_err(|e| ctx("methods.toml", e))?;
    let templates: Templates =
        toml::from_str(files[2]).map_err(|e| ctx("items/templates.toml", e))?;
    let companions: Companions = toml::from_str(files[3]).map_err(|e| ctx("companions.toml", e))?;
    let pack = Pack {
        meta,
        methods: methods.methods,
        templates: templates.templates,
        companions: companions.companions,
    };
    check(id, &pack)?;
    Ok(pack)
}

fn known_exam_type(t: &str) -> bool {
    BASE_METHODS.iter().any(|(k, _)| *k == t)
}

fn check(id: &str, p: &Pack) -> Result<(), String> {
    let err = |m: String| Err(format!("{id}: {m}"));
    if p.meta.id != id {
        return err(format!("id '{}' does not match its directory", p.meta.id));
    }
    if p.meta.keywords.is_empty() || p.meta.keywords.iter().any(|k| k != &k.to_lowercase()) {
        return err("keywords must be present and lowercase".into());
    }
    if !(0.5..=2.0).contains(&p.meta.effort_factor) {
        return err("effort_factor must be between 0.5 and 2.0".into());
    }
    for t in p.meta.exam_types.iter().chain(p.methods.keys()) {
        if !known_exam_type(t) {
            return err(format!("unknown exam type '{t}'"));
        }
    }
    for q in &p.meta.questions {
        if !q.id.starts_with(QUESTION_PREFIX) || q.prompt.is_empty() {
            return err(format!(
                "question '{}' needs the pack_ prefix and a prompt",
                q.id
            ));
        }
    }
    let mut ids = BTreeSet::new();
    for t in &p.templates {
        if !ids.insert(&t.id) {
            return err(format!("template id '{}' is repeated", t.id));
        }
        if !ITEM_TYPES.contains(&t.kind.as_str()) {
            return err(format!("template '{}' has unknown type '{}'", t.id, t.kind));
        }
        if !known_exam_type(&t.exam_type) {
            return err(format!("template '{}' has unknown exam type", t.id));
        }
        let open = RUBRIC_TYPES.contains(&t.kind.as_str());
        if open == t.rubric.is_empty() {
            return err(format!(
                "template '{}': open types need a rubric and closed types must not have one",
                t.id
            ));
        }
    }
    for c in &p.companions {
        if c.detect.is_empty() || c.detect.iter().any(|d| !d.starts_with('.')) {
            return err(format!(
                "companion '{}' needs extensions like \".blend\"",
                c.name
            ));
        }
        if c.url.is_none() && c.search.is_none() {
            return err(format!(
                "companion '{}' needs a url or search words",
                c.name
            ));
        }
    }
    Ok(())
}

/// All built-in packs, parsed once.
pub fn packs() -> &'static [Pack] {
    static PACKS: OnceLock<Vec<Pack>> = OnceLock::new();
    PACKS.get_or_init(|| {
        EMBEDDED
            .iter()
            // The embedded files are checked by `domain_packs_load_and_validate`;
            // a pack that fails to parse is left out rather than failing the engine.
            .filter_map(|(id, files)| parse(id, *files).ok())
            .collect()
    })
}

/// The pack for a programme name, matched by keyword in pack order.
pub fn pack_for(programme: &str) -> Option<&'static Pack> {
    let p = programme.to_lowercase();
    packs()
        .iter()
        .find(|pack| pack.meta.keywords.iter().any(|k| p.contains(k.as_str())))
}

/// The pack for the workspace profile's programme.
pub fn pack_for_state(state: &State) -> Option<&'static Pack> {
    state
        .get(col::PROFILE, "programme")
        .and_then(Value::as_str)
        .and_then(pack_for)
}

/// Session method for an exam type: the pack override, else the base table.
pub fn method_for(exam_type: &str, pack: Option<&Pack>) -> Option<String> {
    pack.and_then(|p| p.methods.get(exam_type).cloned())
        .or_else(|| {
            BASE_METHODS
                .iter()
                .find(|(k, _)| *k == exam_type)
                .map(|(_, m)| (*m).to_owned())
        })
}

/// Onboarding questions of the matched pack that are neither answered nor
/// skipped.
pub fn pending_questions(state: &State) -> Vec<Value> {
    let Some(pack) = pack_for_state(state) else {
        return Vec::new();
    };
    let skipped: Vec<&str> = state
        .get(col::PROFILE, "skipped_fields")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    pack.meta
        .questions
        .iter()
        .filter(|q| state.get(col::PROFILE, &q.id).is_none() && !skipped.contains(&q.id.as_str()))
        .map(|q| json!({"field": q.id, "prompt": q.prompt, "example": q.example}))
        .collect()
}

/// Whether `field` is an onboarding question of the matched pack.
pub fn is_pack_question(state: &State, field: &str) -> bool {
    pack_for_state(state).is_some_and(|p| p.meta.questions.iter().any(|q| q.id == field))
}

/// Companion MCP servers to suggest for files in the workspace (SPEC 20, S44).
/// Lucubra recommends; the user connects the server in their host.
pub fn recommendations(state: &State) -> Vec<Value> {
    let extensions: BTreeSet<String> = state
        .collection(col::PATHS)
        .filter_map(|(path, _)| {
            let name = path.rsplit('/').next()?;
            let dot = name.rfind('.')?;
            Some(name[dot..].to_lowercase())
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for pack in packs() {
        for c in &pack.companions {
            let found: Vec<&String> = c
                .detect
                .iter()
                .filter(|d| extensions.contains(*d))
                .collect();
            if found.is_empty() || !seen.insert(c.name.clone()) {
                continue;
            }
            out.push(json!({
                "name": c.name,
                "purpose": c.purpose,
                "found": found,
                "url": c.url,
                "search": c.search,
                "pack": pack.meta.id,
                "note": "Lucubra does not operate this software and installs nothing. To let your assistant work in it, connect this MCP server in your host yourself.",
            }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_packs_load_and_validate() {
        for (id, files) in EMBEDDED {
            let pack = parse(id, *files).unwrap_or_else(|e| panic!("{e}"));
            assert!(!pack.templates.is_empty(), "{id} has item templates");
            assert!(
                !pack.meta.questions.is_empty(),
                "{id} has onboarding questions"
            );
            for (name, text) in ["pack", "methods", "templates", "companions"]
                .iter()
                .zip(files)
            {
                // Data files carry neither header line (ADR 0007, D46).
                assert!(!text.contains("SPDX"), "{id} {name} has a licence header");
                assert!(
                    !text.contains("uncoalesced"),
                    "{id} {name} has provenance text"
                );
                assert!(!text.contains('\u{2014}'), "{id} {name} has an em dash");
                for banned in ["install", "command", "run =", "exec"] {
                    if *name == "companions" {
                        assert!(
                            !text.lines().any(|l| l.trim_start().starts_with(banned)),
                            "{id} companions carry no '{banned}' key"
                        );
                    }
                }
            }
        }
        assert_eq!(packs().len(), 9, "all nine v1 packs load");
    }

    #[test]
    fn domain_companions_refuse_install_commands() {
        let (id, files) = EMBEDDED[0];
        let bad = "[[companions]]\nname = \"X\"\npurpose = \"x\"\ndetect = [\".x\"]\nsearch = \"x\"\ninstall = \"curl x | sh\"\n";
        assert!(parse(id, [files[0], files[1], files[2], bad]).is_err());
    }

    #[test]
    fn domain_pack_selection_and_methods() {
        let id = |p: &str| pack_for(p).map(|p| p.meta.id.as_str());
        assert_eq!(
            id("B.Tech Computer Science and Engineering"),
            Some("computer-science")
        );
        assert_eq!(id("B.Tech Civil Engineering"), Some("mechanical-civil"));
        assert_eq!(id("MBBS"), Some("medicine"));
        assert_eq!(id("B.Arch"), Some("architecture"));
        assert_eq!(id("LLB"), Some("humanities-law"));
        assert_eq!(id("B.Com Accounting"), Some("commerce-finance"));
        assert_eq!(id("BSc Physics"), Some("sciences-mathematics"));
        assert_eq!(id("BA French"), Some("languages"));
        assert_eq!(id("B.Tech ECE"), Some("electronics"));
        assert_eq!(id("Underwater basket weaving"), None);

        let med = pack_for("MBBS");
        assert_eq!(
            method_for("objective", None).as_deref(),
            Some("Timed drills"),
            "base table without a pack"
        );
        let osce = method_for("osce", med).unwrap();
        assert_ne!(osce, BASE_METHODS[5].1, "medicine overrides OSCE");
        assert_eq!(method_for("unknown", med), None);
    }
}
