//! FAA CIFP ARINC 424 Ingestion Adapter.
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
//! canonical entities (CanonicalWaypoint / CanonicalNavaid /
//! CanonicalAirwayLeg / CanonicalProcedureLeg)
//! ```
//!
//! Supported record classes (ARINC 424-18 / FAA CIFP, verified against
//! CIFP cycle 2608 and cross-checked against Laminar convert424toxplane
//! v12.4 output):
//! - Enroute waypoints: section `E`, subsection `A` (`EA`).
//! - VHF navaids: section `D`, subsection blank (`D `): VOR, VOR-DME,
//!   VORTAC, DME-only, TACAN-only and ILS localizers (class code `I`).
//!   VOR-DME/VORTAC facilities emit their paired DME component (row 12),
//!   and ILS localizers emit their DME-ILS component, from the dedicated
//!   DME position columns.
//! - NDB navaids: section `D`, subsection `B` (`DB`) and section `P`,
//!   subsection `N` (`PN`, terminal NDBs).
//! - Enroute airways: section `E`, subsection `R` (`ER`): consecutive
//!   records are chained into airway segments.
//!
//! Everything else is decoded as an explicit [`CifpRecord::Unsupported`]:
//! the raw line is preserved and never reinterpreted.
//!
//! Known gaps (documented, not hidden):
//! - ILS localizer bearing, glideslope angle and ILS category are not
//!   decodable from `D` records (convert424toxplane synthesizes the
//!   glideslope geometry and reads the category from `PF` records).
//!   Canonical localizers therefore carry `localizer_bearing_* = None`
//!   and exporters must refuse bearing-dependent rows.
//! - Terminal waypoints (`PC`) are not emitted for US airspace in CIFP;
//!   terminal procedures (PD/PE/PF) are future work.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use openairac_model::*;
use openairac_store::WorldStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

    /// Slice columns `start`..=`end` (1-based, inclusive). Callers trim.
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

fn parse_coord_pair(lat_field: &str, lon_field: &str) -> Result<Option<(f64, f64)>> {
    let lat = lat_field.trim();
    let lon = lon_field.trim();
    if lat.is_empty() || lon.is_empty() {
        return Ok(None);
    }
    Ok(Some(parse_arinc_coordinate(&format!("{lat}{lon}"))?))
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

/// Altitude field in feet (5 chars, right-ish aligned, e.g. `05000`).
fn parse_altitude_ft(field: &str) -> Option<u32> {
    let s = field.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u32>().ok().filter(|v| *v > 0)
}

// ---------------------------------------------------------------------------
// Layer 2: raw decoded records
// ---------------------------------------------------------------------------

/// A decoded CIFP record: either a supported record class with its verified
/// fields, or an explicit unsupported marker preserving the raw line.
/// These are transient decode artifacts (never stored); the size spread
/// between variants is acceptable.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum CifpRecord {
    Waypoint {
        record_type: String,
        /// ARINC 5.6: `ENRT` for enroute records.
        airport_ident: String,
        ident: String,
        /// ARINC 5.14 ICAO code (e.g. `K2`, `CY`, `K ` for US blanks).
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
        /// Navaid class (columns 28-30), e.g. `VTH`, ` IT`, ` DH`.
        class: String,
        latitude_deg: f64,
        longitude_deg: f64,
        /// DME component position (columns 56-74), when present.
        dme_latitude_deg: Option<f64>,
        dme_longitude_deg: Option<f64>,
        magnetic_variation_deg: Option<f64>,
        /// Station elevation in feet (tenths in the record).
        elevation_ft: Option<i32>,
        name: String,
        raw: String,
    },
    IlsLocalizerRecord {
        record_type: String,
        airport_ident: String,
        icao_code: String,
        ident: String,
        frequency_khz: u32,
        /// Runway end designator (e.g. `06`).
        runway: String,
        latitude_deg: f64,
        longitude_deg: f64,
        /// Station declination (cols 91-95), e.g. `W0120` -> -12.0.
        magnetic_variation_deg: Option<f64>,
        /// Station elevation in feet (cols 98-102).
        elevation_ft: Option<i32>,
        /// Glideslope antenna position (cols 56-74).
        glideslope_latitude_deg: Option<f64>,
        glideslope_longitude_deg: Option<f64>,
        /// Glideslope angle in degrees (cols 88-90, hundredths).
        glideslope_angle_deg: Option<f64>,
        /// Localizer front course, magnetic, degrees (cols 52-55,
        /// tenths). The authoritative front course: PF-derived
        /// association courses can disagree between procedure
        /// variants (IIPJ: I23-Y 233.0 vs I23-Z 223.0; the PI record
        /// publishes 233.2, matching convert424toxplane).
        front_course_mag_deg: Option<f64>,
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
    Airway {
        record_type: String,
        /// Route identifier (columns 14-18).
        route_ident: String,
        /// Route type (column 45): `O` conventional, `R` RNAV.
        route_type: char,
        /// Level (column 46): `H`, `L` or blank.
        level: Option<char>,
        /// Sequence number (columns 26-29).
        sequence_number: u32,
        /// Fix identifier of this record (columns 30-34).
        fix_ident: String,
        /// Fix ICAO code (columns 35-36).
        fix_icao_code: String,
        /// Fix section/subsection code (columns 37-38), e.g. `EA`, `D `.
        fix_section: String,
        /// MEA, feet (columns 84-88).
        minimum_altitude_ft: Option<u32>,
        /// Maximum altitude, feet (columns 94-98).
        maximum_altitude_ft: Option<u32>,
        raw: String,
    },
    ProcedureLeg {
        record_type: String,
        /// Record kind: D = SID, E = STAR, F = approach.
        procedure_kind: char,
        airport_ident: String,
        icao_code: String,
        procedure_ident: String,
        route_type: String,
        transition_ident: String,
        sequence_number: u32,
        fix_ident: String,
        fix_icao_code: String,
        fix_section: String,
        waypoint_description: String,
        turn_direction: Option<char>,
        rnp_nm: Option<f64>,
        path_terminator: String,
        recommended_navaid: Option<String>,
        arc_radius_nm: Option<f64>,
        course_a_deg: Option<f64>,
        distance_a_nm: Option<f64>,
        course_b_deg: Option<f64>,
        distance_b_nm: Option<f64>,
        altitude_descriptor: Option<char>,
        altitude_1_ft: Option<u32>,
        altitude_2_ft: Option<u32>,
        speed_limit_kts: Option<u32>,
        course_c_deg: Option<u32>,
        vertical_angle_deg: Option<f64>,
        msa_center_fix: Option<String>,
        route_qualifiers: String,
        raw: String,
    },
    /// PA terminal airport (verified vs real cycle 2608 records).
    Airport {
        airport_ident: String,
        icao_code: String,
        name: String,
        latitude_deg: f64,
        longitude_deg: f64,
        elevation_ft: Option<f64>,
    },
    /// PG terminal runway END (one threshold per record; ends are
    /// paired into runways by reciprocal designator).
    Runway {
        airport_ident: String,
        icao_code: String,
        designator: String,
        length_ft: u32,
        le_lat: f64,
        le_lon: f64,
    },
    Unsupported {
        record_type: String,
        section: char,
        subsection: char,
        /// Kind discriminator for polymorphic P-blank records (col 13).
        kind: char,
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
            kind: cifp.field(13, 13).chars().next().unwrap_or(' '),
            reason,
            raw: line.to_string(),
        })
    };

    match (section, subsection) {
        ('E', 'A') => {
            let (latitude_deg, longitude_deg) =
                parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
                    .ok_or_else(|| anyhow!("EA record without coordinates"))?;
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
                // NOTE: kept untrimmed on purpose — US records code `K `
                // (letter + blank), which X-Plane accepts (convert424toxplane
                // emits it verbatim).
                icao_code: cifp.field(20, 21).to_string(),
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
            // columns; VOR-family records carry it in the VOR columns with
            // the (possibly distinct) DME position at 56-74.
            let is_ils = class.as_bytes().get(1) == Some(&b'I');
            let vor_pos = parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?;
            let dme_pos = parse_coord_pair(cifp.field(56, 64), cifp.field(65, 74))?;
            let (latitude_deg, longitude_deg) = if is_ils {
                dme_pos.ok_or_else(|| anyhow!("ILS record '{ident}' without position"))?
            } else {
                // VOR-family: VOR columns; DME-only facilities carry their
                // position in the DME columns.
                vor_pos
                    .or(dme_pos)
                    .ok_or_else(|| anyhow!("D record '{ident}' without position"))?
            };
            // Elevation: cols 80-84, whole feet. Verified against
            // cycle 2608 records and convert424toxplane v12.4 output:
            // DBL (Red Table VOR) '11800' -> 11,800 ft, CDO '00069'
            // -> 69 ft, ISFO '00020' -> 20 ft. Magnetic variation is
            // cols 75-79 ('E0120' -> 12.0E); col 85 onward is a
            // separate field.
            let elevation_ft = cifp.field(80, 84).trim().parse::<i32>().ok();

            Ok(CifpRecord::VhfNavaid {
                record_type,
                airport_ident: cifp.field(7, 10).trim().to_string(),
                ident,
                icao_code: cifp.field(20, 21).trim().to_string(),
                frequency_khz,
                class,
                latitude_deg,
                longitude_deg,
                dme_latitude_deg: dme_pos.map(|p| p.0),
                dme_longitude_deg: dme_pos.map(|p| p.1),
                magnetic_variation_deg: parse_mag_variation(cifp.field(75, 79)),
                elevation_ft,
                name: cifp.field(91, 120).trim().to_string(),
                raw: line.to_string(),
            })
        }
        ('D', 'B') | ('P', 'N') => decode_ndb(&cifp, record_type, line),
        ('E', 'R') => {
            let route_ident = cifp.field(14, 18).trim().to_string();
            if route_ident.is_empty() {
                return Err(anyhow!("ER record without route identifier"));
            }
            let fix_ident = cifp.field(30, 34).trim().to_string();
            if fix_ident.is_empty() {
                return Err(anyhow!("ER record '{route_ident}' without fix identifier"));
            }
            let sequence_number: u32 = cifp
                .field(26, 29)
                .trim()
                .parse()
                .map_err(|_| anyhow!("ER record '{route_ident}': invalid sequence number"))?;
            Ok(CifpRecord::Airway {
                record_type,
                route_ident,
                route_type: cifp.field(45, 45).chars().next().unwrap_or(' '),
                level: cifp.field(46, 46).chars().next().filter(|c| *c != ' '),
                sequence_number,
                fix_ident,
                fix_icao_code: cifp.field(35, 36).to_string(),
                fix_section: cifp.field(37, 38).to_string(),
                minimum_altitude_ft: parse_altitude_ft(cifp.field(84, 88)),
                maximum_altitude_ft: parse_altitude_ft(cifp.field(94, 98)),
                raw: line.to_string(),
            })
        }
        ('P', ' ') => {
            // P-blank records are polymorphic: airports (PA), runways (PG),
            // terminal waypoints (PC) and procedure legs (PD/PE/PF).
            let terminator = cifp.field(48, 49).to_string();
            let kind_char = cifp.field(13, 13).chars().next().unwrap_or(' ');
            let is_leg = matches!(
                terminator.as_str(),
                "IF" | "TF"
                    | "CF"
                    | "DF"
                    | "RF"
                    | "AF"
                    | "VA"
                    | "VD"
                    | "VI"
                    | "VM"
                    | "CA"
                    | "CI"
                    | "CR"
                    | "FA"
                    | "FC"
                    | "FD"
                    | "FM"
                    | "HA"
                    | "HF"
                    | "HM"
            ) && matches!(kind_char, 'D' | 'E' | 'F');
            if is_leg {
                let sequence_number: u32 = cifp
                    .field(27, 29)
                    .trim()
                    .parse()
                    .map_err(|_| anyhow!("PD/PE/PF record: invalid sequence number"))?;
                let altitude = |field: &str| -> Option<u32> {
                    let f = field.trim();
                    if f.is_empty() {
                        return None;
                    }
                    if let Some(fl) = f.strip_prefix("FL") {
                        return fl.parse::<u32>().ok().map(|v| v * 100);
                    }
                    f.parse::<u32>().ok().filter(|v| *v > 0)
                };
                let navaid_ref = {
                    let ident = cifp.field(51, 54).trim().to_string();
                    if ident.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "{ident}:{}:{}:{}",
                            cifp.field(55, 56).trim(),
                            cifp.field(79, 79).trim(),
                            cifp.field(80, 80).trim()
                        ))
                    }
                };
                let msa_center = {
                    let ident = cifp.field(107, 111).trim().to_string();
                    if ident.is_empty() {
                        None
                    } else {
                        Some(format!(
                            "{ident}:{}:{}:{}",
                            cifp.field(113, 114).trim(),
                            cifp.field(115, 115).trim(),
                            cifp.field(116, 116).trim()
                        ))
                    }
                };
                Ok(CifpRecord::ProcedureLeg {
                    record_type,
                    procedure_kind: kind_char,
                    airport_ident: cifp.field(7, 10).trim().to_string(),
                    icao_code: cifp.field(11, 12).trim().to_string(),
                    procedure_ident: cifp.field(14, 19).trim().to_string(),
                    route_type: cifp.field(20, 20).to_string(),
                    transition_ident: cifp.field(21, 25).trim().to_string(),
                    sequence_number,
                    fix_ident: cifp.field(30, 34).trim().to_string(),
                    fix_icao_code: cifp.field(35, 36).to_string(),
                    fix_section: cifp.field(37, 38).to_string(),
                    waypoint_description: cifp.field(40, 43).to_string(),
                    turn_direction: cifp.field(44, 44).chars().next().filter(|c| *c != ' '),
                    rnp_nm: cifp
                        .field(45, 47)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| *v > 0.0)
                        .map(|v| v / 100.0),
                    path_terminator: terminator,
                    recommended_navaid: navaid_ref,
                    arc_radius_nm: cifp
                        .field(57, 62)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| *v > 0.0)
                        .map(|v| v / 100.0),
                    course_a_deg: cifp
                        .field(63, 66)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 10.0),
                    distance_a_nm: cifp
                        .field(67, 70)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 10.0),
                    course_b_deg: cifp
                        .field(71, 74)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 10.0),
                    distance_b_nm: cifp
                        .field(75, 78)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 10.0),
                    altitude_descriptor: cifp.field(83, 83).chars().next().filter(|c| *c != ' '),
                    altitude_1_ft: altitude(cifp.field(85, 89)),
                    altitude_2_ft: altitude(cifp.field(90, 94)),
                    speed_limit_kts: cifp
                        .field(95, 99)
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .map(|v| v / 100),
                    course_c_deg: cifp
                        .field(100, 102)
                        .trim()
                        .parse::<u32>()
                        .ok()
                        .filter(|v| *v > 0),
                    vertical_angle_deg: cifp
                        .field(103, 106)
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .filter(|v| *v != 0.0)
                        .map(|v| v / 100.0),
                    msa_center_fix: msa_center,
                    route_qualifiers: cifp.field(119, 120).to_string(),
                    raw: line.to_string(),
                })
            } else if kind_char == 'I' {
                // PI airport/heliport localizer. Verified against real
                // cycle 2608 records + convert424toxplane v12.4
                // output (IABE at KABE: position 40.661766667 /
                // -75.426430556, 110.70 MHz, RW06):
                // ident 14-17 (continuation at 18: '2' = second PI
                // record for the facility), frequency 22-27,
                // runway designator 30-32 ('RW' at 28-29),
                // position 33-51. The localizer bearing
                // columns are NOT decoded (not yet verified); the
                // bearing is enriched later from the PF-derived ILS
                // association, which is published and verified data.
                let airport_ident = cifp.field(7, 10).trim().to_string();
                let icao_code = cifp.field(11, 12).trim().to_string();
                let ident = cifp.field(14, 17).trim().to_string();
                if airport_ident.is_empty() || ident.is_empty() {
                    return unsupported("localizer record without ident/airport");
                }
                let frequency_khz: u32 = match cifp.field(22, 27).trim().parse::<u32>() {
                    Ok(v) if v > 0 => v * 10, // "011070" -> 110.70 MHz
                    _ => return unsupported("localizer record without frequency"),
                };
                let runway = cifp.field(30, 32).trim().to_string();
                let (latitude_deg, longitude_deg) =
                    parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
                        .ok_or_else(|| anyhow!("PI record '{ident}' without coordinates"))?;
                // Glideslope antenna position (cols 56-74), decoded
                // like the D record's DME position pair. Absent for
                // localizer-only facilities: the record then yields
                // only the localizer (fail-closed, no fabrication).
                let glideslope_position = parse_coord_pair(cifp.field(56, 64), cifp.field(65, 74))?;
                // Verified against real records + converter output:
                // IABE declination W0120 -> -12.0, elevation 00385 ->
                // 385 ft; IBZY E0110 -> +11.0, elevation 05304 ->
                // 5,304 ft. Front course 52-55 in tenths (IIPJ
                // '2332' -> 233.2, IAAD '2822' -> 282.2).
                let magnetic_variation_deg = parse_mag_variation(cifp.field(91, 95));
                let elevation_ft = cifp.field(98, 102).trim().parse::<i32>().ok();
                let glideslope_angle_deg = cifp
                    .field(88, 90)
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                    .map(|v| v / 100.0);
                let front_course_mag_deg = cifp
                    .field(52, 55)
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| *v > 0.0)
                    .map(|v| v / 10.0);
                Ok(CifpRecord::IlsLocalizerRecord {
                    record_type,
                    airport_ident,
                    icao_code,
                    ident,
                    frequency_khz,
                    runway,
                    latitude_deg,
                    longitude_deg,
                    magnetic_variation_deg,
                    elevation_ft,
                    glideslope_latitude_deg: glideslope_position.map(|p| p.0),
                    glideslope_longitude_deg: glideslope_position.map(|p| p.1),
                    glideslope_angle_deg,
                    front_course_mag_deg,
                    raw: line.to_string(),
                })
            } else if kind_char == 'A' {
                // PA terminal airport. Layout verified against real
                // cycle 2608 records for KSFO/KDEN/KJFK/KSEA:
                // ident 7-10, ICAO 11-12, coordinates 33-41/42-51,
                // magvar 52-57 (E/W at 52, value/10 at 53-57 — NOT
                // stored: CanonicalAirport has no magvar field),
                // elevation feet 58-62, name 91-120.
                let ident = cifp.field(7, 10).trim().to_string();
                if ident.is_empty() {
                    return unsupported("airport record without identifier");
                }
                let (latitude_deg, longitude_deg) =
                    parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
                        .ok_or_else(|| anyhow!("PA record '{ident}' without coordinates"))?;
                let elevation_ft = cifp.field(58, 62).trim().parse::<f64>().ok();
                Ok(CifpRecord::Airport {
                    airport_ident: ident,
                    icao_code: cifp.field(11, 12).to_string(),
                    name: cifp.field(91, 120).trim().to_string(),
                    latitude_deg,
                    longitude_deg,
                    elevation_ft,
                })
            } else if kind_char == 'G' {
                // PG terminal runway END. Layout verified against real
                // cycle 2608 records: ident 7-10, ICAO 11-12,
                // designator 16-18, length feet 22-26, threshold
                // coordinates 33-41/42-51. Width is NOT published in a
                // verified field -> None (never fabricated). Ends are
                // paired into runways by reciprocal designator during
                // interpretation.
                let airport_ident = cifp.field(7, 10).trim().to_string();
                let designator = cifp.field(16, 18).trim().to_string();
                if airport_ident.is_empty() || designator.is_empty() {
                    return unsupported("runway record without ident/designator");
                }
                // The published field is tens of feet: '01187' =
                // 11,870 ft (verified: KSFO 10L/28R 11,870 ft,
                // KDEN 16R/34L 16,000 ft, KSFO 01L/19R 7,650 ft).
                let length_ft: u32 = cifp
                    .field(22, 26)
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| anyhow!("PG record '{designator}': invalid length"))?
                    * 10;
                let (le_lat, le_lon) = parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
                    .ok_or_else(|| anyhow!("PG record '{designator}' without coordinates"))?;
                Ok(CifpRecord::Runway {
                    airport_ident,
                    icao_code: cifp.field(11, 12).to_string(),
                    designator,
                    length_ft,
                    le_lat,
                    le_lon,
                })
            } else if cifp.field(22, 22) == "0" && !cifp.field(33, 41).trim().is_empty() {
                // Terminal waypoint (PC): EA-style layout with the parent
                // airport in columns 7-10.
                let ident = cifp.field(14, 18).trim().to_string();
                if ident.is_empty() {
                    return unsupported("airport/runway record (PA/PG)");
                }
                let (latitude_deg, longitude_deg) =
                    parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
                        .ok_or_else(|| anyhow!("PC record '{ident}' without coordinates"))?;
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
                    icao_code: cifp.field(20, 21).to_string(),
                    waypoint_type: cifp.field(27, 29).to_string(),
                    latitude_deg,
                    longitude_deg,
                    magnetic_variation_deg: parse_mag_variation(cifp.field(75, 80)),
                    raw: line.to_string(),
                })
            } else {
                unsupported("unrecognized P-blank record kind")
            }
        }
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
        parse_coord_pair(cifp.field(33, 41), cifp.field(42, 51))?
            .ok_or_else(|| anyhow!("NDB record '{ident}' without coordinates"))?;
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

