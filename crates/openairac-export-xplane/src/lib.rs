//! X-Plane 12 navdata exporter.
//!
//! Implements Laminar's published file specifications:
//! - `earth_fix.dat` — XPFIX1200
//! - `earth_nav.dat` — XPNAV1200
//! - `earth_awy.dat` — XPAWY1101
//!
//! Conventions cross-checked against Laminar convert424toxplane v12.4 output
//! for the same FAA CIFP input (cycle 2608).
//!
//! Export is fail-closed:
//! * Records missing fields the X-Plane format requires (ICAO region,
//!   elevation, slaved variation, service class, waypoint type, localizer
//!   bearings, airway level, airway endpoint references) are SKIPPED with a
//!   per-record diagnostic — values are never fabricated. This is stricter
//!   than convert424toxplane, which defaults unknown elevations to 0; the
//!   divergence is deliberate and documented.
//! * Files are generated into a staging directory, recorded in a manifest,
//!   and swapped in atomically only when every file succeeded.
//! * An export that would produce an incomplete layer (any of the three
//!   files empty, or airway endpoints missing) is refused unless
//!   `allow_empty`, so an incomplete world database can never silently
//!   destroy a working simulator installation.
//! * A full X-Plane layer requires `earth_awy.dat` plus cycle-consistent
//!   fix/nav references — the exporter treats airway export as part of the
//!   layer, never optional. Without it, X-Plane's referential integrity
//!   checks reject the layer.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Deterministic export outcome report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportReport {
    pub fixes_written: usize,
    pub fixes_skipped: usize,
    pub navaids_written: usize,
    pub navaids_skipped: usize,
    pub airway_legs_written: usize,
    pub airway_legs_skipped: usize,
    /// First skipped records with reasons (bounded; see MAX_DIAGNOSTICS).
    pub diagnostics: Vec<String>,
}

const MAX_DIAGNOSTICS: usize = 100;

impl ExportReport {
    fn skip(&mut self, what: &str, ident: &str, reason: String) {
        match what {
            "fix" => self.fixes_skipped += 1,
            "navaid" => self.navaids_skipped += 1,
            "airway" => self.airway_legs_skipped += 1,
            _ => {}
        }
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics
                .push(format!("skipped {what} '{ident}': {reason}"));
        }
    }
}

/// AIRAC cycle number (`YYNN`) effective at `date`.
///
/// Cycles start 2020-01-30 (cycle 2001) and repeat every 28 days.
/// Verified against published cycle dates: 2513 effective 2025-12-25,
/// 2601 effective 2026-01-22, 2608 effective 2026-08-06 (FAA CIFP Readme).
pub fn airac_cycle(date: DateTime<Utc>) -> String {
    let epoch = NaiveDate::from_ymd_opt(2020, 1, 30).expect("static date");
    let days = date.date_naive().signed_duration_since(epoch).num_days();
    let cycle_index = days / 28;
    let year = 2020 + cycle_index / 13;
    let number = cycle_index % 13 + 1;
    format!("{}{:02}", year % 100, number)
}

/// A 2-character ICAO 7910 region code. Also accepts the US one-letter
/// codes with a trailing blank (`K `, `P `) that the FAA CIFP uses for
/// records without a published region and that convert424toxplane emits
/// verbatim.
fn is_icao_region(region: &str) -> bool {
    region.len() == 2
        && region
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ')
        && region.as_bytes()[0].is_ascii_uppercase()
}

