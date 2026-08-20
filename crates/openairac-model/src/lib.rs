pub mod airac;
pub mod fra;
pub mod policy;
pub use airac::*;
pub use fra::*;
pub use policy::*;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LpvFasId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MsaId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MoraId(pub String);

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

/// Source Document Taxonomy classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceDocumentTaxonomy {
    /// Complete structured machine-readable dataset (e.g. FAA CIFP, AIXM 4.5/5.1 XML).
    StructuredNavDataset,
    /// Official published procedure coding table / database requirements (e.g. SIA DATA SID/STAR/RNP, ENAIRE tabular descriptions).
    StructuredProcedurePublication,
    /// Human-readable graphical chart plate (e.g. IAC, VAC, SID/STAR graphical PDF plate).
    HumanReadableChart,
    /// Geometrically derived or reconstructed auxiliary layer (e.g. FIR polygon boundaries, magnetic variation grids).
    DerivedGeometry,
}

impl SourceDocumentTaxonomy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StructuredNavDataset => "structured_nav_dataset",
            Self::StructuredProcedurePublication => "structured_procedure_publication",
            Self::HumanReadableChart => "human_readable_chart",
            Self::DerivedGeometry => "derived_geometry",
        }
    }

    pub fn display_badge(&self) -> &'static str {
        match self {
            Self::StructuredNavDataset => "[DATASET]",
            Self::StructuredProcedurePublication => "[PROCEDURE_PUB]",
            Self::HumanReadableChart => "[CHART]",
            Self::DerivedGeometry => "[DERIVED]",
        }
    }

    pub fn is_machine_readable_procedure_source(&self) -> bool {
        matches!(
            self,
            Self::StructuredNavDataset | Self::StructuredProcedurePublication
        )
    }
}

/// Source Provenance tracking data origins & licenses
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemporalValidity {
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub source_snapshot_id: SourceSnapshotId,
}

/// Canonical Waypoint (Fix)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Canonical LPV/LP Final Approach Segment (FAS) data (ARINC 424 PP Path Point records)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalLpvFas {
    pub object_id: LpvFasId,
    pub airport_ident: String,
    pub icao_code: String,
    pub approach_ident: String,
    pub runway_ident: String,
    pub ref_path_ident: String,
    pub gnss_channel: u32,
    pub app_type: String,
    pub ltp_latitude: f64,
    pub ltp_longitude: f64,
    pub fpap_latitude: f64,
    pub fpap_longitude: f64,
    pub bearing_true_deg: f64,
    pub elevation_ft: i32,
    pub length_offset_m: f64,
    pub tch_ft: f64,
    pub gpa_deg: f64,
    pub temporal: TemporalValidity,
}

/// One sector within a Minimum Sector Altitude (MSA) ring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MsaSector {
    pub bearing_deg: u32,
    pub altitude_hundreds_ft: u32,
    pub radius_nm: u32,
}

/// Canonical Minimum Sector Altitude (MSA / Section PS records)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalMsa {
    pub object_id: MsaId,
    pub airport_ident: String,
    pub icao_code: String,
    pub center_fix: String,
    pub center_icao_code: String,
    pub center_section: String,
    pub fix_type: u8,
    pub magnetic_true_indicator: char,
    pub sectors: Vec<MsaSector>,
    pub temporal: TemporalValidity,
}

/// Canonical Grid MORA block (ARINC 424 AS records)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalMora {
    pub object_id: MoraId,
    pub start_latitude: String,
    pub start_longitude: String,
    pub mora_values: Vec<u32>,
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
    Rsbn,
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
            NavaidKind::Rsbn => "RSBN",
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
            "RSBN" | "РСБН" => Some(NavaidKind::Rsbn),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Canonical RSBN (Радиотехническая система ближней навигации) Station.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalRsbnStation {
    pub object_id: NavaidId,
    pub ident: String,
    pub name: String,
    pub channel: u8,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: Option<i32>,
    pub range_km: Option<f64>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
    pub temporal: TemporalValidity,
}

