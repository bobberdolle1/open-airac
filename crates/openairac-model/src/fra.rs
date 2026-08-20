//! Free Route Airspace (FRA) and Enroute Airspace Structures.
//!
//! Provides canonical representations for modern Free Route Airspace implementations,
//! including FIR-level FRA boundaries, Significant Entry/Exit Points (E/X), Intermediate
//! Points (I), and temporal availability windows.

use crate::TemporalValidity;
use serde::{Deserialize, Serialize};

/// Type of Free Route Airspace point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FraPointKind {
    /// Significant Entry Point into FRA volume.
    EntryPoint,
    /// Significant Exit Point from FRA volume.
    ExitPoint,
    /// Significant Intermediate Point / Connecting Point within FRA volume.
    IntermediatePoint,
    /// Combined Entry and Exit Point.
    EntryExitPoint,
}

/// Geometric and spatial boundary status for an FRA volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FraGeometryStatus {
    FullyModeled,
    MetadataOnly,
    FraKnownExistsGeometryUnavailable,
}
/// A significant navigation point designated for FRA operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FraPoint {
    pub ident: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub point_kind: FraPointKind,
    pub associated_fir: String,
    pub min_fl: Option<u32>,
    pub max_fl: Option<u32>,
    pub remarks: Option<String>,
}

/// Defined Free Route Airspace volume within an FIR / UIR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreeRouteAirspace {
    pub fra_ident: String,
    pub name: String,
    pub fir_ident: String,
    pub lower_fl: u32,
    pub upper_fl: u32,
    pub is_h24_active: bool,
    pub entry_points: Vec<FraPoint>,
    pub exit_points: Vec<FraPoint>,
    pub intermediate_points: Vec<FraPoint>,
    pub geometry_status: FraGeometryStatus,
    pub temporal: TemporalValidity,
}

impl FreeRouteAirspace {
    pub fn is_point_available_for_entry(&self, fix_ident: &str) -> bool {
        self.entry_points.iter().any(|p| {
            p.ident == fix_ident
                && (p.point_kind == FraPointKind::EntryPoint
                    || p.point_kind == FraPointKind::EntryExitPoint)
        })
    }

    pub fn is_point_available_for_exit(&self, fix_ident: &str) -> bool {
        self.exit_points.iter().any(|p| {
            p.ident == fix_ident
                && (p.point_kind == FraPointKind::ExitPoint
                    || p.point_kind == FraPointKind::EntryExitPoint)
        })
    }
}