fn write_header<W: Write>(
    writer: &mut W,
    version: &str,
    metadata: &str,
    cycle: &str,
    build_date: &str,
) -> Result<()> {
    writeln!(writer, "I")?;
    writeln!(
        writer,
        "{version} Version - data cycle {cycle}, build {build_date}, metadata {metadata} OpenAIRAC {}",
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(writer)?;
    Ok(())
}

pub struct XPlane12Exporter;

impl XPlane12Exporter {
    /// Export waypoints into `earth_fix.dat` (FIX1200).
    ///
    /// Row: latitude, longitude, ID, terminal area (`ENRT`), ICAO region
    /// code, waypoint type (ARINC 5.42 as u32), name.
    pub fn export_earth_fix<W: Write>(
        waypoints: &[CanonicalWaypoint],
        cycle: &str,
        build_date: &str,
        mut writer: W,
        report: &mut ExportReport,
    ) -> Result<()> {
        write_header(&mut writer, "1200", "FixXP1200.", cycle, build_date)?;

        let mut sorted: Vec<&CanonicalWaypoint> = waypoints.iter().collect();
        sorted.sort_by(|a, b| {
            (a.ident.as_str(), a.name.as_str()).cmp(&(b.ident.as_str(), b.name.as_str()))
        });

        for wp in sorted {
            if !is_icao_region(&wp.region_code) {
                report.skip(
                    "fix",
                    &wp.ident,
                    format!("missing or invalid ICAO region code '{:?}'", wp.region_code),
                );
                continue;
            }
            let Some(waypoint_type) = wp.waypoint_type else {
                report.skip(
                    "fix",
                    &wp.ident,
                    "missing ARINC 424 waypoint type (5.42)".to_string(),
                );
                continue;
            };
            // Terminal-area waypoints (non-ENRT) are not yet ingestible;
            // the model keeps `is_enroute` for the PC-record work.
            let terminal_area = "ENRT";
            writeln!(
                writer,
                "{:12.9} {:13.9} {:5} {} {} {} {}",
                wp.latitude,
                wp.longitude,
                wp.ident,
                terminal_area,
                wp.region_code,
                waypoint_type,
                wp.name
            )?;
            report.fixes_written += 1;
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Export navaids into `earth_nav.dat` (NAV1200).
    ///
    /// Rows are emitted sorted by row code, which satisfies the spec's
    /// ordering constraints (glideslopes after localizers, DME after VORs).
    pub fn export_earth_nav<W: Write>(
        &self,
        navaids: &[CanonicalNavaid],
        cycle: &str,
        build_date: &str,
        mut writer: W,
        report: &mut ExportReport,
    ) -> Result<()> {
        write_header(&mut writer, "1200", "NavXP1200.", cycle, build_date)?;

        let mut sorted: Vec<(u8, &CanonicalNavaid)> =
            navaids.iter().map(|nav| (row_code_for(nav), nav)).collect();
        sorted.sort_by_key(|(code, nav)| (*code, nav.ident.clone(), nav.name.clone()));
        for (code, nav) in sorted {
            match code {
                2 => self.write_ndb(nav, &mut writer, report)?,
                3 => self.write_vor(nav, &mut writer, report)?,
                4 => self.write_localizer(nav, &mut writer, report)?,
                6 => self.write_glideslope(nav, &mut writer, report)?,
                12 => self.write_paired_dme(nav, &mut writer, report)?,
                13 => self.write_standalone_dme(nav, &mut writer, report)?,
                _ => report.skip(
                    "navaid",
                    &nav.ident,
                    format!("unexportable navaid kind {}", nav.kind.as_str()),
                ),
            }
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Export airway legs into `earth_awy.dat` (XPAWY1101).
    ///
    /// Row: start fix, start region, start type (11 fix / 2 NDB / 3 VHF),
    /// end fix, end region, end type, direction, level (1/2), base in
    /// hundreds of feet, top in hundreds of feet, airway name(s).
    ///
    /// Referential integrity: a segment is emitted only when BOTH endpoints
    /// exist in the exported fix/nav layer. Segments shared by several
    /// airways are merged into one row with hyphen-joined names.
    pub fn export_earth_awy<W: Write>(
        legs: &[CanonicalAirwayLeg],
        fixes: &[CanonicalWaypoint],
        navaids: &[CanonicalNavaid],
        cycle: &str,
        build_date: &str,
        mut writer: W,
        report: &mut ExportReport,
    ) -> Result<()> {
        write_header(&mut writer, "1100", "AwyXP1100.", cycle, build_date)?;

        use std::collections::BTreeMap;
        let fix_index: std::collections::HashSet<(String, String)> = fixes
            .iter()
            .filter(|w| w.is_enroute)
            .map(|w| (w.ident.trim().to_string(), w.region_code.clone()))
            .collect();
        let nav_index: std::collections::HashSet<(String, String)> = navaids
            .iter()
            .filter(|n| n.region_code.is_some())
            .map(|n| {
                (
                    n.ident.trim().to_string(),
                    n.region_code.clone().unwrap_or_default(),
                )
            })
            .collect();

        fn endpoint_type(
            ident: &str,
            region: &str,
            fix_index: &std::collections::HashSet<(String, String)>,
            nav_index: &std::collections::HashSet<(String, String)>,
        ) -> Option<u8> {
            if fix_index.contains(&(ident.to_string(), region.to_string())) {
                return Some(11);
            }
            if nav_index.contains(&(ident.to_string(), region.to_string())) {
                return Some(3); // VHF navaid (NDB type 2 not distinguished here)
            }
            None
        }

        /// One merged airway segment row.
        struct Segment {
            start_ident: String,
            start_region: String,
            start_type: u8,
            end_ident: String,
            end_region: String,
            end_type: u8,
            direction: char,
            level: char,
            names: Vec<String>,
            minimum_altitude_ft: Option<u32>,
            maximum_altitude_ft: Option<u32>,
        }

        let mut merged: BTreeMap<(String, String, String, String, char, char), Segment> =
            BTreeMap::new();
        for leg in legs {
            let Some(level) = leg.level else {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    "missing published level (H/L)".to_string(),
                );
                continue;
            };
            let start_ident = leg.start_fix.trim().to_string();
            let start_region = leg.start_icao_code.clone();
            let end_ident = leg.end_fix.trim().to_string();
            let end_region = leg.end_icao_code.clone();
            if !is_icao_region(&start_region) {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    format!("invalid start region '{start_region}'"),
                );
                continue;
            }
            if !is_icao_region(&end_region) {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    format!("invalid end region '{end_region}'"),
                );
                continue;
            }
            let Some(start_type) =
                endpoint_type(&start_ident, &start_region, &fix_index, &nav_index)
            else {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    format!("start fix '{start_ident}/{start_region}' not in exported layer"),
                );
                continue;
            };
            let Some(end_type) = endpoint_type(&end_ident, &end_region, &fix_index, &nav_index)
            else {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    format!("end fix '{end_ident}/{end_region}' not in exported layer"),
                );
                continue;
            };
            let key = (
                start_ident.clone(),
                start_region.clone(),
                end_ident.clone(),
                end_region.clone(),
                leg.direction,
                level,
            );
            merged
                .entry(key)
                .and_modify(|seg| {
                    if !seg.names.iter().any(|n| n == &leg.route_ident) {
                        seg.names.push(leg.route_ident.clone());
                    }
                })
                .or_insert_with(|| Segment {
                    start_ident: start_ident.clone(),
                    start_region: start_region.clone(),
                    start_type,
                    end_ident: end_ident.clone(),
                    end_region: end_region.clone(),
                    end_type,
                    direction: leg.direction,
                    level,
                    names: vec![leg.route_ident.clone()],
                    minimum_altitude_ft: leg.minimum_altitude_ft,
                    maximum_altitude_ft: leg.maximum_altitude_ft,
                });
            report.airway_legs_written += 1;
        }

        for seg in merged.values() {
            let level_num = if seg.level == 'H' { 2 } else { 1 };
            let base = seg.minimum_altitude_ft.map(|v| v / 100).unwrap_or(0);
            let top = seg.maximum_altitude_ft.map(|v| v / 100).unwrap_or(0);
            let names = seg.names.join("-");
            writeln!(
                writer,
                "{:5} {} {:2} {:5} {} {:2} {} {} {:3} {:3} {}",
                seg.start_ident,
                seg.start_region,
                seg.start_type,
                seg.end_ident,
                seg.end_region,
                seg.end_type,
                seg.direction,
                level_num,
                base,
                top,
                names
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Full export from the temporal store: query at `date`, stage the three
    /// dat files plus a manifest next to `out_dir`, validate, swap in.
    pub fn export_from_db<P: AsRef<Path>>(
        store: &WorldStore,
        date: DateTime<Utc>,
        out_dir: P,
        allow_empty: bool,
    ) -> Result<ExportReport> {
        let dir = out_dir.as_ref();
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating output directory {:?}", dir))?;
        let parent = dir
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        let waypoints = store.query_waypoints_at(date)?;
        let navaids = store.query_navaids_at(date)?;
        let airway_legs = store.query_airway_legs_at(date)?;

        let mut report = ExportReport::default();
        let cycle = airac_cycle(date);
        let build_date = date.format("%Y%m%d").to_string();

        // Stage files first; only then swap them into place.
        let staging = tempfile_dir(parent)?;
        let staged_fix = staging.join("earth_fix.dat");
        let staged_nav = staging.join("earth_nav.dat");
        let staged_awy = staging.join("earth_awy.dat");
        let staged_manifest = staging.join("manifest.json");

        let fix_file = std::fs::File::create(&staged_fix)
            .with_context(|| format!("creating staged {:?}", staged_fix))?;
        Self::export_earth_fix(&waypoints, &cycle, &build_date, fix_file, &mut report)?;

        let nav_file = std::fs::File::create(&staged_nav)
            .with_context(|| format!("creating staged {:?}", staged_nav))?;
        XPlane12Exporter.export_earth_nav(&navaids, &cycle, &build_date, nav_file, &mut report)?;

        let awy_file = std::fs::File::create(&staged_awy)
            .with_context(|| format!("creating staged {:?}", staged_awy))?;
        Self::export_earth_awy(
            &airway_legs,
            &waypoints,
            &navaids,
            &cycle,
            &build_date,
            awy_file,
            &mut report,
        )?;

        // X-Plane loads the layer as a unit: an incomplete layer (missing
        // or empty file) destroys referential integrity on install.
        let incomplete = report.fixes_written == 0
            || report.navaids_written == 0
            || report.airway_legs_written == 0;
        if !allow_empty && incomplete {
            let _ = std::fs::remove_dir_all(&staging);
            bail!(
                "refusing incomplete X-Plane layer: {} fixes / {} navaids / {} airway legs written \
                 (skipped {} fixes, {} navaids, {} airway legs). A global X-Plane layer requires \
                 earth_fix.dat, earth_nav.dat AND earth_awy.dat with referential integrity. \
                 Re-run with --allow-empty to override (do not install the result).",
                report.fixes_written,
                report.navaids_written,
                report.airway_legs_written,
                report.fixes_skipped,
                report.navaids_skipped,
                report.airway_legs_skipped
            );
        }

        // Manifest records exactly what was staged.
        let manifest = NavdataLayerManifest {
            generator: format!("openairac {}", env!("CARGO_PKG_VERSION")),
            cycle: cycle.clone(),
            build_date: build_date.clone(),
            generated_at: Utc::now().to_rfc3339(),
            files: vec![
                manifest_file_entry(&staged_fix, report.fixes_written)?,
                manifest_file_entry(&staged_nav, report.navaids_written)?,
                manifest_file_entry(&staged_awy, report.airway_legs_written)?,
            ],
            allow_empty,
        };
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&staged_manifest, manifest_json)?;

        swap_file(&staged_fix, &dir.join("earth_fix.dat"))?;
        swap_file(&staged_nav, &dir.join("earth_nav.dat"))?;
        swap_file(&staged_awy, &dir.join("earth_awy.dat"))?;
        swap_file(&staged_manifest, &dir.join("manifest.json"))?;
        let _ = std::fs::remove_dir_all(&staging);

        Ok(report)
    }

    /// Row 2: NDB. No fabricated defaults: elevation, class and region must
    /// all be source-provided or the row is skipped with a diagnostic.
    fn write_ndb<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        let Some(region) = nav.region_code.as_deref().filter(|r| is_icao_region(r)) else {
            report.skip("navaid", &nav.ident, "missing ICAO region code".to_string());
            return Ok(());
        };
        let Some(elevation) = nav.elevation_ft else {
            report.skip(
                "navaid",
                &nav.ident,
                "missing elevation (source does not publish it)".to_string(),
            );
            return Ok(());
        };
        let Some(class) = nav.service_volume_nm else {
            report.skip(
                "navaid",
                &nav.ident,
                "missing NDB class (source does not publish it)".to_string(),
            );
            return Ok(());
        };
        let area = nav
            .associated_airport
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or("ENRT");
        writeln!(
            writer,
            "{:>2} {:.9} {:.9} {:5} {:5} {:3} {:.3} {:4} {:5} {} {}",
            2,
            nav.latitude,
            nav.longitude,
            elevation,
            nav.frequency.0,
            class,
            0.0,
            nav.ident,
            area,
            region,
            nav.name
        )?;
        report.navaids_written += 1;
        Ok(())
    }

    /// Row 3: VOR, VOR-DME, VORTAC or TACAN (frequency in MHz * 100).
    /// Requires elevation, slaved variation, class and region from the source.
    fn write_vor<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        let Some(region) = nav.region_code.as_deref().filter(|r| is_icao_region(r)) else {
            report.skip("navaid", &nav.ident, "missing ICAO region code".to_string());
            return Ok(());
        };
        let Some(elevation) = nav.elevation_ft else {
            report.skip("navaid", &nav.ident, "missing elevation".to_string());
            return Ok(());
        };
        let Some(slaved_var) = nav.slaved_variation_deg else {
            report.skip(
                "navaid",
                &nav.ident,
                "missing slaved variation (0-radial direction)".to_string(),
            );
            return Ok(());
        };
        let Some(class) = nav.service_volume_nm else {
            report.skip("navaid", &nav.ident, "missing VOR class".to_string());
            return Ok(());
        };
        let freq = nav.frequency.0 / 10; // kHz -> MHz*100
        let name = ensure_name_suffix(&nav.name, nav.kind);
        writeln!(
            writer,
            "{:>2} {:.9} {:.9} {:5} {:5} {:3} {:7.3} {:4} {:5} {} {}",
            3,
            nav.latitude,
            nav.longitude,
            elevation,
            freq,
            class,
            slaved_var,
            nav.ident,
            "ENRT",
            region,
            name
        )?;
        report.navaids_written += 1;
        Ok(())
    }

    /// Row 4: ILS localizer. Refused unless every required field is known
    /// (bearing, runway, region, airport, elevation). ILS category is not
    /// decodable from the source records and bearing is absent, so current
    /// sources never satisfy this — that is the honest behavior.
    fn write_localizer<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        let required = ils_required(nav);
        if let Err(reason) = required {
            report.skip("navaid", &nav.ident, reason);
            return Ok(());
        }
        let (airport, region, runway, bearing_mag, bearing_true) = required.unwrap();
        let Some(elevation) = nav.elevation_ft else {
            report.skip(
                "navaid",
                &nav.ident,
                "missing elevation (ILS rows require it)".to_string(),
            );
            return Ok(());
        };
        let front_course = bearing_mag.round() as i64 * 360;
        let bearing_field = front_course as f64 + bearing_true;
        let freq = nav.frequency.0 / 10;
        writeln!(
            writer,
            "{:>2} {:.9} {:.9} {:5} {:5} {:3} {:10.3} {:4} {:5} {} {} ILS-cat-I",
            4,
            nav.latitude,
            nav.longitude,
            elevation,
            freq,
            25,
            bearing_field,
            nav.ident,
            airport,
            region,
            runway
        )?;
        report.navaids_written += 1;
        Ok(())
    }

    /// Row 6: glideslope. Refused unless every required field is known.
    /// convert424toxplane synthesizes the glideslope position and reads the
    /// angle from PF records; we do not synthesize geometry, so current
    /// sources never satisfy this.
    fn write_glideslope<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        let required = ils_required(nav);
        if let Err(reason) = required {
            report.skip("navaid", &nav.ident, reason);
            return Ok(());
        }
        let (airport, region, runway, _bearing_mag, bearing_true) = required.unwrap();
        let Some(angle) = nav.glideslope_angle_deg else {
            report.skip("navaid", &nav.ident, "missing glideslope angle".to_string());
            return Ok(());
        };
        let Some(elevation) = nav.elevation_ft else {
            report.skip("navaid", &nav.ident, "missing elevation".to_string());
            return Ok(());
        };
        let angle_field = (angle * 100.0).round() * 1000.0 + bearing_true;
        let freq = nav.frequency.0 / 10;
        writeln!(
            writer,
            "{:>2} {:.9} {:.9} {:5} {:5} {:3} {:10.3} {:4} {:5} {} {} GS",
            6,
            nav.latitude,
            nav.longitude,
            elevation,
            freq,
            25,
            angle_field,
            nav.ident,
            airport,
            region,
            runway
        )?;
        report.navaids_written += 1;
        Ok(())
    }

    /// Row 12: paired DME (chart frequency suppressed): VOR-DME/VORTAC and
    /// ILS DME components. Requires elevation and class from the source.
    fn write_paired_dme<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        self.write_dme_common(nav, writer, report, 12)
    }

    /// Row 13: standalone DME (frequency displayed on charts).
    fn write_standalone_dme<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        self.write_dme_common(nav, writer, report, 13)
    }

    fn write_dme_common<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
        row: u8,
    ) -> Result<()> {
        let Some(region) = nav.region_code.as_deref().filter(|r| is_icao_region(r)) else {
            report.skip("navaid", &nav.ident, "missing ICAO region code".to_string());
            return Ok(());
        };
        let Some(elevation) = nav.elevation_ft else {
            report.skip("navaid", &nav.ident, "missing elevation".to_string());
            return Ok(());
        };
        let Some(service_volume) = nav.service_volume_nm else {
            report.skip(
                "navaid",
                &nav.ident,
                "missing DME service volume".to_string(),
            );
            return Ok(());
        };
        let area = nav
            .associated_airport
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or("ENRT");
        let freq = nav.frequency.0 / 10;
        let name = ensure_name_suffix(&nav.name, NavaidKind::Dme);
        writeln!(
            writer,
            "{:>2} {:.9} {:.9} {:5} {:5} {:3} {:.3} {:4} {:5} {} {}",
            row,
            nav.latitude,
            nav.longitude,
            elevation,
            freq,
            service_volume,
            0.0,
            nav.ident,
            area,
            region,
            name
        )?;
        report.navaids_written += 1;
        Ok(())
    }
}

