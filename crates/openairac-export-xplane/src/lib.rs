//! X-Plane 12 navdata exporter.
//!
//! Implements Laminar's published file specifications:
//! - `earth_fix.dat` — XPFIX1200
//!   (https://developer.x-plane.com/wp-content/uploads/2021/09/XP-FIX1200-Spec.pdf)
//! - `earth_nav.dat` — XPNAV1200
//!   (https://developer.x-plane.com/wp-content/uploads/2016/10/XP-NAV1200-Spec-2.pdf)
//!
//! Export is fail-closed:
//! * Records missing fields that the X-Plane format requires (ICAO region,
//!   waypoint type, localizer bearings, ...) are SKIPPED with a per-record
//!   diagnostic — values are never fabricated.
//! * Files are generated into a staging directory and atomically renamed into
//!   place only when every file succeeded.
//! * An export that would produce an empty `earth_nav.dat` / `earth_fix.dat`
//!   is refused unless `allow_empty` is set, so an incomplete world database
//!   can never silently destroy a working simulator installation.

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
    /// First skipped records with reasons (bounded; see MAX_DIAGNOSTICS).
    pub diagnostics: Vec<String>,
}

const MAX_DIAGNOSTICS: usize = 100;

impl ExportReport {
    fn skip(&mut self, what: &str, ident: &str, reason: String) {
        if what == "fix" {
            self.fixes_skipped += 1;
        } else {
            self.navaids_skipped += 1;
        }
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics
                .push(format!("skipped {what} '{ident}': {reason}"));
        }
    }
}

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

