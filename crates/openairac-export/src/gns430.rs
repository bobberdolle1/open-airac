//! Legacy Garmin GNS430 Navdata Exporter.
//!
//! Generates the standard legacy GNS430 navigation dataset used by X-Plane GNS430/530 GPS units,
//! classic aircraft FMC plugins, and third-party legacy avionics.
//!
//! Output Directory Structure:
//! - `Airports.txt`: Airport header and reference information.
//! - `Navaids.txt`: VOR, VOR-DME, VORTAC, NDB, and DME radio navigation facilities.
//! - `Waypoints.txt`: 5-letter enroute and terminal navigation waypoints.
//! - `ATS.txt`: High and Low enroute airway segments.
//! - `Proc/<ICAO>.txt`: Individual terminal procedure files (SIDs, STARs, Approaches).
//! - `cycle_info.txt`: Cycle metadata and validity header.

use crate::{ArtifactEntry, FormatExporter, GeneratedArtifactSet, families};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Legacy GNS430 Format Exporter
pub struct Gns430Exporter;

impl FormatExporter for Gns430Exporter {
    fn family(&self) -> crate::FormatFamilyId {
        families::gns430_text()
    }

    fn export(
        &self,
        store: &WorldStore,
        as_of: DateTime<Utc>,
        out_dir: &Path,
    ) -> Result<GeneratedArtifactSet> {
        let cycle = store
            .query_dataset_versions()?
            .iter()
            .filter_map(|v| v.airac_cycle.clone())
            .max()
            .unwrap_or_else(|| "2608".to_string());
        let world_fp = format!("fp-gns430-{}-{}", cycle, as_of.timestamp());
        std::fs::create_dir_all(out_dir).context("creating gns430 export dir")?;
        let proc_dir = out_dir.join("Proc");
        std::fs::create_dir_all(&proc_dir).context("creating gns430 Proc dir")?;

        let mut artifacts = Vec::new();

        // 1. Airports.txt
        let airports = store.query_airports_at(as_of)?;
        let mut airports_txt = String::new();
        airports_txt.push_str("// OpenAIRAC GNS430 Airports\n");
        airports_txt.push_str(
            "// Format: A,ICAO,Name,Lat,Lon,Elevation,TransAlt,SpeedLimit,SpeedLimitAlt\n",
        );

        for apt in &airports {
            let elev = apt.elevation_ft.unwrap_or(0.0).round() as i32;
            airports_txt.push_str(&format!(
                "A,{},{},{:.6},{:.6},{},18000,250,10000\n",
                apt.ident,
                apt.name.replace(',', " "),
                apt.latitude,
                apt.longitude,
                elev
            ));

            for rwy in &apt.runways {
                let length = rwy.length_ft;
                let width = rwy.width_ft.unwrap_or(150);
                let hdg = rwy.true_heading().round() as i32;
                airports_txt.push_str(&format!(
                    "R,{},{},{},{},0,0.0,0,{:.6},{:.6},{},3.00,50,{}\n",
                    rwy.official_designator,
                    hdg,
                    length,
                    width,
                    rwy.le_lat,
                    rwy.le_lon,
                    elev,
                    rwy.surface.as_deref().unwrap_or("ASPH")
                ));
            }
        }

        let airports_path = out_dir.join("Airports.txt");
        std::fs::write(&airports_path, &airports_txt)?;
        artifacts.push(ArtifactEntry {
            path: "Airports.txt".to_string(),
            sha256: sha256_hex(airports_txt.as_bytes()),
            size: airports_txt.len() as u64,
            kind: "navdata-airports".to_string(),
        });

        // 2. Navaids.txt
        let navaids = store.query_navaids_at(as_of)?;
        let mut navaids_txt = String::new();
        navaids_txt.push_str("// OpenAIRAC GNS430 Navaids\n");
        navaids_txt
            .push_str("// Format: Kind,Ident,Name,Freq,Class,Lat,Lon,Elev,SlavedVar,MagVar\n");

        for nav in &navaids {
            let elev = nav.elevation_ft.unwrap_or(0);
            let mag_var = nav.magnetic_variation_deg.unwrap_or(0.0);
            let slaved = nav.slaved_variation_deg.unwrap_or(mag_var);
            let freq_mhz = nav.frequency.to_mhz();

            match nav.kind {
                NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac | NavaidKind::Rsbn => {
                    navaids_txt.push_str(&format!(
                        "V,{},{},{:.3},H,{:.6},{:.6},{},{:.1},{:.1}\n",
                        nav.ident,
                        nav.name.replace(',', " "),
                        freq_mhz,
                        nav.latitude,
                        nav.longitude,
                        elev,
                        slaved,
                        mag_var
                    ));
                }
                NavaidKind::Ndb => {
                    let freq_khz = nav.frequency.0;
                    navaids_txt.push_str(&format!(
                        "N,{},{},{},H,{:.6},{:.6},{},{:.1}\n",
                        nav.ident,
                        nav.name.replace(',', " "),
                        freq_khz,
                        nav.latitude,
                        nav.longitude,
                        elev,
                        mag_var
                    ));
                }
                NavaidKind::Dme => {
                    navaids_txt.push_str(&format!(
                        "D,{},{},{:.3},H,{:.6},{:.6},{},{:.1},{:.1}\n",
                        nav.ident,
                        nav.name.replace(',', " "),
                        freq_mhz,
                        nav.latitude,
                        nav.longitude,
                        elev,
                        slaved,
                        mag_var
                    ));
                }
                _ => {}
            }
        }

        let navaids_path = out_dir.join("Navaids.txt");
        std::fs::write(&navaids_path, &navaids_txt)?;
        artifacts.push(ArtifactEntry {
            path: "Navaids.txt".to_string(),
            sha256: sha256_hex(navaids_txt.as_bytes()),
            size: navaids_txt.len() as u64,
            kind: "navdata-navaids".to_string(),
        });

        // 3. Waypoints.txt
        let waypoints = store.query_waypoints_at(as_of)?;
        let mut waypoints_txt = String::new();
        waypoints_txt.push_str("// OpenAIRAC GNS430 Waypoints\n");
        waypoints_txt.push_str("// Format: W,Ident,Lat,Lon,Collocated,Region\n");

        for wp in &waypoints {
            waypoints_txt.push_str(&format!(
                "W,{},{:.6},{:.6},0,{}\n",
                wp.ident,
                wp.latitude,
                wp.longitude,
                if wp.region_code.is_empty() {
                    "K2"
                } else {
                    &wp.region_code
                }
            ));
        }

        let waypoints_path = out_dir.join("Waypoints.txt");
        std::fs::write(&waypoints_path, &waypoints_txt)?;
        artifacts.push(ArtifactEntry {
            path: "Waypoints.txt".to_string(),
            sha256: sha256_hex(waypoints_txt.as_bytes()),
            size: waypoints_txt.len() as u64,
            kind: "navdata-waypoints".to_string(),
        });

        // 4. ATS.txt (Airways)
        let airway_legs = store.query_airway_legs_at(as_of)?;
        let wp_map: BTreeMap<String, (f64, f64)> = waypoints
            .iter()
            .map(|w| (w.ident.clone(), (w.latitude, w.longitude)))
            .collect();

        let mut ats_txt = String::new();
        ats_txt.push_str("// OpenAIRAC GNS430 Airways\n");
        ats_txt.push_str("// Format: A,Airway,Seq,StartFix,StartLat,StartLon,EndFix,EndLat,EndLon,Dir,MinAlt,MaxAlt\n");

        for leg in &airway_legs {
            let start_pt = wp_map.get(&leg.start_fix).copied().unwrap_or((0.0, 0.0));
            let end_pt = wp_map.get(&leg.end_fix).copied().unwrap_or((0.0, 0.0));
            let min_alt = leg.minimum_altitude_ft.unwrap_or(1000);
            let max_alt = leg.maximum_altitude_ft.unwrap_or(45000);

            ats_txt.push_str(&format!(
                "A,{},{},{},{:.6},{:.6},{},{:.6},{:.6},{},{},{}\n",
                leg.route_ident,
                leg.sequence_number,
                leg.start_fix,
                start_pt.0,
                start_pt.1,
                leg.end_fix,
                end_pt.0,
                end_pt.1,
                leg.direction,
                min_alt,
                max_alt
            ));
        }

        let ats_path = out_dir.join("ATS.txt");
        std::fs::write(&ats_path, &ats_txt)?;
        artifacts.push(ArtifactEntry {
            path: "ATS.txt".to_string(),
            sha256: sha256_hex(ats_txt.as_bytes()),
            size: ats_txt.len() as u64,
            kind: "navdata-airways".to_string(),
        });

        // 5. Proc/<ICAO>.txt (Terminal Procedures)
        let proc_legs = store.query_procedure_legs_at(as_of)?;
        let mut airport_procs: BTreeMap<String, Vec<CanonicalProcedureLeg>> = BTreeMap::new();
        for leg in proc_legs {
            airport_procs
                .entry(leg.airport_ident.clone())
                .or_default()
                .push(leg);
        }

        for (apt_ident, legs) in airport_procs {
            let mut proc_content = String::new();
            proc_content.push_str(&format!(
                "// OpenAIRAC GNS430 Procedures for {}\n",
                apt_ident
            ));
            proc_content.push_str(&format!("A,{}\n", apt_ident));

            // Group by (procedure_kind, procedure_ident)
            let mut groups: BTreeMap<(char, String), Vec<CanonicalProcedureLeg>> = BTreeMap::new();
            for leg in legs {
                groups
                    .entry((leg.procedure_kind, leg.procedure_ident.clone()))
                    .or_default()
                    .push(leg);
            }

            for ((kind_char, proc_ident), mut plegs) in groups {
                plegs.sort_by_key(|l| l.sequence_number);
                let record_code = match kind_char {
                    'D' => 'S', // SID
                    'E' => 'A', // STAR (Arrival)
                    _ => 'I',   // Approach
                };

                let rwy = plegs
                    .first()
                    .map(|l| l.transition_ident.clone())
                    .unwrap_or_default();
                proc_content.push_str(&format!("{},{},ALL,{}\n", record_code, proc_ident, rwy));

                for leg in &plegs {
                    let pt = wp_map.get(&leg.fix_ident).copied().unwrap_or((0.0, 0.0));
                    let term = if leg.path_terminator.is_empty() {
                        "TF"
                    } else {
                        &leg.path_terminator
                    };
                    let alt1 = leg.altitude_1_ft.unwrap_or(0);
                    let alt2 = leg.altitude_2_ft.unwrap_or(0);
                    let spd = leg.speed_limit_kts.unwrap_or(0);
                    let crs = leg.course_a_deg.unwrap_or(0.0);
                    let dist = leg.distance_a_nm.unwrap_or(0.0);

                    proc_content.push_str(&format!(
                        "{},{},{:.6},{:.6},0, ,{},{},{},{},{:.1},{:.1}\n",
                        term,
                        leg.fix_ident,
                        pt.0,
                        pt.1,
                        leg.altitude_descriptor.unwrap_or(' '),
                        alt1,
                        alt2,
                        spd,
                        crs,
                        dist
                    ));
                }
            }

            let file_rel = format!("Proc/{}.txt", apt_ident);
            let file_path = out_dir.join(&file_rel);
            std::fs::write(&file_path, &proc_content)?;
            artifacts.push(ArtifactEntry {
                path: file_rel,
                sha256: sha256_hex(proc_content.as_bytes()),
                size: proc_content.len() as u64,
                kind: "procedure-terminal".to_string(),
            });
        }

        // 6. cycle_info.txt
        let cycle_info = format!(
            "AIRAC Cycle: {}\nEffective: {}\nGenerator: OpenAIRAC GNS430 Exporter v1.0\nWorld Fingerprint: {}\n",
            cycle,
            as_of.to_rfc3339(),
            world_fp
        );
        let cycle_info_path = out_dir.join("cycle_info.txt");
        std::fs::write(&cycle_info_path, &cycle_info)?;
        artifacts.push(ArtifactEntry {
            path: "cycle_info.txt".to_string(),
            sha256: sha256_hex(cycle_info.as_bytes()),
            size: cycle_info.len() as u64,
            kind: "cycle-metadata".to_string(),
        });

        artifacts.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: "openairac-export-gns430".to_string(),
            world_fingerprint: world_fp,
            artifacts,
        })
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gns430_export_structure() {
        let store = WorldStore::open_in_memory().unwrap();
        let exporter = Gns430Exporter;
        let temp_dir = std::env::temp_dir().join(format!(
            "openairac_test_gns430_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let res = exporter
            .export(&store, Utc::now(), &temp_dir)
            .expect("export gns430");
        assert_eq!(res.family, families::gns430_text());
        assert!(temp_dir.join("Airports.txt").exists());
        assert!(temp_dir.join("Navaids.txt").exists());
        assert!(temp_dir.join("Waypoints.txt").exists());
        assert!(temp_dir.join("ATS.txt").exists());
        assert!(temp_dir.join("cycle_info.txt").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
