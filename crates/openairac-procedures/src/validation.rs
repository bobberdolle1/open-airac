//! Worldwide Procedure Semantic and Geometric Validation Layer.
//!
//! Provides comprehensive validation of aeronautical instrument procedures (SIDs, STARs, Approaches):
//! - Fix resolution & coordinate integrity
//! - Sequence continuity & duplicate sequence detection
//! - Runway binding & association verification
//! - Leg transition chaining (initial fix, terminators, hold/arc parameters)
//! - Altitude and speed constraint monotonicity and physical bounds
//! - Geometric discontinuity detection (excessive inter-leg distance jumps / coordinate corruption)
use crate::{AltitudeConstraint, PathTerminator, Procedure, ProcedureKind, ProcedureLeg};
use serde::{Deserialize, Serialize};
/// Severity level of a procedure validation issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IssueSeverity {
    /// Non-fatal advisory or minor formatting anomaly.
    Warning,
    /// Severe defect that prevents safe automated navigation or flight-plan rendering.
    Error,
}

impl IssueSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
        }
    }
}

/// Category of validation issue detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueCategory {
    UnresolvedFix,
    SequenceDiscontinuity,
    UnboundRunway,
    InvalidLegGeometry,
    InvalidConstraint,
    GeometricDiscontinuity,
    UnsupportedTerminator,
}

/// A specific issue found during procedure validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureIssue {
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    pub transition_ident: String,
    pub sequence_number: Option<u32>,
    pub fix_ident: Option<String>,
    pub message: String,
}

/// Full validation report for an instrument procedure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcedureValidationReport {
    pub procedure_ident: String,
    pub airport_ident: String,
    pub kind: String,
    pub total_legs: usize,
    pub total_transitions: usize,
    pub issues: Vec<ProcedureIssue>,
    /// Whether the procedure is structurally flyable (contains 0 Error-severity issues).
    pub is_flyable: bool,
}

impl ProcedureValidationReport {
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }
}

pub type FixLookupFn<'a> = Box<dyn Fn(&str) -> Option<(f64, f64)> + 'a>;
pub type RunwayLookupFn<'a> = Box<dyn Fn(&str, &str) -> bool + 'a>;

/// Comprehensive Worldwide Procedure Validator.
pub struct ProcedureValidator<'a> {
    fix_lookup: FixLookupFn<'a>,
    runway_lookup: Option<RunwayLookupFn<'a>>,
}

impl<'a> ProcedureValidator<'a> {
    pub fn new<F>(fix_lookup: F) -> Self
    where
        F: Fn(&str) -> Option<(f64, f64)> + 'a,
    {
        Self {
            fix_lookup: Box::new(fix_lookup),
            runway_lookup: None,
        }
    }

    pub fn with_runway_lookup<R>(mut self, runway_lookup: R) -> Self
    where
        R: Fn(&str, &str) -> bool + 'a,
    {
        self.runway_lookup = Some(Box::new(runway_lookup));
        self
    }

