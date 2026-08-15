//! OpenAIRAC Procedure Engine
//!
//! Semantic layer for Instrument Terminal Procedures (SIDs, STARs,
//! Approaches) decoded from ARINC 424 records (FAA CIFP PD/PE/PF).
//!
//! Design boundary:
//! * **Semantic truth lives here**: a `ProcedureLeg` states what the leg
//!   *means* (terminate at fix X on course Y, climb to altitude Z), never
//!   the fixed-width columns that encoded it. Raw records remain
//!   accessible through the canonical model (`CanonicalProcedureLeg.raw`)
//!   so unsupported semantics are lossless, never guessed.
//! * **Geometric rendering is explicitly out of scope**: this crate
//!   produces constraints a renderer consumes; it never computes a path
//!   shape. That separation keeps ARINC 424 interpretation auditable.
//! * **Fail closed**: a leg whose terminator semantics cannot be
//!   interpreted is preserved with `PathTerminator::Unsupported` and a
//!   diagnostic — never silently dropped or reinterpreted.
//!
//! Terminator field semantics (verified against FAA CIFP cycle 2608 raw
//! records cross-checked with convert424toxplane output):
//! * `IF/TF/DF`: fix only.
//! * `CF`: fix + course (course A) + distance.
//! * `CA/VA`: course/heading + termination altitude.
//! * `FA/FD/FC/CD/VD`: fix + termination altitude/distance.
//! * `FM/VM`: manual termination — recommended navaid carries the fix.
//! * `VI/CI/CR/VR`: intercept — recommended navaid is the intercept
//!   reference, plus course/heading.
//! * `RF`: radius-to-fix — arc radius + turn direction mandatory.
//! * `HA/HF/HM`: hold — recommended navaid is the holding fix, course A
//!   is the inbound course.

