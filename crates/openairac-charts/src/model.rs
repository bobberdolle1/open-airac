//! Canonical Chart Domain Model for OpenAIRAC.
//!
//! Charts represent published aeronautical documents (e.g. FAA d-TPP, France SIA eAIP)
//! and are strictly decoupled from machine-readable navigation procedures (`CanonicalProcedureLeg`).

use chrono::{DateTime, Utc};
use openairac_model::RedistributionPermission;
use serde::{Deserialize, Serialize};

/// Unique identifier for a chart document (e.g. `faa:2608:KJFK:00610IL4L` or `sia:2608:LFPG:ADC_01`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChartDocumentId(pub String);

impl std::fmt::Display for ChartDocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Normalized category for aeronautical charts across international authorities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedChartType {
    /// Aerodrome / Airport Layout Diagram (e.g. APD, ADC)
    AirportDiagram,
    /// Aircraft Parking & Docking Chart (e.g. APDC, PDC)
    ParkingDocking,
    /// Ground Movement Chart (e.g. GMC)
    GroundMovement,
    /// Standard Instrument Departure / Departure Procedure (e.g. DP, SID)
    Sid,
    /// Standard Terminal Arrival Route (e.g. STAR)
    Star,
    /// Standard Instrument Approach Procedure / Instrument Approach Chart (e.g. IAP, IAC)
    Approach,
    /// Visual Approach Chart (e.g. VAC, CVFP)
    ApproachVisual,
    /// Takeoff Minima Textual Procedure (e.g. TAKEOFF MINIMUMS)
    TakeoffMinima,
    /// Alternate Airport Minima (e.g. ALTERNATE MINIMUMS)
    AlternateMinima,
    /// Radar Minimum Vectoring Altitude (e.g. RADAR MINIMUMS, MVA)
    RadarMinima,
    /// Airport Hot Spots / Complex Geometry Warning (e.g. HOT SPOT)
    HotSpot,
    /// Holding Patterns Chart
    Holding,
    /// Aerodrome Obstacle Chart (e.g. Type A/B)
    Obstacle,
    /// Noise Abatement Procedure Chart
    Noise,
    /// Legend, Table of Contents, General Textual Information
    GeneralInfo,
    /// Other Authority-Specific Document
    Other,
}

impl NormalizedChartType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NormalizedChartType::AirportDiagram => "airport_diagram",
            NormalizedChartType::ParkingDocking => "parking_docking",
            NormalizedChartType::GroundMovement => "ground_movement",
            NormalizedChartType::Sid => "sid",
            NormalizedChartType::Star => "star",
            NormalizedChartType::Approach => "approach",
            NormalizedChartType::ApproachVisual => "approach_visual",
            NormalizedChartType::TakeoffMinima => "takeoff_minima",
            NormalizedChartType::AlternateMinima => "alternate_minima",
            NormalizedChartType::RadarMinima => "radar_minima",
            NormalizedChartType::HotSpot => "hot_spot",
            NormalizedChartType::Holding => "holding",
            NormalizedChartType::Obstacle => "obstacle",
            NormalizedChartType::Noise => "noise",
            NormalizedChartType::GeneralInfo => "general_info",
            NormalizedChartType::Other => "other",
        }
    }

    pub fn display_group(&self) -> &'static str {
        match self {
            NormalizedChartType::AirportDiagram
            | NormalizedChartType::ParkingDocking
            | NormalizedChartType::GroundMovement
            | NormalizedChartType::HotSpot
            | NormalizedChartType::Obstacle => "Airport",
            NormalizedChartType::Sid | NormalizedChartType::TakeoffMinima => "Departure (SID)",
            NormalizedChartType::Star => "Arrival (STAR)",
            NormalizedChartType::Approach
            | NormalizedChartType::ApproachVisual
            | NormalizedChartType::AlternateMinima
            | NormalizedChartType::RadarMinima => "Approach",
            _ => "General & Info",
        }
    }

    pub fn from_faa_code(code: &str) -> Self {
        match code.trim().to_uppercase().as_str() {
            "APD" => NormalizedChartType::AirportDiagram,
            "DP" => NormalizedChartType::Sid,
            "STAR" | "STR" => NormalizedChartType::Star,
            "IAP" => NormalizedChartType::Approach,
            "CVFP" => NormalizedChartType::ApproachVisual,
            "MIN" => NormalizedChartType::TakeoffMinima,
            "HOT" => NormalizedChartType::HotSpot,
            "LAHSO" => NormalizedChartType::GeneralInfo,
            _ => NormalizedChartType::Other,
        }
    }

    pub fn from_eai_code(code: &str) -> Self {
        match code.trim().to_uppercase().as_str() {
            "ADC" => NormalizedChartType::AirportDiagram,
            "APDC" => NormalizedChartType::ParkingDocking,
            "GMC" => NormalizedChartType::GroundMovement,
            "SID" => NormalizedChartType::Sid,
            "STAR" => NormalizedChartType::Star,
            "IAC" => NormalizedChartType::Approach,
            "VAC" => NormalizedChartType::ApproachVisual,
            "MVA" => NormalizedChartType::RadarMinima,
            "AOC" => NormalizedChartType::Obstacle,
            _ => NormalizedChartType::Other,
        }
    }
}