/// Canonical Airport & Runway with Dual Designators
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalRunway {
    pub id: RunwayId,
    pub airport_id: AirportId,
    pub airport_ident: String,
    pub official_designator: String,                  // e.g. "09"
    pub computed_magnetic_designator: Option<String>, // WMM analysis; None = unknown
    pub true_heading_deg: Option<f64>,                // None = source did not publish it
    pub length_ft: u32,
    pub width_ft: Option<u32>,
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

impl CanonicalRunway {
    /// Authoritative true heading in degrees [0, 360) from Low-End (primary) to High-End (secondary).
    /// Uses published true heading if present, otherwise computes geodesic bearing between endpoints,
    /// falling back to nominal heading derived from the runway designator.
    pub fn true_heading(&self) -> f64 {
        if let Some(h) = self
            .true_heading_deg
            .filter(|h| h.is_finite() && (0.0..=360.0).contains(h))
        {
            return h;
        }
        if (self.le_lat - self.he_lat).abs() > 1e-6 || (self.le_lon - self.he_lon).abs() > 1e-6 {
            return geodesic_bearing_deg(self.le_lat, self.le_lon, self.he_lat, self.he_lon);
        }
        nominal_heading_from_designator(&self.le_ident).unwrap_or(0.0)
    }

    /// Reciprocal true heading in degrees [0, 360) for High-End (secondary) threshold.
    pub fn reciprocal_true_heading(&self) -> f64 {
        (self.true_heading() + 180.0).rem_euclid(360.0)
    }
}

/// Initial geodesic bearing from point 1 (lat1, lon1) to point 2 (lat2, lon2) in true degrees [0, 360).
pub fn geodesic_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let delta_lambda = (lon2 - lon1).to_radians();
    let y = delta_lambda.sin() * phi2.cos();
    let x = phi1.cos() * phi2.sin() - phi1.sin() * phi2.cos() * delta_lambda.cos();
    let theta = y.atan2(x);
    (theta.to_degrees() + 360.0).rem_euclid(360.0)
}

/// Calculate endpoint at distance (meters) along bearing (true degrees) from (lat, lon).
pub fn geodesic_endpoint(lat: f64, lon: f64, dist_m: f64, bearing_deg: f64) -> (f64, f64) {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let d_r = dist_m / EARTH_RADIUS_M;
    let brng = bearing_deg.to_radians();
    let phi1 = lat.to_radians();
    let lam1 = lon.to_radians();

    let phi2 = (phi1.sin() * d_r.cos() + phi1.cos() * d_r.sin() * brng.cos()).asin();
    let lam2 =
        lam1 + (brng.sin() * d_r.sin() * phi1.cos()).atan2(d_r.cos() - phi1.sin() * phi2.sin());
    let lon2 = (lam2.to_degrees() + 540.0).rem_euclid(360.0) - 180.0;
    (phi2.to_degrees(), lon2)
}

/// Extracts the nominal magnetic heading (e.g. "03" -> 30°, "28R" -> 280°) from a runway designator.
pub fn nominal_heading_from_designator(designator: &str) -> Option<f64> {
    let clean = designator
        .trim()
        .trim_start_matches("RW")
        .trim_start_matches('R');
    let digits: String = clean.chars().take_while(|c| c.is_ascii_digit()).collect();
    if let Some(num) = digits.parse::<u32>().ok().filter(|n| (1..=36).contains(n)) {
        let hdg = (num * 10) as f64;
        return Some(if hdg == 360.0 { 360.0 } else { hdg });
    }
    None
}

/// Classification tiers for airports and landing facilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AirportTier {
    CertifiedNavigationAirport,
    PublicCivilAirport,
    RegionalAirport,
    Airfield,
    Heliport,
    Closed,
}

impl AirportTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CertifiedNavigationAirport => "certified_navigation_airport",
            Self::PublicCivilAirport => "public_civil_airport",
            Self::RegionalAirport => "regional_airport",
            Self::Airfield => "airfield",
            Self::Heliport => "heliport",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

impl CanonicalAirport {
    pub fn tier(&self) -> AirportTier {
        if self.airport_type.eq_ignore_ascii_case("heliport") {
            AirportTier::Heliport
        } else if self.airport_type.eq_ignore_ascii_case("closed") {
            AirportTier::Closed
        } else if self.airport_type.eq_ignore_ascii_case("large_airport") {
            AirportTier::CertifiedNavigationAirport
        } else if self.airport_type.eq_ignore_ascii_case("medium_airport") {
            AirportTier::PublicCivilAirport
        } else if self.airport_type.eq_ignore_ascii_case("small_airport") {
            AirportTier::RegionalAirport
        } else {
            AirportTier::Airfield
        }
    }
}

