//! FAA CIFP ARINC 424 Experimental Ingestion Adapter.
//!
//! Layered design so incremental support is possible without string-slicing
//! leaking into the domain layer:
//!
//! ```text
//! fixed-width decoding (CifpLine)
//!         ↓
//! raw CIFP records (CifpRecord)
//!         ↓
//! semantic interpretation (interpret → CifpInterpretation)
//!         ↓
//! canonical entities (CanonicalWaypoint / CanonicalNavaid)
//! ```
//!
//! Supported record classes (ARINC 424-18 / FAA CIFP, verified against
//! CIFP cycle 2608):
//! - Enroute waypoints: section `E`, subsection `A` (`EA`).
//! - VHF navaids: section `D`, subsection blank (`D `): VOR, VOR-DME,
//!   VORTAC, DME-only, TACAN-only and ILS localizers (class code `I`).
//! - NDB navaids: section `D`, subsection `B` (`DB`) and section `P`,
//!   subsection `N` (`PN`, terminal NDBs).
//!
//! Everything else is decoded as an explicit [`CifpRecord::Unsupported`]:
//! the raw line is preserved and never reinterpreted.
//!
//! Known gaps (documented, not hidden):
//! - ILS localizer bearing is not decodable from the `D` record alone;
//!   canonical localizers therefore carry `localizer_bearing_* = None` and
//!   exporters must refuse to emit bearing-dependent rows.
//! - Terminal waypoints (`PC`) are not emitted for US airspace in CIFP;
//!   terminal navaid/waypoint population from `PA`/`PD`/`PE`/`PF` records is
//!   future work.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Layer 1: fixed-width decoding
// ---------------------------------------------------------------------------

/// One fixed-width ARINC 424 line, 132 columns, 1-based field access.
pub struct CifpLine<'a> {
    raw: &'a str,
}

impl<'a> CifpLine<'a> {
    pub fn new(raw: &'a str) -> Self {
        Self { raw }
    }

    /// Record type, columns 1-4 (e.g. `SUSA`, `SCAN`).
    pub fn record_type(&self) -> &'a str {
        self.field(1, 4)
    }

    /// Section code, column 5 (e.g. `E` for enroute, `D` for navaids).
    pub fn section(&self) -> char {
        self.raw.chars().nth(4).unwrap_or(' ')
    }

    /// Subsection code, column 6.
    pub fn subsection(&self) -> char {
        self.raw.chars().nth(5).unwrap_or(' ')
    }

    /// Slice columns `start`..=`end` (1-based, inclusive), right-trimmed of
    /// neither padding — callers trim as needed.
    pub fn field(&self, start: usize, end: usize) -> &'a str {
        if end > self.raw.len() {
            return &self.raw[start - 1..];
        }
        &self.raw[start - 1..end]
    }

    pub fn raw(&self) -> &'a str {
        self.raw
    }
}

/// ARINC 424 fixed-width coordinate `NDDMMSSHH` / `WDDDMMSSHH` → decimal.
pub fn parse_arinc_coordinate(coord_str: &str) -> Result<(f64, f64)> {
    let s = coord_str.trim();
    if s.len() < 18 {
        return Err(anyhow!("ARINC 424 coordinate string too short: '{s}'"));
    }

    let lat_dir = &s[0..1];
    if lat_dir != "N" && lat_dir != "S" {
        return Err(anyhow!("invalid latitude direction in '{s}'"));
    }
    let lat_deg: f64 = s[1..3].parse()?;
    let lat_min: f64 = s[3..5].parse()?;
    let lat_sec: f64 = s[5..7].parse()?;
    let lat_hundredths: f64 = s[7..9].parse()?;

    let mut lat = lat_deg + (lat_min / 60.0) + ((lat_sec + lat_hundredths / 100.0) / 3600.0);
    if lat_dir == "S" {
        lat = -lat;
    }

    let lon_dir = &s[9..10];
    if lon_dir != "E" && lon_dir != "W" {
        return Err(anyhow!("invalid longitude direction in '{s}'"));
    }
    let lon_deg: f64 = s[10..13].parse()?;
    let lon_min: f64 = s[13..15].parse()?;
    let lon_sec: f64 = s[15..17].parse()?;
    let lon_hundredths: f64 = s[17..19].parse()?;

    let mut lon = lon_deg + (lon_min / 60.0) + ((lon_sec + lon_hundredths / 100.0) / 3600.0);
    if lon_dir == "W" {
        lon = -lon;
    }

    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(anyhow!("coordinate out of range: '{s}'"));
    }
    Ok((lat, lon))
}

