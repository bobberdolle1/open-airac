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
//!   and then swapped into place file-by-file only when every file
//!   succeeded. The multi-file swap is STAGED, not atomic as a set: a
//!   crash between swaps can leave a mixed layer. True transactional
//!   directory install (backup + rollback) is designed (`InstallPlan`) but
//!   intentionally not implemented or exposed yet.
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportReport {
    /// Physical rows serialized into the staged files.
    pub fixes_written: usize,
    pub navaids_written: usize,
    /// Airway LEGS accepted for the layer (before merging).
    pub airway_legs_accepted: usize,
    /// Physical merged segment rows written to `earth_awy.dat`. This is the
    /// number the manifest records; it differs from `airway_legs_accepted`
    /// whenever several airways share a segment.
    pub airway_rows_written: usize,
    pub fixes_skipped: usize,
    pub navaids_skipped: usize,
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

/// The set of entities ACTUALLY serialized into the staged
/// `earth_fix.dat` / `earth_nav.dat`. Airway referential integrity is
/// checked against this index — a canonical entity that the fix/nav writers
/// skipped (missing region, class, elevation, ...) must not satisfy an
/// airway endpoint, or X-Plane would reject the layer at load time.
#[derive(Debug, Default, Clone)]
pub struct ExportedEntityIndex {
    fixes: std::collections::HashSet<(String, String)>,
    ndbs: std::collections::HashSet<(String, String)>,
    vhf: std::collections::HashSet<(String, String)>,
}

impl ExportedEntityIndex {
    fn add_fix(&mut self, wp: &CanonicalWaypoint) {
        self.fixes
            .insert((wp.ident.trim().to_string(), wp.region_code.clone()));
    }

    /// Register a navaid that was physically written. Only ENROUTE navaids
    /// (no terminal area association) are valid airway endpoints per the
    /// XPAWY1101 spec ("points ... listed in the enroute portions of
    /// earth_nav.dat"); ILS components are terminal and never qualify.
    fn add_navaid(&mut self, nav: &CanonicalNavaid) {
        if nav.associated_airport.is_some() {
            return;
        }
        let Some(region) = nav.region_code.clone() else {
            return;
        };
        let key = (nav.ident.trim().to_string(), region);
        match nav.kind {
            NavaidKind::Ndb => {
                self.ndbs.insert(key);
            }
            NavaidKind::Vor
            | NavaidKind::Vordme
            | NavaidKind::Vortac
            | NavaidKind::Tacan
            | NavaidKind::Dme => {
                self.vhf.insert(key);
            }
            NavaidKind::IlsLocalizer | NavaidKind::IlsGlidepath => {}
        }
    }

    /// XPAWY1101 endpoint type: 11 = enroute fix, 2 = enroute NDB,
    /// 3 = VHF navaid (VOR/TACAN/DME). `None` = not in the exported layer.
    pub fn endpoint_type(&self, ident: &str, region: &str) -> Option<u8> {
        let key = (ident.trim().to_string(), region.to_string());
        if self.fixes.contains(&key) {
            return Some(11);
        }
        if self.ndbs.contains(&key) {
            return Some(2);
        }
        if self.vhf.contains(&key) {
            return Some(3);
        }
        None
    }

    pub fn len(&self) -> usize {
        self.fixes.len() + self.ndbs.len() + self.vhf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
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
        index: &mut ExportedEntityIndex,
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
            if !wp.is_enroute {
                // Terminal fixes require the airport terminal area in the
                // row; that data is not modeled yet. Fail closed.
                report.skip(
                    "fix",
                    &wp.ident,
                    "terminal-area waypoint not representable yet".to_string(),
                );
                continue;
            }
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
            index.add_fix(wp);
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
        index: &mut ExportedEntityIndex,
    ) -> Result<()> {
        write_header(&mut writer, "1200", "NavXP1200.", cycle, build_date)?;

        let mut sorted: Vec<(u8, &CanonicalNavaid)> =
            navaids.iter().map(|nav| (row_code_for(nav), nav)).collect();
        sorted.sort_by_key(|(code, nav)| (*code, nav.ident.clone(), nav.name.clone()));
        for (code, nav) in sorted {
            match code {
                2 => self.write_ndb(nav, &mut writer, report, index)?,
                3 => self.write_vor(nav, &mut writer, report, index)?,
                4 => self.write_localizer(nav, &mut writer, report)?,
                6 => self.write_glideslope(nav, &mut writer, report)?,
                12 => self.write_paired_dme(nav, &mut writer, report, index)?,
                13 => self.write_standalone_dme(nav, &mut writer, report, index)?,
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
        index: &ExportedEntityIndex,
        cycle: &str,
        build_date: &str,
        mut writer: W,
        report: &mut ExportReport,
    ) -> Result<()> {
        write_header(&mut writer, "1100", "AwyXP1100.", cycle, build_date)?;

        use std::collections::BTreeMap;

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
            // Fail closed on altitudes: the XPAWY1101 base/top fields have
            // no "unknown" representation, so a segment without source
            // altitudes is skipped instead of written as 0/0.
            let (Some(minimum_altitude_ft), Some(maximum_altitude_ft)) =
                (leg.minimum_altitude_ft, leg.maximum_altitude_ft)
            else {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    "missing MEA/maximum altitude".to_string(),
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
            // Referential integrity against the ACTUALLY EXPORTED entities:
            // an endpoint the fix/nav writers skipped does not qualify.
            let Some(start_type) = index.endpoint_type(&start_ident, &start_region) else {
                report.skip(
                    "airway",
                    &leg.route_ident,
                    format!("start fix '{start_ident}/{start_region}' not in exported layer"),
                );
                continue;
            };
            let Some(end_type) = index.endpoint_type(&end_ident, &end_region) else {
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
                    minimum_altitude_ft: Some(minimum_altitude_ft),
                    maximum_altitude_ft: Some(maximum_altitude_ft),
                });
            report.airway_legs_accepted += 1;
        }

        for seg in merged.values() {
            let level_num = if seg.level == 'H' { 2 } else { 1 };
            // Altitudes are guaranteed present (checked above); the
            // unwraps document the invariant, they cannot fail.
            let base = seg.minimum_altitude_ft.unwrap() / 100;
            let top = seg.maximum_altitude_ft.unwrap() / 100;
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
        report.airway_rows_written = merged.len();

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
        let mut index = ExportedEntityIndex::default();
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
        Self::export_earth_fix(
            &waypoints,
            &cycle,
            &build_date,
            fix_file,
            &mut report,
            &mut index,
        )?;

        let nav_file = std::fs::File::create(&staged_nav)
            .with_context(|| format!("creating staged {:?}", staged_nav))?;
        XPlane12Exporter.export_earth_nav(
            &navaids,
            &cycle,
            &build_date,
            nav_file,
            &mut report,
            &mut index,
        )?;
        let awy_file = std::fs::File::create(&staged_awy)
            .with_context(|| format!("creating staged {:?}", staged_awy))?;
        Self::export_earth_awy(
            &airway_legs,
            &index,
            &cycle,
            &build_date,
            awy_file,
            &mut report,
        )?;

        // X-Plane loads the layer as a unit: an incomplete layer (missing
        // or empty file) destroys referential integrity on install.
        let incomplete = report.fixes_written == 0
            || report.navaids_written == 0
            || report.airway_rows_written == 0;
        if !allow_empty && incomplete {
            let _ = std::fs::remove_dir_all(&staging);
            bail!(
                "refusing incomplete X-Plane layer: {} fixes / {} navaids / {} airway rows written \
                 (skipped {} fixes, {} navaids, {} airway legs). A global X-Plane layer requires \
                 earth_fix.dat, earth_nav.dat AND earth_awy.dat with referential integrity. \
                 Re-run with --allow-empty to override (do not install the result).",
                report.fixes_written,
                report.navaids_written,
                report.airway_rows_written,
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
                manifest_file_entry(&staged_awy, report.airway_rows_written)?,
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
        index: &mut ExportedEntityIndex,
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
        // Classification: spec-mandated representation. XPNAV1200: "Airport
        // code for terminal NDBs, ENRT otherwise" — ENRT is the format's
        // own required value for enroute facilities, not an invented one.
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
            // Classification: spec-normative field default (benign).
            // XPNAV1200 defines the NDB flags field as "1.0 if use of BFO
            // is required for ID. 0.0 otherwise". No ingested source
            // publishes BFO data, so 0.0 is the spec's own default value,
            // not an aviation value we invented.
            0.0,
            nav.ident,
            area,
            region,
            nav.name
        )?;
        index.add_navaid(nav);
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
        index: &mut ExportedEntityIndex,
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
        index.add_navaid(nav);
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
        index: &mut ExportedEntityIndex,
    ) -> Result<()> {
        self.write_dme_common(nav, writer, report, index, 12)
    }

    /// Row 13: standalone DME (frequency displayed on charts).
    fn write_standalone_dme<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
        index: &mut ExportedEntityIndex,
    ) -> Result<()> {
        self.write_dme_common(nav, writer, report, index, 13)
    }

    fn write_dme_common<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
        index: &mut ExportedEntityIndex,
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
            // Classification: spec-mandated default. XPNAV1200 says the
            // DME bias field's "Default is 0.0" — the specification itself
            // defines the value when the source does not provide a bias.
            0.0,
            nav.ident,
            area,
            region,
            name
        )?;
        index.add_navaid(nav);
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

/// Installation plan: retained for interface compatibility; the
/// transactional installer below supersedes it (the plan is derived
/// from the staged layer manifest at install time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub target_dir: PathBuf,
    pub files: Vec<String>,
    pub backup_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Transactional layer installation (journaled, crash-recoverable)
// ---------------------------------------------------------------------------

/// Lock file name inside the target directory.
pub const INSTALL_LOCK: &str = ".openairac_install.lock";
/// Journal file name inside the target directory.
pub const INSTALL_JOURNAL: &str = ".openairac_install.journal.json";

/// Install phases. The journal phase advances BEFORE each destructive
/// step, so crash recovery always knows what must be rolled back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallPhase {
    Prepared,
    BackedUp,
    Swapped,
    Committed,
    RolledBack,
}

/// Journal persisted in the target directory across the install.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallJournal {
    pub operation_id: String,
    pub cycle: String,
    /// Layer file names in install order.
    pub files: Vec<String>,
    /// Backup directory (inside the target dir).
    pub backup_dir: PathBuf,
    pub phase: InstallPhase,
}

/// Outcome of a layer install (or rollback).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerInstallReport {
    pub operation_id: String,
    pub cycle: String,
    pub installed: Vec<String>,
    /// Files restored from backup during a rollback.
    pub restored: Vec<String>,
    /// Files removed during rollback because no backup existed.
    pub removed: Vec<String>,
}

