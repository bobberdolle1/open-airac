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
//! canonical entities (CanonicalWaypoint / CanonicalNavaid / CanonicalAirwayLeg)
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
                dme_latitude_deg: dme_pos.map(|p| p.0),
                dme_longitude_deg: dme_pos.map(|p| p.1),
                magnetic_variation_deg: parse_mag_variation(cifp.field(75, 80)),
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
                        .filter(|v| *v > 0.0)
                        .map(|v| v / 100.0),
                    msa_center_fix: msa_center,
                    route_qualifiers: cifp.field(119, 120).to_string(),
                    raw: line.to_string(),
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
                unsupported("airport/runway record (PA/PG)")
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
/// Transient decode artifact; the size spread is acceptable.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum CifpInterpretation {
    Waypoint(CanonicalWaypoint),
    Navaid(CanonicalNavaid),
    AirwayLeg(CanonicalAirwayLeg),
    ProcedureLeg(CanonicalProcedureLeg),
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
                Some(b'U') => Some(if kind == NavaidKind::Dme { 125 } else { 150 }),
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
        CifpRecord::Unsupported { reason, raw, .. } => vec![CifpInterpretation::Unsupported {
            reason: reason.to_string(),
            raw: raw.clone(),
        }],
    }
}

fn record_to_raw(record: &CifpRecord) -> String {
    match record {
        CifpRecord::Waypoint { raw, .. }
        | CifpRecord::VhfNavaid { raw, .. }
        | CifpRecord::NdbNavaid { raw, .. }
        | CifpRecord::Airway { raw, .. }
        | CifpRecord::ProcedureLeg { raw, .. }
        | CifpRecord::Unsupported { raw, .. } => raw.clone(),
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
    pub unsupported_records: usize,
    pub decode_errors: usize,
    pub unsupported_reasons: Vec<String>,
}

pub struct FaaCifpAdapter;

impl FaaCifpAdapter {
    /// Decode and interpret a complete CIFP content string.
    ///
    /// Returns the accepted canonical entities plus a deterministic scan
    /// report. ER airway records are chained into segments: each consecutive
    /// record of a route becomes a segment from the previous fix to this fix.
    pub fn parse_cifp_content(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
    ) -> (
        Vec<CanonicalWaypoint>,
        Vec<CanonicalNavaid>,
        Vec<CanonicalAirwayLeg>,
        Vec<CanonicalProcedureLeg>,
        CifpScanReport,
    ) {
        let mut waypoints = Vec::new();
        let mut navaids = Vec::new();
        let mut airway_legs = Vec::new();
        let mut procedure_legs = Vec::new();
        let mut report = CifpScanReport::default();

        // Per-route previous record for ER chaining.
        #[derive(Clone)]
        struct Prev {
            fix_ident: String,
            fix_icao_code: String,
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
                            minimum_altitude_ft,
                            maximum_altitude_ft,
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
                            CifpInterpretation::Unsupported { reason, raw } => {
                                report.unsupported_records += 1;
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

        (waypoints, navaids, airway_legs, procedure_legs, report)
    }

    /// Parse CIFP content and ingest the canonical entities into the store
    /// transactionally. Returns scan statistics.
    pub fn ingest_cifp(
        content: &str,
        snapshot_id: &SourceSnapshotId,
        valid_from: DateTime<Utc>,
        store: &mut WorldStore,
    ) -> Result<CifpScanReport> {
        let (waypoints, navaids, airway_legs, procedure_legs, report) =
            Self::parse_cifp_content(content, snapshot_id, valid_from);

        store.transact(|conn| {
            for wp in &waypoints {
                openairac_store::insert_waypoint_conn(conn, wp)?;
            }
            for nav in &navaids {
                openairac_store::insert_navaid_conn(conn, nav)?;
            }
            for leg in &airway_legs {
                openairac_store::insert_airway_leg_conn(conn, leg)?;
            }
            for leg in &procedure_legs {
                openairac_store::insert_procedure_leg_conn(conn, leg)?;
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
        match decode_line(PA_AIRPORT).unwrap() {
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
        let (waypoints, _navaids, legs, _procedures, report) =
            FaaCifpAdapter::parse_cifp_content(&content, &snapshot_id, Utc::now());

        assert_eq!(waypoints.len(), 1);
        assert_eq!(legs.len(), 2);
        assert_eq!(legs[0].start_fix, "ZBV");
        assert_eq!(legs[0].end_fix, "SWIMM");
        assert_eq!(legs[0].minimum_altitude_ft, Some(8000));
        assert_eq!(legs[1].start_fix, "SWIMM");
        assert_eq!(legs[1].end_fix, "TINKY");
        assert_eq!(report.airway_legs_decoded, 2);
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
}