/// Magnetic variation field (columns 75-80 of D/DB/PN/EA records):
/// direction `E`/`W` followed by 4-5 digits. 4 digits = tenths of a degree,
/// 5 digits = hundredths (both paddings occur in real CIFP data).
fn parse_mag_variation(field: &str) -> Option<f64> {
    let s = field.trim();
    if s.len() < 2 {
        return None;
    }
    let dir = s.as_bytes()[0];
    let digits = &s[1..];
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: f64 = digits.parse().ok()?;
    let scaled = match digits.len() {
        4 => value / 10.0,
        5 => value / 100.0,
        _ => return None,
    };
    match dir {
        b'E' => Some(scaled),
        b'W' => Some(-scaled),
        _ => None,
    }
}

/// ARINC 424 field 5.42 (3 columns) encoded as a little-endian u32 with the
/// 4th byte 0, matching the X-Plane FIX1200 waypoint type representation.
fn waypoint_type_u32(field: &str) -> Option<u32> {
    let bytes = field.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    let b = [bytes[0], bytes[1], bytes[2], 0];
    Some(u32::from_le_bytes(b))
}

// ---------------------------------------------------------------------------
// Layer 2: raw decoded records
// ---------------------------------------------------------------------------

/// A decoded CIFP record: either a supported record class with its verified
/// fields, or an explicit unsupported marker preserving the raw line.
#[derive(Debug, Clone, PartialEq)]
pub enum CifpRecord {
    Waypoint {
        record_type: String,
        /// ARINC 5.6: `ENRT` for enroute records.
        airport_ident: String,
        ident: String,
        /// ARINC 5.14 ICAO code (e.g. `K2`, `CY`); may be `K` + blank for US.
        icao_code: String,
        /// ARINC 5.42 waypoint type (3 chars).
        waypoint_type: String,
        latitude_deg: f64,
        longitude_deg: f64,
        magnetic_variation_deg: Option<f64>,
        name: String,
        raw: String,
    },
    VhfNavaid {
        record_type: String,
        /// ARINC 5.6 airport ident; populated for ILS localizers.
        airport_ident: String,
        ident: String,
        icao_code: String,
        frequency_khz: u32,
        /// Navaid class (columns 28-30).
        class: String,
        latitude_deg: f64,
        longitude_deg: f64,
        magnetic_variation_deg: Option<f64>,
        /// Station elevation in feet (tenths in the record).
        elevation_ft: Option<i32>,
        name: String,
        raw: String,
    },
    NdbNavaid {
        record_type: String,
        airport_ident: String,
        ident: String,
        icao_code: String,
        frequency_khz: u32,
        class: String,
        latitude_deg: f64,
        longitude_deg: f64,
        magnetic_variation_deg: Option<f64>,
        name: String,
        raw: String,
    },
    Unsupported {
        record_type: String,
        section: char,
        subsection: char,
        reason: &'static str,
        raw: String,
    },
}

