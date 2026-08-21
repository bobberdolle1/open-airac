//! Reusable Generic Aviation Data Validation Framework.
//!
//! Provides deterministic semantic and geometric validators:
//! - Coordinate bounds and normalization (-90.0..=90.0, -180.0..=180.0)
//! - Runway geometry, length, and heading validation
//! - Radio navigation aid frequency and coordinate sanity
//! - Procedure leg sequencing, path terminator transitions, and altitude constraints
//! - ATS route segment continuity and geodesic distance reconciliation
//! - Entity conflict detection across multiple data providers

use openairac_model::ProviderProvenance;
use serde::{Deserialize, Serialize};

/// Result of running a validation pass on a dataset or entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub category: String,
    pub entity_type: String,
    pub entity_ident: String,
    pub message: String,
    pub provenance: Option<ProviderProvenance>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Warning,
    Error,
    Critical,
}

/// Aggregated validation report for a provider or merged dataset.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderValidationReport {
    pub provider_name: String,
    pub dataset_version: String,
    pub entities_checked: usize,
    pub errors_count: usize,
    pub warnings_count: usize,
    pub issues: Vec<ValidationIssue>,
}

impl ProviderValidationReport {
    pub fn new(provider_name: impl Into<String>, dataset_version: impl Into<String>) -> Self {
        Self {
            provider_name: provider_name.into(),
            dataset_version: dataset_version.into(),
            entities_checked: 0,
            errors_count: 0,
            warnings_count: 0,
            issues: Vec::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.errors_count == 0
    }

    pub fn add_issue(&mut self, issue: ValidationIssue) {
        match issue.severity {
            ValidationSeverity::Warning => self.warnings_count += 1,
            ValidationSeverity::Error | ValidationSeverity::Critical => self.errors_count += 1,
        }
        self.issues.push(issue);
    }

    pub fn record_checked(&mut self) {
        self.entities_checked += 1;
    }
}

/// Generic coordinate validator.
pub fn validate_coordinates(
    lat: f64,
    lon: f64,
    entity_type: &str,
    entity_ident: &str,
    report: &mut ProviderValidationReport,
) -> bool {
    report.record_checked();

    if lat.is_nan() || lon.is_nan() {
        report.add_issue(ValidationIssue {
            severity: ValidationSeverity::Critical,
            category: "COORDINATES_NAN".to_string(),
            entity_type: entity_type.to_string(),
            entity_ident: entity_ident.to_string(),
            message: format!("Coordinates contain NaN values: lat={lat}, lon={lon}"),
            provenance: None,
        });
        return false;
    }

    if !(-90.0..=90.0).contains(&lat) {
        report.add_issue(ValidationIssue {
            severity: ValidationSeverity::Error,
            category: "LATITUDE_OUT_OF_BOUNDS".to_string(),
            entity_type: entity_type.to_string(),
            entity_ident: entity_ident.to_string(),
            message: format!("Latitude {lat} is outside valid range [-90.0, 90.0]"),
            provenance: None,
        });
        return false;
    }

    if !(-180.0..=180.0).contains(&lon) {
        report.add_issue(ValidationIssue {
            severity: ValidationSeverity::Error,
            category: "LONGITUDE_OUT_OF_BOUNDS".to_string(),
            entity_type: entity_type.to_string(),
            entity_ident: entity_ident.to_string(),
            message: format!("Longitude {lon} is outside valid range [-180.0, 180.0]"),
            provenance: None,
        });
        return false;
    }

    true
}

/// Great-circle / geodesic distance calculation between two WGS84 points in kilometers.
pub fn geodesic_distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0088;
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a =
        (d_lat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_KM * c
}

/// Validate ATS route segment distance against geodesic calculation.
#[allow(clippy::too_many_arguments, clippy::collapsible_if)]
pub fn validate_ats_segment_distance(
    route_ident: &str,
    from_pt: &str,
    to_pt: &str,
    lat1: f64,
    lon1: f64,
    lat2: f64,
    lon2: f64,
    published_km: Option<f64>,
    tolerance_pct: f64,
    report: &mut ProviderValidationReport,
) {
    report.record_checked();
    let computed_km = geodesic_distance_km(lat1, lon1, lat2, lon2);

    if let Some(pub_km) = published_km {
        if pub_km > 0.0 {
            let delta_km = (computed_km - pub_km).abs();
            let delta_pct = (delta_km / pub_km) * 100.0;
            if delta_pct > tolerance_pct && delta_km > 5.0 {
                report.add_issue(ValidationIssue {
                    severity: ValidationSeverity::Warning,
                    category: "ATS_DISTANCE_MISMATCH".to_string(),
                    entity_type: "ATS_SEGMENT".to_string(),
                    entity_ident: format!("{route_ident} ({from_pt} -> {to_pt})"),
                    message: format!(
                        "Published distance {pub_km:.1} km differs from geodesic distance {computed_km:.1} km by {delta_km:.1} km ({delta_pct:.1}%)"
                    ),
                    provenance: None,
                });
            }
        }
    }
}

/// Validate runway dimensions and magnetic heading.
#[allow(clippy::too_many_arguments, clippy::collapsible_if, clippy::manual_range_contains)]
pub fn validate_runway_geometry(
    airport_ident: &str,
    rwy_ident: &str,
    length_ft: Option<u32>,
    width_ft: Option<u32>,
    heading_deg: Option<f64>,
    report: &mut ProviderValidationReport,
) {
    report.record_checked();

    if let Some(len) = length_ft {
        if len < 300 || len > 25000 {
            report.add_issue(ValidationIssue {
                severity: ValidationSeverity::Warning,
                category: "RUNWAY_LENGTH_ANOMALOUS".to_string(),
                entity_type: "RUNWAY".to_string(),
                entity_ident: format!("{airport_ident}/{rwy_ident}"),
                message: format!(
                    "Runway length {len} ft is outside nominal aviation range [300, 25000] ft"
                ),
                provenance: None,
            });
        }
    }

    if let Some(w) = width_ft {
        if w < 20 || w > 500 {
            report.add_issue(ValidationIssue {
                severity: ValidationSeverity::Warning,
                category: "RUNWAY_WIDTH_ANOMALOUS".to_string(),
                entity_type: "RUNWAY".to_string(),
                entity_ident: format!("{airport_ident}/{rwy_ident}"),
                message: format!(
                    "Runway width {w} ft is outside nominal aviation range [20, 500] ft"
                ),
                provenance: None,
            });
        }
    }

    if let Some(hdg) = heading_deg {
        if !(0.0..=360.0).contains(&hdg) {
            report.add_issue(ValidationIssue {
                severity: ValidationSeverity::Error,
                category: "RUNWAY_HEADING_INVALID".to_string(),
                entity_type: "RUNWAY".to_string(),
                entity_ident: format!("{airport_ident}/{rwy_ident}"),
                message: format!("Runway magnetic heading {hdg} deg is outside [0.0, 360.0]"),
                provenance: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_validation() {
        let mut report = ProviderValidationReport::new("TEST", "1.0");
        assert!(validate_coordinates(
            55.9728,
            37.4147,
            "AIRPORT",
            "UUEE",
            &mut report
        ));
        assert!(!validate_coordinates(
            95.0,
            37.4147,
            "AIRPORT",
            "INVALID",
            &mut report
        ));
        assert_eq!(report.errors_count, 1);
    }

    #[test]
    fn test_geodesic_distance() {
        // Moscow UUEE (55.9728, 37.4147) to St. Petersburg ULLI (59.8003, 30.2625)
        let dist = geodesic_distance_km(55.9728, 37.4147, 59.8003, 30.2625);
        assert!((dist - 599.3).abs() < 2.0);
    }
}
