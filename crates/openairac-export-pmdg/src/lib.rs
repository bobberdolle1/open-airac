//! PMDG classic plaintext navigation data exporter (`wpNav*.txt`).
//!
//! Implementation authority: the public AIRNAV Navdata Data File
//! Definition (Richard Stefan / Terry Yingling) and the PMDG Navdata
//! technical glossary.
//!
//! Formats generated:
//! - `wpNavFIX.txt`: Waypoints and fixes (5-char ident, latitude, longitude)
//! - `wpNavAID.txt`: VOR, NDB, DME navaids with frequencies and range
//! - `wpNavAPT.txt`: Airports and runways with ILS/LOC localizer frequencies
//! - `wpNavRTE.txt`: Airway segments (route name, from fix, to fix)

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_export::{ArtifactEntry, FormatExporter, GeneratedArtifactSet, families};
use openairac_model::NavaidKind;
use openairac_store::WorldStore;
use sha2::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

pub struct PmdgNavdataExporter;

impl FormatExporter for PmdgNavdataExporter {
    fn family(&self) -> openairac_export::FormatFamilyId {
        families::pmdg_text()
    }

    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet> {
        std::fs::create_dir_all(out_dir).with_context(|| format!("creating {:?}", out_dir))?;

        let airports = store.query_airports_at(as_of)?;
        let navaids = store.query_navaids_at(as_of)?;
        let waypoints = store.query_waypoints_at(as_of)?;
        let airway_legs = store.query_airway_legs_at(as_of)?;

        // 1. wpNavFIX.txt
        let mut fix_buf = Vec::new();
        let mut written_fixes: BTreeSet<String> = BTreeSet::new();
        for wp in &waypoints {
            let ident = wp.ident.trim();
            if ident.is_empty() || ident.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if written_fixes.insert(ident.to_string()) {
                writeln!(
                    fix_buf,
                    "{:<5} {:>10.6} {:>11.6}",
                    ident, wp.latitude, wp.longitude
                )?;
            }
        }
        let fix_path = out_dir.join("wpNavFIX.txt");
        std::fs::write(&fix_path, &fix_buf)?;

        // 2. wpNavAID.txt
        let mut aid_buf = Vec::new();
        for nav in &navaids {
            let ident = nav.ident.trim();
            if ident.is_empty() {
                continue;
            }
            let (ntype, freq_str) = match nav.kind {
                NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac => {
                    ("VOR", format!("{:.2}", nav.frequency.0 as f64 / 1000.0))
                }
                NavaidKind::Ndb => ("NDB", format!("{:.2}", nav.frequency.0 as f64 / 100.0)),
                NavaidKind::Dme => ("DME", format!("{:.2}", nav.frequency.0 as f64 / 1000.0)),
                NavaidKind::Tacan => ("TACAN", format!("{:.2}", nav.frequency.0 as f64 / 1000.0)),
                _ => continue, // ILS is in wpNavAPT.txt
            };
            let elev = nav.elevation_ft.unwrap_or(0);
            let range = nav.service_volume_nm.unwrap_or(40);
            let magvar = nav.magnetic_variation_deg.unwrap_or(0.0);
            let name = if nav.name.len() > 24 {
                &nav.name[..24]
            } else {
                &nav.name
            };
            writeln!(
                aid_buf,
                "{:<24} {:<5} {:>7} {:<5} {:>10.6} {:>11.6} {:>5} {:>3} {:>5.1}",
                name, ident, freq_str, ntype, nav.latitude, nav.longitude, elev, range, magvar
            )?;
        }
        let aid_path = out_dir.join("wpNavAID.txt");
        std::fs::write(&aid_path, &aid_buf)?;

        // 3. wpNavAPT.txt - index ILS localizers by (airport, runway)
        // end once; avoids an O(runways x navaids) scan per row.
        let mut ils_index: std::collections::HashMap<
            (String, String),
            &openairac_model::CanonicalNavaid,
        > = std::collections::HashMap::new();
        for nav in &navaids {
            if nav.kind != NavaidKind::IlsLocalizer {
                continue;
            }
            if let (Some(apt), Some(rwy)) = (
                nav.associated_airport.as_deref(),
                nav.associated_runway.as_deref(),
            ) {
                ils_index
                    .entry((apt.to_string(), rwy.to_string()))
                    .or_insert(nav);
            }
        }

        let mut apt_buf = Vec::new();
        for airport in &airports {
            let apt_ident = airport.ident.trim();
            let apt_name = if airport.name.len() > 24 {
                &airport.name[..24]
            } else {
                &airport.name
            };
            let apt_elev = airport.elevation_ft.unwrap_or(0.0) as i32;

            for rwy in &airport.runways {
                // Check if an ILS exists for this runway end
                let ils_end = ils_index.get(&(apt_ident.to_string(), rwy.le_ident.clone()));
                let (ils_freq, ils_hdg, ils_type) = if let Some(ils) = ils_end {
                    let f = format!("{:.2}", ils.frequency.0 as f64 / 1000.0);
                    let h = ils
                        .localizer_bearing_mag_deg
                        .or(ils.localizer_bearing_true_deg)
                        .unwrap_or(0.0)
                        .round() as i32;
                    (f, h, "ILS")
                } else {
                    ("0.00".to_string(), 0, "NONE")
                };
                let hdg = rwy.true_heading_deg.unwrap_or(0.0).round() as i32;
                writeln!(
                    apt_buf,
                    "{:<24} {:<4} {:<4} {:>5} {:>3} {:>10.6} {:>11.6} {:>5} {:>6} {:>3} {:<4}",
                    apt_name,
                    apt_ident,
                    rwy.le_ident,
                    rwy.length_ft,
                    hdg,
                    rwy.le_lat,
                    rwy.le_lon,
                    apt_elev,
                    ils_freq,
                    ils_hdg,
                    ils_type
                )?;
            }
        }
        let apt_path = out_dir.join("wpNavAPT.txt");
        std::fs::write(&apt_path, &apt_buf)?;

        // 4. wpNavRTE.txt
        let mut rte_buf = Vec::new();
        let mut routes: BTreeMap<String, Vec<&openairac_model::CanonicalAirwayLeg>> =
            BTreeMap::new();
        for leg in &airway_legs {
            routes.entry(leg.route_ident.clone()).or_default().push(leg);
        }
        for (name, mut legs) in routes {
            legs.sort_by_key(|l| l.sequence_number);
            for leg in legs {
                let start = leg.start_fix.trim();
                let end = leg.end_fix.trim();
                if !start.is_empty() && !end.is_empty() {
                    writeln!(rte_buf, "{:<6} {:<5} {:<5}", name, start, end)?;
                }
            }
        }
        let rte_path = out_dir.join("wpNavRTE.txt");
        std::fs::write(&rte_path, &rte_buf)?;

        // Cycle metadata
        let cycle = openairac_export_xplane::airac_cycle(as_of);
        let meta = serde_json::json!({
            "generator": format!("openairac {}", env!("CARGO_PKG_VERSION")),
            "cycle": cycle,
            "as_of": as_of.to_rfc3339(),
            "format_family": "pmdg-text",
            "schema_authority": "AIRNAV Navdata Data File Definition / PMDG Navdata Technical Glossary (public domain / open interface reference)",
            "support_state": "EXPERIMENTAL"
        });
        std::fs::write(
            out_dir.join("cycle.json"),
            serde_json::to_string_pretty(&meta)?,
        )?;

        let artifact_entries = vec![
            artifact_entry(out_dir, "wpNavFIX.txt", "fixes"),
            artifact_entry(out_dir, "wpNavAID.txt", "navaids"),
            artifact_entry(out_dir, "wpNavAPT.txt", "airports"),
            artifact_entry(out_dir, "wpNavRTE.txt", "routes"),
            artifact_entry(out_dir, "cycle.json", "cycle-metadata"),
        ];

        let meta_sha = artifact_entries.last().unwrap().sha256.clone();

        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: format!("openairac {}", env!("CARGO_PKG_VERSION")),
            world_fingerprint: meta_sha,
            artifacts: artifact_entries,
        })
    }
}