/// MIME format of chart asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChartMimeType {
    Pdf,
    Png,
    Svg,
    Jpeg,
    Other(String),
}

impl ChartMimeType {
    pub fn as_str(&self) -> &str {
        match self {
            ChartMimeType::Pdf => "application/pdf",
            ChartMimeType::Png => "image/png",
            ChartMimeType::Svg => "image/svg+xml",
            ChartMimeType::Jpeg => "image/jpeg",
            ChartMimeType::Other(s) => s.as_str(),
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ChartMimeType::Pdf => "pdf",
            ChartMimeType::Png => "png",
            ChartMimeType::Svg => "svg",
            ChartMimeType::Jpeg => "jpg",
            ChartMimeType::Other(_) => "bin",
        }
    }
}

/// Status of geospatial coordinate registration for chart documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeoreferenceStatus {
    /// Standard document without geospatial registration (manual reference only).
    NotGeoreferenced,
    /// Officially calibrated coordinates available for moving map overlay.
    Georeferenced,
    /// Estimated/approximate bounds (never navigation-grade).
    Approximate,
    /// Unsupported format or structure for georeferencing.
    Unsupported,
}

/// Canonical metadata for a published aeronautical chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartDocument {
    pub id: ChartDocumentId,
    pub provider_id: String,
    pub airport_icao: String,
    pub airport_iata: Option<String>,
    pub chart_type: NormalizedChartType,
    pub provider_chart_type: String,
    pub title: String,
    pub procedure_name: Option<String>,
    pub runway: Option<String>,
    pub effective_from: Option<DateTime<Utc>>,
    pub effective_to: Option<DateTime<Utc>>,
    pub revision_date: Option<DateTime<Utc>>,
    pub airac_cycle: String,
    pub language: Option<String>,
    pub source_url: String,
    pub source_document_id: Option<String>,
    pub license_policy: RedistributionPermission,
    pub attribution: String,
    pub mime_type: ChartMimeType,
    pub asset_sha256: Option<String>,
    pub file_size_bytes: Option<u64>,
    pub georeference_status: GeoreferenceStatus,
    pub change_flag: Option<String>,
}

/// Confidence rating for procedure-to-chart associations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationConfidence {
    /// Exact match on procedure ident, runway, and type.
    Exact,
    /// High-probability match on procedure prefix and runway.
    Likely,
    /// Multiple possible charts qualify; user selection required.
    Ambiguous,
    /// No authoritative chart found.
    Unmatched,
}

/// Reference association between a machine-readable navigation procedure and a chart document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartAssociation {
    pub procedure_ident: String,
    pub procedure_kind: char, // 'D' = SID, 'E' = STAR, 'F' = Approach
    pub airport_icao: String,
    pub runway: Option<String>,
    pub chart_id: ChartDocumentId,
    pub confidence: AssociationConfidence,
    pub match_reason: String,
}
