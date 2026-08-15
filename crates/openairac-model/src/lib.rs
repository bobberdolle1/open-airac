use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Strongly typed ID for domain entities
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AirportId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunwayId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NavaidId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WaypointId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AirwayLegId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSnapshotId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorldRevisionId(pub String);

/// Frequency wrapper with explicit units (kHz)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrequencyKhz(pub u32);

impl FrequencyKhz {
    pub fn from_mhz(mhz: f64) -> Self {
        Self((mhz * 1000.0).round() as u32)
    }

    pub fn to_mhz(&self) -> f64 {
        self.0 as f64 / 1000.0
    }
}

/// Source Provenance tracking data origins & licenses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub id: SourceSnapshotId,
    pub provider: String,
    pub dataset: String,
    pub provider_revision: Option<String>,
    pub airac_cycle: Option<String>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
    pub retrieved_at: DateTime<Utc>,
    pub source_uri: String,
    pub content_sha256: String,
    pub license_id: Option<String>,
    pub license_notes: Option<String>,
    pub parser_version: String,
}

/// World Revision snapshot metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldRevision {
    pub id: WorldRevisionId,
    pub created_at: DateTime<Utc>,
    pub source_snapshot_id: SourceSnapshotId,
    pub schema_version: String,
    pub notes: Option<String>,
}

/// Temporal Validity Envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalValidity {
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub source_snapshot_id: SourceSnapshotId,
}

/// Canonical Waypoint (Fix)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalWaypoint {
    pub object_id: WaypointId,
    pub ident: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub is_enroute: bool,
    pub region_code: String,
    /// Airport the waypoint belongs to (terminal waypoints only, ARINC 5.6).
    pub terminal_area_ident: Option<String>,
    /// ARINC 424-18 field 5.42 (3 columns, cols 27-29) encoded as a
    /// little-endian u32 with the 4th byte 0 (X-Plane FIX1200 waypoint type).
    pub waypoint_type: Option<u32>,
    pub temporal: TemporalValidity,
}

/// Canonical Navaid (VOR, NDB, DME, ILS)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavaidKind {
    Vor,
    Vordme,
    Vortac,
    Ndb,
    Dme,
    Tacan,
    IlsLocalizer,
    IlsGlidepath,
}

impl NavaidKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NavaidKind::Vor => "VOR",
            NavaidKind::Vordme => "VOR-DME",
            NavaidKind::Vortac => "VORTAC",
            NavaidKind::Ndb => "NDB",
            NavaidKind::Dme => "DME",
            NavaidKind::Tacan => "TACAN",
            NavaidKind::IlsLocalizer => "ILS-LOC",
            NavaidKind::IlsGlidepath => "ILS-GS",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "VOR" => Some(NavaidKind::Vor),
            "VOR-DME" | "VORDME" => Some(NavaidKind::Vordme),
            "VORTAC" => Some(NavaidKind::Vortac),
            "NDB" => Some(NavaidKind::Ndb),
            "DME" => Some(NavaidKind::Dme),
            "TACAN" => Some(NavaidKind::Tacan),
            "ILS-LOC" | "LOC" | "ILS" => Some(NavaidKind::IlsLocalizer),
            "ILS-GS" | "GS" => Some(NavaidKind::IlsGlidepath),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalNavaid {
    pub object_id: NavaidId,
    pub ident: String,
    pub name: String,
    pub kind: NavaidKind,
    pub frequency: FrequencyKhz,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<i32>,
    pub region_code: Option<String>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
    /// Direction of the 0 radial in true degrees (the XPNAV1200 "slaved
    /// variation"), only when the source published it. Exporters MUST NOT
    /// substitute station declination for this value.
    pub slaved_variation_deg: Option<f64>,
    /// Service volume / class in nautical miles (XPNAV1200 class field:
    /// VOR 25/40/130/125, NDB 15/25/50/75, DME 25/40/70/120/125/130/150).
    /// Mapped at ingest from provider class data; `None` = source silent.
    pub service_volume_nm: Option<u16>,
    /// True when this DME row is the paired component of a VOR/ILS/TACAN
    /// (XPNAV1200 row 12, chart frequency suppressed). Standalone DMEs and
    /// NDB-DME components use row 13.
    pub dme_paired: bool,
    /// ILS-specific data. `None` when the kind is not an ILS component or the
    /// source did not provide the value; exporters MUST NOT fabricate it.
    pub associated_runway: Option<String>,
    pub localizer_bearing_true_deg: Option<f64>,
    pub localizer_bearing_mag_deg: Option<f64>,
    pub glideslope_angle_deg: Option<f64>,
    pub temporal: TemporalValidity,
}

