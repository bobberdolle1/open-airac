//! OpenAIRAC Procedure Engine
//!
//! Provides canonical data models for Instrument Terminal Procedures (SIDs, STARs, Approaches)
//! and ARINC 424 Path Terminator representations.

use openairac_model::TemporalValidity;
use serde::{Deserialize, Serialize};

/// Type of Instrument Procedure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcedureKind {
    Sid,
    Star,
    Approach,
}

impl ProcedureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProcedureKind::Sid => "SID",
            ProcedureKind::Star => "STAR",
            ProcedureKind::Approach => "APPROACH",
        }
    }
}

/// ARINC 424 Path Terminator definitions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathTerminator {
    IF, // Initial Fix
    TF, // Track to Fix
    CF, // Course to Fix
    DF, // Direct to Fix
    FA, // Fix to Altitude
    FC, // Track to Distance
    FD, // Track to Altitude
    FM, // Fix to Manual
    CA, // Course to Altitude
    CD, // Course to Distance
    CI, // Course to Intercept
    CR, // Course to Radial
    VA, // Heading to Altitude
    VD, // Heading to Distance
    VI, // Heading to Intercept
    VM, // Heading to Manual
    VR, // Heading to Radial
    HA, // Hold to Altitude
    HF, // Hold to Fix
    HM, // Hold to Manual
    RF, // Radius to Fix
    Unsupported(String),
}

impl PathTerminator {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_uppercase().as_str() {
            "IF" => PathTerminator::IF,
            "TF" => PathTerminator::TF,
            "CF" => PathTerminator::CF,
            "DF" => PathTerminator::DF,
            "FA" => PathTerminator::FA,
            "FC" => PathTerminator::FC,
            "FD" => PathTerminator::FD,
            "FM" => PathTerminator::FM,
            "CA" => PathTerminator::CA,
            "CD" => PathTerminator::CD,
            "CI" => PathTerminator::CI,
            "CR" => PathTerminator::CR,
            "VA" => PathTerminator::VA,
            "VD" => PathTerminator::VD,
            "VI" => PathTerminator::VI,
            "VM" => PathTerminator::VM,
            "VR" => PathTerminator::VR,
            "HA" => PathTerminator::HA,
            "HF" => PathTerminator::HF,
            "HM" => PathTerminator::HM,
            "RF" => PathTerminator::RF,
            other => PathTerminator::Unsupported(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            PathTerminator::IF => "IF",
            PathTerminator::TF => "TF",
            PathTerminator::CF => "CF",
            PathTerminator::DF => "DF",
            PathTerminator::FA => "FA",
            PathTerminator::FC => "FC",
            PathTerminator::FD => "FD",
            PathTerminator::FM => "FM",
            PathTerminator::CA => "CA",
            PathTerminator::CD => "CD",
            PathTerminator::CI => "CI",
            PathTerminator::CR => "CR",
            PathTerminator::VA => "VA",
            PathTerminator::VD => "VD",
            PathTerminator::VI => "VI",
            PathTerminator::VM => "VM",
            PathTerminator::VR => "VR",
            PathTerminator::HA => "HA",
            PathTerminator::HF => "HF",
            PathTerminator::HM => "HM",
            PathTerminator::RF => "RF",
            PathTerminator::Unsupported(s) => s.as_str(),
        }
    }
}

/// Altitude constraint specification for procedure legs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AltitudeConstraint {
    At(u32),
    AtOrAbove(u32),
    AtOrBelow(u32),
    Between(u32, u32),
}

/// Speed constraint specification for procedure legs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpeedConstraint {
    At(u32),
    AtOrBelow(u32),
}

/// Single Procedure Leg
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureLeg {
    pub sequence_number: u32,
    pub path_terminator: PathTerminator,
    pub fix_ident: String,
    pub fix_latitude: Option<f64>,
    pub fix_longitude: Option<f64>,
    pub true_track_deg: Option<f64>,
    pub distance_nm: Option<f64>,
    pub altitude_constraint: Option<AltitudeConstraint>,
    pub speed_constraint: Option<SpeedConstraint>,
}

/// Procedure Transition (e.g. Enroute transition or Runway transition)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureTransition {
    pub transition_ident: String,
    pub legs: Vec<ProcedureLeg>,
}

/// Canonical Instrument Procedure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Procedure {
    pub id: String,
    pub airport_ident: String,
    pub name: String,
    pub kind: ProcedureKind,
    pub main_legs: Vec<ProcedureLeg>,
    pub transitions: Vec<ProcedureTransition>,
    pub temporal: TemporalValidity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_terminator_parsing() {
        assert_eq!(PathTerminator::parse("IF"), PathTerminator::IF);
        assert_eq!(PathTerminator::parse("TF"), PathTerminator::TF);
        assert_eq!(PathTerminator::parse("RF"), PathTerminator::RF);
        assert_eq!(
            PathTerminator::parse("XYZ"),
            PathTerminator::Unsupported("XYZ".to_string())
        );
    }
}