/// Canonical Airway Segment (one leg of an enroute airway, ARINC 424 `ER`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub total_lpv_fas: usize,
    pub total_msa: usize,
    pub total_mora: usize,
}

// ---------------------------------------------------------------------------
// AIRAC lifecycle domain (v0.4)
// ---------------------------------------------------------------------------

/// AIRAC cycle identifier (e.g. `"2608"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CycleId(pub String);

/// Lifecycle state of a published cycle.
///
/// Legal transitions (validated by the store):
/// ```text
/// Discovered  -> Preloaded | Superseded | Expired
/// Preloaded   -> Active | Superseded | Expired
/// Active      -> Superseded | Expired | RolledBack
/// RolledBack  -> Superseded
/// Superseded / Expired: terminal
/// ```
/// `Active` is bookkeeping recorded by the `Observed` event; data queries
/// are time-driven and NEVER gate on cycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleStatus {
    Discovered,
    Preloaded,
    Active,
    Superseded,
    Expired,
    RolledBack,
}

impl CycleStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CycleStatus::Discovered => "Discovered",
            CycleStatus::Preloaded => "Preloaded",
            CycleStatus::Active => "Active",
            CycleStatus::Superseded => "Superseded",
            CycleStatus::Expired => "Expired",
            CycleStatus::RolledBack => "RolledBack",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Discovered" => Some(CycleStatus::Discovered),
            "Preloaded" => Some(CycleStatus::Preloaded),
            "Active" => Some(CycleStatus::Active),
            "Superseded" => Some(CycleStatus::Superseded),
            "Expired" => Some(CycleStatus::Expired),
            "RolledBack" => Some(CycleStatus::RolledBack),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, CycleStatus::Superseded | CycleStatus::Expired)
    }

    pub fn legal_transition(from: CycleStatus, to: CycleStatus) -> bool {
        use CycleStatus::*;
        matches!(
            (from, to),
            (Discovered, Preloaded | Superseded | Expired)
                | (Preloaded, Active | Superseded | Expired)
                | (Active, Superseded | Expired | RolledBack)
                | (RolledBack, Superseded)
        )
    }
}

/// One published AIRAC cycle in the catalog. The catalog is metadata:
/// the temporal data rows are the only source of truth for `world_at(t)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiracCycle {
    pub id: CycleId,
    /// `None` = not yet confirmed from the source (fail-closed: such a
    /// cycle can never be scheduled/activated).
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_until: Option<DateTime<Utc>>,
    pub status: CycleStatus,
    /// Source location this cycle was discovered from (e.g. the CIFP zip URL).
    pub source_uri: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub notes: Option<String>,
}

/// Audit/journal entry kind. The journal records intents and facts
/// (schedule, observed transition, rollback) and is display/audit
/// metadata; it never gates queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CycleEventKind {
    /// The cycle's datasets were ingested for its `effective_from`
    /// (recorded in the same transaction as the preload ingest).
    Scheduled,
    /// Bookkeeping mark that the cycle's effective window was reached.
    Observed,
    /// The cycle was rolled back by re-publishing the pre-cycle state as
    /// new revisions at the rollback instant.
    Rollback,
}

impl CycleEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            CycleEventKind::Scheduled => "Scheduled",
            CycleEventKind::Observed => "Observed",
            CycleEventKind::Rollback => "Rollback",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Scheduled" => Some(CycleEventKind::Scheduled),
            "Observed" => Some(CycleEventKind::Observed),
            "Rollback" => Some(CycleEventKind::Rollback),
            _ => None,
        }
    }
}

/// One journal entry. Deliberately carries NO `world_revision_id`: a
/// rollback re-publishes rows from multiple historical snapshots and the
/// per-row provenance lives in those rows' own `source_snapshot_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleEvent {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub kind: CycleEventKind,
    /// Subject cycle.
    pub cycle_id: CycleId,
    /// Required for `Rollback`: which cycle was re-published.
    pub restored_cycle_id: Option<CycleId>,
    pub notes: Option<String>,
}

/// Whether a dataset publication is the cycle's baseline or a correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionKind {
    Baseline,
    Correction,
}