/// A 2-character ICAO 7910 region code (`K1`..`K7`, `CY`, `EG`, ...).
fn is_icao_region(region: &str) -> bool {
    region.len() == 2
        && region
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
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
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - data cycle {cycle}, build {build_date}, metadata OpenAIRAC {}",
            env!("CARGO_PKG_VERSION")
        )?;
        writeln!(writer)?;

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
                "{:12.8} {:13.8} {:5} {} {} {} {}",
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
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - data cycle {cycle}, build {build_date}, metadata OpenAIRAC {}",
            env!("CARGO_PKG_VERSION")
        )?;
        writeln!(writer)?;

        // (row_code, canonical) sorted so ordering constraints hold.
        let mut sorted: Vec<(u8, &CanonicalNavaid)> =
            navaids.iter().map(|nav| (row_code_for(nav), nav)).collect();
        sorted.sort_by_key(|(code, nav)| (*code, nav.ident.clone(), nav.name.clone()));

        for (code, nav) in sorted {
            match code {
                2 => self.write_ndb(nav, &mut writer, report)?,
                3 => self.write_vor(nav, &mut writer, report)?,
                4 => self.write_localizer(nav, &mut writer, report)?,
                6 => self.write_glideslope(nav, &mut writer, report)?,
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

    /// Full export from the temporal store: query at `date`, stage the two
    /// dat files next to `out_dir`, validate, and swap them in atomically.
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

        let mut report = ExportReport::default();
        let cycle = airac_cycle(date);
        let build_date = date.format("%Y%m%d").to_string();

        // Stage both files first; only then swap them into place.
        let staging = tempfile_dir(parent)?;
        let staged_fix = staging.join("earth_fix.dat");
        let staged_nav = staging.join("earth_nav.dat");

        let fix_file = std::fs::File::create(&staged_fix)
            .with_context(|| format!("creating staged {:?}", staged_fix))?;
        Self::export_earth_fix(&waypoints, &cycle, &build_date, fix_file, &mut report)?;

        let nav_file = std::fs::File::create(&staged_nav)
            .with_context(|| format!("creating staged {:?}", staged_nav))?;
        XPlane12Exporter.export_earth_nav(&navaids, &cycle, &build_date, nav_file, &mut report)?;

        // Refuse to overwrite a working install with an empty layer.
        if !allow_empty && (report.fixes_written == 0 || report.navaids_written == 0) {
            let _ = std::fs::remove_dir_all(&staging);
            bail!(
                "refusing incomplete export: {} fixes / {} navaids written \
                 (skipped {} fixes, {} navaids). Re-run with --allow-empty to override.",
                report.fixes_written,
                report.navaids_written,
                report.fixes_skipped,
                report.navaids_skipped
            );
        }

        swap_file(&staged_fix, &dir.join("earth_fix.dat"))?;
        swap_file(&staged_nav, &dir.join("earth_nav.dat"))?;
        let _ = std::fs::remove_dir_all(&staging);

        Ok(report)
    }

    /// Row 2: NDB. Requires an ICAO region (enroute) — or the terminal
    /// airport carries one implicitly; without a region the row is skipped.
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
        let area = nav
            .associated_airport
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or("ENRT");
        let class = 50u8; // normal-power NDB; power class is not yet modeled
        let elevation = nav.elevation_ft.unwrap_or(0);
        if nav.elevation_ft.is_none() {
            report.diagnostics.push(format!(
                "navaid '{}': unknown elevation exported as 0",
                nav.ident
            ));
        }
        writeln!(
            writer,
            "{:>2} {:12.8} {:13.8} {:5} {:5} {:3} {:5.1} {:4} {:5} {} {}",
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
        let freq = nav.frequency.0 / 10; // kHz -> MHz*100
        let class = 125u16; // unspecified, likely high power (per NAV1200)
        let slaved_var = nav.magnetic_variation_deg.unwrap_or(0.0);
        if nav.magnetic_variation_deg.is_none() {
            report.diagnostics.push(format!(
                "navaid '{}': unknown slaved variation exported as 0.0",
                nav.ident
            ));
        }
        let elevation = nav.elevation_ft.unwrap_or(0);
        if nav.elevation_ft.is_none() {
            report.diagnostics.push(format!(
                "navaid '{}': unknown elevation exported as 0",
                nav.ident
            ));
        }
        let name = ensure_name_suffix(&nav.name, nav.kind);
        writeln!(
            writer,
            "{:>2} {:12.8} {:13.8} {:5} {:5} {:3} {:7.3} {:4} {:5} {} {}",
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

    /// Row 4: ILS localizer. Refused unless every required field is known.
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
        let front_course = bearing_mag.round() as i64 * 360;
        let bearing_field = front_course as f64 + bearing_true;
        let freq = nav.frequency.0 / 10;
        let elevation = match nav.elevation_ft {
            Some(e) => e,
            None => {
                report.skip(
                    "navaid",
                    &nav.ident,
                    "missing elevation (ILS rows require it)".to_string(),
                );
                return Ok(());
            }
        };
        writeln!(
            writer,
            "{:>2} {:12.8} {:13.8} {:5} {:5} {:3} {:10.3} {:4} {:5} {} {} ILS-cat-I",
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
        let angle_field = (angle * 100.0).round() * 1000.0 + bearing_true;
        let freq = nav.frequency.0 / 10;
        let elevation = nav.elevation_ft.unwrap_or(0);
        writeln!(
            writer,
            "{:>2} {:12.8} {:13.8} {:5} {:5} {:3} {:10.3} {:4} {:5} {} {} GS",
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

    /// Row 13: standalone DME (frequency displayed on charts).
    fn write_standalone_dme<W: Write>(
        &self,
        nav: &CanonicalNavaid,
        writer: &mut W,
        report: &mut ExportReport,
    ) -> Result<()> {
        let Some(region) = nav.region_code.as_deref().filter(|r| is_icao_region(r)) else {
            report.skip("navaid", &nav.ident, "missing ICAO region code".to_string());
            return Ok(());
        };
        let area = nav
            .associated_airport
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or("ENRT");
        let freq = nav.frequency.0 / 10;
        let service_volume = 40u16; // D-OSV/class unknown; conservative default
        let elevation = nav.elevation_ft.unwrap_or(0);
        if nav.elevation_ft.is_none() {
            report.diagnostics.push(format!(
                "navaid '{}': unknown elevation exported as 0",
                nav.ident
            ));
        }
        let name = ensure_name_suffix(&nav.name, NavaidKind::Dme);
        writeln!(
            writer,
            "{:>2} {:12.8} {:13.8} {:5} {:5} {:3} {:5.1} {:4} {:5} {} {}",
            13,
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
            associated_runway: None,
            localizer_bearing_true_deg: None,
            localizer_bearing_mag_deg: None,
            glideslope_angle_deg: None,
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
    fn test_export_earth_fix_golden() {
        let waypoints = vec![
            waypoint("AAYRR", 46.646819444, -123.722388889, "K1", Some(4530263)),
            waypoint("AAAME", 37.770908333, -122.082811111, "K2", Some(4530263)),
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
1200 Version - data cycle 2608, build 20260806, metadata OpenAIRAC 0.2.0

 37.77090833 -122.08281111 AAAME ENRT K2 4530263 AAAME
 46.64681944 -123.72238889 AAYRR ENRT K1 4530263 AAYRR
99
";
        assert_eq!(content, expected, "actual:\n{content}");
        assert_eq!(report.fixes_written, 2);
        assert_eq!(report.fixes_skipped, 2);
    }

    #[test]
    fn test_export_earth_nav_golden() {
        let mut navaids = vec![
            navaid("BF", NavaidKind::Ndb, 362, Some("K1")),
            navaid("SEA", NavaidKind::Vortac, 116_800, Some("K1")),
            navaid("NODME", NavaidKind::Dme, 110_300, Some("K1")),
            navaid("NOREG", NavaidKind::Vor, 115_000, None), // skipped
        ];
        // A complete ILS localizer + glideslope pair (synthetic but valid).
        let mut loc = navaid("ISNQ", NavaidKind::IlsLocalizer, 110_300, Some("K1"));
        loc.associated_airport = Some("KSEA".to_string());
        loc.associated_runway = Some("16L".to_string());
        loc.localizer_bearing_mag_deg = Some(164.0);
        loc.localizer_bearing_true_deg = Some(180.343);
        loc.elevation_ft = Some(338);
        let mut gs = navaid("ISNQ", NavaidKind::IlsGlidepath, 110_300, Some("K1"));
        gs.associated_airport = Some("KSEA".to_string());
        gs.associated_runway = Some("16L".to_string());
        gs.localizer_bearing_mag_deg = Some(164.0);
        gs.localizer_bearing_true_deg = Some(180.343);
        gs.glideslope_angle_deg = Some(3.00);
        navaids.push(loc);
        navaids.push(gs);
        // An ILS localizer without bearing must be refused.
        let mut bad_loc = navaid("IBAD", NavaidKind::IlsLocalizer, 111_300, Some("K1"));
        bad_loc.associated_airport = Some("KSEA".to_string());
        bad_loc.associated_runway = Some("16R".to_string());
        navaids.push(bad_loc);

        let mut report = ExportReport::default();
        let mut buf = Vec::new();
        XPlane12Exporter
            .export_earth_nav(&navaids, "2608", "20260806", &mut buf, &mut report)
            .unwrap();
        let content = String::from_utf8(buf).unwrap();

        let expected = "\
I
1200 Version - data cycle 2608, build 20260806, metadata OpenAIRAC 0.2.0

 2  47.43538889 -122.30961111   354   362  50   0.0 BF   ENRT  K1 BF NAME
 3  47.43538889 -122.30961111   354 11680 125  19.000 SEA  ENRT  K1 SEA NAME VORTAC
 4  47.43538889 -122.30961111   338 11030  25  59220.343 ISNQ KSEA  K1 16L ILS-cat-I
 6  47.43538889 -122.30961111   354 11030  25 300180.343 ISNQ KSEA  K1 16L GS
13  47.43538889 -122.30961111   354 11030  40   0.0 NODME ENRT  K1 NODME NAME DME
99
";
        assert_eq!(content, expected, "actual:\n{content}");
        assert_eq!(report.navaids_written, 5);
        assert_eq!(report.navaids_skipped, 2);
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

        // With a region-complete navaid the export succeeds and writes files.
        // build a minimal snapshot
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
        let _ = &snapshot;
        let mut n = navaid("SFO", NavaidKind::Vordme, 115_800, Some("K2"));
        n.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        store
            .transact(|conn| {
                openairac_store::insert_source_snapshot_conn(conn, &snapshot)?;
                openairac_store::insert_navaid_conn(conn, &n)?;
                Ok(())
            })
            .unwrap();
        // also one waypoint
        let mut w = waypoint("SFO", 37.6195, -122.3739, "K2", Some(4530263));
        w.temporal.source_snapshot_id = SourceSnapshotId("snap-test".to_string());
        store
            .transact(|conn| {
                openairac_store::insert_waypoint_conn(conn, &w)?;
                Ok(())
            })
            .unwrap();

        let report = XPlane12Exporter::export_from_db(&store, Utc::now(), &out_dir, false).unwrap();
        assert_eq!(report.fixes_written, 1);
        assert_eq!(report.navaids_written, 1);
        assert!(out_dir.join("earth_nav.dat").exists());
        assert!(out_dir.join("earth_fix.dat").exists());
        let _ = std::fs::remove_dir_all(&out_dir);
    }
}