fn row_code_for(nav: &CanonicalNavaid) -> u8 {
    match nav.kind {
        NavaidKind::Ndb => 2,
        NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac | NavaidKind::Tacan => 3,
        NavaidKind::IlsLocalizer => 4,
        NavaidKind::IlsGlidepath => 6,
        NavaidKind::Dme if nav.dme_paired => 12,
        NavaidKind::Dme => 13,
    }
}

/// Fields every ILS component row needs; error names the first missing one.
fn ils_required(nav: &CanonicalNavaid) -> Result<(&str, &str, &str, f64, f64), String> {
    let Some(airport) = nav.associated_airport.as_deref().filter(|a| !a.is_empty()) else {
        return Err("missing associated airport".to_string());
    };
    let Some(region) = nav.region_code.as_deref().filter(|r| is_icao_region(r)) else {
        return Err("missing ICAO region code".to_string());
    };
    let Some(runway) = nav.associated_runway.as_deref() else {
        return Err("missing associated runway".to_string());
    };
    let Some(bearing_mag) = nav.localizer_bearing_mag_deg else {
        return Err("missing localizer magnetic bearing".to_string());
    };
    let Some(bearing_true) = nav.localizer_bearing_true_deg else {
        return Err("missing localizer true bearing".to_string());
    };
    Ok((airport, region, runway, bearing_mag, bearing_true))
}

