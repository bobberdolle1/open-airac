//! Russian Federation CAICA ATS Route & Enroute Airway Parser.
//!
//! Ingests ATS routes, RNAV enroute airways, and significant reporting points
//! published by CAICA / Rosaviatsiya in the official Manual of ATS Routes
//! (Сборник маршрутов ОВД Российской Федерации) for the Local AIP Vault.
//!
//! Fields:
//! - Route Designator: e.g. `B210`, `G370`, `M864`, `N869`, `T562`, `W31`
//! - Route Segments: Start Fix, End Fix, Sequence
//! - Directionality: `BOTH`, `FORWARD` (EVEN/ODD), `BACKWARD`
//! - Altitude limits: Lower FL / Upper FL / MEA / MORA
//! - Navigation Specification: `RNAV 5`, `RNAV 2`, `CONVENTIONAL`
//! - Associated FIR / Control Sector

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_model::{
    AirwayLegId, CanonicalAirwayLeg, SourceSnapshot, SourceSnapshotId, TemporalValidity,
};
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Summary and validation analysis of the Russian ATS route graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaicaAtsGraphSummary {
    pub total_routes: usize,
    pub total_segments: usize,
    pub unique_nodes: usize,
    pub one_way_segments: usize,
    pub bidirectional_segments: usize,
    pub validation_errors: Vec<String>,
}
pub enum AirwayDirectionality {
    Both,
    ForwardOnly,
    BackwardOnly,
}

/// A parsed segment of an enroute ATS airway.
#[derive(Debug, Clone, PartialEq)]
pub struct CaicaAtsSegment {
    pub route_designator: String,
    pub sequence_number: u32,
    pub start_fix: String,
    pub start_lat: f64,
    pub start_lon: f64,
    pub end_fix: String,
    pub end_lat: f64,
    pub end_lon: f64,
    pub directionality: AirwayDirectionality,
    pub min_altitude_fl: Option<u32>,
    pub max_altitude_fl: Option<u32>,
    pub mea_ft: Option<u32>,
    pub nav_spec: Option<String>,
    pub fir_ident: Option<String>,
}

/// Provider for Russian Federation CAICA ATS Route Manual in Local AIP Vault.
pub struct CaicaAtsProvider {
    pub provider_name: String,
    pub namespace: String,
    pub license: String,
}

impl Default for CaicaAtsProvider {
    fn default() -> Self {
        Self::new("RU_CAICA_ATS_LOCAL", "caica_ats", "CAICA-TermsOfUse")
    }
}

impl CaicaAtsProvider {
    pub fn new(provider_name: &str, namespace: &str, license: &str) -> Self {
        Self {
            provider_name: provider_name.to_string(),
            namespace: namespace.to_string(),
            license: license.to_string(),
        }
    }

    /// Parse ATS route table text or CSV.
    /// Format: `ROUTE,SEQ,START_FIX,START_LAT,START_LON,END_FIX,END_LAT,END_LON,DIR,MIN_FL,MAX_FL,MEA,NAV_SPEC,FIR`
    pub fn parse_ats_table(text: &str) -> Result<Vec<CaicaAtsSegment>> {
        let mut segments = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
            if parts.len() < 8 {
                continue;
            }

            if parts[0].eq_ignore_ascii_case("ROUTE") || parts[0].contains("МАРШРУТ") {
                continue;
            }

            let route = parts[0].to_uppercase();
            let seq: u32 = parts[1].parse().unwrap_or(10);
            let start_fix = parts[2].to_uppercase();
            let start_lat: f64 = parts[3].parse()?;
            let start_lon: f64 = parts[4].parse()?;
            let end_fix = parts[5].to_uppercase();
            let end_lat: f64 = parts[6].parse()?;
            let end_lon: f64 = parts[7].parse()?;

            let dir = if parts.len() > 8 {
                match parts[8].to_uppercase().as_str() {
                    "F" | "FORWARD" | "EVEN" => AirwayDirectionality::ForwardOnly,
                    "B" | "BACKWARD" | "ODD" => AirwayDirectionality::BackwardOnly,
                    _ => AirwayDirectionality::Both,
                }
            } else {
                AirwayDirectionality::Both
            };

            let min_fl: Option<u32> = if parts.len() > 9 && !parts[9].is_empty() {
                parts[9].parse().ok()
            } else {
                None
            };

            let max_fl: Option<u32> = if parts.len() > 10 && !parts[10].is_empty() {
                parts[10].parse().ok()
            } else {
                None
            };

            let mea: Option<u32> = if parts.len() > 11 && !parts[11].is_empty() {
                parts[11].parse().ok()
            } else {
                None
            };

            let nav_spec = if parts.len() > 12 && !parts[12].is_empty() {
                Some(parts[12].to_string())
            } else {
                None
            };

            let fir = if parts.len() > 13 && !parts[13].is_empty() {
                Some(parts[13].to_string())
            } else {
                None
            };

            segments.push(CaicaAtsSegment {
                route_designator: route,
                sequence_number: seq,
                start_fix,
                start_lat,
                start_lon,
                end_fix,
                end_lat,
                end_lon,
                directionality: dir,
                min_altitude_fl: min_fl,
                max_altitude_fl: max_fl,
                mea_ft: mea,
                nav_spec,
                fir_ident: fir,
            });
        }