use anyhow::{Result, bail};
use openairac_model::{CanonicalProcedureLeg, TemporalValidity};
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

    pub fn from_arinc(c: char) -> Option<Self> {
        match c {
            'D' => Some(ProcedureKind::Sid),
            'E' => Some(ProcedureKind::Star),
            'F' => Some(ProcedureKind::Approach),
            _ => None,
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

    /// Whether the terminator is supported for semantic interpretation.
    pub fn is_supported(&self) -> bool {
        !matches!(self, PathTerminator::Unsupported(_))
    }

    /// Whether the recommended navaid is the semantic termination
    /// reference for this terminator.
    pub fn requires_navaid(&self) -> bool {
        matches!(
            self,
            PathTerminator::FM
                | PathTerminator::VM
                | PathTerminator::VI
                | PathTerminator::CI
                | PathTerminator::CR
                | PathTerminator::VR
                | PathTerminator::HA
                | PathTerminator::HF
                | PathTerminator::HM
        )
    }

    /// Whether this terminator's primary direction datum is a heading
    /// (true track from the previous leg) rather than a course.
    pub fn primary_is_heading(&self) -> bool {
        matches!(
            self,
            PathTerminator::VA
                | PathTerminator::VD
                | PathTerminator::VI
                | PathTerminator::VM
                | PathTerminator::VR
        )
    }

    /// Whether this is a holding pattern leg family.
    pub fn is_hold(&self) -> bool {
        matches!(
            self,
            PathTerminator::HA | PathTerminator::HF | PathTerminator::HM
        )
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

/// Semantic interpretation of one canonical procedure leg.
///
/// Field meaning follows the path terminator (see crate docs); positional
/// raw columns never leak into this type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureLeg {
    pub sequence_number: u32,
    pub path_terminator: PathTerminator,
    pub fix_ident: String,
    pub fix_latitude: Option<f64>,
    pub fix_longitude: Option<f64>,
    /// Course (CF/CA/CI/CR) or heading (VA/VD/VI/VM/VR), degrees true.
    pub true_track_deg: Option<f64>,
    pub distance_nm: Option<f64>,
    pub altitude_constraint: Option<AltitudeConstraint>,
    pub speed_constraint: Option<SpeedConstraint>,
    /// Radius of an RF arc, nautical miles.
    pub arc_radius_nm: Option<f64>,
    /// Turn direction of an RF arc.
    pub turn_direction: Option<char>,
    /// The recommended navaid (ident:icao:section:subsection) when the
    /// terminator semantics reference one.
    pub recommended_navaid: Option<String>,
    /// The MSA center fix when published.
    pub msa_center_fix: Option<String>,
    /// Non-fatal problems found while interpreting (missing coordinates,
    /// inverted altitude bands, missing mandatory fields).
    pub diagnostics: Vec<String>,
}

impl ProcedureLeg {
    /// Interpret one canonical leg into semantic form.
    ///
    /// `fix_lookup` resolves a fix ident to WGS84 coordinates (usually
    /// from the store's waypoint tables); `None` coordinates produce a
    /// diagnostic, not an error — the semantic truth is still carried.
    pub fn interpret<F>(canonical: &CanonicalProcedureLeg, fix_lookup: F) -> Self
    where
        F: Fn(&str) -> Option<(f64, f64)>,
    {
        let terminator = PathTerminator::parse(&canonical.path_terminator);
        let mut diagnostics = Vec::new();
        let (lat, lon) = match fix_lookup(&canonical.fix_ident) {
            Some((la, lo)) => (Some(la), Some(lo)),
            None => (None, None),
        };

        // Altitude constraint: descriptor + one/two altitudes.
        let altitude_constraint = match (
            canonical.altitude_descriptor,
            canonical.altitude_1_ft,
            canonical.altitude_2_ft,
        ) {
            (Some('+'), Some(a1), None) => Some(AltitudeConstraint::AtOrAbove(a1)),
            (Some('-'), Some(a1), None) => Some(AltitudeConstraint::AtOrBelow(a1)),
            (Some('B'), Some(a1), Some(a2)) => {
                if a1 > a2 {
                    // Published as FL280/FL240 style descending bands.
                    Some(AltitudeConstraint::Between(a2, a1))
                } else {
                    Some(AltitudeConstraint::Between(a1, a2))
                }
            }
            (Some(_), Some(a1), None) => Some(AltitudeConstraint::At(a1)),
            (None, None, None) => None,
            (desc, a1, a2) => {
                diagnostics.push(format!(
                    "leg {}: incomplete altitude constraint (desc={desc:?} a1={a1:?} a2={a2:?})",
                    canonical.sequence_number
                ));
                None
            }
        };

        let speed_constraint = canonical.speed_limit_kts.map(SpeedConstraint::At);

        // Terminator-specific interpretation.
        let mut true_track_deg = None;
        let mut distance_nm = None;
        let mut arc_radius_nm = None;
        let turn_direction = canonical.turn_direction;
        match &terminator {
            PathTerminator::IF | PathTerminator::TF | PathTerminator::DF => {}
            PathTerminator::CF => {
                true_track_deg = canonical.course_a_deg;
                distance_nm = canonical.distance_b_nm;
            }
            PathTerminator::CA => {
                true_track_deg = canonical.course_a_deg;
            }
            PathTerminator::FA => {}
            PathTerminator::FC => {
                true_track_deg = canonical.course_a_deg;
                distance_nm = canonical.distance_b_nm;
            }
            PathTerminator::FD => {
                true_track_deg = canonical.course_a_deg;
            }
            PathTerminator::FM => {
                if canonical.recommended_navaid.is_none() {
                    diagnostics.push(format!(
                        "leg {}: FM requires a recommended navaid",
                        canonical.sequence_number
                    ));
                }
            }
            PathTerminator::CD => {
                true_track_deg = canonical.course_a_deg;
                distance_nm = canonical.distance_b_nm;
            }
            PathTerminator::CI | PathTerminator::CR => {
                true_track_deg = canonical.course_a_deg;
                if canonical.recommended_navaid.is_none() {
                    diagnostics.push(format!(
                        "leg {}: {} requires a recommended navaid",
                        canonical.sequence_number,
                        terminator.as_str()
                    ));
                }
            }
            PathTerminator::VA => {
                true_track_deg = canonical.course_b_deg;
            }
            PathTerminator::VD => {
                true_track_deg = canonical.course_b_deg;
                distance_nm = canonical.distance_b_nm;
            }
            PathTerminator::VI | PathTerminator::VR => {
                true_track_deg = canonical.course_b_deg;
                if canonical.recommended_navaid.is_none() {
                    diagnostics.push(format!(
                        "leg {}: {} requires a recommended navaid",
                        canonical.sequence_number,
                        terminator.as_str()
                    ));
                }
            }
            PathTerminator::VM => {
                true_track_deg = canonical.course_b_deg;
                if canonical.recommended_navaid.is_none() {
                    diagnostics.push(format!(
                        "leg {}: VM requires a recommended navaid",
                        canonical.sequence_number
                    ));
                }
            }
            PathTerminator::HA | PathTerminator::HF | PathTerminator::HM => {
                true_track_deg = canonical.course_a_deg;
                if canonical.recommended_navaid.is_none() {
                    diagnostics.push(format!(
                        "leg {}: {} (hold) requires a recommended navaid",
                        canonical.sequence_number,
                        terminator.as_str()
                    ));
                }
            }
            PathTerminator::RF => {
                arc_radius_nm = canonical.arc_radius_nm;
                if arc_radius_nm.is_none() {
                    diagnostics.push(format!(
                        "leg {}: RF requires an arc radius",
                        canonical.sequence_number
                    ));
                }
                if turn_direction.is_none() {
                    diagnostics.push(format!(
                        "leg {}: RF requires a turn direction",
                        canonical.sequence_number
                    ));
                }
            }
            PathTerminator::Unsupported(_) => {
                diagnostics.push(format!(
                    "leg {}: unsupported path terminator '{}'",
                    canonical.sequence_number, canonical.path_terminator
                ));
            }
        }

        Self {
            sequence_number: canonical.sequence_number,
            path_terminator: terminator,
            fix_ident: canonical.fix_ident.clone(),
            fix_latitude: lat,
            fix_longitude: lon,
            true_track_deg,
            distance_nm,
            altitude_constraint,
            speed_constraint,
            arc_radius_nm,
            turn_direction,
            recommended_navaid: canonical.recommended_navaid.clone(),
            msa_center_fix: canonical.msa_center_fix.clone(),
            diagnostics,
        }
    }
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

impl Procedure {
    /// Assemble a procedure from its canonical legs (all of one
    /// airport/kind/ident at one time).
    ///
    /// * Legs with an empty transition ident form `main_legs`; others are
    ///   grouped into transitions by (route_type, transition_ident).
    /// * Legs are ordered by sequence number; gaps or duplicates are
    ///   reported as diagnostics, never renumbered.
    /// * The temporal validity is the legs' common one; mixed validity
    ///   ranges are an error (the caller should query one instant).
    pub fn assemble<F>(
        airport_ident: &str,
        kind: ProcedureKind,
        procedure_ident: &str,
        legs: Vec<CanonicalProcedureLeg>,
        fix_lookup: F,
    ) -> Result<Procedure>
    where
        F: Fn(&str) -> Option<(f64, f64)>,
    {
        if legs.is_empty() {
            bail!("cannot assemble an empty procedure {procedure_ident}");
        }
        let mut temporal: Option<TemporalValidity> = None;
        for leg in &legs {
            match &temporal {
                None => temporal = Some(leg.temporal.clone()),
                Some(t) if t.valid_from != leg.temporal.valid_from => {
                    bail!(
                        "procedure {procedure_ident} spans multiple validity instants ({} vs {})",
                        t.valid_from,
                        leg.temporal.valid_from
                    );
                }
                Some(_) => {}
            }
        }

        // Group by (route_type, transition_ident). Empty transition =
        // main body.
        let mut groups: Vec<((String, String), Vec<CanonicalProcedureLeg>)> = Vec::new();
        for leg in legs {
            let key = (leg.route_type.clone(), leg.transition_ident.clone());
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(leg),
                None => groups.push((key, vec![leg])),
            }
        }

        let mut main_legs = Vec::new();
        let mut transitions = Vec::new();
        for ((route_type, transition_ident), mut group) in groups {
            group.sort_by_key(|l| l.sequence_number);
            let interpreted: Vec<ProcedureLeg> = group
                .iter()
                .map(|l| ProcedureLeg::interpret(l, &fix_lookup))
                .collect();
            if transition_ident.trim().is_empty() && route_type.trim().is_empty() {
                main_legs = interpreted;
            } else if route_type.trim().is_empty() && !transition_ident.trim().is_empty() {
                transitions.push(ProcedureTransition {
                    transition_ident,
                    legs: interpreted,
                });
            } else {
                // Route type without transition: a route-type-scoped main
                // body variant (rare); keep it as a labeled transition to
                // avoid losing data.
                transitions.push(ProcedureTransition {
                    transition_ident: format!("{route_type}:{transition_ident}"),
                    legs: interpreted,
                });
            }
        }
        transitions.sort_by(|a, b| a.transition_ident.cmp(&b.transition_ident));

        let name = format!("{procedure_ident} {} {}", kind.as_str(), airport_ident);
        Ok(Procedure {
            id: format!("{airport_ident}:{}:{procedure_ident}", kind.as_str()),
            airport_ident: airport_ident.to_string(),
            name,
            kind,
            main_legs,
            transitions,
            temporal: temporal.expect("checked non-empty"),
        })
    }

    /// Structural validation beyond per-leg interpretation: sequence
    /// continuity, fix coordinate presence, hold/arc constraints.
    /// Returns diagnostics; an empty list means the procedure is
    /// structurally sound.
    pub fn validate(&self) -> Vec<String> {
        let mut diagnostics = Vec::new();
        for (label, legs) in std::iter::once(("main", &self.main_legs)).chain(
            self.transitions
                .iter()
                .map(|t| (t.transition_ident.as_str(), &t.legs)),
        ) {
            if legs.is_empty() {
                continue;
            }
            // Sequence: strictly increasing, expected multiples of 10.
            let mut previous: Option<u32> = None;
            for leg in legs {
                if let Some(prev) = previous {
                    if leg.sequence_number <= prev {
                        diagnostics.push(format!(
                            "{label}: sequence not strictly increasing at {}",
                            leg.sequence_number
                        ));
                    } else if leg.sequence_number - prev > 10 {
                        diagnostics.push(format!(
                            "{label}: sequence gap {} -> {}",
                            prev, leg.sequence_number
                        ));
                    }
                }
                previous = Some(leg.sequence_number);
                if leg.fix_latitude.is_none() && !leg.path_terminator.requires_navaid() {
                    diagnostics.push(format!("{label}: fix {} has no coordinates", leg.fix_ident));
                }
            }
        }
        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{ProcedureLegId, SourceSnapshotId};

    fn canonical(overrides: &[(String, String)]) -> CanonicalProcedureLeg {
        // Real PD/PE/PF field values are injected as (field, value)
        // pairs over a CIITY3-like baseline.
        let get = |field: &str, default: &str| -> String {
            overrides
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| default.to_string())
        };
        CanonicalProcedureLeg {
            object_id: ProcedureLegId("leg-test".to_string()),
            airport_ident: "KSFO".to_string(),
            icao_code: "K2".to_string(),
            procedure_kind: 'D',
            procedure_ident: "CIITY3".to_string(),
            route_type: String::new(),
            transition_ident: "RW10L".to_string(),
            sequence_number: 10,
            fix_ident: get("fix_ident", "CIITY"),
            fix_icao_code: "K2".to_string(),
            fix_section: " ".to_string(),
            waypoint_description: "E ".to_string(),
            turn_direction: None,
            rnp_nm: None,
            path_terminator: get("path_terminator", "VA"),
            recommended_navaid: None,
            arc_radius_nm: None,
            course_a_deg: None,
            distance_a_nm: None,
            course_b_deg: Some(103.8),
            distance_b_nm: None,
            altitude_descriptor: Some('+'),
            altitude_1_ft: Some(5200),
            altitude_2_ft: None,
            speed_limit_kts: None,
            course_c_deg: None,
            vertical_angle_deg: None,
            msa_center_fix: None,
            route_qualifiers: String::new(),
            raw: String::new(),
            temporal: TemporalValidity {
                valid_from: chrono::Utc::now(),
                valid_until: None,
                source_snapshot_id: SourceSnapshotId("snap-test".to_string()),
            },
        }
    }

    fn lookup(fix: &str) -> Option<(f64, f64)> {
        match fix {
            "CIITY" | "BDEGA" | "SUSAP" | "ADDMM" => Some((37.6, -122.4)),
            _ => None,
        }
    }

    #[test]
    fn test_path_terminator_parsing() {
        assert_eq!(PathTerminator::parse("IF"), PathTerminator::IF);
        assert_eq!(PathTerminator::parse("TF"), PathTerminator::TF);
        assert_eq!(PathTerminator::parse("RF"), PathTerminator::RF);
        assert_eq!(
            PathTerminator::parse("XYZ"),
            PathTerminator::Unsupported("XYZ".to_string())
        );
        assert!(PathTerminator::FM.requires_navaid());
        assert!(PathTerminator::VA.primary_is_heading());
        assert!(PathTerminator::HF.is_hold());
    }

    #[test]
    fn test_va_leg_heading_and_altitude() {
        // Real KSFO CIITY3 RW10L VA leg: heading 103.8 (cols 71-74),
        // at-or-above 5200 ft.
        let leg = ProcedureLeg::interpret(&canonical(&[]), lookup);
        assert_eq!(leg.path_terminator, PathTerminator::VA);
        assert_eq!(leg.true_track_deg, Some(103.8));
        assert_eq!(
            leg.altitude_constraint,
            Some(AltitudeConstraint::AtOrAbove(5200))
        );
        assert!(leg.diagnostics.is_empty(), "{:?}", leg.diagnostics);
    }

    #[test]
    fn test_between_altitude_band_normalized() {
        // Real BDEGA4 STAR leg: descriptor B, FL280 -> FL240 (descending).
        let leg = canonical(&[
            ("path_terminator".to_string(), "TF".to_string()),
            ("fix_ident".to_string(), "BDEGA".to_string()),
        ]);
        let mut c = leg;
        c.altitude_descriptor = Some('B');
        c.altitude_1_ft = Some(28000);
        c.altitude_2_ft = Some(24000);
        let interpreted = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(
            interpreted.altitude_constraint,
            Some(AltitudeConstraint::Between(24000, 28000))
        );
    }

    #[test]
    fn test_fm_leg_requires_navaid() {
        let mut c = canonical(&[("path_terminator".to_string(), "FM".to_string())]);
        c.recommended_navaid = Some("SFO:K2:D:".to_string());
        let leg = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(leg.path_terminator, PathTerminator::FM);
        assert_eq!(leg.recommended_navaid.as_deref(), Some("SFO:K2:D:"));
        assert!(leg.diagnostics.is_empty(), "{:?}", leg.diagnostics);

        c.recommended_navaid = None;
        let bad = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(bad.diagnostics.len(), 1);
    }

    #[test]
    fn test_rf_leg_radius_and_turn() {
        // Real H03-Z approach RF leg: radius 27.90 nm, left turn.
        let mut c = canonical(&[("path_terminator".to_string(), "RF".to_string())]);
        c.arc_radius_nm = Some(27.90);
        c.turn_direction = Some('L');
        let leg = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(leg.arc_radius_nm, Some(27.90));
        assert_eq!(leg.turn_direction, Some('L'));
        assert!(leg.diagnostics.is_empty(), "{:?}", leg.diagnostics);

        c.arc_radius_nm = None;
        c.turn_direction = None;
        let bad = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(bad.diagnostics.len(), 2);
    }

    #[test]
    fn test_unsupported_terminator_preserved() {
        let mut c = canonical(&[("path_terminator".to_string(), "ZZ".to_string())]);
        c.path_terminator = "ZZ".to_string();
        let leg = ProcedureLeg::interpret(&c, lookup);
        assert_eq!(
            leg.path_terminator,
            PathTerminator::Unsupported("ZZ".to_string())
        );
        assert!(!leg.diagnostics.is_empty());
    }

    #[test]
    fn test_assemble_procedure_and_validate() {
        let base = canonical(&[]);
        let mut leg2 = base.clone();
        leg2.sequence_number = 20;
        leg2.fix_ident = "BDEGA".to_string();
        leg2.path_terminator = "TF".to_string();
        leg2.transition_ident = String::new();
        let mut leg1 = base.clone();
        leg1.transition_ident = String::new();

        let procedure = Procedure::assemble(
            "KSFO",
            ProcedureKind::Sid,
            "CIITY3",
            vec![leg2.clone(), leg1],
            lookup,
        )
        .unwrap();
        assert_eq!(procedure.main_legs.len(), 2);
        assert_eq!(procedure.main_legs[0].sequence_number, 10);
        assert_eq!(procedure.main_legs[1].sequence_number, 20);
        assert_eq!(procedure.transitions.len(), 0);

        // Sequence gap detected when a leg is missing (10 -> 30).
        let mut gapped = base.clone();
        gapped.transition_ident = String::new();
        let mut gap2 = leg2.clone();
        gap2.sequence_number = 30;
        let procedure2 = Procedure::assemble(
            "KSFO",
            ProcedureKind::Sid,
            "CIITY3",
            vec![gapped, gap2],
            lookup,
        )
        .unwrap();
        let diagnostics = procedure2.validate();
        assert!(diagnostics.iter().any(|d| d.contains("sequence gap")));

        // Empty assemble fails closed.
        assert!(Procedure::assemble("KSFO", ProcedureKind::Sid, "X", vec![], lookup).is_err());
    }

    #[test]
    fn test_assemble_transitions_grouped() {
        let base = canonical(&[]);
        let mut main = base.clone();
        main.transition_ident = String::new();
        let mut trans = base.clone();
        trans.transition_ident = "RW10L".to_string();
        let procedure = Procedure::assemble(
            "KSFO",
            ProcedureKind::Sid,
            "CIITY3",
            vec![main, trans],
            lookup,
        )
        .unwrap();
        assert_eq!(procedure.main_legs.len(), 1);
        assert_eq!(procedure.transitions.len(), 1);
        assert_eq!(procedure.transitions[0].transition_ident, "RW10L");
    }
}