impl RevisionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RevisionKind::Baseline => "Baseline",
            RevisionKind::Correction => "Correction",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Baseline" => Some(RevisionKind::Baseline),
            "Correction" => Some(RevisionKind::Correction),
            _ => None,
        }
    }
}

/// Whether a dataset publication covers the whole dataset or only
/// changed records. `close_absent_at` semantics depend ONLY on this:
/// full-snapshot removals must be applied even for corrected re-publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Coverage {
    FullSnapshot,
    Partial,
}

impl Coverage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Coverage::FullSnapshot => "FullSnapshot",
            Coverage::Partial => "Partial",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "FullSnapshot" => Some(Coverage::FullSnapshot),
            "Partial" => Some(Coverage::Partial),
            _ => None,
        }
    }
}

/// Append-only record of one observed dataset publication. The latest row
/// (max `retrieved_at`) per `(provider, dataset, airac_cycle)` is the
/// version compared for skip-by-hash; corrected re-publishes of the same
/// cycle append new rows instead of overwriting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetVersion {
    pub id: i64,
    pub provider: String,
    pub dataset: String,
    pub airac_cycle: Option<String>,
    pub content_sha256: String,
    pub retrieved_at: DateTime<Utc>,
    pub revision_kind: RevisionKind,
    pub coverage: Coverage,
    /// Publication identity: two files for the same cycle are not
    /// necessarily the same publication. Same identity + same checksum =
    /// replay; same identity + different checksum = conflict unless the
    /// publication is a Correction.
    pub publication_id: Option<String>,
    /// The effective instant this publication's entities carry.
    pub valid_from: Option<DateTime<Utc>>,
    pub notes: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider manifest registry (v0.4)
// ---------------------------------------------------------------------------

/// One published dataset of a provider.
#[derive(Debug, Clone, Copy)]
pub struct DatasetManifest {
    pub name: &'static str,
    /// Entity tables this dataset writes (store table names).
    pub entity_tables: &'static [&'static str],
}

/// Geographic coverage declared by a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoverageScope {
    /// One nation's official data (e.g. US FAA CIFP).
    Nationwide,
    /// Global dataset without procedure coverage.
    Worldwide,
}

impl CoverageScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            CoverageScope::Nationwide => "nationwide",
            CoverageScope::Worldwide => "worldwide",
        }
    }
}

/// How a provider publishes revisions over time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalModel {
    /// 28-day AIRAC cycles with confirmed effective instants.
    AiracCycle,
    /// Continuous (daily) snapshots without cycles.
    Continuous,
}

impl TemporalModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            TemporalModel::AiracCycle => "airac_cycle",
            TemporalModel::Continuous => "continuous",
        }
    }
}

/// The publication shapes a provider can deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateModel {
    FullSnapshot,
    FullSnapshotAndDifferential,
}

impl UpdateModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateModel::FullSnapshot => "full_snapshot",
            UpdateModel::FullSnapshotAndDifferential => "full_snapshot_and_differential",
        }
    }
}

/// Declared capabilities of one provider (v0.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub coverage: CoverageScope,
    pub temporal: TemporalModel,
    pub update: UpdateModel,
    /// Authority scope note (entity classes this provider is preferred
    /// for; the reconciliation authority policy remains the decisive
    /// mechanism — this is declarative metadata).
    pub authority_note: &'static str,
}

/// Static metadata of one provider: the ownership contract between
/// providers, object-id namespaces, and entity tables. The object-id
/// prefix (`<namespace>:` in entity ids) is the ONLY ownership signal in
/// the store; full-snapshot removal (`close_absent_at`) and rollback
/// scoping derive from this registry + `cycle_snapshots ->
/// source_snapshots.provider`.
#[derive(Debug, Clone, Copy)]
pub struct ProviderManifest {
    /// Provider string as stored in `source_snapshots.provider`.
    pub name: &'static str,
    /// Object-id namespace prefix (ids are `<namespace>:...`).
    pub namespace: &'static str,
    pub capabilities: ProviderCapabilities,
    pub datasets: &'static [DatasetManifest],
}

