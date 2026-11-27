// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! MCP host profile detection and capability recording (`HOST_MATRIX.md`,
//! SPEC section 5.6 & 7).

use lucubra_core::Workspace;
use lucubra_core::state::col;
use rmcp::model::{ClientCapabilities, InitializeRequestParams};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::budget::BudgetTier;

/// Known MCP client profiles plus [`HostKind::Generic`] fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostKind {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    Antigravity,
    Cursor,
    Windsurf,
    VsCodeCopilot,
    LmStudio,
    HermesAgent,
    JanOrOpenWebUi,
    Generic,
}

impl HostKind {
    /// All 10 named profiles in display/table order plus `Generic`.
    pub const ALL: &[Self] = &[
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Codex,
        Self::Antigravity,
        Self::Cursor,
        Self::Windsurf,
        Self::VsCodeCopilot,
        Self::LmStudio,
        Self::HermesAgent,
        Self::JanOrOpenWebUi,
        Self::Generic,
    ];

    /// Stable kebab-case host identifier recorded in `profile.set`.
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude-desktop",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
            Self::Windsurf => "windsurf",
            Self::VsCodeCopilot => "vscode-copilot",
            Self::LmStudio => "lm-studio",
            Self::HermesAgent => "hermes-agent",
            Self::JanOrOpenWebUi => "jan-open-webui",
            Self::Generic => "generic",
        }
    }

    /// Static capability/policy defaults derived from `HOST_MATRIX.md`.
    pub fn profile(self) -> HostProfile {
        match self {
            Self::ClaudeDesktop => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: true,
                export_recipe: "Save file in workspace or attach/paste into Claude Project / Drive connector",
                message_chars: None,
            },
            Self::ClaudeCode => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Paid,
                pointer_files: &["CLAUDE.md"],
                renders_artifacts: false,
                export_recipe: "Save file in workspace path; share via CLI/git or Drive sync",
                message_chars: None,
            },
            Self::Codex => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Write to workspace path or export .ics/.apkg from packs/ or schedule/",
                message_chars: None,
            },
            Self::Antigravity => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &["GEMINI.md"],
                renders_artifacts: true,
                export_recipe: "Save to workspace (packs/, schedule/, sessions/); sync via Drive or Workspace artifact",
                message_chars: None,
            },
            Self::Cursor => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Save file in workspace; open or share from workspace folder",
                message_chars: None,
            },
            Self::Windsurf => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Save file in workspace; open or share from workspace folder",
                message_chars: None,
            },
            Self::VsCodeCopilot => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Save file in workspace; export via local filesystem",
                message_chars: None,
            },
            Self::LmStudio => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Local,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Fully private local mode: keep exports on local disk only unless user copies them out",
                message_chars: None,
            },
            Self::HermesAgent => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Send file path/summary through active messaging gateway or synced workspace folder",
                message_chars: Some(4096),
            },
            Self::JanOrOpenWebUi => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Local,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Keep local file in workspace or copy text directly into chat",
                message_chars: None,
            },
            Self::Generic => HostProfile {
                kind: self,
                id: self.id(),
                default_tier: BudgetTier::Free,
                pointer_files: &[],
                renders_artifacts: false,
                export_recipe: "Save file in workspace (packs/, schedule/, sessions/) and copy or upload as needed",
                message_chars: None,
            },
        }
    }
}

/// Resolved policy for one detected MCP host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostProfile {
    /// Enum variant.
    pub kind: HostKind,
    /// Stable wire/log identifier.
    pub id: &'static str,
    /// Default budget tier when user profile does not override.
    pub default_tier: BudgetTier,
    /// Host pointer filenames (`CLAUDE.md`, `GEMINI.md`, or empty when `AGENTS.md` suffices).
    pub pointer_files: &'static [&'static str],
    /// Whether host UI renders interactive files/artifacts inline.
    pub renders_artifacts: bool,
    /// Guidance string surfaced by `lucubra_export`.
    pub export_recipe: &'static str,
    /// Per-message character limit of the channel the user reads replies in,
    /// when it is a messaging app (Telegram `sendMessage`: 4096, S22).
    pub message_chars: Option<usize>,
}