fn ensure_name_suffix(name: &str, kind: NavaidKind) -> String {
    let suffix = match kind {
        NavaidKind::Vordme => "VOR-DME",
        NavaidKind::Vortac => "VORTAC",
        NavaidKind::Tacan => "TACAN",
        NavaidKind::Dme => "DME",
        _ => return name.to_string(),
    };
    if name.to_uppercase().ends_with(suffix) {
        name.to_string()
    } else {
        format!("{name} {suffix}")
    }
}

/// Create a unique staging directory next to `parent`.
fn tempfile_dir(parent: &Path) -> Result<PathBuf> {
    let base = format!(
        "openairac-export-{}-{}.tmp",
        std::process::id(),
        Utc::now().timestamp_millis()
    );
    let path = parent.join(base);
    std::fs::create_dir_all(&path)
        .with_context(|| format!("creating staging directory {:?}", path))?;
    Ok(path)
}

/// Atomically replace `dest` with `src` (same filesystem: rename).
fn swap_file(src: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        if dest.exists() {
            std::fs::remove_file(dest).with_context(|| format!("removing previous {:?}", dest))?;
        }
    }
    std::fs::rename(src, dest).with_context(|| format!("installing {:?} -> {:?}", src, dest))?;
    Ok(())
}

// Navdata layer manifest & installation design
// ---------------------------------------------------------------------------