/// Known providers.
///
/// | snapshot.provider | CLI key      | namespace     |
/// |-------------------|--------------|---------------|
/// | `OurAirports`     | `ourairports`| `ourairports` |
/// | `FAA_CIFP`        | `faa_cifp`   | `faa`         |
pub const PROVIDER_MANIFESTS: &[ProviderManifest] = &[
    ProviderManifest {
        name: "OurAirports",
        namespace: "ourairports",
        capabilities: ProviderCapabilities {
            coverage: CoverageScope::Worldwide,
            temporal: TemporalModel::Continuous,
            update: UpdateModel::FullSnapshot,
            authority_note: "worldwide airport/runway/navaid metadata",
        },
        datasets: &[
            DatasetManifest {
                name: "airports",
                entity_tables: &["airports"],
            },
            DatasetManifest {
                name: "runways",
                entity_tables: &["runways"],
            },
            DatasetManifest {
                name: "navaids",
                entity_tables: &["navaids"],
            },
        ],
    },
    ProviderManifest {
        name: "FAA_CIFP",
        namespace: "faa",
        capabilities: ProviderCapabilities {
            coverage: CoverageScope::Nationwide,
            temporal: TemporalModel::AiracCycle,
            update: UpdateModel::FullSnapshot,
            authority_note: "US navigation semantics: procedures, navaids, fixes, terminals",
        },
        datasets: &[DatasetManifest {
            name: "FAACIFP18",
            // The decoder emits airports and runways (PA/PG, paired by
            // reciprocal designator), waypoints, navaids, airway legs
            // and procedure legs.
            entity_tables: &[
                "airports",
                "runways",
                "waypoints",
                "navaids",
                "airway_legs",
                "procedure_legs",
                "lpv_fas",
                "msa",
                "mora",
            ],
        }],
    },
    ProviderManifest {
        name: "FAA_AIXM",
        namespace: "faa_aixm",
        capabilities: ProviderCapabilities {
            coverage: CoverageScope::Nationwide,
            temporal: TemporalModel::AiracCycle,
            update: UpdateModel::FullSnapshot,
            authority_note: "US AIXM 5.1 data: procedures, navaids, fixes, airports",
        },
        datasets: &[DatasetManifest {
            name: "AIXM5",
            entity_tables: &[
                "airports",
                "runways",
                "waypoints",
                "navaids",
                "airway_legs",
                "procedure_legs",
            ],
        }],
    },
    ProviderManifest {
        name: "BYOD_AIXM",
        namespace: "byod",
        capabilities: ProviderCapabilities {
            coverage: CoverageScope::Worldwide,
            temporal: TemporalModel::Continuous,
            update: UpdateModel::FullSnapshot,
            authority_note: "User Bring-Your-Own-Data AIXM 5.x dataset",
        },
        datasets: &[DatasetManifest {
            name: "AIXM5",
            entity_tables: &[
                "airports",
                "runways",
                "waypoints",
                "navaids",
                "airway_legs",
                "procedure_legs",
            ],
        }],
    },
    ProviderManifest {
        name: "FR_SIA",
        namespace: "sia",
        capabilities: ProviderCapabilities {
            coverage: CoverageScope::Nationwide,
            temporal: TemporalModel::AiracCycle,
            update: UpdateModel::FullSnapshot,
            authority_note: "French SIA official AIP / AIXM 4.5 aeronautical data",
        },
        datasets: &[DatasetManifest {
            name: "AIXM4.5",
            entity_tables: &[
                "airports",
                "runways",
                "waypoints",
                "navaids",
                "airway_legs",
                "procedure_legs",
            ],
        }],
    },
];

/// Manifest by stored provider string (source_snapshots.provider).
pub fn manifest_for_provider(provider: &str) -> Option<&'static ProviderManifest> {
    PROVIDER_MANIFESTS.iter().find(|m| m.name == provider)
}

/// Namespace prefix by stored provider string.
pub fn namespace_for_provider(provider: &str) -> Option<&'static str> {
    manifest_for_provider(provider).map(|m| m.namespace)
}

/// Entity tables a provider publishes (union over its datasets, sorted).
pub fn tables_for_provider(provider: &str) -> Option<Vec<&'static str>> {
    let manifest = manifest_for_provider(provider)?;
    let mut tables: Vec<&'static str> = manifest
        .datasets
        .iter()
        .flat_map(|d| d.entity_tables.iter().copied())
        .collect();
    tables.sort_unstable();
    tables.dedup();
    Some(tables)
}

// ---------------------------------------------------------------------------
// ILS associations (v0.5)
// ---------------------------------------------------------------------------

