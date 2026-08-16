//! Conservative deterministic matchers per entity kind.
//!
//! Thresholds are explicit constants. Precision beats recall: a matcher
//! that is not confident never merges; Ambiguous candidates stay
//! separate and are surfaced as diagnostics.

use openairac_model::*;
use sha2::{Digest, Sha256};

/// Great-circle distance below which coordinates confirm identity.
pub const EXACT_NM: f64 = 3.0;
/// Distance below which identity is probable but not certain.
pub const PROBABLE_NM: f64 = 30.0;
/// Distance below which an identifier change with country agreement is
/// treated as same-facility continuity (300 m).
pub const IDENT_CHANGE_NM: f64 = 0.3;
/// Endpoint tolerance for physical runway identity (~185 m).
pub const RUNWAY_ENDPOINT_NM: f64 = 0.1;

/// Matching outcome. Only Exact/Probable create memberships.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchOutcome {
    Exact(Vec<EvidenceFact>),
    Probable(Vec<EvidenceFact>),
    Ambiguous(Vec<EvidenceFact>),
    Conflict {
        category: String,
        severity: ConflictSeverity,
        evidence: Vec<EvidenceFact>,
    },
    Distinct(Vec<EvidenceFact>),
}

/// Great-circle distance, nautical miles.
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 3440.065; // mean earth radius, nautical miles
    let to_rad = std::f64::consts::PI / 180.0;
    let (p1, l1) = (lat1 * to_rad, lon1 * to_rad);
    let (p2, l2) = (lat2 * to_rad, lon2 * to_rad);
    let dp = p2 - p1;
    let dl = l2 - l1;
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Deterministic, stable canonical id from an identity key — never the
/// provider entity id, never order-dependent.
pub fn canonical_id_for(prefix: &str, key: &str) -> CanonicalEntityId {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    CanonicalEntityId(format!("{prefix}:{}", hex_digest(&digest)))
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()[..16].to_string()
}

/// Recognized ICAO airport ident: four uppercase letters.
pub fn is_icao_ident(ident: &str) -> bool {
    ident.len() == 4 && ident.chars().all(|c| c.is_ascii_uppercase())
}

/// Airport identity key: ICAO ident, or country+local ident for
/// non-ICAO idents (never name-only).
pub fn airport_identity_key(a: &CanonicalAirport) -> Option<String> {
    let ident = a.ident.trim().to_uppercase();
    if ident.is_empty() {
        return None;
    }
    if is_icao_ident(&ident) {
        Some(format!("icao:{ident}"))
    } else {
        let country = a.iso_country.as_deref().unwrap_or("").to_uppercase();
        Some(format!("local:{country}:{ident}"))
    }
}

fn navaid_kind_class(kind: &NavaidKind) -> &'static str {
    match kind {
        NavaidKind::Ndb => "ndb",
        _ => "vor",
    }
}

/// Navaid identity key: kind class + ident + region. A VOR and an NDB
/// sharing an ident are never the same candidate group.
pub fn navaid_identity_key(n: &CanonicalNavaid) -> String {
    let region = n.region_code.as_deref().unwrap_or("").trim().to_uppercase();
    format!(
        "{}:{}:{}",
        navaid_kind_class(&n.kind),
        n.ident.trim().to_uppercase(),
        region
    )
}

/// Navaid fallback key for providers without ICAO region codes:
/// (kind class, ident). Pairs are coordinate-gated in the matcher.
pub fn navaid_fallback_key(n: &CanonicalNavaid) -> String {
    format!(
        "{}:{}",
        navaid_kind_class(&n.kind),
        n.ident.trim().to_uppercase()
    )
}

/// Waypoint identity key: ident + ICAO region. Same name in different
/// regions stays distinct.
pub fn waypoint_identity_key(w: &CanonicalWaypoint) -> String {
    format!(
        "{}:{}",
        w.ident.trim().to_uppercase(),
        w.region_code.trim().to_uppercase()
    )
}

/// Physical runway identity: parent airport canonical + rounded,
/// sorted endpoint coordinates. Published designators are payload, not
/// identity (09 -> 10 is the SAME physical runway).
pub fn runway_geometry_key(parent_canonical: &CanonicalEntityId, r: &CanonicalRunway) -> String {
    let e1 = (r.le_lat, r.le_lon);
    let e2 = (r.he_lat, r.he_lon);
    let (a, b) = if e1.0 < e2.0 || (e1.0 == e2.0 && e1.1 <= e2.1) {
        (e1, e2)
    } else {
        (e2, e1)
    };
    format!(
        "{}:{:.5},{:.5}|{:.5},{:.5}",
        parent_canonical.0, a.0, a.1, b.0, b.1
    )
}

