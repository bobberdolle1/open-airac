//! OpenAIRAC multi-source entity reconciliation (v0.4 S8).
//!
//! Answers "do these provider records describe the same real-world
//! aviation entity?" WITHOUT destroying provider-native identity,
//! provenance, or temporal history.
//!
//! Core principle: provider rows are immutable facts; reconciliation
//! creates relationships ABOVE them (canonical identities +
//! memberships + conflicts). A matcher that is not confident never
//! merges: only Exact/Probable create memberships; Ambiguous stays
//! separate and is surfaced as a diagnostic. Precision beats recall —
//! a missed match is preferable to a false merge.

mod authority;
mod matchers;
mod resolve;

pub use matchers::{
    EXACT_NM, IDENT_CHANGE_NM, MatchOutcome, PROBABLE_NM, RUNWAY_ENDPOINT_NM, airport_identity_key,
    canonical_id_for, is_icao_ident, navaid_canonical_key, navaid_fallback_key,
    navaid_identity_key, runway_geometry_key, waypoint_identity_key,
};
pub use resolve::{FIELD_SELECTORS, resolved_entity};

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::{
    Connection, WorldStore, insert_identity_continuity_conn, insert_reconciliation_conflict_conn,
    query_airports_at_conn, query_navaids_at_conn, query_waypoints_at_conn,
    upsert_canonical_identity_conn, upsert_membership_conn,
};
use rusqlite::OptionalExtension;
use std::collections::HashMap;

pub type RunStats = ReconciliationStats;

/// The reconciler. Deterministic and idempotent: re-running on
/// unchanged source state writes nothing new (all upserts keyed) and
/// returns the same statistics.
pub struct Reconciler<'a> {
    store: &'a WorldStore,
}

impl<'a> Reconciler<'a> {
    pub fn new(store: &'a WorldStore) -> Self {
        Self { store }
    }

    /// Reconcile the world valid at `as_of`.
    pub fn reconcile(&self, as_of: DateTime<Utc>) -> Result<RunStats> {
        let conn = self.store.raw_conn();
        let provider_of = |sid: &SourceSnapshotId| -> String {
            conn.query_row(
                "SELECT provider FROM source_snapshots WHERE id = ?1",
                [sid.0.as_str()],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or_else(|| "unknown".to_string())
        };

        let mut stats = RunStats::default();
        let airports = query_airports_at_conn(conn, as_of)?;
        let navaids = query_navaids_at_conn(conn, as_of)?;
        let waypoints = query_waypoints_at_conn(conn, as_of)?;
        stats.source_entities = airports.len() + navaids.len() + waypoints.len();

        // Candidate indexes by natural identity key (O(1) bucketing).
        let mut airport_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, a) in airports.iter().enumerate() {
            if let Some(key) = airport_identity_key(a) {
                airport_index.entry(key).or_default().push(i);
            }
        }
        let mut navaid_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut navaid_fallback: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in navaids.iter().enumerate() {
            // Paired DME rows are COMPONENTS of a VORTAC/VOR-DME
            // facility, not independent real-world facilities: they must
            // never appear as reconciliation candidates.
            if n.dme_paired {
                continue;
            }
            navaid_index
                .entry(navaid_identity_key(n))
                .or_default()
                .push(i);
            navaid_fallback
                .entry(navaid_fallback_key(n))
                .or_default()
                .push(i);
        }
        let mut waypoint_index: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, w) in waypoints.iter().enumerate() {
            waypoint_index
                .entry(waypoint_identity_key(w))
                .or_default()
                .push(i);
        }

        let mut airport_canonical: HashMap<String, CanonicalEntityId> = HashMap::new();
        for key in airport_index.keys() {
            airport_canonical.insert(key.clone(), canonical_id_for("apt", key));
        }