    /// Validate a canonical procedure and generate a detailed report.
    pub fn validate_procedure(&self, procedure: &Procedure) -> ProcedureValidationReport {
        let mut issues = Vec::new();
        let total_legs = procedure.main_legs.len()
            + procedure
                .transitions
                .iter()
                .map(|t| t.legs.len())
                .sum::<usize>();

        // 1. Validate Main Legs
        self.validate_leg_chain("MAIN", &procedure.main_legs, procedure, &mut issues);

        // 2. Validate Each Transition
        for trans in &procedure.transitions {
            self.validate_leg_chain(&trans.transition_ident, &trans.legs, procedure, &mut issues);

            // Check runway binding if transition ident is a runway (e.g. RW07R, RW25L)
            if trans.transition_ident.starts_with("RW") || trans.transition_ident.starts_with("RNW")
            {
                let rwy_desig = trans
                    .transition_ident
                    .trim_start_matches("RNW")
                    .trim_start_matches("RW");
                if let Some(rwy_check) = &self.runway_lookup
                    && !rwy_check(&procedure.airport_ident, rwy_desig)
                {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::UnboundRunway,
                        transition_ident: trans.transition_ident.clone(),
                        sequence_number: None,
                        fix_ident: None,
                        message: format!(
                            "Runway transition '{}' references runway '{}' not published at airport '{}'",
                            trans.transition_ident, rwy_desig, procedure.airport_ident
                        ),
                    });
                }
            }
        }

        let is_flyable = !issues.iter().any(|i| i.severity == IssueSeverity::Error);

        ProcedureValidationReport {
            procedure_ident: procedure.name.clone(),
            airport_ident: procedure.airport_ident.clone(),
            kind: procedure.kind.as_str().to_string(),
            total_legs,
            total_transitions: procedure.transitions.len(),
            issues,
            is_flyable,
        }
    }

    fn validate_leg_chain(
        &self,
        trans_label: &str,
        legs: &[ProcedureLeg],
        proc: &Procedure,
        issues: &mut Vec<ProcedureIssue>,
    ) {
        if legs.is_empty() {
            return;
        }

        let mut prev_seq: Option<u32> = None;
        let mut prev_coords: Option<(f64, f64)> = None;
        let mut prev_alt: Option<u32> = None;

        for leg in legs {
            let fix = &leg.fix_ident;

            // 1. Sequence checks
            if let Some(prev) = prev_seq {
                if leg.sequence_number <= prev {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Error,
                        category: IssueCategory::SequenceDiscontinuity,
                        transition_ident: trans_label.to_string(),
                        sequence_number: Some(leg.sequence_number),
                        fix_ident: Some(fix.clone()),
                        message: format!(
                            "Sequence number {} is not strictly greater than preceding sequence {}",
                            leg.sequence_number, prev
                        ),
                    });
                } else if leg.sequence_number - prev > 100 {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::SequenceDiscontinuity,
                        transition_ident: trans_label.to_string(),
                        sequence_number: Some(leg.sequence_number),
                        fix_ident: Some(fix.clone()),
                        message: format!(
                            "Abnormally large sequence number gap: {} -> {}",
                            prev, leg.sequence_number
                        ),
                    });
                }
            }
            prev_seq = Some(leg.sequence_number);

            // 2. Fix resolution
            let coords = leg
                .fix_latitude
                .zip(leg.fix_longitude)
                .or_else(|| (self.fix_lookup)(fix));
            if coords.is_none() && !leg.path_terminator.requires_navaid() {
                // Fix cannot be resolved
                issues.push(ProcedureIssue {
                    severity: IssueSeverity::Error,
                    category: IssueCategory::UnresolvedFix,
                    transition_ident: trans_label.to_string(),
                    sequence_number: Some(leg.sequence_number),
                    fix_ident: Some(fix.clone()),
                    message: format!(
                        "Fix '{}' in leg {} ({}) cannot be resolved to spatial coordinates",
                        fix,
                        leg.sequence_number,
                        leg.path_terminator.as_str()
                    ),
                });
            }

            // 3. Path terminator geometry & requirements
            match &leg.path_terminator {
                PathTerminator::RF => {
                    if leg.arc_radius_nm.is_none() || leg.arc_radius_nm.unwrap_or(0.0) <= 0.0 {
                        issues.push(ProcedureIssue {
                            severity: IssueSeverity::Error,
                            category: IssueCategory::InvalidLegGeometry,
                            transition_ident: trans_label.to_string(),
                            sequence_number: Some(leg.sequence_number),
                            fix_ident: Some(fix.clone()),
                            message: format!(
                                "RF leg {} missing positive arc radius",
                                leg.sequence_number
                            ),
                        });
                    }
                    if leg.turn_direction.is_none() {
                        issues.push(ProcedureIssue {
                            severity: IssueSeverity::Error,
                            category: IssueCategory::InvalidLegGeometry,
                            transition_ident: trans_label.to_string(),
                            sequence_number: Some(leg.sequence_number),
                            fix_ident: Some(fix.clone()),
                            message: format!(
                                "RF leg {} missing mandatory turn direction ('L' or 'R')",
                                leg.sequence_number
                            ),
                        });
                    }
                }
                PathTerminator::HA | PathTerminator::HF | PathTerminator::HM => {
                    if leg.true_track_deg.is_none() {
                        issues.push(ProcedureIssue {
                            severity: IssueSeverity::Warning,
                            category: IssueCategory::InvalidLegGeometry,
                            transition_ident: trans_label.to_string(),
                            sequence_number: Some(leg.sequence_number),
                            fix_ident: Some(fix.clone()),
                            message: format!(
                                "Hold leg {} missing inbound course track",
                                leg.sequence_number
                            ),
                        });
                    }
                }
                PathTerminator::Unsupported(s) => {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::UnsupportedTerminator,
                        transition_ident: trans_label.to_string(),
                        sequence_number: Some(leg.sequence_number),
                        fix_ident: Some(fix.clone()),
                        message: format!(
                            "Unsupported ARINC path terminator '{}' in leg {}",
                            s, leg.sequence_number
                        ),
                    });
                }
                _ => {}
            }

            // 4. Altitude & Speed Constraints
            if let Some(alt_c) = &leg.altitude_constraint {
                let current_alt = match alt_c {
                    AltitudeConstraint::At(a) => Some(*a),
                    AltitudeConstraint::AtOrAbove(a) => Some(*a),
                    AltitudeConstraint::AtOrBelow(a) => Some(*a),
                    AltitudeConstraint::Between(a1, a2) => {
                        if a1 >= a2 {
                            issues.push(ProcedureIssue {
                                severity: IssueSeverity::Error,
                                category: IssueCategory::InvalidConstraint,
                                transition_ident: trans_label.to_string(),
                                sequence_number: Some(leg.sequence_number),
                                fix_ident: Some(fix.clone()),
                                message: format!(
                                    "Inverted altitude window {}..{} in leg {}",
                                    a1, a2, leg.sequence_number
                                ),
                            });
                        }
                        Some(*a1)
                    }
                };

                // Check altitude profile gradient for SIDs / STARs
                if let (Some(prev), Some(curr)) = (prev_alt, current_alt) {
                    if proc.kind == ProcedureKind::Sid && curr + 500 < prev {
                        // SIDs should generally climb
                        issues.push(ProcedureIssue {
                            severity: IssueSeverity::Warning,
                            category: IssueCategory::InvalidConstraint,
                            transition_ident: trans_label.to_string(),
                            sequence_number: Some(leg.sequence_number),
                            fix_ident: Some(fix.clone()),
                            message: format!(
                                "Descending altitude constraint in SID profile: {} ft -> {} ft at leg {}",
                                prev, curr, leg.sequence_number
                            ),
                        });
                    } else if (proc.kind == ProcedureKind::Star
                        || proc.kind == ProcedureKind::Approach)
                        && curr > prev + 500
                    {
                        // STARs and Approaches should generally descend
                        issues.push(ProcedureIssue {
                            severity: IssueSeverity::Warning,
                            category: IssueCategory::InvalidConstraint,
                            transition_ident: trans_label.to_string(),
                            sequence_number: Some(leg.sequence_number),
                            fix_ident: Some(fix.clone()),
                            message: format!(
                                "Climbing altitude constraint in arrival/approach profile: {} ft -> {} ft at leg {}",
                                prev, curr, leg.sequence_number
                            ),
                        });
                    }
                }
                prev_alt = current_alt;
            }

            if let Some(spd_c) = &leg.speed_constraint {
                let spd = match spd_c {
                    crate::SpeedConstraint::At(s) => *s,
                    crate::SpeedConstraint::AtOrBelow(s) => *s,
                };
                if !(60..=400).contains(&spd) {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Warning,
                        category: IssueCategory::InvalidConstraint,
                        transition_ident: trans_label.to_string(),
                        sequence_number: Some(leg.sequence_number),
                        fix_ident: Some(fix.clone()),
                        message: format!(
                            "Aeronautically unrealistic speed constraint: {} kts at leg {}",
                            spd, leg.sequence_number
                        ),
                    });
                }
            }

            // 5. Geometric Discontinuity (excessive jump > 250 NM in terminal procedure)
            if let (Some(prev_pt), Some(curr_pt)) = (prev_coords, coords) {
                let dist_nm = great_circle_distance_nm(prev_pt.0, prev_pt.1, curr_pt.0, curr_pt.1);
                if dist_nm > 250.0 {
                    issues.push(ProcedureIssue {
                        severity: IssueSeverity::Error,
                        category: IssueCategory::GeometricDiscontinuity,
                        transition_ident: trans_label.to_string(),
                        sequence_number: Some(leg.sequence_number),
                        fix_ident: Some(fix.clone()),
                        message: format!(
                            "Severe geometric jump between consecutive legs: {:.1} NM (> 250 NM) at leg {}",
                            dist_nm, leg.sequence_number
                        ),
                    });
                }
            }
            if coords.is_some() {
                prev_coords = coords;
            }
        }
    }
}

