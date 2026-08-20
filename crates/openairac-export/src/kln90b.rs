//! Legacy KLN90B GPS Navigation Database Exporter.
//!
//! Generates the navigation database files used by legacy KLN90B GPS simulation units
//! (e.g. Project Tupolev Tu-154, vasFMC, classic Boeing/Airbus addons).
//!
//! Clean-room independent MIT implementation compliant with open KLN90B loaders.
//!
//! Output Layout:
//! - `APT.DAT`: Airport facilities, coordinates, elevations, and runway designations.
//! - `NAV.DAT`: VOR, VOR-DME, NDB, and DME radio navigation facilities.
//! - `WPT.DAT`: Enroute and terminal navigation fixes.
//! - `AWY.DAT`: High and Low airway segment sequences and MEAs.
//! - `FAS.DAT`: Final approach and terminal procedure segment legs.
//! - `cycle.dat`: AIRAC cycle metadata and validity dates.

use crate::{ArtifactEntry, FormatExporter, GeneratedArtifactSet, families};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Legacy KLN90B Format Exporter
pub struct Kln90bExporter;

impl FormatExporter for Kln90bExporter {
    fn family(&self) -> crate::FormatFamilyId {
        families::kln90b_dat()
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

        let world_fp = format!("fp-kln90b-{}-{}", cycle, as_of.timestamp());
        std::fs::create_dir_all(out_dir).context("creating kln90b export dir")?;

        let mut artifacts = Vec::new();

        // 1. APT.DAT (Airports & Runways)
        let airports = store.query_airports_at(as_of)?;
        let mut apt_dat = String::new();
        apt_dat.push_str("; OpenAIRAC KLN90B Airports Database\n");
        apt_dat.push_str("; Format: ICAO,Name,Latitude,Longitude,Elevation,Runways\n");

        for apt in &airports {
            let elev = apt.elevation_ft.unwrap_or(0.0).round() as i32;
            let rwy_str: Vec<String> = apt
                .runways
                .iter()
                .map(|r| r.official_designator.clone())
                .collect();
            apt_dat.push_str(&format!(
                "{},{},{:.6},{:.6},{},{}\n",
                apt.ident,
                apt.name.replace(',', " "),
                apt.latitude,
                apt.longitude,
                elev,
                rwy_str.join("/")
            ));
        }

        let apt_path = out_dir.join("APT.DAT");
        std::fs::write(&apt_path, &apt_dat)?;
        artifacts.push(ArtifactEntry {
            path: "APT.DAT".to_string(),
            sha256: sha256_hex(apt_dat.as_bytes()),
            size: apt_dat.len() as u64,
            kind: "kln-airports".to_string(),
        });

        // 2. NAV.DAT (Navaids)
        let navaids = store.query_navaids_at(as_of)?;
        let mut nav_dat = String::new();
        nav_dat.push_str("; OpenAIRAC KLN90B Radio Navigation Aids\n");
        nav_dat
            .push_str("; Format: Type,Ident,Name,Frequency,Latitude,Longitude,Elevation,MagVar\n");

        for nav in &navaids {
            let elev = nav.elevation_ft.unwrap_or(0);
            let mag_var = nav.magnetic_variation_deg.unwrap_or(0.0);
            let (type_str, freq_val) = match nav.kind {
                NavaidKind::Vor => ("VOR", format!("{:.3}", nav.frequency.to_mhz())),
                NavaidKind::Vordme | NavaidKind::Vortac => {
                    ("VORDME", format!("{:.3}", nav.frequency.to_mhz()))
                }
                NavaidKind::Ndb => ("NDB", format!("{}", nav.frequency.0)),
                NavaidKind::Dme => ("DME", format!("{:.3}", nav.frequency.to_mhz())),
                NavaidKind::IlsLocalizer => ("LOC", format!("{:.3}", nav.frequency.to_mhz())),
                NavaidKind::IlsGlidepath => ("GS", format!("{:.3}", nav.frequency.to_mhz())),
                NavaidKind::Tacan => ("TACAN", format!("{:.3}", nav.frequency.to_mhz())),
            };

            nav_dat.push_str(&format!(
                "{},{},{},{},{:.6},{:.6},{},{:.1}\n",
                type_str,
                nav.ident,
                nav.name.replace(',', " "),
                freq_val,
                nav.latitude,
                nav.longitude,
                elev,
                mag_var
            ));
        }

        let nav_path = out_dir.join("NAV.DAT");
        std::fs::write(&nav_path, &nav_dat)?;
        artifacts.push(ArtifactEntry {
            path: "NAV.DAT".to_string(),
            sha256: sha256_hex(nav_dat.as_bytes()),
            size: nav_dat.len() as u64,
            kind: "kln-navaids".to_string(),
        });

        // 3. WPT.DAT (Waypoints)
        let waypoints = store.query_waypoints_at(as_of)?;
        let mut wpt_dat = String::new();
        wpt_dat.push_str("; OpenAIRAC KLN90B Waypoints\n");
        wpt_dat.push_str("; Format: Ident,Latitude,Longitude,Region\n");

        for wp in &waypoints {
            wpt_dat.push_str(&format!(
                "{},{:.6},{:.6},{}\n",
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

        let wpt_path = out_dir.join("WPT.DAT");
        std::fs::write(&wpt_path, &wpt_dat)?;
        artifacts.push(ArtifactEntry {
            path: "WPT.DAT".to_string(),
            sha256: sha256_hex(wpt_dat.as_bytes()),
            size: wpt_dat.len() as u64,
            kind: "kln-waypoints".to_string(),
        });

        // 4. AWY.DAT (Airways)
        let airway_legs = store.query_airway_legs_at(as_of)?;
        let wp_map: BTreeMap<String, (f64, f64)> = waypoints
            .iter()
            .map(|w| (w.ident.clone(), (w.latitude, w.longitude)))
            .collect();

        let mut awy_dat = String::new();
        awy_dat.push_str("; OpenAIRAC KLN90B Airways\n");
        awy_dat.push_str("; Format: RouteIdent,Seq,FixIdent,Latitude,Longitude,MinAltitude\n");

        for leg in &airway_legs {
            let pt = wp_map.get(&leg.start_fix).copied().unwrap_or((0.0, 0.0));
            let min_alt = leg.minimum_altitude_ft.unwrap_or(1000);
            awy_dat.push_str(&format!(
                "{},{},{},{:.6},{:.6},{}\n",
                leg.route_ident, leg.sequence_number, leg.start_fix, pt.0, pt.1, min_alt
            ));
        }

        let awy_path = out_dir.join("AWY.DAT");
        std::fs::write(&awy_path, &awy_dat)?;
        artifacts.push(ArtifactEntry {
            path: "AWY.DAT".to_string(),
            sha256: sha256_hex(awy_dat.as_bytes()),
            size: awy_dat.len() as u64,
            kind: "kln-airways".to_string(),
        });

        // 5. FAS.DAT (Procedures)
        let proc_legs = store.query_procedure_legs_at(as_of)?;
        let mut fas_dat = String::new();
        fas_dat.push_str("; OpenAIRAC KLN90B Terminal Procedures\n");
        fas_dat.push_str("; Format: Airport,Kind,Procedure,Seq,Terminator,Fix,Latitude,Longitude,Alt1,Alt2,Speed\n");

        for leg in &proc_legs {
            let pt = wp_map.get(&leg.fix_ident).copied().unwrap_or((0.0, 0.0));
            let kind_str = match leg.procedure_kind {
                'D' => "SID",
                'E' => "STAR",
                _ => "APP",
            };
            let term = if leg.path_terminator.is_empty() {
                "TF"
            } else {
                &leg.path_terminator
            };
            let alt1 = leg.altitude_1_ft.unwrap_or(0);
            let alt2 = leg.altitude_2_ft.unwrap_or(0);
            let spd = leg.speed_limit_kts.unwrap_or(0);

            fas_dat.push_str(&format!(
                "{},{},{},{},{},{},{:.6},{:.6},{},{},{}\n",
                leg.airport_ident,
                kind_str,
                leg.procedure_ident,
                leg.sequence_number,
                term,
                leg.fix_ident,
                pt.0,
                pt.1,
                alt1,
                alt2,
                spd
            ));
        }

        let fas_path = out_dir.join("FAS.DAT");
        std::fs::write(&fas_path, &fas_dat)?;
        artifacts.push(ArtifactEntry {
            path: "FAS.DAT".to_string(),
            sha256: sha256_hex(fas_dat.as_bytes()),
            size: fas_dat.len() as u64,
            kind: "kln-procedures".to_string(),
        });

        // 6. cycle.dat
        let cycle_dat = format!(
            "AIRAC={}\nEFFECTIVE={}\nGENERATOR=OpenAIRAC KLN90B Exporter v1.0\nFINGERPRINT={}\n",
            cycle,
            as_of.to_rfc3339(),
            world_fp
        );
        let cycle_path = out_dir.join("cycle.dat");
        std::fs::write(&cycle_path, &cycle_dat)?;
        artifacts.push(ArtifactEntry {
            path: "cycle.dat".to_string(),
            sha256: sha256_hex(cycle_dat.as_bytes()),
            size: cycle_dat.len() as u64,
            kind: "cycle-metadata".to_string(),
        });

        artifacts.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(GeneratedArtifactSet {
            family: self.family(),
            cycle,
            as_of: as_of.to_rfc3339(),
            generator: "openairac-export-kln90b".to_string(),
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
    fn test_kln90b_export_structure() {
        let store = WorldStore::open_in_memory().unwrap();
        let exporter = Kln90bExporter;
        let temp_dir = std::env::temp_dir().join(format!(
            "openairac_test_kln90b_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);

        let res = exporter
            .export(&store, Utc::now(), &temp_dir)
            .expect("export kln90b");
        assert_eq!(res.family, families::kln90b_dat());
        assert!(temp_dir.join("APT.DAT").exists());
        assert!(temp_dir.join("NAV.DAT").exists());
        assert!(temp_dir.join("WPT.DAT").exists());
        assert!(temp_dir.join("AWY.DAT").exists());
        assert!(temp_dir.join("FAS.DAT").exists());
        assert!(temp_dir.join("cycle.dat").exists());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