        // ---- Airports ----
        for idxs in airport_index.values() {
            let (pairs, ambiguous) = cross_provider_pairs(
                idxs,
                &airports,
                &|a: &CanonicalAirport| &a.temporal.source_snapshot_id,
                &provider_of,
            );
            for (a, b) in pairs {
                stats.candidate_pairs += 1;
                let key = airport_identity_key(&airports[a]).expect("indexed");
                let canonical = canonical_id_for("apt", &key);
                let ref_a = SourceEntityRef {
                    provider: provider_of(&airports[a].temporal.source_snapshot_id),
                    entity_table: "airports".to_string(),
                    entity_id: airports[a].id.0.clone(),
                };
                let ref_b = SourceEntityRef {
                    provider: provider_of(&airports[b].temporal.source_snapshot_id),
                    entity_table: "airports".to_string(),
                    entity_id: airports[b].id.0.clone(),
                };
                if ambiguous {
                    self.apply_ambiguous(conn, &canonical, "airports", &ref_a, &ref_b, &mut stats)?;
                    continue;
                }
                let outcome = matchers::match_airports(&airports[a], &airports[b]);
                self.apply_outcome(
                    conn,
                    &outcome,
                    &canonical,
                    "airports",
                    (&ref_a, &airports[a].temporal),
                    (&ref_b, &airports[b].temporal),
                    &mut stats,
                )?;
            }
        }

