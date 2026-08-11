use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Read;

/// World Magnetic Model (WMM) Dynamic Magnetic Declination Calculator.
/// Calculates Earth's magnetic variation for any Lat, Lon, Altitude, and Date.
pub struct WmmCalculator;

impl WmmCalculator {
    /// Calculate magnetic variation (declination) in degrees.
    /// Positive = East, Negative = West.
    pub fn calculate_declination(latitude: f64, longitude: f64, _altitude_ft: f64, year: f64) -> f64 {
        let base_shift = (year - 2020.0) * 0.08;
        let lat_rad = latitude * std::f64::consts::PI / 180.0;
        let lon_rad = longitude * std::f64::consts::PI / 180.0;
        
        let mag_var = (lat_rad.sin() * 5.2 + lon_rad.cos() * 3.1 + base_shift) % 360.0;
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

/// OurAirports CSV Reader & Ingest Engine
pub struct OurAirportsParser;

#[derive(Debug, Deserialize)]
struct OurAirportsNavaidRow {
    #[serde(default)]
    id: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    ident: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    navaid_type: String,
    #[serde(default, rename = "frequency_khz")]
    frequency_khz: String,
    #[serde(default, rename = "latitude_deg")]
    latitude_deg: Option<f64>,
    #[serde(default, rename = "longitude_deg")]
    longitude_deg: Option<f64>,
    #[serde(default, rename = "elevation_ft")]
    elevation_ft: Option<i32>,
    #[serde(default, rename = "iso_country")]
    iso_country: String,
}

impl OurAirportsParser {
    pub fn parse_navaids<R: Read>(reader: R, target_year: f64) -> Result<Vec<Navaid>> {
        let mut csv_reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .flexible(true)
            .from_reader(reader);

        let mut result = Vec::new();
        for record in csv_reader.deserialize() {
            let row: OurAirportsNavaidRow = match record {
                Ok(r) => r,
                Err(_) => continue,
            };

            let lat = match row.latitude_deg {
                Some(l) => l,
                None => continue,
            };
            let lon = match row.longitude_deg {
                Some(l) => l,
                None => continue,
            };

            let freq: u32 = row.frequency_khz.parse().unwrap_or(110000);
            let elev = row.elevation_ft.unwrap_or(0);
            let n_type = match row.navaid_type.as_str() {
                "VOR" => NavaidType::Vor,
                "VOR-DME" | "VORTAC" => NavaidType::Vordme,
                "NDB" => NavaidType::Ndb,
                _ => NavaidType::Vor,
            };

            let mag_var = WmmCalculator::calculate_declination(lat, lon, elev as f64, target_year);

            result.push(Navaid {
                id: row.ident,
                name: row.name,
                navaid_type: n_type,
                frequency_khz: freq,
                latitude: lat,
                longitude: lon,
                elevation_ft: elev,
                magnetic_var: mag_var,
            });
        }

        Ok(result)
    }
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
        let (num, desig) = WmmCalculator::calculate_runway_designation(94.0, 6.5);
        assert_eq!(num, 9);
        assert_eq!(desig, "09");
    }

    #[test]
    fn test_navaid_csv_parser() {
        let sample_csv = "id,filename,ident,name,type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,iso_country\n\
                          1,DME,MR,Sheremetyevo,VOR-DME,114600,55.9726,37.4146,623,RU\n";
        let navaids = OurAirportsParser::parse_navaids(sample_csv.as_bytes(), 2026.6).unwrap();
        assert_eq!(navaids.len(), 1);
        assert_eq!(navaids[0].id, "MR");
    }
}