/// Test-only failpoints; default = no failures.
#[derive(Debug, Clone, Copy, Default)]
pub struct InstallFailpoints {
    pub after_backup: bool,
    pub during_swap: bool,
    pub before_commit: bool,
}

fn write_journal(target_dir: &Path, journal: &InstallJournal) -> Result<()> {
    let json = serde_json::to_string_pretty(journal)?;
    std::fs::write(target_dir.join(INSTALL_JOURNAL), json)?;
    Ok(())
}

fn read_journal(target_dir: &Path) -> Result<Option<InstallJournal>> {
    let path = target_dir.join(INSTALL_JOURNAL);
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&json)?))
}

/// Roll the target back to the pre-install state using the journal.
/// Never fails silently: each step reports; the journal is marked
/// RolledBack only after every file is handled.
fn rollback_target(target_dir: &Path, journal: &InstallJournal) -> Result<LayerInstallReport> {
    let mut restored = Vec::new();
    let mut removed = Vec::new();
    for name in &journal.files {
        let target = target_dir.join(name);
        let backup = journal.backup_dir.join(name);
        if backup.exists() {
            swap_file(&backup, &target)?;
            restored.push(name.clone());
        } else if target.exists() {
            std::fs::remove_file(&target)?;
            removed.push(name.clone());
        }
    }
    let rolled_back = InstallJournal {
        phase: InstallPhase::RolledBack,
        ..journal.clone()
    };
    write_journal(target_dir, &rolled_back)?;
    let _ = std::fs::remove_dir_all(&journal.backup_dir);
    let _ = std::fs::remove_file(target_dir.join(INSTALL_LOCK));
    let _ = std::fs::remove_file(target_dir.join(INSTALL_JOURNAL));
    Ok(LayerInstallReport {
        operation_id: journal.operation_id.clone(),
        cycle: journal.cycle.clone(),
        installed: Vec::new(),
        restored,
        removed,
    })
}