        Ok(segments)
    }

    /// Analyze and validate the ATS route network graph.
    pub fn analyze_graph(segments: &[CaicaAtsSegment]) -> CaicaAtsGraphSummary {
        let mut routes = BTreeSet::new();
        let mut nodes = BTreeSet::new();
        let mut one_way = 0;
        let mut bi = 0;
        let mut errors = Vec::new();

        for seg in segments {
            routes.insert(seg.route_designator.clone());
            nodes.insert(seg.start_fix.clone());
            nodes.insert(seg.end_fix.clone());

            if seg.start_fix == seg.end_fix {
                errors.push(format!(
                    "Self-loop on route {} fix {}",
                    seg.route_designator, seg.start_fix
                ));
            }

            match seg.directionality {
                AirwayDirectionality::Both => bi += 1,
                AirwayDirectionality::ForwardOnly | AirwayDirectionality::BackwardOnly => {
                    one_way += 1
                }
            }
        }

        CaicaAtsGraphSummary {
            total_routes: routes.len(),
            total_segments: segments.len(),
            unique_nodes: nodes.len(),
            one_way_segments: one_way,
            bidirectional_segments: bi,
            validation_errors: errors,
        }
    }

    /// Ingest ATS segments into WorldStore.
    pub fn ingest_ats_segments(
        &self,
        store: &mut WorldStore,
        segments: &[CaicaAtsSegment],
        effective_from: DateTime<Utc>,
        airac_cycle: Option<&str>,
        source_uri: &str,
    ) -> Result<crate::provider::IngestReport> {
        let content_hash = format!("{:x}", Sha256::digest(source_uri.as_bytes()));
        let snap_id = SourceSnapshotId(format!("caica_ats_{}", &content_hash[..8]));
        let snapshot = SourceSnapshot {
            id: snap_id.clone(),
            provider: self.provider_name.clone(),
            dataset: "CAICA_ATS_ROUTES".to_string(),
            provider_revision: airac_cycle.map(|s| s.to_string()),
            airac_cycle: airac_cycle.map(|s| s.to_string()),
            effective_from: Some(effective_from),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: source_uri.to_string(),
            content_sha256: content_hash.clone(),
            license_id: Some(self.license.clone()),
            license_notes: Some(
                "Official CAICA Russian Federation ATS Route Manual (Local AIP Vault)".to_string(),
            ),
            parser_version: "1.0.0".to_string(),
        };

        store.insert_source_snapshot(&snapshot)?;
        let mut report = crate::provider::IngestReport::new(
            &self.provider_name,
            "CAICA_ATS_ROUTES",
            &content_hash,
        );

        let temporal = TemporalValidity {
            valid_from: effective_from,
            valid_until: None,
            source_snapshot_id: snap_id.clone(),
        };

        store.transact(|conn| {
            for seg in segments {
                let airway_id = AirwayLegId(format!(
                    "{}_{}_{:03}_{}_{}",
                    self.namespace,
                    seg.route_designator,
                    seg.sequence_number,
                    seg.start_fix,
                    seg.end_fix
                ));

                let is_high = seg.min_altitude_fl.unwrap_or(0) >= 180
                    || seg.route_designator.starts_with('M')
                    || seg.route_designator.starts_with('N')
                    || seg.route_designator.starts_with('T');

                let canonical_leg = CanonicalAirwayLeg {
                    object_id: airway_id,
                    route_ident: seg.route_designator.clone(),
                    sequence_number: seg.sequence_number,
                    start_fix: seg.start_fix.clone(),
                    end_fix: seg.end_fix.clone(),
                    route_type: if seg.nav_spec.is_some() {
                        "R".to_string()
                    } else {
                        "O".to_string()
                    },
                    level: Some(if is_high { 'H' } else { 'L' }),
                    start_icao_code: "UU".to_string(),
                    end_icao_code: "UU".to_string(),
                    direction: match seg.directionality {
                        AirwayDirectionality::ForwardOnly => 'F',
                        AirwayDirectionality::BackwardOnly => 'B',
                        AirwayDirectionality::Both => 'N',
                    },
                    minimum_altitude_ft: seg.mea_ft.or(seg.min_altitude_fl.map(|fl| fl * 100)),
                    maximum_altitude_ft: seg.max_altitude_fl.map(|fl| fl * 100),
                    temporal: temporal.clone(),
                };

                openairac_store::insert_airway_leg_conn(conn, &canonical_leg)?;
                report.records_created += 1;
            }
            Ok(())
        })?;

        Ok(report)
    }
}