/// Manifest describing one generated X-Plane navdata layer. Written next to
/// the dat files. The installation machinery (backup/rollback) is designed
/// but intentionally NOT exposed through the CLI yet: installing into a
/// live simulator is a future, product-level step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavdataLayerManifest {
    pub generator: String,
    pub cycle: String,
    pub build_date: String,
    pub generated_at: String,
    pub files: Vec<ManifestFileEntry>,
    pub allow_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    pub name: String,
    pub sha256: String,
    pub rows: usize,
}

fn manifest_file_entry(path: &Path, rows: usize) -> Result<ManifestFileEntry> {
    let content = std::fs::read(path)?;
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&content);
    let sha256 = format!("{:x}", hasher.finalize());
    Ok(ManifestFileEntry {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        sha256,
        rows,
    })
}

/// Installation plan: the library-side design for a transactional simulator
/// install. Not wired into the CLI: OpenAIRAC must not touch live simulator
/// installations yet. Sequence for the future implementation:
///
/// ```text
/// generate into staging directory      (done: export_from_db)
///   → validate manifest checksums       (planned: validate_layer)
///   → backup existing files             (planned: backup_existing)
///   → controlled swap                   (planned: apply_plan)
///   → post-install validation           (planned: re-read manifest)
///   → rollback on failure               (planned: rollback)
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub target_dir: PathBuf,
    pub files: Vec<String>,
    pub backup_dir: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporal() -> TemporalValidity {
        TemporalValidity {
            valid_from: Utc::now(),
            valid_until: None,
            source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
        }
    }

    fn waypoint(
        ident: &str,
        lat: f64,
        lon: f64,
        region: &str,
        wptype: Option<u32>,
    ) -> CanonicalWaypoint {
        CanonicalWaypoint {
            object_id: WaypointId(format!("WP-{ident}")),
            ident: ident.to_string(),
            name: ident.to_string(),
            latitude: lat,
            longitude: lon,
            is_enroute: true,
            region_code: region.to_string(),
            waypoint_type: wptype,
            temporal: temporal(),
        }
    }

    fn navaid(
        ident: &str,
        kind: NavaidKind,
        freq_khz: u32,
        region: Option<&str>,
    ) -> CanonicalNavaid {
        CanonicalNavaid {
            object_id: NavaidId(format!("NAV-{ident}")),
            ident: ident.to_string(),
            name: format!("{ident} NAME"),
            kind,
            frequency: FrequencyKhz(freq_khz),
            latitude: 47.43538889,
            longitude: -122.30961111,
            elevation_ft: Some(354),
            region_code: region.map(|r| r.to_string()),
            associated_airport: None,
            magnetic_variation_deg: Some(19.0),
            slaved_variation_deg: Some(19.0),
            service_volume_nm: Some(130),
            dme_paired: false,
            associated_runway: None,
            localizer_bearing_true_deg: None,
            localizer_bearing_mag_deg: None,
            glideslope_angle_deg: None,
            temporal: temporal(),
        }
    }

    fn leg(
        route: &str,
        start: &str,
        start_region: &str,
        end: &str,
        end_region: &str,
        level: Option<char>,
    ) -> CanonicalAirwayLeg {
        CanonicalAirwayLeg {
            object_id: AirwayLegId(format!("LEG-{route}")),
            route_ident: route.to_string(),
            route_type: "O".to_string(),
            level,
            sequence_number: 2,
            start_fix: start.to_string(),
            start_icao_code: start_region.to_string(),
            end_fix: end.to_string(),
            end_icao_code: end_region.to_string(),
            direction: 'N',
            minimum_altitude_ft: Some(11_500),
            maximum_altitude_ft: Some(17_500),
            temporal: temporal(),
        }
    }

    #[test]
    fn test_airac_cycle_numbers() {
        let d = |s: &str| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        assert_eq!(airac_cycle(d("2020-01-30T00:00:00Z")), "2001");
        assert_eq!(airac_cycle(d("2026-01-01T00:00:00Z")), "2513");
        assert_eq!(airac_cycle(d("2026-01-22T00:00:00Z")), "2601");
        assert_eq!(airac_cycle(d("2026-08-06T00:00:00Z")), "2608");
        assert_eq!(airac_cycle(d("2026-09-03T00:00:00Z")), "2609");
    }

    #[test]
    fn test_is_icao_region() {
        assert!(is_icao_region("K1"));
        assert!(is_icao_region("CY"));
        assert!(is_icao_region("K ")); // US blank region, per convert424toxplane
        assert!(!is_icao_region("K"));
        assert!(!is_icao_region(" "));
        assert!(!is_icao_region("1K"));
    }

    #[test]
    fn test_export_earth_fix_golden() {
        let waypoints = vec![
            waypoint("AAYRR", 46.646819444, -123.722388889, "K1", Some(4530263)),
            waypoint("AAAME", 37.770908333, -122.082811111, "K2", Some(4530263)),
            waypoint("AAARG", 32.693963889, -78.051294444, "K ", Some(2105431)),
            // missing region -> skipped
            waypoint("BADREG", 10.0, 20.0, "K", Some(4530263)),
            // missing waypoint type -> skipped
            waypoint("NOTYPE", 11.0, 21.0, "K2", None),
        ];
        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter::export_earth_fix(&waypoints, "2608", "20260806", &mut buf, &mut report)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        let expected = "\
I
1200 Version - data cycle 2608, build 20260806, metadata FixXP1200. OpenAIRAC 0.2.0

37.770908333 -122.082811111 AAAME ENRT K2 4530263 AAAME
32.693963889 -78.051294444 AAARG ENRT K  2105431 AAARG
46.646819444 -123.722388889 AAYRR ENRT K1 4530263 AAYRR
99
";
        assert_eq!(content, expected, "actual:\n{content}");
        assert_eq!(report.fixes_written, 3);
        assert_eq!(report.fixes_skipped, 2);
    }

    #[test]
    fn test_export_earth_nav_matches_laminar_examples() {
        // The XPNAV1200 spec's Seattle example rows, whitespace-normalized.
        // Fields are compared per-column against the official example.
        let mut sea = navaid("SEA", NavaidKind::Vortac, 116_800, Some("K1"));
        sea.name = "SEATTLE VORTAC".to_string();
        sea.latitude = 47.435372222;
        sea.longitude = -122.309616667;
        sea.elevation_ft = Some(348);
        sea.slaved_variation_deg = Some(19.0);
        sea.service_volume_nm = Some(130);
        let mut sea_dme = sea.clone();
        sea_dme.object_id = NavaidId("NAV-SEA-dme".to_string());
        sea_dme.kind = NavaidKind::Dme;
        sea_dme.dme_paired = true;
        sea_dme.slaved_variation_deg = None;
        sea_dme.name = "SEATTLE VORTAC DME".to_string();

        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(&[sea, sea_dme], "2608", "20260806", &mut buf, &mut report)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();
        let rows: Vec<Vec<&str>> = content
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                let first = t.split_whitespace().next().unwrap_or("");
                matches!(
                    first,
                    "2" | "3"
                        | "4"
                        | "5"
                        | "6"
                        | "7"
                        | "8"
                        | "9"
                        | "12"
                        | "13"
                        | "14"
                        | "15"
                        | "16"
                )
            })
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            [
                "3",
                "47.435372222",
                "-122.309616667",
                "348",
                "11680",
                "130",
                "19.000",
                "SEA",
                "ENRT",
                "K1",
                "SEATTLE",
                "VORTAC"
            ]
        );
        assert_eq!(
            rows[1],
            [
                "12",
                "47.435372222",
                "-122.309616667",
                "348",
                "11680",
                "130",
                "0.000",
                "SEA",
                "ENRT",
                "K1",
                "SEATTLE",
                "VORTAC",
                "DME"
            ]
        );
    }

    #[test]
    fn test_no_fabricated_defaults() {
        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        // Missing elevation -> skipped, not zero-filled.
        let mut nav = navaid("NODEF", NavaidKind::Vor, 115_000, Some("K1"));
        nav.elevation_ft = None;
        XPlane12Exporter
            .export_earth_nav(&[nav], "2608", "20260806", &mut buf, &mut report)
            .unwrap();
        assert_eq!(report.navaids_written, 0);
        assert_eq!(report.navaids_skipped, 1);

        // Missing slaved variation -> skipped.
        let mut nav2 = navaid("NOVAR", NavaidKind::Vor, 115_000, Some("K1"));
        nav2.slaved_variation_deg = None;
        let mut report2 = ExportReport::default();
        let mut buf2 = Vec::new();
        XPlane12Exporter
            .export_earth_nav(&[nav2], "2608", "20260806", &mut buf2, &mut report2)
            .unwrap();
        assert_eq!(report2.navaids_written, 0);

        // Missing class -> skipped.
        let mut nav3 = navaid("NOCLS", NavaidKind::Ndb, 362, Some("K1"));
        nav3.service_volume_nm = None;
        let mut report3 = ExportReport::default();
        let mut buf3 = Vec::new();
        XPlane12Exporter
            .export_earth_nav(&[nav3], "2608", "20260806", &mut buf3, &mut report3)
            .unwrap();
        assert_eq!(report3.navaids_written, 0);
        assert!(report3.diagnostics.iter().any(|d| d.contains("class")));
    }

    #[test]
    fn test_export_earth_awy_golden() {
        // The XPAWY1101 spec's example segment (ABCDE/K1 -> ABC/K1, J13),
        // extended with a second airway on the same segment (J13-J14).
        let fixes = vec![
            waypoint("ABCDE", 10.0, 10.0, "K1", Some(2105431)),
            waypoint("ABC", 11.0, 11.0, "K1", Some(2105431)),
        ];
        let legs = vec![
            leg("J13", "ABCDE", "K1", "ABC", "K1", Some('L')),
            leg("J14", "ABCDE", "K1", "ABC", "K1", Some('L')),
            // endpoint missing from the layer -> skipped
            leg("V99", "ABC", "K1", "NOPE", "K1", Some('L')),
        ];
        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter::export_earth_awy(
            &legs,
            &fixes,
            &[],
            "2608",
            "20260806",
            &mut buf,
            &mut report,
        )
        .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(content.contains("ABCDE K1 11 ABC   K1 11 N 1 115 175 J13-J14"));
        assert_eq!(report.airway_legs_written, 2);
        assert_eq!(report.airway_legs_skipped, 1);
        assert!(content.ends_with("99\n"));
    }

    #[test]
    fn test_ils_without_bearing_is_refused() {
        let mut loc = navaid("IBAD", NavaidKind::IlsLocalizer, 111_300, Some("K1"));
        loc.associated_airport = Some("KSEA".to_string());
        loc.associated_runway = Some("16R".to_string());
        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(&[loc], "2608", "20260806", &mut buf, &mut report)
            .unwrap();
        assert_eq!(report.navaids_written, 0);
        assert_eq!(report.navaids_skipped, 1);
    }

    #[test]
    fn test_export_from_db_refuses_empty_and_stages() {
        let store = WorldStore::open_in_memory().unwrap();
        let out_dir =
            std::env::temp_dir().join(format!("openairac_xp12_test_{}", std::process::id()));

        // Empty store: refuse without allow_empty.
        let err = XPlane12Exporter::export_from_db(&store, Utc::now(), &out_dir, false);
        assert!(err.is_err());
        assert!(!out_dir.join("earth_nav.dat").exists());

        // With a complete mini-layer the export succeeds and writes all files.
        let snapshot = openairac_model::SourceSnapshot {
            id: SourceSnapshotId("snap-test".to_string()),
            provider: "Test".to_string(),
            dataset: "navaids".to_string(),
            provider_revision: None,
            airac_cycle: None,
            effective_from: Some(Utc::now()),
            effective_until: None,
            retrieved_at: Utc::now(),
            source_uri: "http://test".to_string(),
            content_sha256: "hash".to_string(),
            license_id: None,
            license_notes: None,
            parser_version: "0.2.0".to_string(),
        };
        let mut store = store;
        let mut n = navaid("SFO", NavaidKind::Vordme, 115_800, Some("K2"));
        n.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        let mut w = waypoint("SFO", 37.6195, -122.3739, "K2", Some(4530263));
        w.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        let mut l = leg("V257", "AADCO", "K2", "LOLIC", "K2", Some('L'));
        l.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        store
            .transact(|conn| {
                openairac_store::insert_source_snapshot_conn(conn, &snapshot)?;
                openairac_store::insert_navaid_conn(conn, &n)?;
                openairac_store::insert_waypoint_conn(conn, &w)?;
                openairac_store::insert_airway_leg_conn(conn, &l)?;
                Ok(())
            })
            .unwrap();
        // airway endpoints must exist in the fix layer
        let mut w2 = waypoint("AADCO", 39.5, -104.5, "K2", Some(4530263));
        w2.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        let mut w3 = waypoint("LOLIC", 39.6, -104.6, "K2", Some(4530263));
        w3.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        store
            .transact(|conn| {
                openairac_store::insert_waypoint_conn(conn, &w2)?;
                openairac_store::insert_waypoint_conn(conn, &w3)?;
                Ok(())
            })
            .unwrap();

        let report = XPlane12Exporter::export_from_db(&store, Utc::now(), &out_dir, false).unwrap();
        assert_eq!(report.fixes_written, 3);
        assert_eq!(report.navaids_written, 1);
        assert_eq!(report.airway_legs_written, 1);
        assert!(out_dir.join("earth_nav.dat").exists());
        assert!(out_dir.join("earth_fix.dat").exists());
        assert!(out_dir.join("earth_awy.dat").exists());
        assert!(out_dir.join("manifest.json").exists());
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
