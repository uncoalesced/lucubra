// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Token estimation, plan-tier response limits, and cumulative wrap-up budgets
//! (SPEC section 7.1, 15; scenarios S12, S37).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Plan or host budget tier (SPEC section 7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetTier {
    /// Default on Free-tier consumer hosts.
    Free,
    /// Explicit paid plan or Claude Code terminal tier.
    Paid,
    /// Strict compact tier for local small-context models (LM Studio, Jan).
    Local,
}

impl BudgetTier {
    /// Wire identifier for this budget tier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Paid => "paid",
            Self::Local => "local",
        }
    }

    /// Parses `"free"`, `"paid"`, or `"local"` case-insensitively.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "free" => Some(Self::Free),
            "paid" => Some(Self::Paid),
            "local" => Some(Self::Local),
            _ => None,
        }
    }

    /// Response and session caps defined in SPEC section 7.1.
    pub fn limits(self) -> BudgetLimits {
        match self {
            Self::Free => BudgetLimits {
                orient_limit: 1_000,
                response_limit: 2_000,
                wrap_up_limit: 25_000,
                pack_limit: Some(20_000),
            },
            Self::Paid => BudgetLimits {
                orient_limit: 1_500,
                response_limit: 6_000,
                wrap_up_limit: 80_000,
                pack_limit: Some(40_000),
            },
            Self::Local => BudgetLimits {
                orient_limit: 700,
                response_limit: 1_500,
                wrap_up_limit: 12_000,
                pack_limit: None,
            },
        }
    }
}

/// Token limits for a resolved [`BudgetTier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetLimits {
    /// Maximum tokens for `lucubra_orient`.
    pub orient_limit: usize,
    /// Maximum tokens for general tool responses.
    pub response_limit: usize,
    /// Cumulative served-token threshold triggering `next: "wrap_up"`.
    pub wrap_up_limit: u64,
    /// Soft Pack mode budget (used by M7; informative only in M3).
    pub pack_limit: Option<usize>,
}

/// Compact wire budget block carried in every envelope (`SPEC 8.2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetInfo {
    /// Cumulative estimated tokens served in the active MCP session.
    pub served: u64,
    /// Active session wrap-up threshold (`wrap_up_limit`).
    pub limit: u64,
}

/// Estimates token count using Unicode scalar length divided by 4, rounded up
/// (SPEC section 7.1).
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// Resolves the active [`BudgetTier`] using profile precedence before host default:
/// 1. `profile.local_model == true` -> `Local`
/// 2. `profile.plan_tier_by_host[host_id]`
/// 3. `profile.plan_tier`
/// 4. `host_default`
pub fn resolve_tier(
    profile: Option<&BTreeMap<String, Value>>,
    host_id: &str,
    host_default: BudgetTier,
) -> BudgetTier {
    let Some(map) = profile else {
        return host_default;
    };
    if map.get("local_model").and_then(Value::as_bool) == Some(true) {
        return BudgetTier::Local;
    }
    if let Some(tier) = map
        .get("plan_tier_by_host")
        .and_then(|v| v.get(host_id))
        .and_then(Value::as_str)
        .and_then(BudgetTier::parse)
    {
        return tier;
    }
    if let Some(tier) = map
        .get("plan_tier")
        .and_then(Value::as_str)
        .and_then(BudgetTier::parse)
    {
        return tier;
    }
    host_default
}

/// Truncates `text` to at most `max_chars` Unicode scalar values.
///
/// Returns `(sliced, was_truncated)`.
pub fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut end_byte = None;
    for (count, (idx, _)) in text.char_indices().enumerate() {
        if count == max_chars {
            end_byte = Some(idx);
            break;
        }
    }
    match end_byte {
        Some(idx) => (text[..idx].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// Character budget left for tool payload content after reserving outer envelope metadata.
pub fn max_payload_chars(per_call_token_limit: usize) -> usize {
    per_call_token_limit.saturating_sub(180).max(64) * 4
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn estimate_tokens_handles_empty_ascii_and_indic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        // 4 Unicode scalars (even though 12 UTF-8 bytes) -> 1 token; 7 scalars -> 2 tokens.
        assert_eq!(estimate_tokens("കോശം"), 1);
        assert_eq!(estimate_tokens("കോശങ്ങൾ"), 2);
    }

    #[test]
    fn resolve_tier_honors_precedence() {
        let mut prof = BTreeMap::new();
        prof.insert("plan_tier".into(), json!("paid"));
        prof.insert(
            "plan_tier_by_host".into(),
            json!({"claude-desktop": "free"}),
        );
        assert_eq!(
            resolve_tier(Some(&prof), "claude-desktop", BudgetTier::Free),
            BudgetTier::Free
        );
        assert_eq!(
            resolve_tier(Some(&prof), "cursor", BudgetTier::Free),
            BudgetTier::Paid
        );
        prof.insert("local_model".into(), json!(true));
        assert_eq!(
            resolve_tier(Some(&prof), "claude-code", BudgetTier::Paid),
            BudgetTier::Local
        );
    }

    #[test]
    fn truncate_chars_cuts_on_scalar_boundary() {
        let s = "അവൻപ്രകാശം";
        let (cut, truncated) = truncate_chars(s, 3);
        assert!(truncated);
        assert_eq!(cut.chars().count(), 3);
        let (full, untruncated) = truncate_chars(s, 50);
        assert!(!untruncated);
        assert_eq!(full, s);
    }
}
