//! Provider authority policy: explicit, declarative field preference —
//! never scattered hard-coded "X always wins" conditionals.
//!
//! Raw provider facts stay untouched; the resolved view SELECTS values
//! per rule and models disagreements as conflicts instead of deleting
//! the losing value.

/// One authority rule: preferred provider order for one field of one
/// entity table.
#[derive(Debug, Clone, Copy)]
pub struct AuthorityRule {
    pub entity_table: &'static str,
    pub field: &'static str,
    /// Preferred provider order (first wins when present).
    pub preference: &'static [&'static str],
}

/// The policy. FAA CIFP is the authority for US navigation semantics
/// and navaid parameters; everything unlisted resolves by the default
/// provider order (FAA_CIFP, then OurAirports).
pub const AUTHORITY_RULES: &[AuthorityRule] = &[
    AuthorityRule {
        entity_table: "airports",
        field: "ident",
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "airports",
        field: "name",
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "airports",
        field: "elevation_ft",
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "frequency_khz",
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "kind",
        preference: &["FAA_CIFP", "OurAirports"],
    },
    AuthorityRule {
        entity_table: "navaids",
        field: "name",
        preference: &["FAA_CIFP", "OurAirports"],
    },
];

/// Default preference when no explicit rule exists.
pub const DEFAULT_PREFERENCE: &[&str] = &["FAA_CIFP", "OurAirports"];

/// Preferred provider order for one (table, field).
pub fn preference_for(table: &str, field: &str) -> &'static [&'static str] {
    AUTHORITY_RULES
        .iter()
        .find(|r| r.entity_table == table && r.field == field)
        .map(|r| r.preference)
        .unwrap_or(DEFAULT_PREFERENCE)
}

/// Rank of a provider for one (table, field); lower = more preferred.
pub fn provider_rank(table: &str, field: &str, provider: &str) -> usize {
    preference_for(table, field)
        .iter()
        .position(|p| *p == provider)
        .unwrap_or(usize::MAX)
}