/// A verified ILS association derived from FAA CIFP PF approach records:
/// the final-approach RW leg's recommended navaid is the localizer, its
/// course is the localizer bearing, and its vertical angle is the
/// glideslope angle. ILS category is NOT published in CIFP and stays
/// out of this model (never fabricated).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IlsAssociation {
    pub airport_ident: String,
    pub icao_code: String,
    /// Approach ident (e.g. `I28L`).
    pub approach_ident: String,
    /// Runway END this approach serves (e.g. `28L`).
    pub runway_end: String,
    pub localizer_ident: String,
    /// ICAO region of the localizer.
    pub localizer_region: String,
    /// Localizer course, magnetic degrees.
    pub localizer_bearing_mag_deg: f64,
    /// Glideslope angle, degrees (absolute value).
    pub glideslope_angle_deg: f64,
    pub source_snapshot_id: SourceSnapshotId,
}

// ---------------------------------------------------------------------------
// Publication semantics (v0.4 S7)
// ---------------------------------------------------------------------------

/// What a publication MEANS for the dataset's entities.
///
/// * `FullSnapshot`: "this file is the whole dataset" — entities absent
///   from the file MAY be closed (subject to masking rules).
/// * `Differential`: "this file contains only changes" — absence means
///   NOTHING; removals require explicit tombstones.
/// * `Correction`: a re-publication that replaces/supersedes not-yet-
///   effective publication state, or adds post-effective revisions at a
///   new instant. Never a silent rewrite of effective history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateKind {
    FullSnapshot,
    Differential,
    Correction { coverage: Coverage },
}

impl UpdateKind {
    /// The orthogonal (RevisionKind, Coverage) axes persisted in
    /// `dataset_versions`.
    pub fn components(&self) -> (RevisionKind, Coverage) {
        match self {
            UpdateKind::FullSnapshot => (RevisionKind::Baseline, Coverage::FullSnapshot),
            UpdateKind::Differential => (RevisionKind::Baseline, Coverage::Partial),
            UpdateKind::Correction { coverage } => (RevisionKind::Correction, *coverage),
        }
    }

    pub fn from_components(kind: RevisionKind, coverage: Coverage) -> Self {
        match (kind, coverage) {
            (RevisionKind::Baseline, Coverage::FullSnapshot) => UpdateKind::FullSnapshot,
            (RevisionKind::Baseline, Coverage::Partial) => UpdateKind::Differential,
            (RevisionKind::Correction, coverage) => UpdateKind::Correction { coverage },
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateKind::FullSnapshot => "FullSnapshot",
            UpdateKind::Differential => "Differential",
            UpdateKind::Correction { .. } => "Correction",
        }
    }

    /// Whether full-snapshot absence semantics apply (close_absent).
    pub fn closes_absent(&self) -> bool {
        matches!(
            self,
            UpdateKind::FullSnapshot
                | UpdateKind::Correction {
                    coverage: Coverage::FullSnapshot
                }
        )
    }
}

/// A first-class removal fact: the provider explicitly published that
/// this entity no longer exists from `effective_from` onward. Absence in
/// a differential publication is NEVER a tombstone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tombstone {
    pub provider: String,
    pub dataset: String,
    /// Store entity table name (closed set).
    pub entity_table: String,
    pub entity_id: String,
    pub effective_from: DateTime<Utc>,
    pub source_snapshot_id: SourceSnapshotId,
    /// Provider-supplied reason/code when available.
    pub reason: Option<String>,
}

/// Outcome of applying one tombstone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TombstoneOutcome {
    /// An open row was closed at the effective instant.
    Closed,
    /// The entity was already closed/absent at the effective instant
    /// (idempotent replay or superseded).
    AlreadyClosed,
    /// No row for this entity exists at any time: deterministic
    /// diagnostic, nothing fabricated.
    Unknown,
}

/// Outcome of recording a dataset publication under its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationOutcome {
    /// New publication: recorded and applied.
    Recorded,
    /// Exact replay: same identity + same checksum, nothing re-applied.
    Duplicate,
}

/// Report of one `observe_cycles` transaction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObserveReport {
    pub activated: Vec<CycleId>,
    pub superseded: Vec<CycleId>,
    pub expired: Vec<CycleId>,
}

// ---------------------------------------------------------------------------
// Multi-source entity reconciliation (v0.4 S8)
// ---------------------------------------------------------------------------