/// Runtime capabilities declared by the MCP client in `initialize`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedCapabilities {
    /// Client supports MCP form elicitation.
    pub elicitation_form: bool,
    /// Client supports MCP URL elicitation.
    pub elicitation_url: bool,
    /// Client supports MCP sampling.
    pub sampling: bool,
    /// Client supports MCP filesystem roots.
    pub roots: bool,
}

impl DetectedCapabilities {
    /// Extracts capability flags strictly from `request.capabilities` without
    /// assuming support from `clientInfo.name`.
    pub fn from_client_caps(caps: &ClientCapabilities) -> Self {
        let (elicitation_form, elicitation_url) = match &caps.elicitation {
            Some(e) => (e.form.is_some() || e.url.is_none(), e.url.is_some()),
            None => (false, false),
        };
        Self {
            elicitation_form,
            elicitation_url,
            sampling: caps.sampling.is_some(),
            roots: caps.roots.is_some(),
        }
    }
}

/// Complete client metadata observed at `initialize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedHost {
    /// Matched host policy.
    pub profile: HostProfile,
    /// Raw `clientInfo.name`.
    pub client_name: String,
    /// Raw `clientInfo.version`.
    pub client_version: String,
    /// Negotiated or requested protocol string.
    pub protocol_version: String,
    /// Runtime capabilities read exclusively from `request.capabilities`.
    pub capabilities: DetectedCapabilities,
}

fn match_name(raw: &str) -> Option<HostKind> {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower.contains("claude-code") || lower.contains("claude code") {
        return Some(HostKind::ClaudeCode);
    }
    if (lower.contains("claude-ai")
        || lower.contains("claude-desktop")
        || lower.contains("claude desktop"))
        && !lower.contains("code")
    {
        return Some(HostKind::ClaudeDesktop);
    }
    if lower.contains("codex") || lower.contains("openai-codex") {
        return Some(HostKind::Codex);
    }
    if lower.contains("antigravity") || lower.contains("google-antigravity") {
        return Some(HostKind::Antigravity);
    }
    if lower.contains("cursor") {
        return Some(HostKind::Cursor);
    }
    if lower.contains("windsurf") || lower.contains("codeium") {
        return Some(HostKind::Windsurf);
    }
    if lower.contains("vscode") || lower.contains("visual studio code") || lower.contains("copilot")
    {
        return Some(HostKind::VsCodeCopilot);
    }
    if lower.contains("lm-studio") || lower.contains("lm studio") || lower.contains("lmstudio") {
        return Some(HostKind::LmStudio);
    }
    if lower.contains("hermes") {
        return Some(HostKind::HermesAgent);
    }
    // Check word-boundary-safe `jan` match alongside explicit Open WebUI strings.
    if lower.contains("open-webui")
        || lower.contains("open webui")
        || lower == "jan"
        || lower.starts_with("jan-")
        || lower.starts_with("jan ")
    {
        return Some(HostKind::JanOrOpenWebUi);
    }
    None
}

/// Detects host identity and declared capabilities from `InitializeRequestParams`.
pub fn detect_host(params: &InitializeRequestParams, negotiated_protocol: &str) -> DetectedHost {
    let by_name = match_name(&params.client_info.name);
    let by_title = params.client_info.title.as_deref().and_then(match_name);
    let kind = by_name.or(by_title).unwrap_or(HostKind::Generic);

    DetectedHost {
        profile: kind.profile(),
        client_name: params.client_info.name.clone(),
        client_version: params.client_info.version.clone(),
        protocol_version: negotiated_protocol.to_owned(),
        capabilities: DetectedCapabilities::from_client_caps(&params.capabilities),
    }
}

