//! Provider authority policy: explicit, declarative field preference —
//! never scattered hard-coded "X always wins" conditionals.
//!
//! Raw provider facts stay untouched; the resolved view SELECTS values
//! per rule and models disagreements as conflicts instead of deleting
//! the losing value.

/// One authority rule: preferred provider order for one field of one
/// entity table, optionally scoped to a region (ISO country code).
#[derive(Debug, Clone, Copy)]
pub struct AuthorityRule {
    pub entity_table: &'static str,
    pub field: &'static str,
    /// Scope: None = global rule.
    pub region: Option<&'static str>,
    /// Preferred provider order (first wins when present).
    pub preference: &'static [&'static str],
}

/// The policy. FAA CIFP is the authority for US navigation semantics;
/// outside the US, OurAirports provides the best available open data
/// and is preferred for metadata where no stronger source exists.
/// Deterministic and auditable — no scattered "X always wins".
pub const AUTHORITY_RULES: &[AuthorityRule] = &[
    AuthorityRule {
        entity_table: "airports",
        field: "ident",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "airports",
        field: "name",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "airports",
        field: "elevation_ft",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "frequency_khz",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "kind",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "name",
        region: Some("US"),
        preference: &["FAA_CIFP", "OurAirports"],
    },
];

/// Default preference: US = FAA first; elsewhere OurAirports first.
pub const DEFAULT_PREFERENCE_US: &[&str] = &["FAA_CIFP", "OurAirports"];
pub const DEFAULT_PREFERENCE_WORLD: &[&str] = &["OurAirports", "FAA_CIFP"];

/// Preferred provider order for one (table, field, region).
pub fn preference_for(table: &str, field: &str, region: Option<&str>) -> &'static [&'static str] {
    let default = match region {
        Some("US") => DEFAULT_PREFERENCE_US,
        _ => DEFAULT_PREFERENCE_WORLD,
    };
    AUTHORITY_RULES
        .iter()
        .find(|r| {
            r.entity_table == table
                && r.field == field
                && (r.region.is_none() || r.region == region)
        })
        .map(|r| r.preference)
        .unwrap_or(default)
}

/// Rank of a provider for one (table, field, region); lower = better.
pub fn provider_rank(table: &str, field: &str, region: Option<&str>, provider: &str) -> usize {
    preference_for(table, field, region)
        .iter()
        .position(|p| *p == provider)
        .unwrap_or(usize::MAX)
}