/// Stable canonical reconciliation identity, derived deterministically
/// from the entity's natural identity key — independent of which
/// provider happens to be preferred.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalEntityId(pub String);

/// Reference to one provider-native source entity. Provider records
/// remain immutable facts; reconciliation only relates them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceEntityRef {
    pub provider: String,
    pub entity_table: String,
    pub entity_id: String,
}

impl SourceEntityRef {
    pub fn display(&self) -> String {
        format!("{}:{}:{}", self.provider, self.entity_table, self.entity_id)
    }
}

/// Membership confidence. Never merged silently: only Exact and
/// Probable create memberships; Ambiguous stays separate and is
/// surfaced as a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchConfidence {
    Exact,
    Probable,
}

impl MatchConfidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchConfidence::Exact => "Exact",
            MatchConfidence::Probable => "Probable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Exact" => Some(MatchConfidence::Exact),
            "Probable" => Some(MatchConfidence::Probable),
            _ => None,
        }
    }
}

/// Membership lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MembershipStatus {
    Active,
    Superseded,
}

/// Structured reconciliation evidence: WHY two entities were related.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum EvidenceFact {
    /// Natural identifier equal (ICAO ident, fix ident, ...).
    IdentEqual(String),
    /// ICAO region code equal.
    RegionEqual(String),
    /// Associated airport ident equal.
    AirportAssociation(String),
    /// ISO country equal.
    CountryEqual(String),
    /// Great-circle distance between coordinates, nautical miles.
    DistanceNm(f64),
    /// Navaid kind equal (VOR/VOR-DME/VORTAC/NDB/DME/TACAN).
    KindEqual(String),
    /// Frequency equal, kHz.
    FrequencyKhz(u32),
    /// Runway-end designator equal at the compared instant.
    RunwayDesignator(String),
    /// Physical runway geometry equal (endpoint coordinates).
    RunwayGeometryEqual,
    /// Normalized name similarity in [0, 1].
    NameSimilarity(f64),
    /// Published provider cross-reference.
    PublishedCrossReference(String),
    /// Validity windows overlap at the compared instant.
    TemporalOverlap,
}

/// Conflict severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictSeverity {
    Info,
    Warning,
    Error,
}

impl ConflictSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictSeverity::Info => "Info",
            ConflictSeverity::Warning => "Warning",
            ConflictSeverity::Error => "Error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Info" => Some(ConflictSeverity::Info),
            "Warning" => Some(ConflictSeverity::Warning),
            "Error" => Some(ConflictSeverity::Error),
            _ => None,
        }
    }
}

/// One persisted reconciliation conflict/diagnostic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationConflict {
    pub id: i64,
    pub entity_table: String,
    pub canonical_id: Option<CanonicalEntityId>,
    pub ref_a: String,
    pub ref_b: String,
    /// identity | field | geometry | ambiguity
    pub category: String,
    pub field_name: Option<String>,
    pub value_a: Option<String>,
    pub value_b: Option<String>,
    pub severity: ConflictSeverity,
    pub evidence: Vec<EvidenceFact>,
    pub detected_at: DateTime<Utc>,
    /// Resolution status when applicable (None = open).
    pub resolved: Option<String>,
}

/// One resolved field: value + the source it came from (traceability).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedField {
    pub field: String,
    pub value: String,
    pub source: SourceEntityRef,
    pub conflicts: Vec<String>,
}

/// A canonical entity with its members and field provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedEntity {
    pub canonical_id: CanonicalEntityId,
    pub entity_table: String,
    pub members: Vec<SourceEntityRef>,
    pub fields: Vec<ResolvedField>,
}

/// Reconciliation run statistics (S8.12 reporting).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationStats {
    pub source_entities: usize,
    pub candidate_pairs: usize,
    pub exact_matches: usize,
    pub probable_matches: usize,
    pub ambiguous: usize,
    pub conflicts: usize,
    pub distinct_rejected: usize,
}

/// One membership record: a source revision interval related to a
/// canonical identity. Interval semantics match the store: valid_from
/// inclusive, valid_until exclusive/NULL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceMembership {
    pub canonical_id: CanonicalEntityId,
    pub source: SourceEntityRef,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub confidence: MatchConfidence,
    pub match_method: String,
    pub evidence: Vec<EvidenceFact>,
    pub status: MembershipStatus,
}