/// Decode one fixed-width CIFP line into a raw record.
pub fn decode_line(line: &str) -> Result<CifpRecord> {
    let cifp = CifpLine::new(line);
    let record_type = cifp.record_type().to_string();
    let section = cifp.section();
    let subsection = cifp.subsection();

    let unsupported = |reason: &'static str| {
        Ok(CifpRecord::Unsupported {
            record_type: record_type.clone(),
            section,
            subsection,
            reason,
            raw: line.to_string(),
        })
    };

    match (section, subsection) {
        ('E', 'A') => {
            let lat_str = cifp.field(33, 41);
            let lon_str = cifp.field(42, 51);
            let (latitude_deg, longitude_deg) =
                parse_arinc_coordinate(&format!("{lat_str}{lon_str}"))?;
            let ident = cifp.field(14, 18).trim().to_string();
            if ident.is_empty() {
                return Err(anyhow!("EA record without waypoint identifier"));
            }
            let name_field = cifp.field(96, 120).trim().to_string();
            Ok(CifpRecord::Waypoint {
                record_type,
                airport_ident: cifp.field(7, 10).trim().to_string(),
                name: if name_field.is_empty() {
                    ident.clone()
                } else {
                    name_field
                },
                ident,
                icao_code: cifp.field(20, 21).trim().to_string(),
                waypoint_type: cifp.field(27, 29).to_string(),
                latitude_deg,
                longitude_deg,
                magnetic_variation_deg: parse_mag_variation(cifp.field(75, 80)),
                raw: line.to_string(),
            })
        }
        ('D', ' ') => {
            let ident = cifp.field(14, 17).trim().to_string();
            if ident.is_empty() {
                return Err(anyhow!("D record without navaid identifier"));
            }
            let freq_str = cifp.field(23, 27).trim();
            let frequency_khz: u32 = match freq_str.parse::<u32>() {
                Ok(v) if v > 0 => v * 10, // MHz * 100 -> kHz
                _ => {
                    return Err(anyhow!(
                        "D record '{ident}': invalid frequency '{freq_str}'"
                    ));
                }
            };
            let class = cifp.field(28, 30).to_string();

            // ILS localizers (class code 'I') carry the position in the DME
            // columns; all other D records carry it in the VOR columns.
            let is_ils = class.as_bytes().get(1) == Some(&b'I');
            let (lat_str, lon_str) = if is_ils {
                (cifp.field(56, 64), cifp.field(65, 74))
            } else {
                let vor = (cifp.field(33, 41), cifp.field(42, 51));
                if vor.0.trim().is_empty() {
                    (cifp.field(56, 64), cifp.field(65, 74))
                } else {
                    vor
                }
            };
            let (latitude_deg, longitude_deg) =
                parse_arinc_coordinate(&format!("{lat_str}{lon_str}"))?;

            let elevation_ft = match cifp.field(81, 85).trim().parse::<f64>() {
                Ok(tenths) => Some((tenths / 10.0).round() as i32),
                Err(_) => None,
            };

            Ok(CifpRecord::VhfNavaid {
                record_type,
                airport_ident: cifp.field(7, 10).trim().to_string(),
                ident,
                icao_code: cifp.field(20, 21).trim().to_string(),
                frequency_khz,
                class,
                latitude_deg,
                longitude_deg,
                magnetic_variation_deg: parse_mag_variation(cifp.field(75, 80)),
                elevation_ft,
                name: cifp.field(91, 120).trim().to_string(),
                raw: line.to_string(),
            })
        }
        ('D', 'B') => decode_ndb(&cifp, record_type, line),
        ('P', 'N') => decode_ndb(&cifp, record_type, line),
        ('E', 'R') => unsupported("airway record"),
        ('P', ' ') => unsupported("terminal procedure or airport record"),
        ('H', ' ') => unsupported("heliport record"),
        ('U', 'C') => unsupported("controlled airspace record"),
        ('U', 'R') => unsupported("special use airspace record"),
        ('S', ' ') | ('A', 'S') => unsupported("MSA / grid MORA record"),
        _ => unsupported("unrecognized section/subsection"),
    }
}