/// Result of interpreting one raw record into canonical form.
/// One PG runway END before reciprocal pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct CifpRunwayEnd {
    pub airport_ident: String,
    pub icao_code: String,
    pub designator: String,
    pub length_ft: u32,
    pub le_lat: f64,
    pub le_lon: f64,
}

/// Transient decode artifact; the size spread is acceptable.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]

pub enum CifpInterpretation {
    Waypoint(CanonicalWaypoint),
    Navaid(CanonicalNavaid),
    AirwayLeg(CanonicalAirwayLeg),
    ProcedureLeg(CanonicalProcedureLeg),
    Airport(CanonicalAirport),
    RunwayEnd(CifpRunwayEnd),
    Unsupported {
        reason: String,
        raw: String,
        section: char,
        subsection: char,
        kind: char,
    },
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

/// XPNAV1200 class/volume mapping derived from the source class field.
/// The altitude/service class character is ALWAYS column 30 (index 2) of the
/// D-record class field (columns 28-30). Column 29 carries the facility or
/// colocated-component type (V/D/T/I/O). Mapping verified against
/// convert424toxplane v12.4 output row-by-row on CIFP cycle 2608:
/// H -> 130, L -> 40, T -> 25, U -> 150 (VOR family) / 125 (DME).
/// NDB: column 30 'L' -> 15 (locator), 'M' -> 25, blank -> 50. The official
/// tool refines locator classification further with data outside the D
/// record; those cases remain unknown here and export as None (skipped).
fn service_volume_from_cifp_class(kind: NavaidKind, class: &str) -> Option<u16> {
    let b = class.as_bytes();
    match kind {
        NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac | NavaidKind::Dme => {
            match b.get(2).copied() {
                Some(b'H') => Some(130),
                Some(b'L') => Some(40),
                Some(b'T') => Some(25),
                // U = undetermined. VOR-family records map to 150;
                // standalone DME records (' DUx' class) map to 125.
                // Verified against converter v12.4 for cycle 2608:
                // CDO ('VDU' -> 150), ADK/EEA (' DU' -> 125).
                Some(b'U') => Some(if kind == NavaidKind::Dme && !class.starts_with('V') {
                    125
                } else {
                    150
                }),
                _ => None,
            }
        }
        NavaidKind::IlsLocalizer => None,
        NavaidKind::Ndb => match b.get(2).copied() {
            Some(b'L') => Some(15),
            Some(b'M') => Some(25),
            Some(b' ') | None => Some(50),
            _ => None,
        },
        _ => None,
    }
}

/// Reciprocal runway designator: 07 <-> 25, 16L <-> 34R, 08 <-> 26.
/// Letters swap L<->R; C stays C.
fn reciprocal_designator(designator: &str) -> Option<String> {
    let (num_str, letters) = designator.split_at(
        designator
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(designator.len()),
    );
    let num: u32 = num_str.parse().ok()?;
    let recip = (num + 18) % 36;
    let swapped = match letters {
        "L" => "R",
        "R" => "L",
        "C" => "C",
        "" => "",
        _ => return None,
    };
    Some(format!("{recip:02}{swapped}"))
}

fn runway_number(designator: &str) -> u32 {
    designator
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Interpret a raw record into canonical entities. One record may produce
/// several entities (e.g. a VORTAC yields the VOR row plus its paired DME
/// row). Never fabricates values: unknown semantics stay `Unsupported`.
pub fn interpret(
    record: &CifpRecord,
    snapshot_id: &SourceSnapshotId,
    valid_from: DateTime<Utc>,
) -> Vec<CifpInterpretation> {
    let temporal = TemporalValidity {
        valid_from,
        valid_until: None,
        source_snapshot_id: snapshot_id.clone(),
    };

    match record {
        CifpRecord::Waypoint {
            record_type,
            airport_ident,
            ident,
            icao_code,
            waypoint_type,
            latitude_deg,
            longitude_deg,
            name,
            ..
        } => {
            // EA records code "ENRT" in the airport field; PC records code
            // the parent airport ident.
            let is_enroute = airport_ident.is_empty() || airport_ident == "ENRT";
            vec![CifpInterpretation::Waypoint(CanonicalWaypoint {
                object_id: WaypointId(format!("faa:{record_type}:{icao_code}:{ident}")),
                ident: ident.clone(),
                name: name.clone(),
                latitude: *latitude_deg,
                longitude: *longitude_deg,
                is_enroute,
                region_code: icao_code.clone(),
                terminal_area_ident: (!airport_ident.is_empty() && airport_ident != "ENRT")
                    .then(|| airport_ident.clone()),
                waypoint_type: waypoint_type_u32(waypoint_type),
                temporal,
            })]
        }
        CifpRecord::IlsLocalizerRecord {
            record_type,
            airport_ident,
            icao_code,
            ident,
            frequency_khz,
            runway,
            latitude_deg,
            longitude_deg,
            magnetic_variation_deg,
            elevation_ft,
            glideslope_latitude_deg,
            glideslope_longitude_deg,
            glideslope_angle_deg,
            front_course_mag_deg,
            ..
        } => {
            let temporal = temporal.clone();
            let localizer_bearing_true_deg = match (*magnetic_variation_deg, *front_course_mag_deg)
            {
                (Some(var), Some(mag)) => Some((mag + var).rem_euclid(360.0)),
                _ => None,
            };
            let localizer = CanonicalNavaid {
                object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}")),
                ident: ident.clone(),
                name: ident.clone(),
                kind: NavaidKind::IlsLocalizer,
                frequency: FrequencyKhz(*frequency_khz),
                latitude: *latitude_deg,
                longitude: *longitude_deg,
                elevation_ft: *elevation_ft,
                region_code: Some(icao_code.clone()),
                associated_airport: Some(airport_ident.clone()),
                associated_runway: (!runway.is_empty()).then(|| runway.clone()),
                magnetic_variation_deg: *magnetic_variation_deg,
                localizer_bearing_true_deg,
                localizer_bearing_mag_deg: *front_course_mag_deg,
                glideslope_angle_deg: None,
                slaved_variation_deg: None,
                service_volume_nm: Some(18),
                dme_paired: false,
                temporal,
            };
            if let (Some(glideslope_latitude_deg), Some(glideslope_longitude_deg)) =
                (*glideslope_latitude_deg, *glideslope_longitude_deg)
            {
                let glideslope = CanonicalNavaid {
                    object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}:gs")),
                    ident: ident.clone(),
                    name: ident.clone(),
                    kind: NavaidKind::IlsGlidepath,
                    frequency: FrequencyKhz(*frequency_khz),
                    latitude: glideslope_latitude_deg,
                    longitude: glideslope_longitude_deg,
                    elevation_ft: *elevation_ft,
                    region_code: Some(icao_code.clone()),
                    associated_airport: Some(airport_ident.clone()),
                    associated_runway: (!runway.is_empty()).then(|| runway.clone()),
                    magnetic_variation_deg: *magnetic_variation_deg,
                    localizer_bearing_true_deg,
                    localizer_bearing_mag_deg: *front_course_mag_deg,
                    glideslope_angle_deg: *glideslope_angle_deg,
                    slaved_variation_deg: None,
                    service_volume_nm: Some(18),
                    dme_paired: false,
                    temporal: localizer.temporal.clone(),
                };
                vec![
                    CifpInterpretation::Navaid(localizer),
                    CifpInterpretation::Navaid(glideslope),
                ]
            } else {
                vec![CifpInterpretation::Navaid(localizer)]
            }
        }
        CifpRecord::VhfNavaid {
            record_type,
            airport_ident,
            ident,
            icao_code,
            frequency_khz,
            class,
            latitude_deg,
            longitude_deg,
            dme_latitude_deg,
            dme_longitude_deg,
            magnetic_variation_deg,
            elevation_ft,
            name,
            ..
        } => {
            let Some(kind) = navaid_kind_from_class(class) else {
                return vec![CifpInterpretation::Unsupported {
                    reason: format!("unrecognized navaid class '{class}'"),
                    raw: record_to_raw(record),
                    section: 'D',
                    subsection: ' ',
                    kind: ' ',
                }];
            };
            let associated_airport = (!airport_ident.is_empty()).then(|| airport_ident.clone());
            let volume = service_volume_from_cifp_class(kind, class);
            // The VOR's station declination IS the published slaved variation
            // (0-radial direction) for a VOR; convert424toxplane emits the
            // same field value. Not substituted for other kinds.
            let slaved = if matches!(
                kind,
                NavaidKind::Vor | NavaidKind::Vordme | NavaidKind::Vortac
            ) {
                *magnetic_variation_deg
            } else {
                None
            };

            let mut out = vec![CifpInterpretation::Navaid(CanonicalNavaid {
                object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}")),
                ident: ident.clone(),
                name: name.clone(),
                kind,
                frequency: FrequencyKhz(*frequency_khz),
                latitude: *latitude_deg,
                longitude: *longitude_deg,
                elevation_ft: *elevation_ft,
                region_code: Some(icao_code.clone()),
                associated_airport: associated_airport.clone(),
                magnetic_variation_deg: *magnetic_variation_deg,
                slaved_variation_deg: slaved,
                service_volume_nm: volume,
                dme_paired: false,
                associated_runway: None,
                localizer_bearing_true_deg: None,
                localizer_bearing_mag_deg: None,
                glideslope_angle_deg: None,
                temporal: temporal.clone(),
            })];

            let dme_for = match kind {
                NavaidKind::Vordme | NavaidKind::Vortac => "DME",
                NavaidKind::IlsLocalizer => "DME-ILS",
                _ => "",
            };
            if !dme_for.is_empty() {
                let position = match (*dme_latitude_deg, *dme_longitude_deg) {
                    (Some(lat), Some(lon)) => (lat, lon),
                    _ => (*latitude_deg, *longitude_deg),
                };
                out.push(CifpInterpretation::Navaid(CanonicalNavaid {
                    object_id: NavaidId(format!("faa:{record_type}:{icao_code}:{ident}:dme")),
                    ident: ident.clone(),
                    name: format!("{name} {dme_for}"),
                    kind: NavaidKind::Dme,
                    frequency: FrequencyKhz(*frequency_khz),
                    latitude: position.0,
                    longitude: position.1,
                    elevation_ft: *elevation_ft,
                    region_code: Some(icao_code.clone()),
                    associated_airport: associated_airport.clone(),
                    magnetic_variation_deg: *magnetic_variation_deg,
                    slaved_variation_deg: None,
                    // Source-derived only: the class mapping (verified against
                    // convert424toxplane output) maps 'H'/'L'/'T'/'U'. No
                    // fallback value is fabricated here.
                    service_volume_nm: service_volume_from_cifp_class(NavaidKind::Dme, class),
                    dme_paired: true,
                    associated_runway: None,
                    localizer_bearing_true_deg: None,
                    localizer_bearing_mag_deg: None,
                    glideslope_angle_deg: None,
                    temporal,
                }));
            }
            out
        }
        CifpRecord::NdbNavaid {
            record_type,
            airport_ident,
            ident,
            icao_code,
            frequency_khz,
            class,
            latitude_deg,
            longitude_deg,
            magnetic_variation_deg,
            name,
            ..
        } => vec![CifpInterpretation::Navaid(CanonicalNavaid {
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
            slaved_variation_deg: None,
            service_volume_nm: service_volume_from_cifp_class(NavaidKind::Ndb, class),
            dme_paired: false,
            associated_runway: None,
            localizer_bearing_true_deg: None,
            localizer_bearing_mag_deg: None,
            glideslope_angle_deg: None,
            temporal,
        })],
        CifpRecord::Airway { .. } => vec![CifpInterpretation::Unsupported {
            reason: "airway segment (chained by the scanner)".to_string(),
            raw: record_to_raw(record),
            section: 'E',
            subsection: 'R',
            kind: ' ',
        }],
        CifpRecord::ProcedureLeg {
            record_type,
            procedure_kind,
            airport_ident,
            icao_code,
            procedure_ident,
            route_type,
            transition_ident,
            sequence_number,
            fix_ident,
            fix_icao_code,
            fix_section,
            waypoint_description,
            turn_direction,
            rnp_nm,
            path_terminator,
            recommended_navaid,
            arc_radius_nm,
            course_a_deg,
            distance_a_nm,
            course_b_deg,
            distance_b_nm,
            altitude_descriptor,
            altitude_1_ft,
            altitude_2_ft,
            speed_limit_kts,
            course_c_deg,
            vertical_angle_deg,
            msa_center_fix,
            route_qualifiers,
            raw,
        } => vec![CifpInterpretation::ProcedureLeg(CanonicalProcedureLeg {
            object_id: ProcedureLegId(format!(
                "faa:{record_type}:{airport_ident}:{procedure_kind}:{procedure_ident}:{transition_ident}:{sequence_number}"
            )),
            airport_ident: airport_ident.clone(),
            icao_code: icao_code.clone(),
            procedure_kind: *procedure_kind,
            procedure_ident: procedure_ident.clone(),
            route_type: route_type.clone(),
            transition_ident: transition_ident.clone(),
            sequence_number: *sequence_number,
            fix_ident: fix_ident.clone(),
            fix_icao_code: fix_icao_code.clone(),
            fix_section: fix_section.clone(),
            waypoint_description: waypoint_description.clone(),
            turn_direction: *turn_direction,
            rnp_nm: *rnp_nm,
            path_terminator: path_terminator.clone(),
            recommended_navaid: recommended_navaid.clone(),
            arc_radius_nm: *arc_radius_nm,
            course_a_deg: *course_a_deg,
            distance_a_nm: *distance_a_nm,
            course_b_deg: *course_b_deg,
            distance_b_nm: *distance_b_nm,
            altitude_descriptor: *altitude_descriptor,
            altitude_1_ft: *altitude_1_ft,
            altitude_2_ft: *altitude_2_ft,
            speed_limit_kts: *speed_limit_kts,
            course_c_deg: *course_c_deg,
            vertical_angle_deg: *vertical_angle_deg,
            msa_center_fix: msa_center_fix.clone(),
            route_qualifiers: route_qualifiers.clone(),
            raw: raw.clone(),
            temporal,
        })],
        CifpRecord::Airport {
            airport_ident,
            icao_code,
            name,
            latitude_deg,
            longitude_deg,
            elevation_ft,
        } => vec![CifpInterpretation::Airport(CanonicalAirport {
            id: AirportId(format!("faa:{icao_code}:{airport_ident}")),
            ident: airport_ident.clone(),
            name: if name.is_empty() {
                airport_ident.clone()
            } else {
                name.clone()
            },
            airport_type: "PA".to_string(),
            latitude: *latitude_deg,
            longitude: *longitude_deg,
            elevation_ft: *elevation_ft,
            iso_country: None,
            municipality: None,
            runways: Vec::new(),
            temporal: temporal.clone(),
        })],
        CifpRecord::Runway {
            airport_ident,
            icao_code,
            designator,
            length_ft,
            le_lat,
            le_lon,
        } => vec![CifpInterpretation::RunwayEnd(CifpRunwayEnd {
            airport_ident: airport_ident.clone(),
            icao_code: icao_code.clone(),
            designator: designator.clone(),
            length_ft: *length_ft,
            le_lat: *le_lat,
            le_lon: *le_lon,
        })],
        CifpRecord::Unsupported {
            reason,
            raw,
            section,
            subsection,
            kind,
            ..
        } => vec![CifpInterpretation::Unsupported {
            reason: reason.to_string(),
            raw: raw.clone(),
            section: *section,
            subsection: *subsection,
            kind: *kind,
        }],
    }
}