/// Great circle distance in nautical miles between two coordinates.
fn great_circle_distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let a =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    let earth_radius_nm = 3440.065;
    earth_radius_nm * c
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{
        CanonicalProcedureLeg, ProcedureLegId, SourceSnapshotId, TemporalValidity,
    };
    fn test_leg(
        seq: u32,
        term: &str,
        fix: &str,
        alt: Option<u32>,
        valid_from: chrono::DateTime<chrono::Utc>,
    ) -> CanonicalProcedureLeg {
        CanonicalProcedureLeg {
            object_id: ProcedureLegId(format!("leg-{seq}")),
            airport_ident: "EDDF".to_string(),
            icao_code: "ED".to_string(),
            procedure_kind: 'D',
            procedure_ident: "RIDSU1A".to_string(),
            route_type: String::new(),
            transition_ident: String::new(),
            sequence_number: seq,
            fix_ident: fix.to_string(),
            fix_icao_code: "ED".to_string(),
            fix_section: "EA".to_string(),
            waypoint_description: "E   ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: term.to_string(),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: None,
            distance_a_nm: None,
            course_b_deg: None,
            distance_b_nm: None,
            altitude_descriptor: alt.map(|_| '+'),
            altitude_1_ft: alt,
            altitude_2_ft: None,
            speed_limit_kts: Some(220),
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: String::new(),
            temporal: TemporalValidity {
                valid_from,
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
            },
        }
    }

    #[test]
    fn test_procedure_validator_healthy() {
        let now = chrono::Utc::now();
        let legs = vec![
            test_leg(10, "IF", "DF401", Some(3000), now),
            test_leg(20, "TF", "RIDSU", Some(5000), now),
        ];
        let proc = Procedure::assemble("EDDF", ProcedureKind::Sid, "RIDSU1A", legs, |f| match f {
            "DF401" => Some((50.05, 8.65)),
            "RIDSU" => Some((50.15, 8.90)),
            _ => None,
        })
        .unwrap();

        let validator = ProcedureValidator::new(|f| match f {
            "DF401" => Some((50.05, 8.65)),
            "RIDSU" => Some((50.15, 8.90)),
            _ => None,
        });

        let report = validator.validate_procedure(&proc);
        assert!(report.is_flyable);
        assert_eq!(report.error_count(), 0);
    }

    #[test]
    fn test_procedure_validator_catches_unresolved_fix_and_discontinuity() {
        let now = chrono::Utc::now();
        let legs = vec![
            test_leg(10, "IF", "DF401", Some(3000), now),
            test_leg(20, "TF", "UNKNOWN_FIX", Some(5000), now),
            test_leg(30, "TF", "FAR_AWAY_FIX", Some(7000), now),
        ];
        let proc = Procedure::assemble("EDDF", ProcedureKind::Sid, "RIDSU1A", legs, |f| {
            match f {
                "DF401" => Some((50.05, 8.65)),
                "FAR_AWAY_FIX" => Some((25.0, 55.0)), // Jump from Frankfurt to Dubai (> 2500 NM)
                _ => None,
            }
        })
        .unwrap();

        let validator = ProcedureValidator::new(|f| match f {
            "DF401" => Some((50.05, 8.65)),
            "FAR_AWAY_FIX" => Some((25.0, 55.0)),
            _ => None,
        });

        let report = validator.validate_procedure(&proc);
        assert!(!report.is_flyable);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.category == IssueCategory::UnresolvedFix)
        );
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.category == IssueCategory::GeometricDiscontinuity)
        );
    }
}