fn decode_ndb(cifp: &CifpLine<'_>, record_type: String, line: &str) -> Result<CifpRecord> {
    let ident = cifp.field(14, 17).trim().to_string();
    if ident.is_empty() {
        return Err(anyhow!("NDB record without identifier"));
    }
    let freq_str = cifp.field(23, 26).trim();
    let frequency_khz: u32 = match freq_str.parse::<u32>() {
        Ok(v) if v > 0 => v,
        _ => {
            return Err(anyhow!(
                "NDB record '{ident}': invalid frequency '{freq_str}'"
            ));
        }
    };
    let (latitude_deg, longitude_deg) =
        parse_arinc_coordinate(&format!("{}{}", cifp.field(33, 41), cifp.field(42, 51)))?;
    let name_field = cifp.field(91, 120).trim().to_string();
    Ok(CifpRecord::NdbNavaid {
        record_type,
        airport_ident: cifp.field(7, 10).trim().to_string(),
        name: if name_field.is_empty() {
            ident.clone()
        } else {
            name_field
        },
        ident,
        icao_code: cifp.field(20, 21).trim().to_string(),
        frequency_khz,
        class: cifp.field(28, 30).to_string(),
        latitude_deg,
        longitude_deg,
        magnetic_variation_deg: parse_mag_variation(cifp.field(75, 80)),
        raw: line.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Layer 3: semantic interpretation → canonical entities
// ---------------------------------------------------------------------------

/// Result of interpreting one raw record into the canonical model.
#[derive(Debug, Clone)]
pub enum CifpInterpretation {
    Waypoint(CanonicalWaypoint),
    Navaid(CanonicalNavaid),
    Unsupported { reason: String, raw: String },
}

fn navaid_kind_from_class(class: &str) -> Option<NavaidKind> {
    let b = class.as_bytes();
    match (b.first().copied(), b.get(1).copied()) {
        (Some(b'V'), None | Some(b' ') | Some(b'D')) => Some(NavaidKind::Vordme),
        (Some(b'V'), Some(b'T')) => Some(NavaidKind::Vortac),
        (Some(b'V'), _) => Some(NavaidKind::Vor),
        (Some(b' ') | None, Some(b'I')) => Some(NavaidKind::IlsLocalizer),
        (Some(b' ') | None, Some(b'D')) => Some(NavaidKind::Dme),
        (Some(b' ') | None, Some(b'T')) => Some(NavaidKind::Tacan),
        _ => None,
    }
}

/// Interpret a raw record into canonical form. Never fabricates values:
/// unknown semantics stay `Unsupported`.
pub fn interpret(
    record: &CifpRecord,
    snapshot_id: &SourceSnapshotId,
    valid_from: DateTime<Utc>,
) -> CifpInterpretation {
    let temporal = TemporalValidity {
        valid_from,
        valid_until: None,
        source_snapshot_id: snapshot_id.clone(),
    };

    match record {
        CifpRecord::Waypoint {
            record_type,
            ident,
            icao_code,
            waypoint_type,
            latitude_deg,
            longitude_deg,
            name,
            ..
        } => CifpInterpretation::Waypoint(CanonicalWaypoint {
            object_id: WaypointId(format!("faa:{record_type}:{icao_code}:{ident}")),
            ident: ident.clone(),
            name: name.clone(),
            latitude: *latitude_deg,
            longitude: *longitude_deg,
            is_enroute: true,
            region_code: icao_code.clone(),
            waypoint_type: waypoint_type_u32(waypoint_type),
            temporal,
        }),
        CifpRecord::VhfNavaid {
            record_type,
            airport_ident,
            ident,
            icao_code,
            frequency_khz,
            class,
            latitude_deg,
            longitude_deg,
            magnetic_variation_deg,
            elevation_ft,
            name,
            ..
        } => {
            let Some(kind) = navaid_kind_from_class(class) else {
                return CifpInterpretation::Unsupported {
                    reason: format!("unrecognized navaid class '{class}'"),
                    raw: record_to_raw(record),
                };
            };
            CifpInterpretation::Navaid(CanonicalNavaid {
                object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}")),
                ident: ident.clone(),
                name: name.clone(),
                kind,
                frequency: FrequencyKhz(*frequency_khz),
                latitude: *latitude_deg,
                longitude: *longitude_deg,
                elevation_ft: *elevation_ft,
                region_code: Some(icao_code.clone()),
                associated_airport: (!airport_ident.is_empty()).then(|| airport_ident.clone()),
                magnetic_variation_deg: *magnetic_variation_deg,
                associated_runway: None,
                localizer_bearing_true_deg: None,
                localizer_bearing_mag_deg: None,
                glideslope_angle_deg: None,
                temporal,
            })
        }
        CifpRecord::NdbNavaid {
            record_type,
            airport_ident,
            ident,
            icao_code,
            frequency_khz,
            latitude_deg,
            longitude_deg,
            magnetic_variation_deg,
            name,
            ..
        } => CifpInterpretation::Navaid(CanonicalNavaid {
            object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}")),
            ident: ident.clone(),
            name: name.clone(),
            kind: NavaidKind::Ndb,
            frequency: FrequencyKhz(*frequency_khz),
            latitude: *latitude_deg,
            longitude: *longitude_deg,
            elevation_ft: None,
            region_code: Some(icao_code.clone()),
            associated_airport: (!airport_ident.is_empty()).then(|| airport_ident.clone()),
            magnetic_variation_deg: *magnetic_variation_deg,
            associated_runway: None,
            localizer_bearing_true_deg: None,
            localizer_bearing_mag_deg: None,
            glideslope_angle_deg: None,
            temporal,
        }),
        CifpRecord::Unsupported { reason, raw, .. } => CifpInterpretation::Unsupported {
            reason: reason.to_string(),
            raw: raw.clone(),
        },
    }
}

fn record_to_raw(record: &CifpRecord) -> String {
    match record {
        CifpRecord::Waypoint { raw, .. }
        | CifpRecord::VhfNavaid { raw, .. }
        | CifpRecord::NdbNavaid { raw, .. }
        | CifpRecord::Unsupported { raw, .. } => raw.clone(),
    }
}

// ---------------------------------------------------------------------------
// Ingest entry points
// ---------------------------------------------------------------------------

/// Deterministic scan statistics for a full CIFP content pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CifpScanReport {
    pub lines_seen: usize,
    pub records_decoded: usize,
    pub waypoints_decoded: usize,
    pub navaids_decoded: usize,
    pub unsupported_records: usize,
    pub decode_errors: usize,
    pub unsupported_reasons: Vec<String>,
}