fn record_to_raw(record: &CifpRecord) -> String {
    match record {
        CifpRecord::Waypoint { raw, .. }
        | CifpRecord::VhfNavaid { raw, .. }
        | CifpRecord::IlsLocalizerRecord { raw, .. }
        | CifpRecord::NdbNavaid { raw, .. }
        | CifpRecord::Airway { raw, .. }
        | CifpRecord::ProcedureLeg { raw, .. }
        | CifpRecord::Unsupported { raw, .. } => raw.clone(),
        CifpRecord::Airport { .. } | CifpRecord::Runway { .. } => String::new(),
    }
}

// Ingest entry points
// ---------------------------------------------------------------------------

/// Deterministic scan statistics for a full CIFP content pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CifpScanReport {
    pub lines_seen: usize,
    pub records_decoded: usize,
    pub waypoints_decoded: usize,
    pub navaids_decoded: usize,
    pub airway_legs_decoded: usize,
    pub procedure_legs_decoded: usize,
    pub airports_decoded: usize,
    /// Runway ENDS decoded (paired into runways during scanning).
    pub runway_ends_decoded: usize,
    /// Runways produced by reciprocal pairing.
    pub runways_decoded: usize,
    /// Runway ends whose reciprocal was missing (skipped, fail-closed).
    pub unpaired_runway_ends: usize,
    pub unsupported_records: usize,
    pub decode_errors: usize,
    /// Duplicate object ids whose payloads CONFLICT (first occurrence
    /// kept, rest skipped with diagnostics — never merged, never chosen
    /// silently). Found in real cycle 2608 (terminal waypoints shared
    /// across procedures).
    pub duplicate_conflicts: usize,
    pub unsupported_reasons: Vec<String>,
    /// Structured (section, subsection, kind) of unsupported record
    /// classes — the basis for close_absent masking (a parser failure
    /// must never silently become a source deletion).
    pub unsupported_classes: Vec<(char, char, char)>,
}

pub struct FaaCifpAdapter;