/// Airports: same identity key implies ident equality. Coordinates
/// decide Exact / Probable / Conflict. Name-only matching never happens.
pub fn match_airports(a: &CanonicalAirport, b: &CanonicalAirport) -> MatchOutcome {
    let mut evidence = vec![
        EvidenceFact::IdentEqual(a.ident.trim().to_uppercase()),
        EvidenceFact::DistanceNm(distance_nm(
            a.latitude,
            a.longitude,
            b.latitude,
            b.longitude,
        )),
    ];
    if a.iso_country == b.iso_country
        && let Some(country) = &a.iso_country
    {
        evidence.push(EvidenceFact::CountryEqual(country.clone()));
    }
    let d = distance_nm(a.latitude, a.longitude, b.latitude, b.longitude);
    if d <= EXACT_NM {
        MatchOutcome::Exact(evidence)
    } else if d <= PROBABLE_NM {
        MatchOutcome::Probable(evidence)
    } else {
        MatchOutcome::Conflict {
            category: "identity".to_string(),
            severity: ConflictSeverity::Error,
            evidence,
        }
    }
}

/// Navaids: same identity key implies ident + kind class + region
/// equal. Coordinates decide Exact/Probable; far apart = Conflict.
/// Frequency differences are FIELD conflicts recorded by the engine,
/// never identity breaks.
pub fn match_navaids(a: &CanonicalNavaid, b: &CanonicalNavaid) -> MatchOutcome {
    let mut evidence = vec![
        EvidenceFact::IdentEqual(a.ident.trim().to_uppercase()),
        EvidenceFact::KindEqual(navaid_kind_class(&a.kind).to_string()),
        EvidenceFact::DistanceNm(distance_nm(
            a.latitude,
            a.longitude,
            b.latitude,
            b.longitude,
        )),
    ];
    if let (Some(ra), Some(rb)) = (&a.region_code, &b.region_code)
        && ra.trim().eq_ignore_ascii_case(rb.trim())
    {
        evidence.push(EvidenceFact::RegionEqual(ra.clone()));
    }
    let d = distance_nm(a.latitude, a.longitude, b.latitude, b.longitude);
    if d <= EXACT_NM {
        MatchOutcome::Exact(evidence)
    } else if d <= PROBABLE_NM {
        MatchOutcome::Probable(evidence)
    } else {
        MatchOutcome::Conflict {
            category: "identity".to_string(),
            severity: ConflictSeverity::Error,
            evidence,
        }
    }
}

/// Region-less fallback matching: both regions present and DIFFERENT
/// => Distinct (never merged across regions); one side region-less =>
/// coordinates decide Exact/Probable/Distinct. Far apart with a missing
/// region is Distinct, not Conflict: same ident may legitimately exist
/// in another country.
pub fn match_navaids_fallback(a: &CanonicalNavaid, b: &CanonicalNavaid) -> MatchOutcome {
    let evidence = vec![
        EvidenceFact::IdentEqual(a.ident.trim().to_uppercase()),
        EvidenceFact::KindEqual(navaid_kind_class(&a.kind).to_string()),
        EvidenceFact::DistanceNm(distance_nm(
            a.latitude,
            a.longitude,
            b.latitude,
            b.longitude,
        )),
    ];
    if let (Some(ra), Some(rb)) = (&a.region_code, &b.region_code)
        && !ra.trim().eq_ignore_ascii_case(rb.trim())
    {
        return MatchOutcome::Distinct(evidence);
    }
    let d = distance_nm(a.latitude, a.longitude, b.latitude, b.longitude);
    if d <= EXACT_NM {
        MatchOutcome::Exact(evidence)
    } else if d <= PROBABLE_NM {
        MatchOutcome::Probable(evidence)
    } else {
        MatchOutcome::Distinct(evidence)
    }
}

/// Waypoints: same identity key implies ident + region equal; the
/// engine never pairs different regions.
pub fn match_waypoints(a: &CanonicalWaypoint, b: &CanonicalWaypoint) -> MatchOutcome {
    let evidence = vec![
        EvidenceFact::IdentEqual(a.ident.trim().to_uppercase()),
        EvidenceFact::RegionEqual(a.region_code.clone()),
        EvidenceFact::DistanceNm(distance_nm(
            a.latitude,
            a.longitude,
            b.latitude,
            b.longitude,
        )),
    ];
    let d = distance_nm(a.latitude, a.longitude, b.latitude, b.longitude);
    if d <= EXACT_NM {
        MatchOutcome::Exact(evidence)
    } else if d <= PROBABLE_NM {
        MatchOutcome::Probable(evidence)
    } else {
        MatchOutcome::Distinct(evidence)
    }
}