pub struct FaaCifpAdapter;

impl FaaCifpAdapter {
    /// Decode and interpret a complete CIFP content string.
    ///
    /// Returns the accepted canonical entities plus a deterministic scan
    /// report. Unsupported records are counted with their raw lines
    /// preserved in the report; malformed supported records are errors.
    pub fn parse_cifp_content(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
    ) -> (Vec<CanonicalWaypoint>, Vec<CanonicalNavaid>, CifpScanReport) {
        let mut waypoints = Vec::new();
        let mut navaids = Vec::new();
        let mut report = CifpScanReport::default();

        for line in content.lines() {
            if line.len() < 132 {
                continue; // header or short padding line; not a data record
            }
            report.lines_seen += 1;
            match decode_line(line) {
                Ok(record) => match interpret(&record, snapshot_id, valid_from) {
                    CifpInterpretation::Waypoint(wp) => {
                        report.records_decoded += 1;
                        report.waypoints_decoded += 1;
                        waypoints.push(wp);
                    }
                    CifpInterpretation::Navaid(nav) => {
                        report.records_decoded += 1;
                        report.navaids_decoded += 1;
                        navaids.push(nav);
                    }
                    CifpInterpretation::Unsupported { reason, raw } => {
                        report.unsupported_records += 1;
                        if report.unsupported_reasons.len() < 1000 {
                            report
                                .unsupported_reasons
                                .push(format!("{reason}: {}", &raw[..64.min(raw.len())]));
                        }
                    }
                },
                Err(err) => {
                    report.decode_errors += 1;
                    if report.unsupported_reasons.len() < 1000 {
                        report.unsupported_reasons.push(format!(
                            "decode error: {err}: {}",
                            &line[..64.min(line.len())]
                        ));
                    }
                }
            }
        }

        (waypoints, navaids, report)
    }