        // ---- Navaids ----
        for idxs in navaid_index.values() {
            let (pairs, ambiguous) = cross_provider_pairs(
                idxs,
                &navaids,
                &|n: &CanonicalNavaid| &n.temporal.source_snapshot_id,
                &provider_of,
            );
            for (a, b) in pairs {
                stats.candidate_pairs += 1;
                let key = navaid_identity_key(&navaids[a]);
                let canonical = canonical_id_for("nav", &key);
                if ambiguous {
                    let ref_a = SourceEntityRef {
                        provider: provider_of(&navaids[a].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[a].object_id.0.clone(),
                    };
                    let ref_b = SourceEntityRef {
                        provider: provider_of(&navaids[b].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[b].object_id.0.clone(),
                    };
                    self.apply_ambiguous(conn, &canonical, "navaids", &ref_a, &ref_b, &mut stats)?;
                    continue;
                }
                {
                    let ref_a = SourceEntityRef {
                        provider: provider_of(&navaids[a].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[a].object_id.0.clone(),
                    };
                    let ref_b = SourceEntityRef {
                        provider: provider_of(&navaids[b].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[b].object_id.0.clone(),
                    };
                    let outcome = matchers::match_navaids(&navaids[a], &navaids[b]);
                    self.apply_outcome(
                        conn,
                        &outcome,
                        &canonical,
                        "navaids",
                        (&ref_a, &navaids[a].temporal),
                        (&ref_b, &navaids[b].temporal),
                        &mut stats,
                    )?;
                    // Same-instant frequency disagreement is a field
                    // conflict (identity persists; frequency is payload).
                    if navaids[a].frequency != navaids[b].frequency {
                        let severity = ConflictSeverity::Warning;
                        insert_reconciliation_conflict_conn(
                            conn,
                            &ReconciliationConflict {
                                id: 0,
                                entity_table: "navaids".to_string(),
                                canonical_id: Some(canonical),
                                ref_a: ref_a.display(),
                                ref_b: ref_b.display(),
                                category: "field".to_string(),
                                field_name: Some("frequency_khz".to_string()),
                                value_a: Some(navaids[a].frequency.0.to_string()),
                                value_b: Some(navaids[b].frequency.0.to_string()),
                                severity,
                                evidence: vec![
                                    EvidenceFact::IdentEqual(navaids[a].ident.clone()),
                                    EvidenceFact::FrequencyKhz(navaids[a].frequency.0),
                                    EvidenceFact::FrequencyKhz(navaids[b].frequency.0),
                                ],
                                detected_at: as_of,
                                resolved: None,
                            },
                        )?;
                    }
                }
            }
        }

        // ---- Navaids, region-less fallback ----
        // Providers without ICAO region codes (OurAirports) cannot join
        // the strict (kind, ident, region) buckets; pair them by
        // (kind, ident) with coordinate-gated evidence instead.
        {
            let mut full_key_pairs: std::collections::HashSet<(usize, usize)> =
                std::collections::HashSet::new();
            for idxs in navaid_index.values() {
                for i in 0..idxs.len() {
                    for j in (i + 1)..idxs.len() {
                        let pair = (idxs[i].min(idxs[j]), idxs[i].max(idxs[j]));
                        full_key_pairs.insert(pair);
                    }
                }
            }
            for idxs in navaid_fallback.values() {
                let (pairs, _bucket_ambiguous) = cross_provider_pairs(
                    idxs,
                    &navaids,
                    &|n: &CanonicalNavaid| &n.temporal.source_snapshot_id,
                    &provider_of,
                );
                // Coordinate evidence disambiguates: a pair is Ambiguous
                // only when a source has MORE THAN ONE partner within the
                // probable band. Pairs beyond the band are Distinct.
                let mut partner_counts: HashMap<usize, usize> = HashMap::new();
                for &(a, b) in &pairs {
                    let pair = (a.min(b), a.max(b));
                    if full_key_pairs.contains(&pair) {
                        continue;
                    }
                    let d = matchers::distance_nm(
                        navaids[a].latitude,
                        navaids[a].longitude,
                        navaids[b].latitude,
                        navaids[b].longitude,
                    );
                    if d <= PROBABLE_NM {
                        *partner_counts.entry(a).or_default() += 1;
                        *partner_counts.entry(b).or_default() += 1;
                    }
                }
                for (a, b) in pairs {
                    let pair = (a.min(b), a.max(b));
                    if full_key_pairs.contains(&pair) {
                        continue; // already decided by the strict bucket
                    }
                    stats.candidate_pairs += 1;
                    // Candidate discovery key != identity key: the
                    // canonical identity derives from the strongest
                    // natural identity (region-bearing side wins).
                    let key = navaid_canonical_key(&navaids[a], &navaids[b]);
                    let canonical = canonical_id_for("nav", &key);
                    let ref_a = SourceEntityRef {
                        provider: provider_of(&navaids[a].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[a].object_id.0.clone(),
                    };
                    let ref_b = SourceEntityRef {
                        provider: provider_of(&navaids[b].temporal.source_snapshot_id),
                        entity_table: "navaids".to_string(),
                        entity_id: navaids[b].object_id.0.clone(),
                    };
                    let d = matchers::distance_nm(
                        navaids[a].latitude,
                        navaids[a].longitude,
                        navaids[b].latitude,
                        navaids[b].longitude,
                    );
                    let ambiguous = d <= PROBABLE_NM
                        && (partner_counts.get(&a).copied().unwrap_or(0) > 1
                            || partner_counts.get(&b).copied().unwrap_or(0) > 1);
                    if ambiguous {
                        self.apply_ambiguous(
                            conn, &canonical, "navaids", &ref_a, &ref_b, &mut stats,
                        )?;
                        continue;
                    }
                    let outcome = matchers::match_navaids_fallback(&navaids[a], &navaids[b]);
                    self.apply_outcome(
                        conn,
                        &outcome,
                        &canonical,
                        "navaids",
                        (&ref_a, &navaids[a].temporal),
                        (&ref_b, &navaids[b].temporal),
                        &mut stats,
                    )?;
                }
            }
        }

        // ---- Waypoints ----
        for idxs in waypoint_index.values() {
            let (pairs, ambiguous) = cross_provider_pairs(
                idxs,
                &waypoints,
                &|w: &CanonicalWaypoint| &w.temporal.source_snapshot_id,
                &provider_of,
            );
            for (a, b) in pairs {
                stats.candidate_pairs += 1;
                let key = waypoint_identity_key(&waypoints[a]);
                let canonical = canonical_id_for("wpt", &key);
                if ambiguous {
                    let ref_a = SourceEntityRef {
                        provider: provider_of(&waypoints[a].temporal.source_snapshot_id),
                        entity_table: "waypoints".to_string(),
                        entity_id: waypoints[a].object_id.0.clone(),
                    };
                    let ref_b = SourceEntityRef {
                        provider: provider_of(&waypoints[b].temporal.source_snapshot_id),
                        entity_table: "waypoints".to_string(),
                        entity_id: waypoints[b].object_id.0.clone(),
                    };
                    self.apply_ambiguous(
                        conn,
                        &canonical,
                        "waypoints",
                        &ref_a,
                        &ref_b,
                        &mut stats,
                    )?;
                    continue;
                }
                {
                    let ref_a = SourceEntityRef {
                        provider: provider_of(&waypoints[a].temporal.source_snapshot_id),
                        entity_table: "waypoints".to_string(),
                        entity_id: waypoints[a].object_id.0.clone(),
                    };
                    let ref_b = SourceEntityRef {
                        provider: provider_of(&waypoints[b].temporal.source_snapshot_id),
                        entity_table: "waypoints".to_string(),
                        entity_id: waypoints[b].object_id.0.clone(),
                    };
                    let outcome = matchers::match_waypoints(&waypoints[a], &waypoints[b]);
                    self.apply_outcome(
                        conn,
                        &outcome,
                        &canonical,
                        "waypoints",
                        (&ref_a, &waypoints[a].temporal),
                        (&ref_b, &waypoints[b].temporal),
                        &mut stats,
                    )?;
                }
            }
        }

        // ---- Runway physical identity + renumbering continuity ----
        {
            let mut by_geometry: HashMap<String, Vec<(&CanonicalRunway, String)>> = HashMap::new();
            for airport in &airports {
                let Some(parent_key) = airport_identity_key(airport) else {
                    continue;
                };
                let parent_canonical = airport_canonical
                    .get(&parent_key)
                    .cloned()
                    .unwrap_or_else(|| canonical_id_for("apt", &parent_key));
                for runway in &airport.runways {
                    by_geometry
                        .entry(runway_geometry_key(&parent_canonical, runway))
                        .or_default()
                        .push((runway, provider_of(&airport.temporal.source_snapshot_id)));
                }
            }
            for (geometry_key, idxs) in &by_geometry {
                for i in 0..idxs.len() {
                    for j in (i + 1)..idxs.len() {
                        let ((a, pa), (b, pb)) = (&idxs[i], &idxs[j]);
                        if pa == pb {
                            continue;
                        }
                        stats.candidate_pairs += 1;
                        let canonical = canonical_id_for("rwy", geometry_key);
                        let ref_a = SourceEntityRef {
                            provider: pa.clone(),
                            entity_table: "runways".to_string(),
                            entity_id: a.id.0.clone(),
                        };
                        let ref_b = SourceEntityRef {
                            provider: pb.clone(),
                            entity_table: "runways".to_string(),
                            entity_id: b.id.0.clone(),
                        };
                        let outcome = MatchOutcome::Exact(vec![EvidenceFact::RunwayGeometryEqual]);
                        self.apply_outcome(
                            conn,
                            &outcome,
                            &canonical,
                            "runways",
                            (&ref_a, &a.temporal),
                            (&ref_b, &b.temporal),
                            &mut stats,
                        )?;
                        if a.official_designator != b.official_designator {
                            insert_reconciliation_conflict_conn(
                                conn,
                                &ReconciliationConflict {
                                    id: 0,
                                    entity_table: "runways".to_string(),
                                    canonical_id: Some(canonical),
                                    ref_a: ref_a.display(),
                                    ref_b: ref_b.display(),
                                    category: "identity".to_string(),
                                    field_name: Some("official_designator".to_string()),
                                    value_a: Some(a.official_designator.clone()),
                                    value_b: Some(b.official_designator.clone()),
                                    severity: ConflictSeverity::Info,
                                    evidence: vec![EvidenceFact::RunwayGeometryEqual],
                                    detected_at: as_of,
                                    resolved: None,
                                },
                            )?;
                        }
                    }
                }
            }
        }

        // ---- Airport identifier-change continuity (cross-key, coords) ----
        {
            let mut by_cell: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
            for (i, a) in airports.iter().enumerate() {
                let cell = (
                    (a.latitude * 10.0).floor() as i64,
                    (a.longitude * 10.0).floor() as i64,
                );
                by_cell.entry(cell).or_default().push(i);
            }
            for cell_idxs in by_cell.values() {
                for i in 0..cell_idxs.len() {
                    for j in (i + 1)..cell_idxs.len() {
                        let (a, b) = (cell_idxs[i], cell_idxs[j]);
                        let provider_a = provider_of(&airports[a].temporal.source_snapshot_id);
                        let provider_b = provider_of(&airports[b].temporal.source_snapshot_id);
                        if provider_a == provider_b {
                            continue;
                        }
                        if airports[a].ident.eq_ignore_ascii_case(&airports[b].ident) {
                            continue; // already handled by identity-key pass
                        }
                        let d = matchers::distance_nm(
                            airports[a].latitude,
                            airports[a].longitude,
                            airports[b].latitude,
                            airports[b].longitude,
                        );
                        let country_eq = airports[a].iso_country.as_deref().unwrap_or("")
                            == airports[b].iso_country.as_deref().unwrap_or("");
                        if country_eq && d <= IDENT_CHANGE_NM {
                            let key_a = airport_identity_key(&airports[a]).expect("indexed");
                            let key_b = airport_identity_key(&airports[b]).expect("indexed");
                            let canonical_a = canonical_id_for("apt", &key_a);
                            let canonical_b = canonical_id_for("apt", &key_b);
                            insert_identity_continuity_conn(
                                conn,
                                &canonical_a,
                                &canonical_b,
                                &[
                                    EvidenceFact::DistanceNm(d),
                                    EvidenceFact::CountryEqual(
                                        airports[a].iso_country.clone().unwrap_or_default(),
                                    ),
                                ],
                            )?;
                        }
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Cross-provider candidate pairs inside one identity-key bucket.
    /// More than one entity per provider side (or >2 providers) makes
    /// every pair Ambiguous: never merged, surfaced as diagnostics.
    fn apply_ambiguous(
        &self,
        conn: &Connection,
        canonical: &CanonicalEntityId,
        entity_table: &str,
        ref_a: &SourceEntityRef,
        ref_b: &SourceEntityRef,
        stats: &mut RunStats,
    ) -> Result<()> {
        stats.ambiguous += 1;
        stats.conflicts += 1;
        insert_reconciliation_conflict_conn(
            conn,
            &ReconciliationConflict {
                id: 0,
                entity_table: entity_table.to_string(),
                canonical_id: None,
                ref_a: ref_a.display(),
                ref_b: ref_b.display(),
                category: "ambiguity".to_string(),
                field_name: None,
                value_a: None,
                value_b: None,
                severity: ConflictSeverity::Info,
                evidence: vec![],
                detected_at: Utc::now(),
                resolved: None,
            },
        )?;
        let _ = canonical;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_outcome(
        &self,
        conn: &Connection,
        outcome: &MatchOutcome,
        canonical: &CanonicalEntityId,
        entity_table: &str,
        member_a: (&SourceEntityRef, &TemporalValidity),
        member_b: (&SourceEntityRef, &TemporalValidity),
        stats: &mut RunStats,
    ) -> Result<()> {
        let (ref_a, temporal_a) = member_a;
        let (ref_b, temporal_b) = member_b;
        match outcome {
            MatchOutcome::Exact(evidence) | MatchOutcome::Probable(evidence) => {
                let confidence = if matches!(outcome, MatchOutcome::Exact(_)) {
                    MatchConfidence::Exact
                } else {
                    MatchConfidence::Probable
                };
                let method = match confidence {
                    MatchConfidence::Exact => "ident+coords",
                    MatchConfidence::Probable => "ident+coords(probable)",
                };
                upsert_canonical_identity_conn(
                    conn,
                    canonical,
                    entity_table,
                    &canonical.0,
                    None,
                    Utc::now(),
                )?;
                for (source_ref, temporal) in [(&ref_a, temporal_a), (&ref_b, temporal_b)] {
                    upsert_membership_conn(
                        conn,
                        &SourceMembership {
                            canonical_id: canonical.clone(),
                            source: (*source_ref).clone(),
                            // The membership interval is the SOURCE
                            // revision interval, copied exactly — never
                            // approximated or left open-ended.
                            valid_from: temporal.valid_from,
                            valid_until: temporal.valid_until,
                            confidence,
                            match_method: method.to_string(),
                            evidence: evidence.clone(),
                            status: MembershipStatus::Active,
                        },
                    )?;
                }
                match confidence {
                    MatchConfidence::Exact => stats.exact_matches += 1,
                    MatchConfidence::Probable => stats.probable_matches += 1,
                }
            }
            MatchOutcome::Ambiguous(evidence) => {
                stats.ambiguous += 1;
                stats.conflicts += 1;
                insert_reconciliation_conflict_conn(
                    conn,
                    &ReconciliationConflict {
                        id: 0,
                        entity_table: entity_table.to_string(),
                        canonical_id: None,
                        ref_a: ref_a.display(),
                        ref_b: ref_b.display(),
                        category: "ambiguity".to_string(),
                        field_name: None,
                        value_a: None,
                        value_b: None,
                        severity: ConflictSeverity::Info,
                        evidence: evidence.clone(),
                        detected_at: Utc::now(),
                        resolved: None,
                    },
                )?;
            }
            MatchOutcome::Conflict {
                category,
                severity,
                evidence,
            } => {
                stats.conflicts += 1;
                insert_reconciliation_conflict_conn(
                    conn,
                    &ReconciliationConflict {
                        id: 0,
                        entity_table: entity_table.to_string(),
                        canonical_id: Some(canonical.clone()),
                        ref_a: ref_a.display(),
                        ref_b: ref_b.display(),
                        category: category.clone(),
                        field_name: None,
                        value_a: None,
                        value_b: None,
                        severity: *severity,
                        evidence: evidence.clone(),
                        detected_at: Utc::now(),
                        resolved: None,
                    },
                )?;
            }
            MatchOutcome::Distinct(_) => {
                stats.distinct_rejected += 1;
            }
        }
        Ok(())
    }
}

/// Cross-provider candidate pairs inside one identity-key bucket.
///
/// Same-provider rows are temporal revisions, never candidates. When
/// any provider side has more than one entity (or more than two
/// providers share the key), every cross pair is Ambiguous.
#[cfg(test)]
mod tests;

fn cross_provider_pairs<T>(
    idxs: &[usize],
    rows: &[T],
    snapshot_of: &impl Fn(&T) -> &SourceSnapshotId,
    provider_of: &impl Fn(&SourceSnapshotId) -> String,
) -> (Vec<(usize, usize)>, bool) {
    let mut by_provider: HashMap<String, Vec<usize>> = HashMap::new();
    for &i in idxs {
        by_provider
            .entry(provider_of(snapshot_of(&rows[i])))
            .or_default()
            .push(i);
    }
    if by_provider.len() < 2 {
        return (Vec::new(), false);
    }
    let ambiguous = by_provider.len() > 2 || by_provider.values().any(|v| v.len() > 1);
    let providers: Vec<&Vec<usize>> = by_provider.values().collect();
    let mut pairs = Vec::new();
    for i in 0..providers.len() {
        for j in (i + 1)..providers.len() {
            for &a in providers[i] {
                for &b in providers[j] {
                    pairs.push((a, b));
                }
            }
        }
    }
    (pairs, ambiguous)
}