/// Canonical Airport & Runway with Dual Designators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRunway {
    pub id: RunwayId,
    pub airport_id: AirportId,
    pub airport_ident: String,
    pub official_designator: String,                  // e.g. "09"
    pub computed_magnetic_designator: Option<String>, // WMM analysis; None = unknown
    pub true_heading_deg: Option<f64>,                // None = source did not publish it
    pub length_ft: u32,
    pub width_ft: u32,
    pub surface: Option<String>,
    pub le_ident: String,
    pub le_lat: f64,
    pub le_lon: f64,
    pub le_elevation_ft: Option<f64>,
    pub he_ident: String,
    pub he_lat: f64,
    pub he_lon: f64,
    pub he_elevation_ft: Option<f64>,
    pub temporal: TemporalValidity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalAirport {
    pub id: AirportId,
    pub ident: String,
    pub name: String,
    pub airport_type: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<f64>,
    pub iso_country: Option<String>,
    pub municipality: Option<String>,
    pub runways: Vec<CanonicalRunway>,
    pub temporal: TemporalValidity,
}

/// Canonical Airway Segment (one leg of an enroute airway, ARINC 424 `ER`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalAirwayLeg {
    pub object_id: AirwayLegId,
    /// Route identifier (e.g. `J1`, `V257`, `A315`).
    pub route_ident: String,
    /// ARINC 424 route type: `O` conventional, `R` RNAV.
    pub route_type: String,
    /// Published level: `H`igh, `L`ow, or `None` when the source does not
    /// say (exporters must not guess).
    pub level: Option<char>,
    /// Position of the END fix within the route (starts at 2: segment 1
    /// connects sequence 1 to sequence 2).
    pub sequence_number: u32,
    pub start_fix: String,
    pub start_icao_code: String,
    pub end_fix: String,
    pub end_icao_code: String,
    /// Directional restriction: `N` none, `F` forward, `B` backward.
    pub direction: char,
    /// Segment minimum enroute altitude, feet (ARINC 5.30 MEA).
    pub minimum_altitude_ft: Option<u32>,
    /// Segment maximum authorized altitude, feet.
    pub maximum_altitude_ft: Option<u32>,
    pub temporal: TemporalValidity,
}

/// Canonical procedure leg (one record of an ARINC 424 PD/PE/PF procedure).
/// convert424toxplane output; anything not decodable stays in `raw`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalProcedureLeg {
    pub object_id: ProcedureLegId,
    pub airport_ident: String,
    pub icao_code: String,
    /// Record kind: `D` = SID, `E` = STAR, `F` = approach.
    pub procedure_kind: char,
    pub procedure_ident: String,
    /// ARINC 5.7 route type (e.g. `4`, `T`, `A`, `F`, `R`).
    pub route_type: String,
    /// ARINC 5.11 transition identifier (blank = common route).
    pub transition_ident: String,
    pub sequence_number: u32,
    pub fix_ident: String,
    pub fix_icao_code: String,
    pub fix_section: String,
    /// ARINC 5.17 waypoint description codes (4 chars).
    pub waypoint_description: String,
    /// ARINC 5.20 turn direction (`L`/`R`).
    pub turn_direction: Option<char>,
    /// ARINC 5.211 required navigation performance, nautical miles.
    pub rnp_nm: Option<f64>,
    /// Path and terminator (ARINC 5.21, cols 48-49), kept verbatim.
    pub path_terminator: String,
    /// Recommended navaid reference (ident + ICAO + section/subsection).
    pub recommended_navaid: Option<String>,
    /// ARINC 5.204 arc radius (RF legs), nautical miles.
    pub arc_radius_nm: Option<f64>,
    /// Course A (cols 63-66), degrees.
    pub course_a_deg: Option<f64>,
    /// Distance A (cols 67-70), nautical miles.
    pub distance_a_nm: Option<f64>,
    /// Course B (cols 71-74), degrees.
    pub course_b_deg: Option<f64>,
    /// Distance B (cols 75-78), nautical miles.
    pub distance_b_nm: Option<f64>,
    /// Altitude descriptor (col 83): `+`, `-`, `B` or blank.
    pub altitude_descriptor: Option<char>,
    /// ARINC 5.30 altitude 1, feet.
    pub altitude_1_ft: Option<u32>,
    /// ARINC 5.30 altitude 2, feet.
    pub altitude_2_ft: Option<u32>,
    /// ARINC 5.53 speed limit, knots.
    pub speed_limit_kts: Option<u32>,
    /// Course C (cols 100-102, e.g. DF legs), whole degrees.
    pub course_c_deg: Option<u32>,
    /// ARINC 5.70 vertical angle, degrees.
    pub vertical_angle_deg: Option<f64>,
    /// MSA center fix (ident + ICAO + section/subsection).
    pub msa_center_fix: Option<String>,
    /// ARINC 5.7 route qualifiers (cols 119-120).
    pub route_qualifiers: String,
    /// The raw 132-column record: lossless preservation of unsupported
    /// semantics.
    pub raw: String,
    pub temporal: TemporalValidity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProcedureLegId(pub String);

/// Database & Storage Status Summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreStatus {
    pub database_path: String,
    pub is_open: bool,
    pub integrity_ok: bool,
    pub migration_version: u32,
    pub total_snapshots: usize,
    pub latest_revision_id: Option<String>,
    pub total_airports: usize,
    pub total_runways: usize,
    pub total_navaids: usize,
    pub total_waypoints: usize,
    pub total_airway_legs: usize,
    pub total_procedure_legs: usize,
}