    /// Parse CIFP content and ingest the canonical entities into the store
    /// transactionally. Returns scan statistics.
    pub fn ingest_cifp(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
        store: &mut WorldStore,
    ) -> Result<CifpScanReport> {
        let (waypoints, navaids, report) =
            Self::parse_cifp_content(content, snapshot_id, valid_from);

        store.transact(|conn| {
            for wp in &waypoints {
                openairac_store::insert_waypoint_conn(conn, wp)?;
            }
            for nav in &navaids {
                openairac_store::insert_navaid_conn(conn, nav)?;
            }
            Ok(())
        })?;

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // Real fixtures, transcribed verbatim from FAA CIFP cycle 2608
    // (FAACIFP18, public domain US Government work, effective 2026-08-06,
    // https://aeronav.faa.gov/Upload_313-d/cifp/CIFP_260806.zip).
    // ---------------------------------------------------------------------
    const EA_AAARG: &str = "SUSAEAENRT   AAARG K 0    W   B N32413827W078030466                       W0094     NAR           AAARG                    272022605";
    const D_ABI_VORTAC: &str = "SUSAD        ABI   K4011370VTHW N32285279W099514843    N32285279W099514843E0100018092     NARABILENE                       249601810";
    const D_AAT_DME_ONLY: &str = "SUSAD        AAT   K2011665 DHW                    AAT N41290007W120334155E0133043782     NARALTURAS                       249592605";
    const DB_AA_NDB: &str = "SUSADB       AA    K3003650HOLW N47003259W096485466                       E0040           NARKENIE                         268711805";
    const PN_AB_NDB: &str = "SUSAPNKABIK4 AB    K4003530HO W N32175591W099402722                       E0050           NARTOMHI                         577831911";
    const D_ISFO_ILS: &str = "SUSAD KSFOK2 ISFO  K2010955 ITW                    ISFON37373954W122234146E0140000200     NARSAN FRANCISCO INTL            267531703";
    const ER_AIRWAY: &str = "SUSAERENRT   J1    K 0J1    001J1  A                                           NAR           J1                         200091305   ";
    #[test]
    fn test_parse_arinc_coordinate() {
        let (lat, lon) = parse_arinc_coordinate("N37371100W122223000").unwrap();
        assert!((lat - 37.61972222222222).abs() < 5e-6);
        assert!((lon - (-122.375)).abs() < 5e-6);
        // south/west signs
        let (lat2, lon2) = parse_arinc_coordinate("S33250000E018300000").unwrap();
        assert!((lat2 - (-33.416666666666664)).abs() < 5e-6);
        assert!((lon2 - 18.5).abs() < 5e-6);
        assert!(parse_arinc_coordinate("X0000000X000000000").is_err());
    }

    #[test]
    fn test_decode_ea_waypoint() {
        match decode_line(EA_AAARG).unwrap() {
            CifpRecord::Waypoint {
                ident,
                icao_code,
                waypoint_type,
                latitude_deg,
                longitude_deg,
                magnetic_variation_deg,
                name,
                ..
            } => {
                assert_eq!(ident, "AAARG");
                assert_eq!(icao_code, "K");
                assert_eq!(waypoint_type, "W  ");
                assert!((latitude_deg - 32.69396388888889).abs() < 5e-6);
                assert!((longitude_deg - (-78.05129444444444)).abs() < 5e-6);
                assert_eq!(magnetic_variation_deg, Some(-9.4));
                assert_eq!(name, "AAARG");
            }
            other => panic!("expected waypoint, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_vortac() {
        match decode_line(D_ABI_VORTAC).unwrap() {
            CifpRecord::VhfNavaid {
                ident,
                icao_code,
                frequency_khz,
                class,
                latitude_deg,
                longitude_deg,
                magnetic_variation_deg,
                elevation_ft,
                ..
            } => {
                assert_eq!(ident, "ABI");
                assert_eq!(icao_code, "K4");
                assert_eq!(frequency_khz, 113_700);
                assert_eq!(class, "VTH");
                assert!((latitude_deg - 32.48133055555556).abs() < 5e-6);
                assert!((longitude_deg - (-99.86345277777777)).abs() < 5e-6);
                assert_eq!(magnetic_variation_deg, Some(10.0));
                assert_eq!(elevation_ft, Some(1809));
            }
            other => panic!("expected VHF navaid, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_dme_only() {
        match decode_line(D_AAT_DME_ONLY).unwrap() {
            CifpRecord::VhfNavaid {
                ident,
                class,
                latitude_deg,
                longitude_deg,
                elevation_ft,
                ..
            } => {
                assert_eq!(ident, "AAT");
                assert_eq!(class, " DH");
                assert!((latitude_deg - 41.48335277777778).abs() < 5e-6);
                assert!((longitude_deg - (-120.56154166666667)).abs() < 5e-6);
                assert_eq!(elevation_ft, Some(4378));
            }
            other => panic!("expected VHF navaid, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_ndb_and_terminal_ndb() {
        match decode_line(DB_AA_NDB).unwrap() {
            CifpRecord::NdbNavaid {
                ident,
                frequency_khz,
                name,
                magnetic_variation_deg,
                ..
            } => {
                assert_eq!(ident, "AA");
                assert_eq!(frequency_khz, 365);
                assert_eq!(name, "NARKENIE");
                assert_eq!(magnetic_variation_deg, Some(4.0));
            }
            other => panic!("expected NDB, got {other:?}"),
        }
        match decode_line(PN_AB_NDB).unwrap() {
            CifpRecord::NdbNavaid {
                airport_ident,
                frequency_khz,
                ..
            } => {
                assert_eq!(airport_ident, "KABI");
                assert_eq!(frequency_khz, 353);
            }
            other => panic!("expected terminal NDB, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_ils_localizer() {
        match decode_line(D_ISFO_ILS).unwrap() {
            CifpRecord::VhfNavaid {
                airport_ident,
                ident,
                frequency_khz,
                class,
                latitude_deg,
                longitude_deg,
                elevation_ft,
                ..
            } => {
                assert_eq!(airport_ident, "KSFO");
                assert_eq!(ident, "ISFO");
                assert_eq!(frequency_khz, 109_550);
                assert_eq!(class, " IT");
                assert!((latitude_deg - 37.62765).abs() < 5e-6);
                assert!((longitude_deg - (-122.39485)).abs() < 5e-6);
                assert_eq!(elevation_ft, Some(20));
            }
            other => panic!("expected ILS localizer, got {other:?}"),
        }
    }

    #[test]
    fn test_unsupported_record_is_explicit() {
        match decode_line(ER_AIRWAY).unwrap() {
            CifpRecord::Unsupported {
                section,
                subsection,
                raw,
                ..
            } => {
                assert_eq!(section, 'E');
                assert_eq!(subsection, 'R');
                assert!(raw.starts_with("SUSAER"));
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }

    #[test]
    fn test_interpret_to_canonical() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let valid_from = Utc::now();

        let wp_rec = decode_line(EA_AAARG).unwrap();
        match interpret(&wp_rec, &snapshot_id, valid_from) {
            CifpInterpretation::Waypoint(wp) => {
                assert_eq!(wp.object_id.0, "faa:SUSA:K:AAARG");
                assert_eq!(wp.waypoint_type, Some(waypoint_type_u32("W  ").unwrap()));
                assert!(wp.is_enroute);
            }
            other => panic!("expected waypoint, got {other:?}"),
        }

        let vor_rec = decode_line(D_ABI_VORTAC).unwrap();
        match interpret(&vor_rec, &snapshot_id, valid_from) {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::Vortac);
                assert_eq!(nav.object_id.0, "faa:SUSA:K4:ABI");
            }
            other => panic!("expected navaid, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_cifp_content_scans_and_filters() {
        let content =
            format!("{EA_AAARG}\n{D_ABI_VORTAC}\n{D_ISFO_ILS}\n{ER_AIRWAY}\n{DB_AA_NDB}\n");
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let (waypoints, navaids, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot_id, Utc::now());

        assert_eq!(waypoints.len(), 1);
        assert_eq!(navaids.len(), 3); // VORTAC + ILS + NDB
        assert_eq!(report.lines_seen, 5);
        assert_eq!(report.records_decoded, 4);
        assert_eq!(report.unsupported_records, 1);

        // ILS localizer must not carry fabricated bearing data.
        let ils = navaids
            .iter()
            .find(|n| n.kind == NavaidKind::IlsLocalizer)
            .unwrap();
        assert_eq!(ils.localizer_bearing_true_deg, None);
        assert_eq!(ils.associated_airport.as_deref(), Some("KSFO"));
    }
}