/// Idempotently records detected host metadata into `profile.set` only when
/// changed compared to the replayed `col::PROFILE` collection.
///
/// Returns `Ok(true)` if an event was appended, `Ok(false)` if unchanged.
pub fn record_host_profile(ws: &Workspace, detected: &DetectedHost) -> lucubra_core::Result<bool> {
    let state = ws.replay()?.state;
    let existing_host = state.get(col::PROFILE, "host").and_then(Value::as_str);
    let existing_name = state
        .get(col::PROFILE, "client_name")
        .and_then(Value::as_str);
    let existing_ver = state
        .get(col::PROFILE, "client_version")
        .and_then(Value::as_str);
    let existing_caps = state.get(col::PROFILE, "client_capabilities");

    let new_caps = json!({
        "elicitation_form": detected.capabilities.elicitation_form,
        "elicitation_url": detected.capabilities.elicitation_url,
        "sampling": detected.capabilities.sampling,
        "roots": detected.capabilities.roots,
    });

    if existing_host == Some(detected.profile.id)
        && existing_name == Some(detected.client_name.as_str())
        && existing_ver == Some(detected.client_version.as_str())
        && existing_caps == Some(&new_caps)
    {
        return Ok(false);
    }

    ws.append(
        "profile.set",
        json!({
            "host": detected.profile.id,
            "client_name": detected.client_name,
            "client_version": detected.client_version,
            "client_capabilities": new_caps,
        }),
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use rmcp::model::{FormElicitationCapability, Implementation};

    use super::*;

    #[test]
    fn host_table_covers_all_ten_profiles_plus_generic() {
        let cases = [
            ("claude-ai", HostKind::ClaudeDesktop, BudgetTier::Free, true),
            ("claude-code", HostKind::ClaudeCode, BudgetTier::Paid, false),
            ("openai-codex", HostKind::Codex, BudgetTier::Free, false),
            (
                "google-antigravity",
                HostKind::Antigravity,
                BudgetTier::Free,
                true,
            ),
            ("cursor", HostKind::Cursor, BudgetTier::Free, false),
            ("windsurf", HostKind::Windsurf, BudgetTier::Free, false),
            (
                "Visual Studio Code Copilot",
                HostKind::VsCodeCopilot,
                BudgetTier::Free,
                false,
            ),
            ("LM Studio", HostKind::LmStudio, BudgetTier::Local, false),
            (
                "Hermes Agent",
                HostKind::HermesAgent,
                BudgetTier::Free,
                false,
            ),
            (
                "open-webui",
                HostKind::JanOrOpenWebUi,
                BudgetTier::Local,
                false,
            ),
            ("unknown-client", HostKind::Generic, BudgetTier::Free, false),
        ];
        for (name, want_kind, want_tier, want_artifacts) in cases {
            let params = InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new(name, "1.0"),
            );
            let detected = detect_host(&params, "2025-11-25");
            assert_eq!(detected.profile.kind, want_kind, "client={name}");
            assert_eq!(detected.profile.default_tier, want_tier, "client={name}");
            assert_eq!(
                detected.profile.renders_artifacts, want_artifacts,
                "client={name}"
            );
            assert!(!detected.capabilities.elicitation_form);
        }
    }

    #[test]
    fn elicitation_reads_from_capabilities_and_profile_set_is_idempotent() {
        let caps = ClientCapabilities::builder()
            .enable_elicitation_with(
                rmcp::model::ElicitationCapability::new()
                    .with_form(FormElicitationCapability::default()),
            )
            .build();
        let params = InitializeRequestParams::new(caps, Implementation::new("claude-code", "2.0"));
        let detected = detect_host(&params, "2025-11-25");
        assert!(detected.capabilities.elicitation_form);

        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let ws = Workspace::init(root.path(), data.path()).unwrap();
        assert!(record_host_profile(&ws, &detected).unwrap());
        assert!(!record_host_profile(&ws, &detected).unwrap());
    }
}