fn artifact_entry(root: &Path, rel: &str, kind: &str) -> ArtifactEntry {
    let data = std::fs::read(root.join(rel)).unwrap_or_default();
    let sha = format!("{:x}", sha2::Sha256::digest(&data));
    ArtifactEntry {
        path: rel.to_string(),
        sha256: sha,
        size: data.len() as u64,
        kind: kind.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        AirportId, CanonicalAirport, CanonicalRunway, RunwayId, SourceSnapshot, SourceSnapshotId,
        TemporalValidity,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_pmdg_test_{}_{}_{n}", std::process::id(), tag))
    }

    fn fixture_store() -> (WorldStore, std::path::PathBuf) {
        let dir = unique_dir("fixture");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = WorldStore::open(dir.join("src.sqlite")).unwrap();
        let t = Utc::now();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-1".to_string()),
                provider: "OurAirports".to_string(),
                dataset: "airports".to_string(),
                provider_revision: None,
                airac_cycle: None,
                effective_from: Some(t),
                effective_until: None,
                retrieved_at: t,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();
        let rwy = CanonicalRunway {
            id: RunwayId("rwy-1".to_string()),
            airport_id: AirportId("ourairports:1".to_string()),
            airport_ident: "KSFO".to_string(),
            official_designator: "28L".to_string(),
            computed_magnetic_designator: None,
            true_heading_deg: Some(284.0),
            length_ft: 11870,
            width_ft: Some(200),
            surface: Some("ASPH".to_string()),
            le_ident: "28L".to_string(),
            le_lat: 37.613539,
            le_lon: -122.357156,
            le_elevation_ft: Some(13.0),
            he_ident: "10R".to_string(),
            he_lat: 37.6188,
            he_lon: -122.375,
            he_elevation_ft: Some(13.0),
            temporal: TemporalValidity {
                valid_from: t,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
            },
        };
        store
            .insert_airport(&CanonicalAirport {
                id: AirportId("ourairports:1".to_string()),
                ident: "KSFO".to_string(),
                name: "San Francisco Intl".to_string(),
                airport_type: "large_airport".to_string(),
                latitude: 37.6188,
                longitude: -122.375,
                elevation_ft: Some(13.0),
                iso_country: Some("US".to_string()),
                municipality: Some("San Francisco".to_string()),
                runways: vec![rwy],
                temporal: TemporalValidity {
                    valid_from: t,
                    valid_until: None,
                    source_snapshot_id: SourceSnapshotId("snap-1".to_string()),
                },
            })
            .unwrap();
        (store, dir)
    }

    #[test]
    fn test_pmdg_export_generates_standard_text_files() {
        let (store, dir) = fixture_store();
        let out = dir.join("pmdg");
        let set = PmdgNavdataExporter
            .export(&store, Utc::now(), &out)
            .unwrap();
        assert_eq!(set.family.as_str(), "pmdg-text");
        assert_eq!(set.artifacts.len(), 5);
        set.verify(&out).unwrap();

        let apt_content = std::fs::read_to_string(out.join("wpNavAPT.txt")).unwrap();
        assert!(
            apt_content.contains("KSFO 28L") && apt_content.contains("11870 284"),
            "{}",
            apt_content
        );
    }
}
