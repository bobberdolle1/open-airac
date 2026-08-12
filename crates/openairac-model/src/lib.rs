use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Source Provenance tracking data origins & licenses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSourceProvider {
    FaaCifp { cycle: String },
    OurAirports { snapshot_date: String },
    OpenFlightmaps { airac_cycle: String },
    OpenAip { dataset_id: String },
    UserCustom { author: String },
}

/// Temporal Validity Envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalValidity {
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub revision_id: String,
    pub source: DataSourceProvider,
}

/// Canonical Waypoint (Fix)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalWaypoint {
    pub object_id: String,
    pub ident: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub is_enroute: bool,
    pub region_code: String,
    pub temporal: TemporalValidity,
}

/// Canonical Navaid (VOR, NDB, DME, ILS)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NavaidKind {
    Vor,
    Vordme,
    Vortac,
    Ndb,
    IlsLocalizer,
    IlsGlidepath,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalNavaid {
    pub object_id: String,
    pub ident: String,
    pub name: String,
    pub kind: NavaidKind,
    pub frequency_khz: u32,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: i32,
    pub official_slaved_magvar_deg: f64,
    pub computed_wmm_magvar_deg: f64,
    pub temporal: TemporalValidity,
}

/// Canonical Airport & Runway with Dual Designators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRunway {
    pub official_designator: String,          // e.g. "09"
    pub computed_magnetic_designator: String, // e.g. "10"
    pub true_heading_deg: f64,
    pub length_ft: u32,
    pub width_ft: u32,
    pub start_lat: f64,
    pub start_lon: f64,
    pub end_lat: f64,
    pub end_lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalAirport {
    pub icao: String,
    pub iata: Option<String>,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: i32,
    pub runways: Vec<CanonicalRunway>,
    pub temporal: TemporalValidity,
}