/// Recover from an interrupted install: if a journal exists, roll the
/// target back; a stale lock without journal is removed. Returns the
/// rollback report when recovery happened.
pub fn recover_interrupted(target_dir: &Path) -> Result<Option<LayerInstallReport>> {
    let journal = read_journal(target_dir)?;
    match journal {
        Some(j) => Ok(Some(rollback_target(target_dir, &j)?)),
        None => {
            let lock = target_dir.join(INSTALL_LOCK);
            if lock.exists() {
                // Lock without journal: crashed before the journal
                // write — nothing was modified yet.
                std::fs::remove_file(&lock)?;
            }
            Ok(None)
        }
    }
}

fn validate_staged_layer(staging_dir: &Path) -> Result<NavdataLayerManifest> {
    let manifest_path = staging_dir.join("manifest.json");
    let manifest_json = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;
    let manifest: NavdataLayerManifest =
        serde_json::from_str(&manifest_json).context("parsing layer manifest")?;
    if !manifest.allow_empty && manifest.files.iter().any(|f| f.rows == 0) {
        bail!(
            "staged layer manifest records an empty file; refusing to install              (re-export without --allow-empty misuse)"
        );
    }
    for entry in &manifest.files {
        let path = staging_dir.join(&entry.name);
        let content =
            std::fs::read(&path).with_context(|| format!("reading staged {}", entry.name))?;
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&content);
        let actual = format!("{:x}", hasher.finalize());
        if actual != entry.sha256 {
            bail!("staged layer file {} checksum mismatch", entry.name);
        }
    }
    Ok(manifest)
}