impl FaaCifpAdapter {
    /// Decode and interpret a complete CIFP content string.
    ///
    /// Returns the accepted canonical entities plus a deterministic scan
    /// report. ER airway records are chained into segments: each consecutive
    /// record of a route becomes a segment from the previous fix to this fix.
    #[allow(clippy::type_complexity)]
    pub fn parse_cifp_content(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
    ) -> (
        Vec<CanonicalWaypoint>,
        Vec<CanonicalNavaid>,
        Vec<CanonicalAirwayLeg>,
        Vec<CanonicalProcedureLeg>,
        Vec<CanonicalAirport>,
        Vec<CanonicalRunway>,
        Vec<IlsAssociation>,
        CifpScanReport,
    ) {
        let mut waypoints = Vec::new();
        let mut navaids = Vec::new();
        let mut airway_legs = Vec::new();
        let mut procedure_legs = Vec::new();
        let mut airports = Vec::new();
        let mut runway_ends = Vec::new();
        let mut report = CifpScanReport::default();

        // Per-route previous record for ER chaining.
        //
        // ARINC 424 semantics: the MEA/MAA on a fix record applies to
        // the segment FOLLOWING that fix (verified against
        // convert424toxplane v12.4 on cycle 2608: AADCO's record MEA
        // 11500 -> leg AADCO->VERNE base FL115). The segment emitted
        // when the next record arrives therefore carries the PREVIOUS
        // record's altitudes, not the current one's.
        #[derive(Clone)]
        struct Prev {
            fix_ident: String,
            fix_icao_code: String,
            minimum_altitude_ft: Option<u32>,
            maximum_altitude_ft: Option<u32>,
        }
        let mut prev_by_route: std::collections::HashMap<String, Prev> = Default::default();

        for line in content.lines() {
            if line.len() < 132 {
                continue; // header or short padding line; not a data record
            }
            report.lines_seen += 1;
            match decode_line(line) {
                Ok(CifpRecord::Airway {
                    record_type,
                    route_ident,
                    route_type,
                    level,
                    sequence_number,
                    fix_ident,
                    fix_icao_code,
                    fix_section: _,
                    minimum_altitude_ft,
                    maximum_altitude_ft,
                    raw: _,
                }) => {
                    if let Some(prev) = prev_by_route.get(&route_ident) {
                        // Consecutive record: segment from previous fix to this.
                        airway_legs.push(CanonicalAirwayLeg {
                            object_id: AirwayLegId(format!(
                                "faa:{record_type}:{route_ident}:{sequence_number}"
                            )),
                            route_ident: route_ident.clone(),
                            route_type: route_type.to_string(),
                            level,
                            sequence_number,
                            start_fix: prev.fix_ident.clone(),
                            start_icao_code: prev.fix_icao_code.clone(),
                            end_fix: fix_ident.clone(),
                            end_icao_code: fix_icao_code.clone(),
                            direction: 'N', // CIFP carries no directional restrictions
                            minimum_altitude_ft: prev.minimum_altitude_ft,
                            maximum_altitude_ft: prev.maximum_altitude_ft,
                            temporal: TemporalValidity {
                                valid_from,
                                valid_until: None,
                                source_snapshot_id: snapshot_id.clone(),
                            },
                        });
                        report.records_decoded += 1;
                        report.airway_legs_decoded += 1;
                    }
                    prev_by_route.insert(
                        route_ident,
                        Prev {
                            fix_ident,
                            fix_icao_code,
                            minimum_altitude_ft,
                            maximum_altitude_ft,
                        },
                    );
                }
                Ok(record) => {
                    for interpretation in interpret(&record, snapshot_id, valid_from) {
                        match interpretation {
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
                            CifpInterpretation::AirwayLeg(leg) => {
                                report.records_decoded += 1;
                                report.airway_legs_decoded += 1;
                                airway_legs.push(leg);
                            }
                            CifpInterpretation::ProcedureLeg(leg) => {
                                report.records_decoded += 1;
                                report.procedure_legs_decoded += 1;
                                procedure_legs.push(leg);
                            }
                            CifpInterpretation::Airport(airport) => {
                                report.records_decoded += 1;
                                report.airports_decoded += 1;
                                airports.push(airport);
                            }
                            CifpInterpretation::RunwayEnd(end) => {
                                report.records_decoded += 1;
                                report.runway_ends_decoded += 1;
                                runway_ends.push(end);
                            }
                            CifpInterpretation::Unsupported {
                                reason,
                                raw,
                                section,
                                subsection,
                                kind,
                            } => {
                                report.unsupported_records += 1;
                                report.unsupported_classes.push((section, subsection, kind));
                                if report.unsupported_reasons.len() < 1000 {
                                    report
                                        .unsupported_reasons
                                        .push(format!("{reason}: {}", &raw[..64.min(raw.len())]));
                                }
                            }
                        }
                    }
                }
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

        // The D record (class 'I') and the PI record describe the same
        // localizer; the PI record carries the authoritative localizer
        // position, elevation, declination and runway (verified on
        // cycle 2608: IAAD PI position 43.571389/-116.240364 matches
        // convert424toxplane row 4 exactly; the D record's DME columns
        // differ by ~50 m and 23 ft elevation). Merge before dedupe:
        // PI fields win on the shared entity; the PI localizer itself
        // is dropped (its glideslope component remains).
        let navaids = {
            // PI-derived localizers carry a runway (the D record does
            // not); that is the discriminator between the two
            // otherwise identically-named localizer entities. PI-only
            // localizers (no D counterpart) must survive the merge.
            let d_loc_keys: std::collections::HashSet<(String, String)> = navaids
                .iter()
                .filter(|n| n.kind == NavaidKind::IlsLocalizer && n.associated_runway.is_none())
                .map(|n| {
                    (
                        n.ident.clone(),
                        n.associated_airport.clone().unwrap_or_default(),
                    )
                })
                .collect();
            let pi_loc: std::collections::HashMap<
                (String, String),
                (
                    f64,
                    f64,
                    Option<i32>,
                    Option<f64>,
                    Option<String>,
                    Option<f64>,
                    Option<f64>,
                ),
            > = navaids
                .iter()
                .filter(|n| n.kind == NavaidKind::IlsLocalizer && n.associated_runway.is_some())
                .map(|n| {
                    (
                        (
                            n.ident.clone(),
                            n.associated_airport.clone().unwrap_or_default(),
                        ),
                        (
                            n.latitude,
                            n.longitude,
                            n.elevation_ft,
                            n.magnetic_variation_deg,
                            n.associated_runway.clone(),
                            n.localizer_bearing_mag_deg,
                            n.localizer_bearing_true_deg,
                        ),
                    )
                })
                .collect();
            navaids
                .into_iter()
                .filter(|n| {
                    !(n.kind == NavaidKind::IlsLocalizer
                        && n.associated_runway.is_some()
                        && d_loc_keys.contains(&(
                            n.ident.clone(),
                            n.associated_airport.clone().unwrap_or_default(),
                        )))
                })
                .map(|mut n| {
                    if n.kind == NavaidKind::IlsLocalizer
                        && let Some((lat, lon, elev, decl, runway, course, true_b)) = pi_loc.get(&(
                            n.ident.clone(),
                            n.associated_airport.clone().unwrap_or_default(),
                        ))
                    {
                        n.latitude = *lat;
                        n.longitude = *lon;
                        n.elevation_ft = *elev;
                        n.magnetic_variation_deg = *decl;
                        n.associated_runway = runway.clone();
                        n.localizer_bearing_mag_deg = *course;
                        n.localizer_bearing_true_deg = *true_b;
                    }
                    n
                })
                .collect::<Vec<_>>()
        };

        // Real-world cycle 2608 contains duplicate object ids with
        // CONFLICTING payloads (terminal waypoints shared across
        // procedures). Fail-closed: the first occurrence is kept,
        // conflicting duplicates are skipped with diagnostics — never
        // merged, never silently chosen.
        let waypoints = dedupe_entities(
            waypoints,
            |w| &w.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let navaids = dedupe_entities(
            navaids,
            |n| &n.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let airway_legs = dedupe_entities(
            airway_legs,
            |l| &l.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let procedure_legs = dedupe_entities(
            procedure_legs,
            |l| &l.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );

        // Pair PG runway ends into runways by reciprocal designator
        // (10L <-> 28R, 16R <-> 34L). Unpaired ends are skipped with
        // diagnostics — never fabricated into half-runways.
        let mut runways = Vec::new();
        {
            let mut by_airport: std::collections::HashMap<
                (String, String),
                std::collections::HashMap<String, CifpRunwayEnd>,
            > = std::collections::HashMap::new();
            for end in &runway_ends {
                by_airport
                    .entry((end.icao_code.clone(), end.airport_ident.clone()))
                    .or_default()
                    .insert(end.designator.clone(), end.clone());
            }
            let mut paired: std::collections::HashSet<String> = std::collections::HashSet::new();
            for ((icao, airport), ends) in &by_airport {
                for (designator, end) in ends {
                    let key = format!("{icao}:{airport}:{designator}");
                    if paired.contains(&key) {
                        continue;
                    }
                    let Some(reciprocal) = reciprocal_designator(designator) else {
                        report.unpaired_runway_ends += 1;
                        report.unsupported_reasons.push(format!(
                            "runway end '{designator}' at {airport}: no reciprocal designator"
                        ));
                        continue;
                    };
                    let Some(other) = ends.get(&reciprocal) else {
                        report.unpaired_runway_ends += 1;
                        if report.unsupported_reasons.len() < 1000 {
                            report.unsupported_reasons.push(format!(
                                "runway end '{designator}' at {airport}: reciprocal '{reciprocal}' missing"
                            ));
                        }
                        continue;
                    };
                    paired.insert(format!("{icao}:{airport}:{reciprocal}"));
                    let (le, he) = if runway_number(designator) <= runway_number(&reciprocal) {
                        (end, other)
                    } else {
                        (other, end)
                    };
                    if le.length_ft != he.length_ft {
                        report.unpaired_runway_ends += 1;
                        if report.unsupported_reasons.len() < 1000 {
                            report.unsupported_reasons.push(format!(
                                "runway {designator}/{reciprocal} at {airport}: length mismatch ({} vs {})",
                                le.length_ft, he.length_ft
                            ));
                        }
                        continue;
                    }
                    let airport_id = AirportId(format!("faa:{icao}:{airport}"));
                    runways.push(CanonicalRunway {
                        id: RunwayId(format!(
                            "faa:{icao}:{airport}:{}-{}",
                            le.designator, he.designator
                        )),
                        airport_id,
                        airport_ident: airport.clone(),
                        official_designator: le.designator.clone(),
                        computed_magnetic_designator: None,
                        true_heading_deg: None,
                        length_ft: le.length_ft,
                        width_ft: None,
                        surface: None,
                        le_ident: le.designator.clone(),
                        le_lat: le.le_lat,
                        le_lon: le.le_lon,
                        le_elevation_ft: None,
                        he_ident: he.designator.clone(),
                        he_lat: he.le_lat,
                        he_lon: he.le_lon,
                        he_elevation_ft: None,
                        temporal: TemporalValidity {
                            valid_from,
                            valid_until: None,
                            source_snapshot_id: snapshot_id.clone(),
                        },
                    });
                    report.runways_decoded += 1;
                }
            }
        }

        // Real-world cycle 2608 contains duplicate object ids with
        // CONFLICTING payloads (terminal waypoints shared across
        // procedures). Fail-closed: the first occurrence is kept,
        // conflicting duplicates are skipped with diagnostics — never
        // merged, never silently chosen.
        let waypoints = dedupe_entities(
            waypoints,
            |w| &w.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let navaids = dedupe_entities(
            navaids,
            |n| &n.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let airway_legs = dedupe_entities(
            airway_legs,
            |l| &l.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let procedure_legs = dedupe_entities(
            procedure_legs,
            |l| &l.object_id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let airports = dedupe_entities(
            airports,
            |a| &a.id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );
        let runways = dedupe_entities(
            runways,
            |r| &r.id.0,
            &mut report.duplicate_conflicts,
            &mut report.unsupported_reasons,
        );

        // Verified ILS associations: for each F-kind approach whose
        // ident starts with 'I', the final RW-fix CF/TF leg's
        // recommended navaid IS the localizer; its course is the
        // bearing and its vertical angle the glideslope.
        let mut ils_associations = Vec::new();
        {
            let mut groups: std::collections::HashMap<
                (String, String),
                Vec<&CanonicalProcedureLeg>,
            > = std::collections::HashMap::new();
            for leg in &procedure_legs {
                if leg.procedure_kind == 'F' && leg.procedure_ident.starts_with('I') {
                    groups
                        .entry((leg.airport_ident.clone(), leg.procedure_ident.clone()))
                        .or_default()
                        .push(leg);
                }
            }
            for ((airport, ident), legs) in groups {
                let Some(final_leg) = legs.iter().find(|l| {
                    l.fix_ident.starts_with("RW")
                        && matches!(l.path_terminator.as_str(), "CF" | "TF")
                        && l.recommended_navaid.is_some()
                }) else {
                    continue;
                };
                let Some(bearing) = final_leg.course_b_deg else {
                    continue;
                };
                let Some(angle) = final_leg.vertical_angle_deg else {
                    continue;
                };
                let nav = final_leg.recommended_navaid.as_deref().unwrap_or("");
                let mut parts = nav.split(':');
                let loc_ident = parts.next().unwrap_or("").trim().to_string();
                let loc_region = parts.next().unwrap_or("").trim().to_string();
                if loc_ident.is_empty() {
                    continue;
                }
                let runway_end = ident
                    .strip_prefix('I')
                    .unwrap_or(ident.as_str())
                    .to_string();
                ils_associations.push(IlsAssociation {
                    airport_ident: airport.clone(),
                    icao_code: final_leg.icao_code.clone(),
                    approach_ident: ident.clone(),
                    runway_end,
                    localizer_ident: loc_ident,
                    localizer_region: loc_region,
                    localizer_bearing_mag_deg: bearing,
                    glideslope_angle_deg: angle.abs(),
                    source_snapshot_id: snapshot_id.clone(),
                });
            }
            // Enrich the canonical localizer navaids with the verified
            // bearing/glideslope (closes the v0.3 D-record gap).
            let mut navs = navaids.clone();
            for n in navs.iter_mut() {
                if matches!(
                    n.kind,
                    NavaidKind::IlsLocalizer | NavaidKind::IlsGlidepath | NavaidKind::Dme
                ) && let Some(assoc) = ils_associations.iter().find(|a| {
                    a.localizer_ident == n.ident
                        && a.airport_ident == n.associated_airport.clone().unwrap_or_default()
                }) {
                    // The PI front course (when present) is authoritative;
                    // the association value fills the D-only case and must
                    // not overwrite it (IIPJ: PI 233.2 vs I23-Z 223.0).
                    if n.localizer_bearing_mag_deg.is_none() {
                        n.localizer_bearing_mag_deg = Some(assoc.localizer_bearing_mag_deg);
                    }
                    n.glideslope_angle_deg = Some(assoc.glideslope_angle_deg);
                    // The D record does not publish the runway; the
                    // PF-derived association does. Without it, D-record
                    // localizers can never emit an X-Plane row 4/5.
                    if n.associated_runway.is_none() {
                        n.associated_runway = Some(assoc.runway_end.clone());
                    }
                }
            }
            let navaids = navs;

            (
                waypoints,
                navaids,
                airway_legs,
                procedure_legs,
                airports,
                runways,
                ils_associations,
                report,
            )
        }
    }

    /// Parse CIFP content and ingest the canonical entities into the store
    /// transactionally. Returns scan statistics.
    pub fn ingest_cifp(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
        store: &mut WorldStore,
    ) -> Result<CifpScanReport> {
        let (
            waypoints,
            navaids,
            airway_legs,
            procedure_legs,
            airports,
            runways,
            ils_associations,
            report,
        ) = Self::parse_cifp_content(content, snapshot_id, valid_from);

        store.transact(|conn| {
            insert_cifp_entities_conn(conn, &waypoints, &navaids, &airway_legs, &procedure_legs)?;
            for airport in &airports {
                openairac_store::insert_airport_conn(conn, airport)?;
            }
            for runway in &runways {
                openairac_store::insert_runway_conn(conn, runway)?;
            }
            for assoc in &ils_associations {
                openairac_store::insert_ils_association_conn(conn, assoc)?;
            }
            Ok(())
        })?;

        Ok(report)
    }
}

/// Documented FAA CIFP download directory.
pub const FAA_CIFP_BASE_URL: &str = "https://aeronav.faa.gov/Upload_313-d/cifp";

/// Insert decoded CIFP entities into the store (connection-level,
/// shared by the direct adapter and the provider orchestration).
pub fn insert_cifp_entities_conn(
    conn: &rusqlite::Connection,
    waypoints: &[CanonicalWaypoint],
    navaids: &[CanonicalNavaid],
    airway_legs: &[CanonicalAirwayLeg],
    procedure_legs: &[CanonicalProcedureLeg],
) -> Result<()> {
    for wp in waypoints {
        openairac_store::insert_waypoint_conn(conn, wp)?;
    }
    for nav in navaids {
        openairac_store::insert_navaid_conn(conn, nav)?;
    }
    for leg in airway_legs {
        openairac_store::insert_airway_leg_conn(conn, leg)?;
    }
    for leg in procedure_legs {
        openairac_store::insert_procedure_leg_conn(conn, leg)?;
    }
    Ok(())
}

/// CIFP entity tables eligible for full-snapshot close semantics.
pub const CIFP_ENTITY_TABLES: [openairac_store::EntityTable; 6] = [
    openairac_store::EntityTable::Airports,
    openairac_store::EntityTable::Runways,
    openairac_store::EntityTable::Waypoints,
    openairac_store::EntityTable::Navaids,
    openairac_store::EntityTable::AirwayLegs,
    openairac_store::EntityTable::ProcedureLegs,
];

/// Which CIFP entity tables must SKIP full-snapshot close semantics for
/// this publication, because unsupported/undecodable records could mask a
/// removal (a parser failure must never silently become a source
/// deletion). Terminal airports (PA) and runways (PG) are known-inert:
/// they never map to CIFP entity tables.
pub fn masked_tables(scan: &CifpScanReport) -> BTreeSet<openairac_store::EntityTable> {
    use openairac_store::EntityTable;
    let mut masked = BTreeSet::new();
    if scan.decode_errors > 0 {
        masked.insert(EntityTable::Waypoints);
        masked.insert(EntityTable::Navaids);
        masked.insert(EntityTable::AirwayLegs);
        masked.insert(EntityTable::ProcedureLegs);
    }
    for &(section, subsection, kind) in &scan.unsupported_classes {
        match (section, subsection, kind) {
            // Terminal airports/runways: never entity rows in our tables.
            ('P', ' ', 'A') | ('P', ' ', 'G') => {}
            // Terminal NDBs map to navaids.
            ('P', 'N', _) => {
                masked.insert(EntityTable::Navaids);
            }
            // Terminal waypoints.
            ('P', ' ', 'C') => {
                masked.insert(EntityTable::Waypoints);
            }
            // Procedure legs.
            ('P', ' ', 'D' | 'E' | 'F') => {
                masked.insert(EntityTable::ProcedureLegs);
            }
            // Unknown P-blank kind: could be waypoint- or leg-shaped.
            ('P', ' ', _) => {
                masked.insert(EntityTable::Waypoints);
                masked.insert(EntityTable::ProcedureLegs);
            }
            ('P', _, _) => {
                masked.insert(EntityTable::Waypoints);
                masked.insert(EntityTable::ProcedureLegs);
            }
            // Enroute-section records could be waypoints or airway legs.
            ('E', _, _) => {
                masked.insert(EntityTable::Waypoints);
                masked.insert(EntityTable::AirwayLegs);
            }
            // Navaid sections.
            ('D', _, _) | ('B', _, _) => {
                masked.insert(EntityTable::Navaids);
            }
            // Holds, MSAs, airports, runways, unknown sections: never map
            // to CIFP entity tables.
            _ => {}
        }
    }
    masked
}

/// Keep the first occurrence of each object id; silently drop exact
/// payload repeats, count + diagnose conflicting repeats.
fn dedupe_entities<T: Clone + PartialEq>(
    items: Vec<T>,
    id_of: impl Fn(&T) -> &str,
    conflicts: &mut usize,
    reasons: &mut Vec<String>,
) -> Vec<T> {
    let mut seen: std::collections::HashMap<String, T> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let id = id_of(&item).to_string();
        match seen.get(&id) {
            None => {
                seen.insert(id, item.clone());
                out.push(item);
            }
            Some(prev) if prev == &item => {
                // exact duplicate record: skip
            }
            Some(_) => {
                *conflicts += 1;
                if reasons.len() < 1000 {
                    reasons.push(format!(
                        "duplicate record '{id}' with conflicting payload; first occurrence kept"
                    ));
                }
            }
        }
    }
    out
}

/// The FAA CIFP as a cycle-aware [`DataProvider`].
///
/// Fetch selector is provider-defined and produced by cycle discovery:
/// a file stem (`CIFP_260806`) or a full URL. The cycle ident is derived
/// from the stem; a selector that is not a recognizable CIFP stem keeps
/// the selector string as the cycle ident (fail-open only for
/// identification, never for data).
pub struct CifpProvider;

impl crate::provider::DataProvider for CifpProvider {
    fn name(&self) -> &'static str {
        "FAA_CIFP"
    }

    fn datasets(&self) -> &'static [&'static str] {
        &["FAACIFP18"]
    }

    fn fetch(
        &self,
        dataset: &str,
        cycle: Option<&crate::provider::CycleSelector>,
    ) -> Result<crate::provider::FetchedDataset> {
        if dataset != "FAACIFP18" {
            anyhow::bail!("unknown FAA CIFP dataset '{dataset}'");
        }
        let selector = cycle.ok_or_else(|| {
            anyhow::anyhow!("FAA CIFP is cycle-aware: fetch requires an explicit CycleSelector")
        })?;
        // Fail-closed: an unconfirmed cycle must never be fetched for
        // preload — its data would become effective at an unknown instant.
        let Some(effective_from) = selector.effective_from else {
            anyhow::bail!(
                "cycle '{}' has unconfirmed effective dates; confirm the \
                 effective date before fetching/preloading",
                selector.cycle_ident
            );
        };
        let uri = &selector.source_uri;
        let url = if uri.starts_with("http://") || uri.starts_with("https://") {
            uri.to_string()
        } else {
            format!("{FAA_CIFP_BASE_URL}/{uri}.zip")
        };
        let mut ds = crate::provider::fetch_url("FAA_CIFP", dataset, &url, Utc::now())?;
        ds.airac_cycle = Some(selector.cycle_ident.clone());
        ds.valid_from = Some(effective_from);
        Ok(ds)
    }

    fn parse_and_ingest(
        &self,
        dataset: &crate::provider::FetchedDataset,
        store: &mut WorldStore,
    ) -> Result<crate::provider::IngestReport> {
        use crate::provider::IngestReport;
        if dataset.dataset_name != "FAACIFP18" {
            anyhow::bail!("unknown FAA CIFP dataset '{}'", dataset.dataset_name);
        }
        // Cycle-aware ingest NEVER infers validity from the wall clock:
        // preloading must land on the confirmed effective_from, or fail.
        let Some(valid_from) = dataset.valid_from else {
            anyhow::bail!(
                "FAA CIFP ingest requires an explicit valid_from (the cycle's \
                 confirmed effective_from); never inferred from Utc::now()"
            );
        };
        let cycle = dataset
            .airac_cycle
            .clone()
            .ok_or_else(|| anyhow::anyhow!("FAA CIFP ingest requires an explicit airac_cycle"))?;
        let snapshot_id = SourceSnapshotId(format!(
            "faa_cifp:{}:{}",
            cycle,
            dataset.content_sha256.get(..16).unwrap_or("unknown")
        ));
        let snapshot = SourceSnapshot {
            id: snapshot_id.clone(),
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            provider_revision: dataset.provider_revision.clone(),
            airac_cycle: dataset.airac_cycle.clone(),
            effective_from: Some(valid_from),
            effective_until: None,
            retrieved_at: dataset.retrieved_at,
            source_uri: dataset.source_uri.clone(),
            content_sha256: dataset.content_sha256.clone(),
            license_id: Some("US-GOV".to_string()),
            license_notes: Some("US Government work (public domain)".to_string()),
            parser_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        // The catalog is the authority on the cycle: cross-check the
        // confirmed effective date (defense in depth behind the CLI).
        // Baseline publications land exactly on the cycle's effective
        // instant; corrections may re-publish at the same instant
        // (preload replacement) or at a later instant (post-effective
        // new revisions) — never earlier, never inferred.
        let cycle_id = CycleId(cycle.clone());
        let catalog = store.query_cycle(&cycle_id)?.ok_or_else(|| {
            anyhow::anyhow!(
                "cycle '{cycle}' is not in the catalog; run `openairac cycle discover` first"
            )
        })?;
        match catalog.effective_from {
            Some(e) => {
                let is_correction =
                    dataset.revision_kind == crate::provider::RevisionKind::Correction;
                if !is_correction && valid_from != e {
                    anyhow::bail!(
                        "catalog effective_from {e} does not match dataset valid_from {valid_from}"
                    );
                }
                if is_correction && valid_from < e {
                    anyhow::bail!(
                        "correction valid_from {valid_from} precedes the cycle's effective {e}"
                    );
                }
            }
            None => {
                anyhow::bail!("cycle '{cycle}' has UNCONFIRMED effective dates; cannot ingest");
            }
        }

        let (
            waypoints,
            navaids,
            airway_legs,
            procedure_legs,
            airports,
            runways,
            ils_associations,
            scan,
        ) = FaaCifpAdapter::parse_cifp_content(&dataset.raw_content, &snapshot_id, valid_from);

        // Full-snapshot removal semantics: build the per-table seen sets
        // from EVERY identified record (decoded entities). Unsupported
        // classes that could mask a removal are computed structurally.
        let masked = masked_tables(&scan);
        let mut seen: BTreeMap<openairac_store::EntityTable, Vec<String>> = BTreeMap::new();
        let mut push = |table: openairac_store::EntityTable, ids: Vec<String>| {
            seen.insert(table, ids);
        };
        push(
            openairac_store::EntityTable::Waypoints,
            waypoints.iter().map(|w| w.object_id.0.clone()).collect(),
        );
        push(
            openairac_store::EntityTable::Navaids,
            navaids.iter().map(|n| n.object_id.0.clone()).collect(),
        );
        push(
            openairac_store::EntityTable::AirwayLegs,
            airway_legs.iter().map(|l| l.object_id.0.clone()).collect(),
        );
        push(
            openairac_store::EntityTable::ProcedureLegs,
            procedure_legs
                .iter()
                .map(|l| l.object_id.0.clone())
                .collect(),
        );

        let start = std::time::Instant::now();
        let update_kind =
            openairac_model::UpdateKind::from_components(dataset.revision_kind, dataset.coverage);
        let publication_id = dataset.publication_id.clone().unwrap_or_else(|| {
            let tag = match update_kind {
                openairac_model::UpdateKind::FullSnapshot => "baseline",
                openairac_model::UpdateKind::Differential => "differential",
                openairac_model::UpdateKind::Correction { .. } => "correction",
            };
            format!("{}:{}:{tag}", dataset.dataset_name, cycle)
        });

        // Publication identity guard: replay is idempotent, conflicting
        // content under one identity fails loudly unless it is a
        // Correction (explicitly modeled replacement).
        let (kind, coverage) = update_kind.components();
        let version = openairac_model::DatasetVersion {
            id: 0,
            provider: "FAA_CIFP".to_string(),
            dataset: dataset.dataset_name.clone(),
            airac_cycle: Some(cycle.clone()),
            content_sha256: dataset.content_sha256.clone(),
            retrieved_at: dataset.retrieved_at,
            revision_kind: kind,
            coverage,
            publication_id: Some(publication_id.clone()),
            valid_from: Some(valid_from),
            notes: None,
        };
        let plan = openairac_store::PublicationPlan {
            namespace: "faa".to_string(),
            kind: update_kind,
            valid_from,
            payloads: openairac_store::EntityPayloads {
                airports: airports.clone(),
                runways: runways.clone(),
                navaids: navaids.clone(),
                waypoints: waypoints.clone(),
                airway_legs: airway_legs.clone(),
                procedure_legs: procedure_legs.clone(),
            },
            tombstones: Vec::new(),
            ils_associations: ils_associations.clone(),
            masked_tables: masked.clone(),
            publication_id: publication_id.clone(),
        };
        // ONE atomic transaction: snapshot, identity guard, entity
        // application, audit row and lifecycle bookkeeping commit
        // together or not at all.
        let applied = store.apply_dataset_publication(
            &snapshot,
            &version,
            &plan,
            Some(&openairac_store::PublicationLifecycle {
                cycle_id: cycle_id.clone(),
                snapshot_id: snapshot_id.clone(),
            }),
        )?;
        let closed_rows = applied.rows_closed;
        let duplicate = applied.duplicate;

        let mut report = IngestReport::new("FAA_CIFP", "FAACIFP18", &dataset.content_sha256);
        report.records_seen = scan.lines_seen;
        report.records_parsed = scan.records_decoded;
        report
            .kind_counts
            .insert("waypoints".into(), scan.waypoints_decoded);
        report
            .kind_counts
            .insert("navaids".into(), scan.navaids_decoded);
        report
            .kind_counts
            .insert("airway_legs".into(), scan.airway_legs_decoded);
        report
            .kind_counts
            .insert("procedure_legs".into(), scan.procedure_legs_decoded);
        report
            .kind_counts
            .insert("airports".into(), scan.airports_decoded);
        report
            .kind_counts
            .insert("runways".into(), scan.runways_decoded);
        report.records_rejected = scan.decode_errors;
        report.records_quarantined = scan.unsupported_records;
        report.warnings = scan.unsupported_reasons;
        // Rejections that mask close_absent semantics: decode failures
        // and unsupported classes mapping to entity tables.
        report.unidentifiable_rejections = scan.decode_errors
            + scan
                .unsupported_classes
                .iter()
                .filter(|c| !matches!(c, ('P', ' ', 'A') | ('P', ' ', 'G')))
                .count();
        if duplicate {
            report.warnings.push(format!(
                "publication {publication_id} is an exact replay; skipped"
            ));
        } else {
            let coverage = dataset.coverage;
            if update_kind.closes_absent() {
                for table in CIFP_ENTITY_TABLES {
                    if masked.contains(&table) {
                        report.warnings.push(format!(
                            "full-snapshot close skipped for {}: unsupported record classes could mask removals",
                            table.as_str()
                        ));
                    } else if seen.get(&table).map(|v| v.len()).unwrap_or(0) == 0
                        && closed_rows == 0
                    {
                        report.warnings.push(format!(
                            "full-snapshot close for {} had an empty seen set",
                            table.as_str()
                        ));
                    }
                }
            } else if coverage == crate::provider::Coverage::Partial {
                // Differential semantics: absence means nothing.
                report
                    .warnings
                    .push("differential publication: no full-snapshot close".to_string());
            }
            if closed_rows > 0 {
                report.warnings.push(format!(
                    "{closed_rows} entity row(s) closed as absent from the snapshot"
                ));
            }
        }
        report.duration_ms = start.elapsed().as_millis() as u64;
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
    const ER_A315_1: &str = "SUSAER       A315        0100ZBV  MYD 0V    O                         13540185     05000     60000                         557442605";
    const ER_A315_2: &str = "SUSAER       A315        0110SWIMMK7EA0E    O                         135404691355 08000     60000                         557452605";
    const ER_A315_3: &str = "SUSAER       A315        0120TINKYK EA0E    O                         135702181357 12500     60000                         557462411";
    const PA_AIRPORT: &str = "SUSAP 00AAK3A        0     034NSN38421448W101282608E005503435         1800018000P    MNAR    AERO B RANCH                  785012605";

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
                assert_eq!(icao_code, "K ");
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
                dme_latitude_deg,
                dme_longitude_deg,
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
                assert_eq!(dme_latitude_deg, Some(32.48133055555556));
                assert_eq!(dme_longitude_deg, Some(-99.86345277777777));
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

    // Real PI localizer record from FAA CIFP cycle 2608 (public
    // domain US Government work); decoded values cross-checked against
    // convert424toxplane v12.4 output for KABE (IABE localizer:
    // 40.661766667 / -75.426430556, 110.70 MHz, RW06).
    const PI_IABE: &str = "SUSAP KABEK6IIABE2   011070RW06 N40394236W0752535150633N40385895W0752652331010 12200466300W01205700385                     129732403";

    #[test]
    fn test_decode_pi_localizer() {
        match decode_line(PI_IABE).unwrap() {
            CifpRecord::IlsLocalizerRecord {
                airport_ident,
                icao_code,
                ident,
                frequency_khz,
                runway,
                latitude_deg,
                longitude_deg,
                magnetic_variation_deg,
                elevation_ft,
                glideslope_angle_deg,
                front_course_mag_deg,
                ..
            } => {
                assert_eq!(airport_ident, "KABE");
                assert_eq!(icao_code, "K6");
                assert_eq!(ident, "IABE");
                assert_eq!(frequency_khz, 110_700);
                assert_eq!(runway, "06");
                assert!((latitude_deg - 40.661766667).abs() < 5e-9);
                assert!((longitude_deg - (-75.426430556)).abs() < 5e-9);
                assert!((magnetic_variation_deg.unwrap() - (-12.0)).abs() < 1e-6);
                assert_eq!(elevation_ft, Some(385));
                assert_eq!(glideslope_angle_deg, Some(3.0));
                // Front course cols 52-55 in tenths: '0633' -> 63.3.
                assert!((front_course_mag_deg.unwrap() - 63.3).abs() < 1e-6);
            }
            other => panic!("expected PI localizer, got {other:?}"),
        }
    }

    #[test]
    fn test_interpret_pi_localizer() {
        let snapshot_id = SourceSnapshotId("snap-pi".to_string());
        let interpreted = interpret(&decode_line(PI_IABE).unwrap(), &snapshot_id, Utc::now());
        assert_eq!(interpreted.len(), 2);
        match &interpreted[0] {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::IlsLocalizer);
                assert_eq!(nav.object_id.0, "faa:SUSA:K6:IABE");
                assert_eq!(nav.associated_airport.as_deref(), Some("KABE"));
                assert_eq!(nav.associated_runway.as_deref(), Some("06"));
                assert_eq!(nav.frequency.0, 110_700);
                assert_eq!(nav.elevation_ft, Some(385));
                assert!((nav.magnetic_variation_deg.unwrap() - (-12.0)).abs() < 1e-6);
                assert!((nav.localizer_bearing_mag_deg.unwrap() - 63.3).abs() < 1e-6);
                assert!((nav.localizer_bearing_true_deg.unwrap() - 51.3).abs() < 1e-6);
            }
            other => panic!("expected localizer navaid, got {other:?}"),
        }
        match &interpreted[1] {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::IlsGlidepath);
                assert_eq!(nav.object_id.0, "faa:SUSA:K6:IABE:gs");
                assert_eq!(nav.glideslope_angle_deg, Some(3.0));
                assert!((nav.localizer_bearing_true_deg.unwrap() - 51.3).abs() < 1e-6);
            }
            other => panic!("expected glideslope navaid, got {other:?}"),
        }
    }

    // Real LOC-only PI record with no glideslope component (PADL DLG RW19)
    const PI_IDLG_LOC_ONLY: &str = "SCANP PADLPAIIDLG0   011190RW19 N59020727W1583052301955                   0607     0570   E0110                            069512409";

    #[test]
    fn test_loc_only_facility_emits_no_fabricated_gs() {
        let snapshot_id = SourceSnapshotId("snap-loc".to_string());
        let record = decode_line(PI_IDLG_LOC_ONLY).unwrap();
        match &record {
            CifpRecord::IlsLocalizerRecord {
                glideslope_latitude_deg,
                glideslope_longitude_deg,
                glideslope_angle_deg,
                front_course_mag_deg,
                ..
            } => {
                assert_eq!(*glideslope_latitude_deg, None);
                assert_eq!(*glideslope_longitude_deg, None);
                assert_eq!(*glideslope_angle_deg, None);
                assert_eq!(*front_course_mag_deg, Some(195.5));
            }
            other => panic!("expected PI record, got {other:?}"),
        }

        let interpreted = interpret(&record, &snapshot_id, Utc::now());
        assert_eq!(interpreted.len(), 1);
        match &interpreted[0] {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::IlsLocalizer);
                assert_eq!(nav.ident, "IDLG");
                assert_eq!(nav.associated_airport.as_deref(), Some("PADL"));
                assert_eq!(nav.associated_runway.as_deref(), Some("19"));
                assert_eq!(nav.localizer_bearing_mag_deg, Some(195.5));
                assert!((nav.localizer_bearing_true_deg.unwrap() - 206.5).abs() < 1e-6);
                assert_eq!(nav.glideslope_angle_deg, None);
            }
            other => panic!("expected localizer, got {other:?}"),
        }
    }

    // Real North-aligned localizer (KMCW RW36, IMCW: mag course 360.0, declination 0.0 -> true 360/0)
    const PI_IMCW_NORTH: &str = "SUSAP KMCWK3IIMCW0   010950RW36 N43102190W0931939793600N43085521W0932000511019 12380482300E000057001188                     178491807";

    #[test]
    fn test_north_aligned_localizer_preserves_bearing() {
        let snapshot_id = SourceSnapshotId("snap-north".to_string());
        let record = decode_line(PI_IMCW_NORTH).unwrap();
        let interpreted = interpret(&record, &snapshot_id, Utc::now());
        assert_eq!(interpreted.len(), 2);
        match &interpreted[0] {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::IlsLocalizer);
                assert_eq!(nav.ident, "IMCW");
                assert_eq!(nav.localizer_bearing_mag_deg, Some(360.0));
                assert_eq!(nav.localizer_bearing_true_deg, Some(0.0));
            }
            other => panic!("expected localizer, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_er_airway() {
        match decode_line(ER_A315_1).unwrap() {
            CifpRecord::Airway {
                route_ident,
                route_type,
                level,
                sequence_number,
                fix_ident,
                fix_icao_code,
                minimum_altitude_ft,
                maximum_altitude_ft,
                ..
            } => {
                assert_eq!(route_ident, "A315");
                assert_eq!(route_type, 'O');
                assert_eq!(level, None);
                assert_eq!(sequence_number, 100);
                assert_eq!(fix_ident, "ZBV");
                assert_eq!(fix_icao_code, "MY");
                assert_eq!(minimum_altitude_ft, Some(5000));
                assert_eq!(maximum_altitude_ft, Some(60000));
            }
            other => panic!("expected airway, got {other:?}"),
        }
    }

    #[test]
    fn test_unsupported_record_is_explicit() {
        // PA airports are decoded now (v0.5); an unknown P-blank kind
        // stays explicit and lossless.
        let mut pz_line: Vec<char> = PA_AIRPORT.chars().collect();
        pz_line[12] = 'Z';
        let pz: String = pz_line.iter().collect();
        match decode_line(&pz).unwrap() {
            CifpRecord::Unsupported {
                section,
                subsection,
                raw,
                ..
            } => {
                assert_eq!(section, 'P');
                assert_eq!(subsection, ' ');
                assert!(raw.starts_with("SUSAP"));
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
        // The unmodified PA record decodes as an airport.
        assert!(matches!(
            decode_line(PA_AIRPORT).unwrap(),
            CifpRecord::Airport { .. }
        ));
    }

    #[test]
    fn test_interpret_vortac_emits_paired_dme() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let rec = decode_line(D_ABI_VORTAC).unwrap();
        let out = interpret(&rec, &snapshot_id, Utc::now());
        assert_eq!(out.len(), 2);
        let vor = match &out[0] {
            CifpInterpretation::Navaid(n) => n.clone(),
            other => panic!("expected navaid, got {other:?}"),
        };
        assert_eq!(vor.kind, NavaidKind::Vortac);
        assert_eq!(vor.slaved_variation_deg, Some(10.0));
        assert_eq!(vor.service_volume_nm, Some(130)); // class 'VTH' col 30 = H (high)
        let dme = match &out[1] {
            CifpInterpretation::Navaid(n) => n.clone(),
            other => panic!("expected paired DME, got {other:?}"),
        };
        assert_eq!(dme.kind, NavaidKind::Dme);
        assert!(dme.dme_paired);
    }

    #[test]
    fn test_interpret_ils_emits_dme_ils() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let rec = decode_line(D_ISFO_ILS).unwrap();
        let out = interpret(&rec, &snapshot_id, Utc::now());
        assert_eq!(out.len(), 2);
        let dme = match &out[1] {
            CifpInterpretation::Navaid(n) => n.clone(),
            other => panic!("expected DME-ILS, got {other:?}"),
        };
        assert_eq!(dme.kind, NavaidKind::Dme);
        assert!(dme.dme_paired);
        assert_eq!(dme.associated_airport.as_deref(), Some("KSFO"));
    }

    #[test]
    fn test_ils_dme_service_volume_from_class_mapping() {
        // The ILS-DME row's class is ' IT' (column 30 = 'T'): the mapping
        // yields 25, verified against convert424toxplane's ILS-DME rows.
        // No fabricated fallback may appear.
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let rec = decode_line(D_ISFO_ILS).unwrap();
        let out = interpret(&rec, &snapshot_id, Utc::now());
        let dme = match &out[1] {
            CifpInterpretation::Navaid(n) => n.clone(),
            other => panic!("expected DME-ILS, got {other:?}"),
        };
        assert_eq!(dme.kind, NavaidKind::Dme);
        assert_eq!(dme.service_volume_nm, Some(25));
    }

    #[test]
    fn test_unmapped_class_yields_none_service_volume() {
        // A class code the mapping does not know must yield None (the
        // exporter will skip the row), never an invented value.
        assert_eq!(service_volume_from_cifp_class(NavaidKind::Vor, "VXZ"), None);
        assert_eq!(
            service_volume_from_cifp_class(NavaidKind::Dme, " IT"),
            Some(25)
        );
        assert_eq!(
            service_volume_from_cifp_class(NavaidKind::Vortac, "VTH"),
            Some(130)
        );
        assert_eq!(
            service_volume_from_cifp_class(NavaidKind::Ndb, "HOL"),
            Some(15)
        );
    }

    #[test]
    fn test_interpret_to_canonical() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let valid_from = Utc::now();

        let wp_rec = decode_line(EA_AAARG).unwrap();
        match &interpret(&wp_rec, &snapshot_id, valid_from)[0] {
            CifpInterpretation::Waypoint(wp) => {
                assert_eq!(wp.object_id.0, "faa:SUSA:K :AAARG");
                assert!(wp.is_enroute);
            }
            other => panic!("expected waypoint, got {other:?}"),
        }

        let vor_rec = decode_line(D_ABI_VORTAC).unwrap();
        match &interpret(&vor_rec, &snapshot_id, valid_from)[0] {
            CifpInterpretation::Navaid(nav) => {
                assert_eq!(nav.kind, NavaidKind::Vortac);
                assert_eq!(nav.object_id.0, "faa:SUSA:K4:ABI");
            }
            other => panic!("expected navaid, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_cifp_content_chains_airway_legs() {
        let content = format!("{ER_A315_1}\n{ER_A315_2}\n{ER_A315_3}\n{EA_AAARG}\n");
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let (waypoints, _navaids, legs, _procedures, _airports, _runways, _ils, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot_id, Utc::now());

        assert_eq!(waypoints.len(), 1);
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[0].start_fix, "ZBV");
        assert_eq!(legs[0].end_fix, "SWIMM");
        // MEA belongs to the segment FOLLOWING the fix record it is
        // published on: the ZBV record's 5000 applies to ZBV->SWIMM.
        assert_eq!(legs[0].minimum_altitude_ft, Some(5000));
        assert_eq!(legs[1].start_fix, "SWIMM");
        assert_eq!(legs[1].end_fix, "TINKY");
        assert_eq!(legs[1].minimum_altitude_ft, Some(8000));
        assert_eq!(report.airway_legs_decoded, 2);
    }

    // Real PI + D record pair from FAA CIFP cycle 2608 (public
    // domain US Government work). The PI record carries the
    // authoritative localizer position/elevation/course/runway; the
    // D record (class 'I') adds the DME component. The merge must
    // prefer PI fields on the shared localizer entity.
    const D_IAAD: &str = "SUSAD KBOIK1 IAAD  K1011015 ITW                    IAADN43341544W116142978E0130028380     NARBOISE AIR TRML/GOWEN FLD      260952014";
    const PI_IAAD: &str = "SUSAP KBOIK1IIAAD1   011015RW28RN43341700W1161425312822N43333262W1161226141011 11570363300E01305502861                     384402412";

    #[test]
    fn test_pi_merges_over_d_localizer() {
        let content = format!("{D_IAAD}\n{PI_IAAD}\n");
        let snapshot_id = SourceSnapshotId("snap-pi-merge".to_string());
        let (_wp, navaids, _legs, _proc, _ap, _rwy, _ils, _report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot_id, Utc::now());
        let loc: Vec<&CanonicalNavaid> = navaids
            .iter()
            .filter(|n| n.kind == NavaidKind::IlsLocalizer && n.ident == "IAAD")
            .collect();
        assert_eq!(loc.len(), 1, "exactly one localizer after merge");
        let loc = loc[0];
        // PI fields win.
        assert!((loc.latitude - 43.571388889).abs() < 5e-9);
        assert!((loc.longitude - (-116.240363889)).abs() < 5e-9);
        assert_eq!(loc.elevation_ft, Some(2861));
        assert!((loc.localizer_bearing_mag_deg.unwrap() - 282.2).abs() < 1e-6);
        assert_eq!(loc.associated_runway.as_deref(), Some("28R"));
        // The DME component from the D record remains.
        assert!(
            navaids
                .iter()
                .any(|n| n.kind == NavaidKind::Dme && n.ident == "IAAD")
        );
        // The glideslope component from the PI record remains.
        assert!(
            navaids
                .iter()
                .any(|n| n.kind == NavaidKind::IlsGlidepath && n.ident == "IAAD")
        );
    }

    #[test]
    fn test_pi_only_localizer_survives_merge() {
        let content = PI_IAAD.to_string();
        let snapshot_id = SourceSnapshotId("snap-pi-only".to_string());
        let (_wp, navaids, _legs, _proc, _ap, _rwy, _ils, _report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot_id, Utc::now());
        let loc: Vec<&CanonicalNavaid> = navaids
            .iter()
            .filter(|n| n.kind == NavaidKind::IlsLocalizer && n.ident == "IAAD")
            .collect();
        assert_eq!(loc.len(), 1, "PI-only localizer must not be dropped");
    }

    // Real KSFO procedure records from FAA CIFP cycle 2608
    // (public domain US Government work), decoded values cross-checked
    // against convert424toxplane's CIFP/KSFO.dat output.
    const PD_CIITY3_VA: &str = "SUSAP KSFOK2DCIITY34RW10L 010         0        VA                     1038        + 00520     18000                        140101509";
    const PD_GAPP7_FM: &str = "SUSAP KSFOK2DGAPP7 TRW01B 020SFO  K2D 0VE      FM SFO K2      000000003500    D                                            140391610";
    const PE_BDEGA4_TF: &str = "SUSAP KSFOK2EBDEGA44AMAKR 020QUINNK2PC0E       TF                                 B FL280FL240     280                     142422407";
    const PF_RF: &str = "SUSAP KABQK2FH03-Z ACMSTR 040GERNLK2PC0EE  L010RF       0027901345    03360049    + 06500                 CFPTK K2PCA FS   135801513";

    #[test]
    fn test_decode_sid_leg_va() {
        match decode_line(PD_CIITY3_VA).unwrap() {
            CifpRecord::ProcedureLeg {
                procedure_kind,
                airport_ident,
                procedure_ident,
                route_type,
                transition_ident,
                sequence_number,
                path_terminator,
                altitude_descriptor,
                altitude_1_ft,
                course_b_deg,
                ..
            } => {
                assert_eq!(procedure_kind, 'D');
                assert_eq!(airport_ident, "KSFO");
                assert_eq!(procedure_ident, "CIITY3");
                assert_eq!(route_type, "4");
                assert_eq!(transition_ident, "RW10L");
                assert_eq!(sequence_number, 10);
                assert_eq!(path_terminator, "VA");
                assert_eq!(course_b_deg, Some(103.8)); // VA heading lives in cols 71-74
                assert_eq!(altitude_descriptor, Some('+'));
                assert_eq!(altitude_1_ft, Some(520));
            }
            other => panic!("expected procedure leg, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_sid_leg_fm_recommended_navaid() {
        match decode_line(PD_GAPP7_FM).unwrap() {
            CifpRecord::ProcedureLeg {
                path_terminator,
                fix_ident,
                recommended_navaid,
                course_b_deg,
                ..
            } => {
                assert_eq!(path_terminator, "FM");
                assert_eq!(fix_ident, "SFO");
                assert_eq!(recommended_navaid.as_deref(), Some("SFO:K2:D:"));
                // 71-74 is the termination ALTITUDE for FM legs; the field
                // is stored positionally and interpreted per terminator.
                assert_eq!(course_b_deg, Some(350.0));
            }
            other => panic!("expected procedure leg, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_star_leg_with_altitudes_and_course() {
        match decode_line(PE_BDEGA4_TF).unwrap() {
            CifpRecord::ProcedureLeg {
                procedure_kind,
                procedure_ident,
                route_type,
                path_terminator,
                altitude_descriptor,
                altitude_1_ft,
                altitude_2_ft,
                course_c_deg,
                ..
            } => {
                assert_eq!(procedure_kind, 'E');
                assert_eq!(procedure_ident, "BDEGA4");
                assert_eq!(route_type, "4");
                assert_eq!(path_terminator, "TF");
                assert_eq!(altitude_descriptor, Some('B'));
                assert_eq!(altitude_1_ft, Some(28_000));
                assert_eq!(altitude_2_ft, Some(24_000));
                assert_eq!(course_c_deg, Some(280));
            }
            other => panic!("expected procedure leg, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_rf_leg() {
        match decode_line(PF_RF).unwrap() {
            CifpRecord::ProcedureLeg {
                procedure_kind,
                procedure_ident,
                path_terminator,
                turn_direction,
                rnp_nm,
                arc_radius_nm,
                msa_center_fix,
                ..
            } => {
                assert_eq!(procedure_kind, 'F');
                assert_eq!(procedure_ident, "H03-Z");
                assert_eq!(path_terminator, "RF");
                assert_eq!(turn_direction, Some('L'));
                assert_eq!(rnp_nm, Some(0.10));
                assert_eq!(arc_radius_nm, Some(27.90));
                assert_eq!(msa_center_fix.as_deref(), Some("CFPTK:K2:P:C"));
            }
            other => panic!("expected procedure leg, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_terminal_waypoint_pc() {
        // Real KSFO terminal waypoint record (cycle 2608).
        const PC_ADDMM: &str = "SUSAP KSFOK2CADDMM K20    W     N37461570W121423091                       E0128     NAR           ADDMM                    139032605";
        match decode_line(PC_ADDMM).unwrap() {
            CifpRecord::Waypoint {
                airport_ident,
                ident,
                icao_code,
                latitude_deg,
                longitude_deg,
                ..
            } => {
                assert_eq!(airport_ident, "KSFO");
                assert_eq!(ident, "ADDMM");
                assert_eq!(icao_code, "K2");
                assert!((latitude_deg - 37.77102778).abs() < 5e-6);
                assert!((longitude_deg - (-121.70858611)).abs() < 5e-6);
            }
            other => panic!("expected terminal waypoint, got {other:?}"),
        }
    }

    #[test]
    fn test_waypoint_region_preserves_blank_icao() {
        let snapshot_id = SourceSnapshotId("snap-faa".to_string());
        let rec = decode_line(EA_AAARG).unwrap();
        let out = interpret(&rec, &snapshot_id, Utc::now());
        match &out[0] {
            CifpInterpretation::Waypoint(wp) => {
                assert_eq!(wp.region_code, "K ");
            }
            other => panic!("expected waypoint, got {other:?}"),
        }
    }

    #[test]
    fn test_cifp_fetch_rejects_unconfirmed_cycle() {
        // No network is touched: the unconfirmed selector is rejected
        // before any request is built.
        let selector = crate::provider::CycleSelector {
            cycle_ident: "2608".to_string(),
            source_uri: "CIFP_260806".to_string(),
            effective_from: None,
        };
        let provider = CifpProvider;
        let err = crate::provider::DataProvider::fetch(&provider, "FAACIFP18", Some(&selector))
            .unwrap_err();
        assert!(err.to_string().contains("unconfirmed"), "{err}");

        // Missing selector entirely.
        let err = crate::provider::DataProvider::fetch(&provider, "FAACIFP18", None).unwrap_err();
        assert!(err.to_string().contains("CycleSelector"), "{err}");

        // Unknown dataset.
        let confirmed = crate::provider::CycleSelector {
            cycle_ident: "2608".to_string(),
            source_uri: "CIFP_260806".to_string(),
            effective_from: Some(Utc::now()),
        };
        let err =
            crate::provider::DataProvider::fetch(&provider, "nope", Some(&confirmed)).unwrap_err();
        assert!(
            err.to_string().contains("unknown FAA CIFP dataset"),
            "{err}"
        );
    }

    #[test]
    fn test_cifp_ingest_never_infers_valid_from() {
        let mut store = WorldStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        let content = EA_AAARG.to_string();
        let dataset = crate::provider::FetchedDataset {
            provider_name: "FAA_CIFP".to_string(),
            dataset_name: "FAACIFP18".to_string(),
            source_uri: "fixture".to_string(),
            content_sha256: crate::provider::sha256_hex(content.as_bytes()),
            retrieved_at: Utc::now(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            revision_kind: crate::provider::RevisionKind::Baseline,
            coverage: crate::provider::Coverage::FullSnapshot,
            valid_from: None, // <- must be rejected, never inferred
            publication_id: None,
            raw_content: content,
        };
        let provider = CifpProvider;
        let err = crate::provider::DataProvider::parse_and_ingest(&provider, &dataset, &mut store)
            .unwrap_err();
        assert!(err.to_string().contains("valid_from"), "{err}");

        // Missing cycle ident is rejected too.
        let mut ds2 = dataset.clone();
        ds2.valid_from = Some(Utc::now());
        ds2.airac_cycle = None;
        let err = crate::provider::DataProvider::parse_and_ingest(&provider, &ds2, &mut store)
            .unwrap_err();
        assert!(err.to_string().contains("airac_cycle"), "{err}");
    }

    #[test]
    fn test_cifp_provider_parity_with_adapter() {
        // The provider path must decode exactly what the direct adapter
        // path decodes from the same real cycle 2608 content.
        let content =
            format!("{EA_AAARG}\n{D_ABI_VORTAC}\n{ER_A315_1}\n{ER_A315_2}\n{ER_A315_3}\n");
        let vf = Utc::now();

        let snapshot = SourceSnapshot {
            id: SourceSnapshotId("snap-direct".to_string()),
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            effective_from: Some(vf),
            effective_until: None,
            retrieved_at: vf,
            source_uri: "fixture".to_string(),
            content_sha256: crate::provider::sha256_hex(content.as_bytes()),
            license_id: Some("US-GOV".to_string()),
            license_notes: Some("US Government work (public domain)".to_string()),
            parser_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        let mut direct_store = WorldStore::open_in_memory().unwrap();
        direct_store.migrate().unwrap();
        direct_store.insert_source_snapshot(&snapshot).unwrap();
        let scan =
            FaaCifpAdapter::ingest_cifp(&content, &snapshot.id, vf, &mut direct_store).unwrap();

        let dataset = crate::provider::FetchedDataset {
            provider_name: "FAA_CIFP".to_string(),
            dataset_name: "FAACIFP18".to_string(),
            source_uri: "fixture".to_string(),
            content_sha256: crate::provider::sha256_hex(content.as_bytes()),
            retrieved_at: vf,
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            revision_kind: crate::provider::RevisionKind::Baseline,
            coverage: crate::provider::Coverage::FullSnapshot,
            valid_from: Some(vf),
            publication_id: None,
            raw_content: content.clone(),
        };
        let mut provider_store = WorldStore::open_in_memory().unwrap();
        provider_store.migrate().unwrap();
        // The provider path requires the cycle in the catalog with a
        // confirmed effective date matching the dataset valid_from.
        provider_store
            .insert_cycle(&AiracCycle {
                id: CycleId("2608".to_string()),
                effective_from: Some(vf),
                effective_until: None,
                status: CycleStatus::Discovered,
                source_uri: Some("fixture".to_string()),
                created_at: vf,
                updated_at: vf,
                notes: None,
            })
            .unwrap();
        let provider = CifpProvider;
        let report = crate::provider::DataProvider::parse_and_ingest(
            &provider,
            &dataset,
            &mut provider_store,
        )
        .unwrap();

        // Bookkeeping: snapshot linked, Scheduled recorded, Preloaded.
        assert_eq!(
            provider_store
                .cycle_snapshot_ids(&CycleId("2608".to_string()))
                .unwrap()
                .len(),
            1
        );
        assert!(
            provider_store
                .query_cycle_events()
                .unwrap()
                .iter()
                .any(|e| e.kind == CycleEventKind::Scheduled)
        );
        assert_eq!(
            provider_store
                .query_cycle(&CycleId("2608".to_string()))
                .unwrap()
                .unwrap()
                .status,
            CycleStatus::Preloaded
        );

        assert_eq!(
            report.kind_counts.get("waypoints").copied(),
            Some(scan.waypoints_decoded)
        );
        assert_eq!(
            report.kind_counts.get("navaids").copied(),
            Some(scan.navaids_decoded)
        );
        assert_eq!(
            report.kind_counts.get("airway_legs").copied(),
            Some(scan.airway_legs_decoded)
        );
        assert_eq!(report.records_seen, scan.lines_seen);
        assert_eq!(report.records_parsed, scan.records_decoded);

        // Entity counts in both stores are identical.
        for store in [&direct_store, &provider_store] {
            assert_eq!(
                store.query_waypoints_at(vf).unwrap().len(),
                scan.waypoints_decoded
            );
            assert_eq!(
                store.query_navaids_at(vf).unwrap().len(),
                scan.navaids_decoded
            );
            assert_eq!(
                store.query_airway_legs_at(vf).unwrap().len(),
                scan.airway_legs_decoded
            );
        }
    }

    fn catalog_cycle(id: &str, eff: DateTime<Utc>, status: CycleStatus) -> AiracCycle {
        AiracCycle {
            id: CycleId(id.to_string()),
            effective_from: Some(eff),
            effective_until: None,
            status,
            source_uri: Some("fixture".to_string()),
            created_at: eff,
            updated_at: eff,
            notes: None,
        }
    }

    fn cifp_dataset(
        content: &str,
        cycle: &str,
        vf: DateTime<Utc>,
    ) -> crate::provider::FetchedDataset {
        crate::provider::FetchedDataset {
            provider_name: "FAA_CIFP".to_string(),
            dataset_name: "FAACIFP18".to_string(),
            source_uri: "fixture".to_string(),
            content_sha256: crate::provider::sha256_hex(content.as_bytes()),
            retrieved_at: vf,
            provider_revision: Some(cycle.to_string()),
            airac_cycle: Some(cycle.to_string()),
            revision_kind: crate::provider::RevisionKind::Baseline,
            coverage: crate::provider::Coverage::FullSnapshot,
            valid_from: Some(vf),
            publication_id: None,
            raw_content: content.to_string(),
        }
    }

    #[test]
    fn test_cifp_ingest_full_snapshot_closes_absent_waypoint() {
        let t0 = Utc::now();
        let eff = t0 + chrono::TimeDelta::seconds(3600);
        let mut store = WorldStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .insert_cycle(&catalog_cycle("2608", eff, CycleStatus::Discovered))
            .unwrap();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-old".to_string()),
                provider: "FAA_CIFP".to_string(),
                dataset: "FAACIFP18".to_string(),
                provider_revision: Some("2607".to_string()),
                airac_cycle: Some("2607".to_string()),
                effective_from: Some(t0),
                effective_until: None,
                retrieved_at: t0,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();

        // A waypoint from the previous cycle that vanishes in 2608.
        let old_wp = CanonicalWaypoint {
            object_id: WaypointId("faa:SUSA:K :OLDWP".to_string()),
            ident: "OLDWP".to_string(),
            name: "OLDWP".to_string(),
            latitude: 30.0,
            longitude: -80.0,
            is_enroute: true,
            region_code: "K ".to_string(),
            terminal_area_ident: None,
            waypoint_type: Some(0x202057),
            temporal: TemporalValidity {
                valid_from: t0,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-old".to_string()),
            },
        };
        store
            .transact(|conn| openairac_store::insert_waypoint_conn(conn, &old_wp))
            .unwrap();

        let provider = CifpProvider;
        let report = crate::provider::DataProvider::parse_and_ingest(
            &provider,
            &cifp_dataset(EA_AAARG, "2608", eff),
            &mut store,
        )
        .unwrap();

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("closed as absent")),
            "{:?}",
            report.warnings
        );
        // OLDWP closed at eff; AAARG present.
        let at_eff = store.query_waypoints_at(eff).unwrap();
        assert!(at_eff.iter().any(|w| w.ident == "AAARG"));
        assert!(!at_eff.iter().any(|w| w.ident == "OLDWP"));
        // History intact before eff.
        let before = store
            .query_waypoints_at(openairac_store::just_before(eff))
            .unwrap();
        assert!(before.iter().any(|w| w.ident == "OLDWP"));
    }

    #[test]
    fn test_cifp_ingest_masked_skip_keeps_entities() {
        let t0 = Utc::now();
        let eff = t0 + chrono::TimeDelta::seconds(3600);
        let mut store = WorldStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .insert_cycle(&catalog_cycle("2608", eff, CycleStatus::Discovered))
            .unwrap();
        store
            .insert_source_snapshot(&SourceSnapshot {
                id: SourceSnapshotId("snap-old".to_string()),
                provider: "FAA_CIFP".to_string(),
                dataset: "FAACIFP18".to_string(),
                provider_revision: Some("2607".to_string()),
                airac_cycle: Some("2607".to_string()),
                effective_from: Some(t0),
                effective_until: None,
                retrieved_at: t0,
                source_uri: "fixture".to_string(),
                content_sha256: "0".repeat(64),
                license_id: None,
                license_notes: None,
                parser_version: "test".to_string(),
            })
            .unwrap();

        let old_wp = CanonicalWaypoint {
            object_id: WaypointId("faa:SUSA:K :OLDWP".to_string()),
            ident: "OLDWP".to_string(),
            name: "OLDWP".to_string(),
            latitude: 30.0,
            longitude: -80.0,
            is_enroute: true,
            region_code: "K ".to_string(),
            terminal_area_ident: None,
            waypoint_type: Some(0x202057),
            temporal: TemporalValidity {
                valid_from: t0,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-old".to_string()),
            },
        };
        store
            .transact(|conn| openairac_store::insert_waypoint_conn(conn, &old_wp))
            .unwrap();

        // A polymorphic P-blank record with unknown kind 'Z': unsupported
        // and waypoint/leg-shaped -> masks close_absent for those tables.
        let mut pz_line: Vec<char> = PA_AIRPORT.chars().collect();
        pz_line[12] = 'Z';
        let content = format!("{EA_AAARG}\n{}\n", pz_line.iter().collect::<String>());

        let provider = CifpProvider;
        let report = crate::provider::DataProvider::parse_and_ingest(
            &provider,
            &cifp_dataset(&content, "2608", eff),
            &mut store,
        )
        .unwrap();

        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("close skipped") && w.contains("waypoints")),
            "{:?}",
            report.warnings
        );
        assert!(report.unidentifiable_rejections >= 1);
        // Fail-closed: the old waypoint must NOT have been deleted by a
        // publication whose parsing could not account for every record.
        let at_eff = store.query_waypoints_at(eff).unwrap();
        assert!(at_eff.iter().any(|w| w.ident == "OLDWP"));
    }

    #[test]
    fn test_cifp_ingest_rejects_catalog_mismatch() {
        let t0 = Utc::now();
        let eff = t0 + chrono::TimeDelta::seconds(3600);
        let mut store = WorldStore::open_in_memory().unwrap();
        store.migrate().unwrap();
        store
            .insert_cycle(&catalog_cycle("2608", eff, CycleStatus::Discovered))
            .unwrap();

        let provider = CifpProvider;
        // valid_from != catalog effective_from.
        let err = crate::provider::DataProvider::parse_and_ingest(
            &provider,
            &cifp_dataset(EA_AAARG, "2608", eff + chrono::TimeDelta::seconds(1)),
            &mut store,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"), "{err}");

        // Unconfirmed catalog cycle.
        let mut store2 = WorldStore::open_in_memory().unwrap();
        store2.migrate().unwrap();
        store2
            .insert_cycle(&AiracCycle {
                id: CycleId("2608".to_string()),
                effective_from: None,
                effective_until: None,
                status: CycleStatus::Discovered,
                source_uri: None,
                created_at: t0,
                updated_at: t0,
                notes: None,
            })
            .unwrap();
        let err = crate::provider::DataProvider::parse_and_ingest(
            &provider,
            &cifp_dataset(EA_AAARG, "2608", eff),
            &mut store2,
        )
        .unwrap_err();
        assert!(err.to_string().contains("UNCONFIRMED"), "{err}");
    }

    // Real PA/PG fixtures, transcribed verbatim from FAA CIFP 2608.

    #[test]
    fn test_decode_pa_airport_ksfo() {
        let line = "SUSAP KSFOK2ASFO     0     118YHN37370770W122223150E014000013         1800018000C    MNAR    SAN FRANCISCO INTL            139021513";
        let record = decode_line(line).unwrap();
        match &record {
            CifpRecord::Airport {
                airport_ident,
                icao_code,
                name,
                latitude_deg,
                longitude_deg,
                elevation_ft,
            } => {
                assert_eq!(airport_ident, "KSFO");
                assert_eq!(icao_code, "K2");
                assert_eq!(name, "SAN FRANCISCO INTL");
                // Coordinates are DDDMMSS.ss: N37370770 = 37 deg
                // 37' 07.70" = 37.618806 (the real KSFO ARP latitude).
                assert!(
                    (*latitude_deg - 37.618806).abs() < 1e-5,
                    "latitude {latitude_deg}"
                );
                assert!(
                    (*longitude_deg - (-122.375417)).abs() < 1e-5,
                    "longitude {longitude_deg}"
                );
                assert_eq!(*elevation_ft, Some(13.0));
            }
            other => panic!("expected Airport, got {other:?}"),
        }

        // Interpretation carries the canonical airport.
        let snapshot = SourceSnapshotId("snap-t".to_string());
        let mut interpretations = interpret(&record, &snapshot, Utc::now());
        assert_eq!(interpretations.len(), 1);
        match interpretations.pop().unwrap() {
            CifpInterpretation::Airport(airport) => {
                assert_eq!(airport.id.0, "faa:K2:KSFO");
                assert_eq!(airport.ident, "KSFO");
                assert_eq!(airport.elevation_ft, Some(13.0));
            }
            other => panic!("expected Airport, got {other:?}"),
        }
    }

    #[test]
    fn test_decode_pg_runway_pair_ksfo() {
        // KSFO 10L and 28R ends: length 11870 ft, thresholds
        // 37.374346/-122.233621 (10L) and the 28R end.
        let content = [
            "SUSAP KSFOK2GRW10L   0118701040 N37374346W122233621         -0030900006000055200R                                          146341707",
            "SUSAP KSFOK2GRW28R   0118701040 N37359023W122231356         -0030900006000055200L                                          146351707",
        ]
        .join("\n");
        let snapshot = SourceSnapshotId("snap-t".to_string());
        let (_wp, _nav, _legs, _proc, _airports, runways, _ils, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot, Utc::now());
        assert_eq!(report.runway_ends_decoded, 2);
        assert_eq!(report.runways_decoded, 1);
        assert_eq!(report.unpaired_runway_ends, 0);
        assert_eq!(runways.len(), 1);
        let runway = &runways[0];
        assert_eq!(runway.id.0, "faa:K2:KSFO:10L-28R");
        assert_eq!(runway.airport_id.0, "faa:K2:KSFO");
        assert_eq!(runway.official_designator, "10L");
        assert_eq!(runway.length_ft, 11870);
        assert_eq!(runway.width_ft, None); // never fabricated
        assert_eq!(runway.le_ident, "10L");
        assert!((runway.le_lat - 37.62873889).abs() < 1e-5);
        assert!((runway.le_lon - (-122.39339167)).abs() < 1e-5);
        assert_eq!(runway.he_ident, "28R");
        assert!(runway.true_heading_deg.is_none());
    }

    #[test]
    fn test_decode_pg_runway_pair_kden_16r() {
        let content = [
            "SUSAP KDENK2GRW16R   0160001730 N39534487W104414590         -00604005322000055200                                         155751707 ",
            "SUSAP KDENK2GRW34L   0160001730 N39515230W104404896         -00604005330000055200                                         155761707 ",
        ]
        .join("\n");
        let snapshot = SourceSnapshotId("snap-t".to_string());
        let (_wp, _nav, _legs, _proc, _airports, runways, _ils, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot, Utc::now());
        assert_eq!(report.runways_decoded, 1);
        assert_eq!(report.unpaired_runway_ends, 0);
        assert_eq!(runways[0].length_ft, 16000);
        assert_eq!(runways[0].id.0, "faa:K2:KDEN:16R-34L");
    }

    #[test]
    fn test_pg_unpaired_end_skipped() {
        // Only one end of 10L/28R: fail closed, no half-runway.
        let content = "SUSAP KSFOK2GRW10L   0118701040 N37374346W122233621         -0030900006000055200R                                          146341707\n";
        let snapshot = SourceSnapshotId("snap-t".to_string());
        let (_wp, _nav, _legs, _proc, _airports, runways, _ils, report) =
            FaaCifpAdapter::parse_cifp_content(content, &snapshot, Utc::now());
        assert_eq!(report.runways_decoded, 0);
        assert_eq!(report.unpaired_runway_ends, 1);
        assert!(runways.is_empty());
    }

    #[test]
    fn test_ils_association_ksfo_i28l() {
        // Real cycle 2608 KSFO I28L main body: final RW28L CF leg with
        // localizer ISFO, bearing 284.0, glideslope 2.85.
        let content = [
            "SUSAP KSFOK2FI28L  I      010HEMANK2PC0E  I    IF ISFOK2      10380120        PI  J 031000180018000                 0 DS   144721310",
            "SUSAP KSFOK2FI28L  I      020DUYETK2PC0E  F    CF ISFOK2      1038007728400043PI  H 0180001800            SFO   K2D 0 DS   144732107",
            "SUSAP KSFOK2FI28L  I      030RW28LK2PG0GY M    CF ISFOK2      1038001928400057PI    00065             -285          0 DS   144741503",
        ]
        .join("\n");
        let snapshot = SourceSnapshotId("snap-t".to_string());
        let (_wp, navaids, _legs, proc_legs, _ap, _rwy, assocs, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot, Utc::now());
        assert_eq!(report.procedure_legs_decoded, 3);
        assert_eq!(assocs.len(), 1);
        let a = &assocs[0];
        assert_eq!(a.airport_ident, "KSFO");
        assert_eq!(a.approach_ident, "I28L");
        assert_eq!(a.runway_end, "28L");
        assert_eq!(a.localizer_ident, "ISFO");
        assert_eq!(a.localizer_region, "K2");
        assert!((a.localizer_bearing_mag_deg - 284.0).abs() < 0.05);
        assert!((a.glideslope_angle_deg - 2.85).abs() < 0.01);
        // The canonical localizer navaid is enriched: the fixture has
        // no D-record, so nothing to check beyond the association.
        let _ = navaids;
        let _ = proc_legs;
    }
}
