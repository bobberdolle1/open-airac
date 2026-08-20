//! Aircraft-aware random flight plan generator with suitability constraints and deterministic seed support.

use crate::Coordinate;
use anyhow::{Result, anyhow};
use openairac_model::CanonicalAirport;
use serde::{Deserialize, Serialize};

/// Aircraft classification category for route suitability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AircraftClass {
    LightPiston,
    Turboprop,
    RegionalJet,
    NarrowbodyJet,
    WidebodyJet,
    B747Class,
    Custom,
}

impl AircraftClass {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::LightPiston => "Light Piston (e.g. C172, PA28)",
            Self::Turboprop => "Turboprop (e.g. King Air, Dash 8)",
            Self::RegionalJet => "Regional Jet (e.g. CRJ, ERJ)",
            Self::NarrowbodyJet => "Narrowbody Jet (e.g. A320, B738)",
            Self::WidebodyJet => "Widebody Jet (e.g. B777, A350)",
            Self::B747Class => "Boeing 747 / Heavy Widebody (B744, B748, A388)",
            Self::Custom => "Custom Aircraft Profile",
        }
    }
}

/// Suitability constraints defining airport and distance requirements for an aircraft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AircraftProfile {
    pub name: String,
    pub icao_type: Option<String>,
    pub aircraft_class: AircraftClass,
    pub min_runway_length_ft: u32,
    pub preferred_runway_length_ft: u32,
    pub min_runway_width_ft: Option<u32>,
    pub min_distance_nm: f64,
    pub max_distance_nm: f64,
    pub preferred_distance_nm: Option<f64>,
    pub requires_hard_surface: bool,
    pub requires_ifr: bool,
    pub requires_tower: bool,
    pub cruise_speed_kts: Option<u32>,
}

impl AircraftProfile {
    pub fn b747_class() -> Self {
        Self {
            name: "Boeing 747-400 / Heavy Widebody".to_string(),
            icao_type: Some("B744".to_string()),
            aircraft_class: AircraftClass::B747Class,
            min_runway_length_ft: 8500,
            preferred_runway_length_ft: 10000,
            min_runway_width_ft: Some(150),
            min_distance_nm: 1000.0,
            max_distance_nm: 7500.0,
            preferred_distance_nm: Some(3500.0),
            requires_hard_surface: true,
            requires_ifr: true,
            requires_tower: true,
            cruise_speed_kts: Some(490),
        }
    }

    pub fn a320_narrowbody() -> Self {
        Self {
            name: "Airbus A320 / Boeing 737 Narrowbody".to_string(),
            icao_type: Some("A320".to_string()),
            aircraft_class: AircraftClass::NarrowbodyJet,
            min_runway_length_ft: 6000,
            preferred_runway_length_ft: 7500,
            min_runway_width_ft: Some(100),
            min_distance_nm: 250.0,
            max_distance_nm: 3200.0,
            preferred_distance_nm: Some(800.0),
            requires_hard_surface: true,
            requires_ifr: true,
            requires_tower: false,
            cruise_speed_kts: Some(450),
        }
    }

    pub fn regional_jet() -> Self {
        Self {
            name: "Embraer E190 / Bombardier CRJ".to_string(),
            icao_type: Some("E190".to_string()),
            aircraft_class: AircraftClass::RegionalJet,
            min_runway_length_ft: 5000,
            preferred_runway_length_ft: 6500,
            min_runway_width_ft: Some(90),
            min_distance_nm: 150.0,
            max_distance_nm: 1800.0,
            preferred_distance_nm: Some(500.0),
            requires_hard_surface: true,
            requires_ifr: true,
            requires_tower: false,
            cruise_speed_kts: Some(420),
        }
    }

    pub fn turboprop() -> Self {
        Self {
            name: "King Air 350 / Dash 8 Turboprop".to_string(),
            icao_type: Some("B350".to_string()),
            aircraft_class: AircraftClass::Turboprop,
            min_runway_length_ft: 3500,
            preferred_runway_length_ft: 4500,
            min_runway_width_ft: None,
            min_distance_nm: 80.0,
            max_distance_nm: 1200.0,
            preferred_distance_nm: Some(300.0),
            requires_hard_surface: false,
            requires_ifr: false,
            requires_tower: false,
            cruise_speed_kts: Some(280),
        }
    }

    pub fn light_piston() -> Self {
        Self {
            name: "Cessna 172 / Light Piston".to_string(),
            icao_type: Some("C172".to_string()),
            aircraft_class: AircraftClass::LightPiston,
            min_runway_length_ft: 1800,
            preferred_runway_length_ft: 2500,
            min_runway_width_ft: None,
            min_distance_nm: 30.0,
            max_distance_nm: 500.0,
            preferred_distance_nm: Some(120.0),
            requires_hard_surface: false,
            requires_ifr: false,
            requires_tower: false,
            cruise_speed_kts: Some(120),
        }
    }