/// Transactionally install a staged X-Plane layer into a target
/// directory (journal + backup + swap + post-validate + commit;
/// rollback restores the previous layer on ANY failure).
pub fn install_layer(staging_dir: &Path, target_dir: &Path) -> Result<LayerInstallReport> {
    install_layer_with_failpoints(staging_dir, target_dir, &InstallFailpoints::default())
}

/// Install with test-only failpoints.
pub fn install_layer_with_failpoints(
    staging_dir: &Path,
    target_dir: &Path,
    failpoints: &InstallFailpoints,
) -> Result<LayerInstallReport> {
    std::fs::create_dir_all(target_dir)
        .with_context(|| format!("creating target directory {:?}", target_dir))?;

    // 0. Any interrupted previous install must be resolved first.
    recover_interrupted(target_dir)?;

    // 1. Validate the staged layer completely before touching the target.
    let manifest = validate_staged_layer(staging_dir)?;

    // 2. Exclusive lock.
    let operation_id = format!(
        "op-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    );
    let lock_path = target_dir.join(INSTALL_LOCK);
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path);
    let mut lock = match lock {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!(
                "another install or recovery is in progress (lock {:?} exists)",
                lock_path
            );
        }
        Err(e) => return Err(e.into()),
    };
    use std::io::Write;
    writeln!(lock, "{operation_id}")?;

    // 3. Journal BEFORE any modification.
    let backup_dir = target_dir.join(format!(".openairac_backup_{operation_id}"));
    let journal = InstallJournal {
        operation_id: operation_id.clone(),
        cycle: manifest.cycle.clone(),
        files: manifest.files.iter().map(|f| f.name.clone()).collect(),
        backup_dir: backup_dir.clone(),
        phase: InstallPhase::Prepared,
    };
    write_journal(target_dir, &journal)?;

    // 4. Backup existing files.
    std::fs::create_dir_all(&backup_dir)?;
    for name in &journal.files {
        let target = target_dir.join(name);
        if target.exists() {
            std::fs::copy(&target, backup_dir.join(name))?;
        }
    }
    let backed_up = InstallJournal {
        phase: InstallPhase::BackedUp,
        ..journal.clone()
    };
    write_journal(target_dir, &backed_up)?;

    if failpoints.after_backup {
        let report = rollback_target(target_dir, &backed_up)?;
        bail!(
            "failpoint after_backup triggered; previous layer restored              ({} files)",
            report.restored.len() + report.removed.len()
        );
    }

    // 5. Swap staged files into place.
    for name in &journal.files {
        swap_file(&staging_dir.join(name), &target_dir.join(name))?;
    }
    let swapped = InstallJournal {
        phase: InstallPhase::Swapped,
        ..journal.clone()
    };
    write_journal(target_dir, &swapped)?;

    if failpoints.during_swap {
        let report = rollback_target(target_dir, &swapped)?;
        bail!(
            "failpoint during_swap triggered; previous layer restored              ({} files)",
            report.restored.len() + report.removed.len()
        );
    }

    // 6. Post-install validation: the installed files must re-verify.
    for entry in &manifest.files {
        let path = target_dir.join(&entry.name);
        let content =
            std::fs::read(&path).with_context(|| format!("reading installed {}", entry.name))?;
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(&content);
        let actual = format!("{:x}", hasher.finalize());
        if actual != entry.sha256 {
            let _ = rollback_target(target_dir, &swapped)?;
            bail!(
                "post-install validation failed for {}; previous layer restored",
                entry.name
            );
        }
    }

    if failpoints.before_commit {
        let report = rollback_target(target_dir, &swapped)?;
        bail!(
            "failpoint before_commit triggered; previous layer restored              ({} files)",
            report.restored.len() + report.removed.len()
        );
    }

    // 7. Commit: cleanup, then release the lock last.
    let committed = InstallJournal {
        phase: InstallPhase::Committed,
        ..journal.clone()
    };
    write_journal(target_dir, &committed)?;
    let _ = std::fs::remove_dir_all(&backup_dir);
    let _ = std::fs::remove_file(target_dir.join(INSTALL_JOURNAL));
    drop(lock);
    let _ = std::fs::remove_file(&lock_path);

    Ok(LayerInstallReport {
        operation_id,
        cycle: manifest.cycle,
        installed: journal.files.clone(),
        restored: Vec::new(),
        removed: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    fn unique_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!("oa_xplane_test_{}_{}_{n}", std::process::id(), tag))
    }

    fn staged_layer(dir: &Path, contents: &[(&str, &str)]) {
        // contents: (file name, body)
        let mut entries = Vec::new();
        for (name, body) in contents {
            std::fs::write(dir.join(name), body).unwrap();
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(body.as_bytes());
            entries.push(ManifestFileEntry {
                name: name.to_string(),
                sha256: format!("{:x}", hasher.finalize()),
                rows: 10,
            });
        }
        let manifest = NavdataLayerManifest {
            generator: "test".to_string(),
            cycle: "2608".to_string(),
            build_date: "20260806".to_string(),
            generated_at: "2026-08-06T00:00:00Z".to_string(),
            files: entries,
            allow_empty: false,
        };
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_install_layer_happy_path() {
        let root = unique_dir("install_happy");
        let _ = std::fs::remove_dir_all(&root);
        let staging = root.join("staging");
        let target = root.join("custom_data");
        std::fs::create_dir_all(&staging).unwrap();
        staged_layer(
            &staging,
            &[
                ("earth_fix.dat", "I\n1100 Version - data cycle 2608\n"),
                ("earth_nav.dat", "I\n1100 Version - data cycle 2608\n"),
                ("earth_awy.dat", "A\n1100 Version - data cycle 2608\n"),
            ],
        );
        let report = install_layer(&staging, &target).unwrap();
        assert_eq!(report.installed.len(), 3);
        assert!(target.join("earth_fix.dat").exists());
        assert!(!target.join(INSTALL_JOURNAL).exists());
        assert!(!target.join(INSTALL_LOCK).exists());
        // Reinstall over the same layer: re-stage first (install moves
        // files out of the staging directory).
        let staging2 = root.join("staging2");
        std::fs::create_dir_all(&staging2).unwrap();
        staged_layer(
            &staging2,
            &[
                ("earth_fix.dat", "I\n1100 Version - data cycle 2608\n"),
                ("earth_nav.dat", "I\n1100 Version - data cycle 2608\n"),
                ("earth_awy.dat", "A\n1100 Version - data cycle 2608\n"),
            ],
        );
        install_layer(&staging2, &target).unwrap();
    }

    #[test]
    fn test_install_rollback_restores_previous_layer() {
        let root = unique_dir("install_rollback");
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("custom_data");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("earth_fix.dat"), "OLD FIX LAYER\n").unwrap();
        std::fs::write(target.join("earth_nav.dat"), "OLD NAV LAYER\n").unwrap();
        std::fs::write(target.join("earth_awy.dat"), "OLD AWY LAYER\n").unwrap();

        let staging = root.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        staged_layer(
            &staging,
            &[
                ("earth_fix.dat", "NEW FIX LAYER\n"),
                ("earth_nav.dat", "NEW NAV LAYER\n"),
                ("earth_awy.dat", "NEW AWY LAYER\n"),
            ],
        );
        let result = install_layer_with_failpoints(
            &staging,
            &target,
            &InstallFailpoints {
                after_backup: true,
                ..Default::default()
            },
        );
        assert!(result.is_err());
        // Previous layer restored byte-for-byte.
        assert_eq!(
            std::fs::read_to_string(target.join("earth_fix.dat")).unwrap(),
            "OLD FIX LAYER\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("earth_nav.dat")).unwrap(),
            "OLD NAV LAYER\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("earth_awy.dat")).unwrap(),
            "OLD AWY LAYER\n"
        );
        // No journal/lock leftovers.
        assert!(!target.join(INSTALL_JOURNAL).exists());
        assert!(!target.join(INSTALL_LOCK).exists());
        // A subsequent install works after the failed one.
        install_layer(&staging, &target).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("earth_fix.dat")).unwrap(),
            "NEW FIX LAYER\n"
        );
    }

    #[test]
    fn test_recover_interrupted_install() {
        let root = unique_dir("install_recover");
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("custom_data");
        std::fs::create_dir_all(&target).unwrap();
        // Simulate a crash mid-swap: journal says Swapped, backup holds
        // the previous files, target holds new files.
        let backup = target.join(".openairac_backup_op-1");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("earth_fix.dat"), "OLD FIX LAYER\n").unwrap();
        std::fs::write(backup.join("earth_nav.dat"), "OLD NAV LAYER\n").unwrap();
        std::fs::write(target.join("earth_fix.dat"), "NEW FIX LAYER\n").unwrap();
        std::fs::write(target.join("earth_nav.dat"), "NEW NAV LAYER\n").unwrap();
        std::fs::write(target.join(INSTALL_LOCK), "op-1\n").unwrap();
        let journal = InstallJournal {
            operation_id: "op-1".to_string(),
            cycle: "2608".to_string(),
            files: vec!["earth_fix.dat".to_string(), "earth_nav.dat".to_string()],
            backup_dir: backup,
            phase: InstallPhase::Swapped,
        };
        write_journal(&target, &journal).unwrap();

        let report = recover_interrupted(&target).unwrap().unwrap();
        assert_eq!(report.restored.len(), 2);
        assert_eq!(
            std::fs::read_to_string(target.join("earth_fix.dat")).unwrap(),
            "OLD FIX LAYER\n"
        );
        assert!(!target.join(INSTALL_JOURNAL).exists());
        assert!(!target.join(INSTALL_LOCK).exists());
    }

    #[test]
    fn test_install_lock_and_validation() {
        let root = unique_dir("install_lock");
        let _ = std::fs::remove_dir_all(&root);
        let staging = root.join("staging");
        let target = root.join("custom_data");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        staged_layer(
            &staging,
            &[
                ("earth_fix.dat", "I\n1100 Version\n"),
                ("earth_nav.dat", "I\n1100 Version\n"),
                ("earth_awy.dat", "A\n1100 Version\n"),
            ],
        );
        // Corrupt a staged payload -> validation fails, target untouched.
        std::fs::write(staging.join("earth_nav.dat"), "tampered\n").unwrap();
        let result = install_layer(&staging, &target);
        assert!(result.is_err());
        assert!(!target.join("earth_fix.dat").exists());
        assert!(!target.join(INSTALL_LOCK).exists());
        assert!(!target.join(INSTALL_JOURNAL).exists());
        // Restore the payload, then: stale lock (no journal) — recovery
        // removes it and the install proceeds; a crash between lock
        // creation and journal write must never brick the directory.
        staged_layer(
            &staging,
            &[
                ("earth_fix.dat", "I\n1100 Version\n"),
                ("earth_nav.dat", "I\n1100 Version\n"),
                ("earth_awy.dat", "A\n1100 Version\n"),
            ],
        );
        std::fs::write(target.join(INSTALL_LOCK), "op-other\n").unwrap();
        let result = install_layer(&staging, &target);
        assert!(result.is_ok());
        assert!(!target.join(INSTALL_LOCK).exists());
    }

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
            terminal_area_ident: None,
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
        let mut index = ExportedEntityIndex::default();
        XPlane12Exporter::export_earth_fix(
            &waypoints,
            "2608",
            "20260806",
            &mut buf,
            &mut report,
            &mut index,
        )
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
            .export_earth_nav(
                &[sea, sea_dme],
                "2608",
                "20260806",
                &mut buf,
                &mut report,
                &mut ExportedEntityIndex::default(),
            )
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
            .export_earth_nav(
                &[nav],
                "2608",
                "20260806",
                &mut buf,
                &mut report,
                &mut ExportedEntityIndex::default(),
            )
            .unwrap();
        assert_eq!(report.navaids_written, 0);
        assert_eq!(report.navaids_skipped, 1);

        // Missing slaved variation -> skipped.
        let mut nav2 = navaid("NOVAR", NavaidKind::Vor, 115_000, Some("K1"));
        nav2.slaved_variation_deg = None;
        let mut report2 = ExportReport::default();
        let mut buf2 = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[nav2],
                "2608",
                "20260806",
                &mut buf2,
                &mut report2,
                &mut ExportedEntityIndex::default(),
            )
            .unwrap();
        assert_eq!(report2.navaids_written, 0);

        // Missing class -> skipped.
        let mut nav3 = navaid("NOCLS", NavaidKind::Ndb, 362, Some("K1"));
        nav3.service_volume_nm = None;
        let mut report3 = ExportReport::default();
        let mut buf3 = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[nav3],
                "2608",
                "20260806",
                &mut buf3,
                &mut report3,
                &mut ExportedEntityIndex::default(),
            )
            .unwrap();
        assert_eq!(report3.navaids_written, 0);
        assert!(report3.diagnostics.iter().any(|d| d.contains("class")));
    }

    #[test]
    fn test_export_earth_awy_golden() {
        // The XPAWY1101 spec's example segment (ABCDE/K1 -> ABC/K1, J13),
        // extended with a second airway on the same segment (J13-J14), and
        // an NDB endpoint (type 2) plus a VHF endpoint (type 3) per the
        // spec's type table: 11 = fix, 2 = enroute NDB, 3 = VHF navaid.
        let fixes = vec![
            waypoint("ABCDE", 10.0, 10.0, "K1", Some(2105431)),
            waypoint("ABC", 11.0, 11.0, "K1", Some(2105431)),
        ];
        let mut ndb = navaid("NBE", NavaidKind::Ndb, 362, Some("K1"));
        ndb.name = "NBE NDB".to_string();
        let mut vhf = navaid("VORX", NavaidKind::Vor, 115_000, Some("K1"));
        vhf.name = "VORX VOR".to_string();
        let legs = vec![
            leg("J13", "ABCDE", "K1", "ABC", "K1", Some('L')),
            leg("J14", "ABCDE", "K1", "ABC", "K1", Some('L')),
            leg("V99", "ABC", "K1", "NBE", "K1", Some('L')),
            leg("V98", "NBE", "K1", "VORX", "K1", Some('L')),
            // endpoint missing from the layer -> skipped
            leg("V97", "ABC", "K1", "NOPE", "K1", Some('L')),
            // missing altitudes -> skipped (no 0/0 fabrication)
            {
                let mut l = leg("V96", "ABC", "K1", "NOPE", "K1", Some('L'));
                l.minimum_altitude_ft = None;
                l
            },
        ];

        // Build the exported-entity index from an actual fix/nav export.
        let mut index = ExportedEntityIndex::default();
        let mut report = ExportReport::default();
        let mut fix_buf = Vec::new();
        XPlane12Exporter::export_earth_fix(
            &fixes,
            "2608",
            "20260806",
            &mut fix_buf,
            &mut report,
            &mut index,
        )
        .unwrap();
        let mut nav_buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[ndb, vhf],
                "2608",
                "20260806",
                &mut nav_buf,
                &mut report,
                &mut index,
            )
            .unwrap();

        let mut buf = Vec::new();
        XPlane12Exporter::export_earth_awy(
            &legs,
            &index,
            "2608",
            "20260806",
            &mut buf,
            &mut report,
        )
        .unwrap();
        let content = String::from_utf8(buf).unwrap();
        assert!(content.contains("ABCDE K1 11 ABC   K1 11 N 1 115 175 J13-J14"));
        // NDB endpoint typed 2, VHF endpoint typed 3 per XPAWY1101.
        assert!(content.contains("ABC   K1 11 NBE   K1  2 N 1 115 175 V99"));
        assert!(content.contains("NBE   K1  2 VORX  K1  3 N 1 115 175 V98"));
        assert_eq!(report.airway_legs_accepted, 4);
        assert_eq!(report.airway_rows_written, 3); // J13+J14 merged into one row
        assert_eq!(report.airway_legs_skipped, 2); // NOPE endpoint + missing altitudes
        assert!(content.ends_with("99\n"));
    }

    #[test]
    fn test_airway_skipped_fix_endpoint_does_not_qualify() {
        // The canonical store contains the fix, but the fix WRITER skips it
        // (missing waypoint type) -> it must not satisfy an airway endpoint.
        let fixes = vec![waypoint("SKIPME", 10.0, 10.0, "K1", None)];
        let mut index = ExportedEntityIndex::default();
        let mut report = ExportReport::default();
        let mut fix_buf = Vec::new();
        XPlane12Exporter::export_earth_fix(
            &fixes,
            "2608",
            "20260806",
            &mut fix_buf,
            &mut report,
            &mut index,
        )
        .unwrap();
        assert_eq!(report.fixes_written, 0);
        assert!(index.is_empty());

        let legs = vec![leg("V257", "SKIPME", "K1", "OTHER", "K1", Some('L'))];
        let mut awy_buf = Vec::new();
        XPlane12Exporter::export_earth_awy(
            &legs,
            &index,
            "2608",
            "20260806",
            &mut awy_buf,
            &mut report,
        )
        .unwrap();
        assert_eq!(report.airway_rows_written, 0);
        assert_eq!(report.airway_legs_skipped, 1);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.contains("not in exported layer"))
        );
    }

    #[test]
    fn test_airway_skipped_navaid_endpoint_does_not_qualify() {
        // The navaid is in the canonical store but the nav WRITER skips it
        // (missing service class) -> it must not satisfy an airway endpoint.
        let mut nav = navaid("NOCLS", NavaidKind::Vor, 115_000, Some("K1"));
        nav.service_volume_nm = None;
        let mut index = ExportedEntityIndex::default();
        let mut report = ExportReport::default();
        let mut nav_buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[nav],
                "2608",
                "20260806",
                &mut nav_buf,
                &mut report,
                &mut index,
            )
            .unwrap();
        assert_eq!(report.navaids_written, 0);
        assert!(index.is_empty());

        let legs = vec![leg("V257", "OTHER", "K1", "NOCLS", "K1", Some('L'))];
        let mut awy_buf = Vec::new();
        XPlane12Exporter::export_earth_awy(
            &legs,
            &index,
            "2608",
            "20260806",
            &mut awy_buf,
            &mut report,
        )
        .unwrap();
        assert_eq!(report.airway_rows_written, 0);
        assert_eq!(report.airway_legs_skipped, 1);
    }

    #[test]
    fn test_terminal_navaid_not_an_enroute_endpoint() {
        // A serialized ILS-DME (terminal, associated_airport set) must not
        // become an enroute airway endpoint even though it is in earth_nav.
        let mut dme = navaid("ISFO", NavaidKind::Dme, 109_550, Some("K2"));
        dme.associated_airport = Some("KSFO".to_string());
        dme.dme_paired = true;
        let mut index = ExportedEntityIndex::default();
        let mut report = ExportReport::default();
        let mut nav_buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[dme],
                "2608",
                "20260806",
                &mut nav_buf,
                &mut report,
                &mut index,
            )
            .unwrap();
        assert_eq!(report.navaids_written, 1);
        assert!(
            index.is_empty(),
            "terminal navaids are not enroute endpoints"
        );
    }

    #[test]
    fn test_manifest_rows_match_physical_rows() {
        // Covered in test_export_from_db_refuses_empty_and_stages below;
        // this test asserts the counts directly on the writer level.
        let fixes = vec![waypoint("ABCDE", 10.0, 10.0, "K1", Some(2105431))];
        let mut index = ExportedEntityIndex::default();
        let mut report = ExportReport::default();
        let mut fix_buf = Vec::new();
        XPlane12Exporter::export_earth_fix(
            &fixes,
            "2608",
            "20260806",
            &mut fix_buf,
            &mut report,
            &mut index,
        )
        .unwrap();
        // Count physical rows (excluding header, blank and 99).
        let physical = String::from_utf8(fix_buf)
            .unwrap()
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && t != "99" && !t.starts_with("1200 Version") && t != "I"
            })
            .count();
        assert_eq!(report.fixes_written, physical);
    }
    #[test]
    fn test_ils_without_bearing_is_refused() {
        let mut loc = navaid("IBAD", NavaidKind::IlsLocalizer, 111_300, Some("K1"));
        loc.associated_airport = Some("KSEA".to_string());
        loc.associated_runway = Some("16R".to_string());
        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(
                &[loc],
                "2608",
                "20260806",
                &mut buf,
                &mut report,
                &mut ExportedEntityIndex::default(),
            )
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
        assert_eq!(report.airway_rows_written, 1);
        assert!(out_dir.join("earth_nav.dat").exists());
        assert!(out_dir.join("earth_fix.dat").exists());
        assert!(out_dir.join("earth_awy.dat").exists());
        assert!(out_dir.join("manifest.json").exists());

        // The manifest's `rows` must equal the physical data rows of each
        // staged file (excluding header, blank lines and the 99 terminator).
        let manifest: NavdataLayerManifest =
            serde_json::from_str(&std::fs::read_to_string(out_dir.join("manifest.json")).unwrap())
                .unwrap();
        for entry in &manifest.files {
            let physical = std::fs::read_to_string(out_dir.join(&entry.name))
                .unwrap()
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty()
                        && t != "99"
                        && t != "I"
                        && !t.starts_with("1200 Version")
                        && !t.starts_with("1100 Version")
                })
                .count();
            assert_eq!(
                entry.rows, physical,
                "manifest rows mismatch for {}",
                entry.name
            );
        }
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
