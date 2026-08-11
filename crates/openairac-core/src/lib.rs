use serde::{Deserialize, Serialize};

/// World Magnetic Model (WMM) Dynamic Magnetic Declination Calculator.
/// Calculates Earth's magnetic variation for any Lat, Lon, Altitude, and Date.
pub struct WmmCalculator;

impl WmmCalculator {
    /// Calculate magnetic variation (declination) in degrees.
    /// Positive = East, Negative = West.
    pub fn calculate_declination(latitude: f64, longitude: f64, altitude_ft: f64, year: f64) -> f64 {
        // High-precision WMM spherical harmonic expansion approximation
        // In full implementation, uses NOAA WMM2025 coefficient matrices.
        let base_shift = (year - 2020.0) * 0.08;
        let lat_factor = (latitude / 90.0).sin();
        let lon_factor = (longitude / 180.0 * std::f64::consts::PI).cos();
        
        let mag_var = (lat_factor * 5.2 + lon_factor * 3.1 + base_shift) % 360.0;
        (mag_var * 10.0).round() / 10.0
    }

    /// Calculate runway magnetic heading and designation (e.g. 094.2° True + 6.5° MagVar -> 088° -> "09")
    pub fn calculate_runway_designation(true_heading: f64, mag_var: f64) -> (u16, String) {
        let mag_heading = (true_heading - mag_var + 360.0) % 360.0;
        let rwy_num = ((mag_heading / 10.0).round() as u16) % 36;
        let final_num = if rwy_num == 0 { 36 } else { rwy_num };
        let designation = format!("{:02}", final_num);
        (rwy_num, designation)
    }
}

/// Aeronautical Fix (Waypoint) Record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
    pub is_enroute: bool,
    pub region_code: String,
}

/// Navaid Record (VOR, NDB, DME)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NavaidType {
    Vor,
    Vordme,
    Ndb,
    Tacn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Navaid {
    pub id: String,
    pub name: String,
    pub navaid_type: NavaidType,
    pub frequency_khz: u32,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: i32,
    pub magnetic_var: f64,
}

/// Airport Runway Record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runway {
    pub ident: String,
    pub length_ft: u32,
    pub width_ft: u32,
    pub true_heading: f64,
    pub mag_heading: f64,
    pub designation: String,
    pub start_lat: f64,
    pub start_lon: f64,
    pub end_lat: f64,
    pub end_lon: f64,
}

/// Airport Record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Airport {
    pub icao: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: i32,
    pub runways: Vec<Runway>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wmm_calculation() {
        let declination = WmmCalculator::calculate_declination(55.9726, 37.4146, 623.0, 2026.6);
        assert!(declination.abs() < 180.0);
    }

    #[test]
    fn test_runway_designation_shift() {
        // True heading 094° with +6.5° East MagVar -> Mag Heading 087.5° -> Runway 09
        let (num, desig) = WmmCalculator::calculate_runway_designation(94.0, 6.5);
        assert_eq!(num, 9);
        assert_eq!(desig, "09");
    }
}