    /// Evaluates whether an airport satisfies this aircraft's physical suitability constraints.
    pub fn evaluate_airport(&self, airport: &CanonicalAirport) -> (bool, Vec<String>) {
        let mut reasons = Vec::new();
        let mut passed = true;

        let longest_rwy = airport.runways.iter().max_by_key(|r| r.length_ft);

        if let Some(rwy) = longest_rwy {
            if rwy.length_ft < self.min_runway_length_ft {
                passed = false;
                reasons.push(format!(
                    "Longest runway {} ft is shorter than required {} ft",
                    rwy.length_ft, self.min_runway_length_ft
                ));
            }

            if let Some(min_w) = self.min_runway_width_ft {
                let w = rwy.width_ft.unwrap_or(0);
                if w < min_w {
                    passed = false;
                    reasons.push(format!(
                        "Runway width {} ft is narrower than required {} ft",
                        w, min_w
                    ));
                }
            }

            if self.requires_hard_surface {
                let is_hard = rwy
                    .surface
                    .as_deref()
                    .map(|surf| {
                        let s_up = surf.to_uppercase();
                        s_up.contains("ASP")
                            || s_up.contains("CON")
                            || s_up.contains("BIT")
                            || s_up.contains("TAR")
                            || s_up.contains("HARD")
                            || s_up == "U"
                    })
                    .unwrap_or(false);

                if !is_hard {
                    passed = false;
                    reasons.push(format!(
                        "Runway surface '{:?}' is not a paved hard surface",
                        rwy.surface
                    ));
                }
            }
        } else {
            passed = false;
            reasons.push("No runways available at airport".to_string());
        }

        (passed, reasons)
    }
}

/// Generated Random Flight Plan Result
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RandomFlightResult {
    pub departure_icao: String,
    pub departure_name: String,
    pub departure_longest_runway_ft: u32,
    pub destination_icao: String,
    pub destination_name: String,
    pub destination_longest_runway_ft: u32,
    pub great_circle_distance_nm: f64,
    pub estimated_time_enroute_minutes: u32,
    pub aircraft_profile: AircraftProfile,
    pub seed_used: u64,
    pub suitability_notes: Vec<String>,
}

/// Pseudo-random number generator using XorShift64 for deterministic reproducible flight generation.
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0xdead_beef_cafe_babe
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn next_bounded(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % (bound as u64)) as usize
        }
    }
}

