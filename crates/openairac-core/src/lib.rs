use anyhow::Result;
pub use magnetic::{analyze_runway_magnetic_drift, Wmm2025, WmmResult};
pub use nav_model::*;
use serde::Deserialize;
use std::io::Read;

/// OurAirports CSV Reader & Ingest Engine into Canonical Temporal Model
pub struct OurAirportsParser;

#[derive(Debug, Deserialize)]
struct OurAirportsNavaidRow {
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
}

impl OurAirportsParser {
    pub fn parse_navaids<R: Read>(reader: R, target_year: f64) -> Result<Vec<CanonicalNavaid>> {
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
            let n_kind = match row.navaid_type.as_str() {
                "VOR" => NavaidKind::Vor,
                "VOR-DME" | "VORTAC" => NavaidKind::Vordme,
                "NDB" => NavaidKind::Ndb,
                _ => NavaidKind::Vor,
            };

            let wmm = Wmm2025::calculate(lat, lon, elev as f64, target_year);

            result.push(CanonicalNavaid {
                object_id: format!("OURAIRPORTS_{}", row.ident),
                ident: row.ident,
                name: row.name,
                kind: n_kind,
                frequency_khz: freq,
                latitude: lat,
                longitude: lon,
                elevation_ft: elev,
                official_slaved_magvar_deg: wmm.declination_deg,
                computed_wmm_magvar_deg: wmm.declination_deg,
                temporal: TemporalValidity {
                    valid_from: chrono::Utc::now(),
                    valid_until: None,
                    revision_id: format!("REV_{:.1}", target_year),
                    source: DataSourceProvider::OurAirports {
                        snapshot_date: chrono::Utc::now().to_rfc3339(),
                    },
                },
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navaid_csv_parser() {
        let sample_csv = "ident,name,type,frequency_khz,latitude_deg,longitude_deg,elevation_ft\n\
                          MR,Sheremetyevo,VOR-DME,114600,55.9726,37.4146,623\n";
        let navaids = OurAirportsParser::parse_navaids(sample_csv.as_bytes(), 2026.0).unwrap();
        assert_eq!(navaids.len(), 1);
        assert_eq!(navaids[0].ident, "MR");
        assert!(navaids[0].computed_wmm_magvar_deg.is_finite());
    }
}