/// Generates a random flight plan matching aircraft suitability constraints.
pub fn generate_random_flight(
    airports: &[CanonicalAirport],
    profile: &AircraftProfile,
    fixed_departure: Option<&str>,
    fixed_destination: Option<&str>,
    custom_seed: Option<u64>,
) -> Result<RandomFlightResult> {
    if airports.is_empty() {
        return Err(anyhow!("No airports provided for random flight generation"));
    }

    let seed = custom_seed.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(42)
    });

    let mut rng = XorShift64::new(seed);

    // Filter suitable candidate airports
    let suitable_airports: Vec<_> = airports
        .iter()
        .filter(|apt| {
            let (ok, _) = profile.evaluate_airport(apt);
            ok
        })
        .collect();

    if suitable_airports.is_empty() {
        return Err(anyhow!(
            "No airports meet the suitability criteria for aircraft profile '{}'",
            profile.name
        ));
    }

    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 2000;

    while attempts < MAX_ATTEMPTS {
        attempts += 1;

        let dep_apt = if let Some(dep_icao) = fixed_departure {
            let clean = dep_icao.trim().to_uppercase();
            airports
                .iter()
                .find(|a| a.ident == clean)
                .ok_or_else(|| anyhow!("Fixed departure airport '{}' not found", clean))?
        } else {
            let idx = rng.next_bounded(suitable_airports.len());
            suitable_airports[idx]
        };

        let dest_apt = if let Some(dest_icao) = fixed_destination {
            let clean = dest_icao.trim().to_uppercase();
            airports
                .iter()
                .find(|a| a.ident == clean)
                .ok_or_else(|| anyhow!("Fixed destination airport '{}' not found", clean))?
        } else {
            let idx = rng.next_bounded(suitable_airports.len());
            suitable_airports[idx]
        };

        if dep_apt.ident == dest_apt.ident {
            continue;
        }

        let dep_coord = Coordinate::new(dep_apt.latitude, dep_apt.longitude)?;
        let dest_coord = Coordinate::new(dest_apt.latitude, dest_apt.longitude)?;
        let dist_nm = dep_coord.distance_nm(&dest_coord);

        if dist_nm >= profile.min_distance_nm && dist_nm <= profile.max_distance_nm {
            let dep_max_rwy = dep_apt
                .runways
                .iter()
                .map(|r| r.length_ft)
                .max()
                .unwrap_or(0);
            let dest_max_rwy = dest_apt
                .runways
                .iter()
                .map(|r| r.length_ft)
                .max()
                .unwrap_or(0);

            let speed = profile.cruise_speed_kts.unwrap_or(450) as f64;
            let ete_minutes = ((dist_nm / speed) * 60.0).round() as u32 + 20; // 20 mins for climb/descent

            let mut notes = Vec::new();
            notes.push(format!(
                "Departure {} runway length {} ft meets minimum {} ft",
                dep_apt.ident, dep_max_rwy, profile.min_runway_length_ft
            ));
            notes.push(format!(
                "Destination {} runway length {} ft meets minimum {} ft",
                dest_apt.ident, dest_max_rwy, profile.min_runway_length_ft
            ));
            notes.push(format!(
                "Direct distance {:.1} NM is within aircraft range [{:.0}, {:.0}] NM",
                dist_nm, profile.min_distance_nm, profile.max_distance_nm
            ));

            return Ok(RandomFlightResult {
                departure_icao: dep_apt.ident.clone(),
                departure_name: dep_apt.name.clone(),
                departure_longest_runway_ft: dep_max_rwy,
                destination_icao: dest_apt.ident.clone(),
                destination_name: dest_apt.name.clone(),
                destination_longest_runway_ft: dest_max_rwy,
                great_circle_distance_nm: dist_nm,
                estimated_time_enroute_minutes: ete_minutes,
                aircraft_profile: profile.clone(),
                seed_used: seed,
                suitability_notes: notes,
            });
        }
    }

    Err(anyhow!(
        "Could not find a suitable departure/destination pair within distance range [{:.0}, {:.0}] NM after {} attempts",
        profile.min_distance_nm,
        profile.max_distance_nm,
        MAX_ATTEMPTS
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openairac_model::{AirportId, CanonicalRunway, RunwayId, TemporalValidity};

    fn make_test_airport(
        ident: &str,
        lat: f64,
        lon: f64,
        rwy_len: u32,
        surf: &str,
    ) -> CanonicalAirport {
        CanonicalAirport {
            id: AirportId(ident.to_string()),
            ident: ident.to_string(),
            name: format!("Airport {}", ident),
            airport_type: "large_airport".to_string(),
            latitude: lat,
            longitude: lon,
            elevation_ft: Some(100.0),
            iso_country: Some("US".to_string()),
            municipality: None,
            runways: vec![CanonicalRunway {
                id: RunwayId("r1".to_string()),
                airport_id: AirportId(ident.to_string()),
                airport_ident: ident.to_string(),
                official_designator: "09/27".to_string(),
                computed_magnetic_designator: None,
                true_heading_deg: Some(90.0),
                length_ft: rwy_len,
                width_ft: Some(150),
                surface: Some(surf.to_string()),
                le_ident: "09".to_string(),
                le_lat: lat,
                le_lon: lon,
                le_elevation_ft: None,
                he_ident: "27".to_string(),
                he_lat: lat,
                he_lon: lon + 0.05,
                he_elevation_ft: None,
                temporal: TemporalValidity {
                    valid_from: chrono::Utc::now(),
                    valid_until: None,
                    source_snapshot_id: openairac_model::SourceSnapshotId("test".to_string()),
                },
            }],
            temporal: TemporalValidity {
                valid_from: chrono::Utc::now(),
                valid_until: None,
                source_snapshot_id: openairac_model::SourceSnapshotId("test".to_string()),
            },
        }
    }

    #[test]
    fn test_b747_suitability_rejects_short_runway() {
        let profile = AircraftProfile::b747_class();
        let tiny_strip = make_test_airport("TINY", 40.0, -74.0, 2500, "GRASS");
        let (passed, reasons) = profile.evaluate_airport(&tiny_strip);
        assert!(!passed);
        assert!(reasons.iter().any(|r| r.contains("shorter than required")));
    }

    #[test]
    fn test_b747_suitability_accepts_large_airport() {
        let profile = AircraftProfile::b747_class();
        let large_hub = make_test_airport("KJFK", 40.64, -73.78, 14500, "ASPHALT");
        let (passed, _) = profile.evaluate_airport(&large_hub);
        assert!(passed);
    }

    #[test]
    fn test_deterministic_random_flight_generation() {
        let profile = AircraftProfile::b747_class();
        let apt1 = make_test_airport("KJFK", 40.64, -73.78, 14500, "ASPHALT");
        let apt2 = make_test_airport("KLAX", 33.94, -118.41, 12000, "ASPHALT");
        let apt3 = make_test_airport("KORD", 41.97, -87.90, 13000, "ASPHALT");
        let airports = vec![apt1, apt2, apt3];

        let res1 = generate_random_flight(&airports, &profile, None, None, Some(12345)).unwrap();
        let res2 = generate_random_flight(&airports, &profile, None, None, Some(12345)).unwrap();

        assert_eq!(res1.departure_icao, res2.departure_icao);
        assert_eq!(res1.destination_icao, res2.destination_icao);
        assert_eq!(res1.great_circle_distance_nm, res2.great_circle_distance_nm);
    }
}
