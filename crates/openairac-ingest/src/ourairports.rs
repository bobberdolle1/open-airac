use crate::provider::{FetchedDataset, IngestReport};
use anyhow::Result;
use chrono::Utc;
use openairac_magnetic::analyze_runway_magnetic_drift;
use openairac_model::*;
use openairac_store::WorldStore;
use serde::Deserialize;

/// OurAirports raw CSV record structures
#[derive(Debug, Deserialize)]
pub struct OurAirportsAirportRecord {
    pub id: String,
    pub ident: String,
    pub airport_type: Option<String>,
    pub name: String,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub elevation_ft: Option<f64>,
    pub iso_country: Option<String>,
    pub municipality: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OurAirportsRunwayRecord {
    pub id: String,
    pub airport_ident: String,
    pub length_ft: Option<u32>,
    pub width_ft: Option<u32>,
    pub surface: Option<String>,
    pub le_ident: String,
    pub le_latitude_deg: Option<f64>,
    pub le_longitude_deg: Option<f64>,
    pub le_elevation_ft: Option<f64>,
    #[serde(rename = "le_heading_degT")]
    pub le_heading_deg_t: Option<f64>,
    pub he_ident: Option<String>,
    pub he_latitude_deg: Option<f64>,
    pub he_longitude_deg: Option<f64>,
    pub he_elevation_ft: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct OurAirportsNavaidRecord {
    pub id: String,
    pub filename: Option<String>,
    pub ident: String,
    pub name: String,
    pub navaid_type: String,
    pub frequency_khz: Option<u32>,
    pub latitude_deg: Option<f64>,
    pub longitude_deg: Option<f64>,
    pub elevation_ft: Option<i32>,
    pub associated_airport: Option<String>,
    pub magnetic_variation_deg: Option<f64>,
}

pub struct OurAirportsImporter;

impl OurAirportsImporter {
    pub fn parse_airports(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
    ) -> (Vec<CanonicalAirport>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "airports");
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut airports = Vec::new();
        let valid_from = Utc::now();

        for result in reader.deserialize::<OurAirportsAirportRecord>() {
            match result {
                Ok(rec) => {
                    let lat = match rec.latitude_deg {
                        Some(l) if (-90.0..=90.0).contains(&l) => l,
                        _ => {
                            report.record_rejected(format!(
                                "Airport {}: invalid latitude",
                                rec.ident
                            ));
                            continue;
                        }
                    };
                    let lon = match rec.longitude_deg {
                        Some(l) if (-180.0..=180.0).contains(&l) => l,
                        _ => {
                            report.record_rejected(format!(
                                "Airport {}: invalid longitude",
                                rec.ident
                            ));
                            continue;
                        }
                    };

                    let airport = CanonicalAirport {
                        id: AirportId(rec.ident.clone()),
                        ident: rec.ident,
                        name: rec.name,
                        airport_type: rec
                            .airport_type
                            .unwrap_or_else(|| "medium_airport".to_string()),
                        latitude: lat,
                        longitude: lon,
                        elevation_ft: rec.elevation_ft,
                        iso_country: rec.iso_country,
                        municipality: rec.municipality,
                        runways: Vec::new(),
                        temporal: TemporalValidity {
                            valid_from,
                            valid_until: None,
                            source_snapshot_id: snapshot_id.clone(),
                        },
                    };
                    airports.push(airport);
                    report.record_accepted();
                }
                Err(err) => {
                    report.record_rejected(format!("Failed to parse airport CSV row: {}", err));
                }
            }
        }

        (airports, report)
    }

    pub fn parse_runways(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
        year: f64,
    ) -> (Vec<CanonicalRunway>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "runways");
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut runways = Vec::new();
        let valid_from = Utc::now();

        for result in reader.deserialize::<OurAirportsRunwayRecord>() {
            match result {
                Ok(rec) => {
                    let le_lat = match rec.le_latitude_deg {
                        Some(l) if (-90.0..=90.0).contains(&l) => l,
                        _ => {
                            report
                                .record_rejected(format!("Runway {}: invalid LE latitude", rec.id));
                            continue;
                        }
                    };
                    let le_lon = match rec.le_longitude_deg {
                        Some(l) if (-180.0..=180.0).contains(&l) => l,
                        _ => {
                            report.record_rejected(format!(
                                "Runway {}: invalid LE longitude",
                                rec.id
                            ));
                            continue;
                        }
                    };

                    let heading = rec.le_heading_deg_t.unwrap_or(0.0);
                    let computed_mag_designator = match analyze_runway_magnetic_drift(
                        &rec.le_ident,
                        heading,
                        le_lat,
                        le_lon,
                        year,
                    ) {
                        Ok(analysis) => analysis.computed_magnetic_designator,
                        Err(_) => rec.le_ident.clone(),
                    };

                    let runway = CanonicalRunway {
                        id: format!("{}-{}", rec.airport_ident, rec.le_ident),
                        airport_ident: rec.airport_ident,
                        official_designator: rec.le_ident.clone(),
                        computed_magnetic_designator: computed_mag_designator,
                        true_heading_deg: heading,
                        length_ft: rec.length_ft.unwrap_or(0),
                        width_ft: rec.width_ft.unwrap_or(0),
                        surface: rec.surface,
                        le_ident: rec.le_ident,
                        le_lat,
                        le_lon,
                        le_elevation_ft: rec.le_elevation_ft,
                        he_ident: rec.he_ident.unwrap_or_default(),
                        he_lat: rec.he_latitude_deg.unwrap_or(le_lat),
                        he_lon: rec.he_longitude_deg.unwrap_or(le_lon),
                        he_elevation_ft: rec.he_elevation_ft,
                        temporal: TemporalValidity {
                            valid_from,
                            valid_until: None,
                            source_snapshot_id: snapshot_id.clone(),
                        },
                    };
                    runways.push(runway);
                    report.record_accepted();
                }
                Err(err) => {
                    report.record_rejected(format!("Failed to parse runway CSV row: {}", err));
                }
            }
        }

        (runways, report)
    }

    pub fn parse_navaids(
        csv_content: &str,
        snapshot_id: &SourceSnapshotId,
    ) -> (Vec<CanonicalNavaid>, IngestReport) {
        let mut report = IngestReport::new("OurAirports", "navaids");
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(csv_content.as_bytes());

        let mut navaids = Vec::new();
        let valid_from = Utc::now();

        for result in reader.deserialize::<OurAirportsNavaidRecord>() {
            match result {
                Ok(rec) => {
                    let lat = match rec.latitude_deg {
                        Some(l) if (-90.0..=90.0).contains(&l) => l,
                        _ => {
                            report
                                .record_rejected(format!("Navaid {}: invalid latitude", rec.ident));
                            continue;
                        }
                    };
                    let lon = match rec.longitude_deg {
                        Some(l) if (-180.0..=180.0).contains(&l) => l,
                        _ => {
                            report.record_rejected(format!(
                                "Navaid {}: invalid longitude",
                                rec.ident
                            ));
                            continue;
                        }
                    };

                    let freq_khz = match rec.frequency_khz {
                        Some(f) if f > 0 => f,
                        _ => {
                            report.record_rejected(format!(
                                "Navaid {}: missing or invalid frequency",
                                rec.ident
                            ));
                            continue;
                        }
                    };

                    let kind = match NavaidKind::parse(&rec.navaid_type) {
                        Some(k) => k,
                        None => {
                            report.record_rejected(format!(
                                "Navaid {}: unknown navaid type '{}'",
                                rec.ident, rec.navaid_type
                            ));
                            continue;
                        }
                    };

                    let navaid = CanonicalNavaid {
                        object_id: NavaidId(rec.ident.clone()),
                        ident: rec.ident,
                        name: rec.name,
                        kind,
                        frequency: FrequencyKhz(freq_khz),
                        latitude: lat,
                        longitude: lon,
                        elevation_ft: rec.elevation_ft.unwrap_or(0),
                        associated_airport: rec.associated_airport,
                        magnetic_variation_deg: rec.magnetic_variation_deg,
                        temporal: TemporalValidity {
                            valid_from,
                            valid_until: None,
                            source_snapshot_id: snapshot_id.clone(),
                        },
                    };
                    navaids.push(navaid);
                    report.record_accepted();
                }
                Err(err) => {
                    report.record_rejected(format!("Failed to parse navaid CSV row: {}", err));
                }
            }
        }

        (navaids, report)
    }

    pub fn ingest_dataset(dataset: &FetchedDataset, store: &WorldStore) -> Result<IngestReport> {
        let snapshot_id =
            SourceSnapshotId(format!("snap-ourairports-{}", &dataset.content_sha256[..8]));
        let snapshot = SourceSnapshot {
            id: snapshot_id.clone(),
            provider: "OurAirports".to_string(),
            dataset: dataset.dataset_name.clone(),
            provider_revision: dataset.provider_revision.clone(),
            airac_cycle: None,
            effective_from: Some(dataset.retrieved_at),
            effective_until: None,
            retrieved_at: dataset.retrieved_at,
            source_uri: dataset.source_uri.clone(),
            content_sha256: dataset.content_sha256.clone(),
            license_notes: Some("Public Domain / CC0".to_string()),
            parser_version: "0.2.0".to_string(),
        };

        store.insert_source_snapshot(&snapshot)?;

        let revision = WorldRevision {
            id: WorldRevisionId(format!("rev-{}", &dataset.content_sha256[..8])),
            created_at: Utc::now(),
            source_snapshot_id: snapshot_id.clone(),
            schema_version: "v1".to_string(),
            notes: Some(format!(
                "Ingested OurAirports dataset {}",
                dataset.dataset_name
            )),
        };
        store.insert_world_revision(&revision)?;

        match dataset.dataset_name.as_str() {
            "airports" => {
                let (airports, report) = Self::parse_airports(&dataset.raw_content, &snapshot_id);
                for ap in &airports {
                    store.insert_airport(ap)?;
                }
                Ok(report)
            }
            "runways" => {
                let (runways, report) =
                    Self::parse_runways(&dataset.raw_content, &snapshot_id, 2026.0);
                for rwy in &runways {
                    store.insert_runway(rwy)?;
                }
                Ok(report)
            }
            "navaids" => {
                let (navaids, report) = Self::parse_navaids(&dataset.raw_content, &snapshot_id);
                for nav in &navaids {
                    store.insert_navaid(nav)?;
                }
                Ok(report)
            }
            other => Err(anyhow::anyhow!("Unknown OurAirports dataset: {}", other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_AIRPORTS_CSV: &str = r#"id,ident,airport_type,name,latitude_deg,longitude_deg,elevation_ft,iso_country,municipality
1,KSFO,large_airport,San Francisco International Airport,37.6188,-122.3750,13,US,San Francisco
2,KJFK,large_airport,John F Kennedy International Airport,40.6398,-73.7789,13,US,New York
3,BAD1,medium_airport,Bad Airport,999.0,-122.0,10,US,Bad City
"#;

    const SAMPLE_RUNWAYS_CSV: &str = r#"id,airport_ident,length_ft,width_ft,surface,le_ident,le_latitude_deg,le_longitude_deg,le_elevation_ft,le_heading_degT,he_ident,he_latitude_deg,he_longitude_deg,he_elevation_ft
101,KSFO,11870,200,ASP,28R,37.6188,-122.3750,13,284.0,10L,37.6140,-122.3900,11
102,KJFK,14511,200,ASP,13L,40.6398,-73.7789,13,134.0,31R,40.6200,-73.7500,11
"#;

    const SAMPLE_NAVAIDS_CSV: &str = r#"id,filename,ident,name,navaid_type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,associated_airport,magnetic_variation_deg
201,SFO.navaid,SFO,San Francisco VOR-DME,VOR-DME,115800,37.6195,-122.3739,13,KSFO,-13.0
202,JFK.navaid,JFK,Kennedy VOR-DME,VOR-DME,115900,40.6397,-73.7789,13,KJFK,-13.0
203,BAD.navaid,BAD,Bad Frequency NDB,NDB,0,37.0,-122.0,10,KSFO,0.0
"#;

    #[test]
    fn test_parse_airports_with_rejection() {
        let snapshot_id = SourceSnapshotId("snap-test".to_string());
        let (airports, report) =
            OurAirportsImporter::parse_airports(SAMPLE_AIRPORTS_CSV, &snapshot_id);

        assert_eq!(airports.len(), 2);
        assert_eq!(report.records_seen, 3);
        assert_eq!(report.records_accepted, 2);
        assert_eq!(report.records_rejected, 1);
        assert_eq!(airports[0].ident, "KSFO");
        assert_eq!(airports[1].ident, "KJFK");
    }
    #[test]
    fn test_parse_runways() {
        let snapshot_id = SourceSnapshotId("snap-test".to_string());
        let (runways, report) =
            OurAirportsImporter::parse_runways(SAMPLE_RUNWAYS_CSV, &snapshot_id, 2026.0);

        assert_eq!(runways.len(), 2);
        assert_eq!(report.records_accepted, 2);
        assert_eq!(runways[0].official_designator, "28R");
    }

    #[test]
    fn test_parse_navaids_with_rejection() {
        let snapshot_id = SourceSnapshotId("snap-test".to_string());
        let (navaids, report) =
            OurAirportsImporter::parse_navaids(SAMPLE_NAVAIDS_CSV, &snapshot_id);

        assert_eq!(navaids.len(), 2);
        assert_eq!(report.records_seen, 3);
        assert_eq!(report.records_accepted, 2);
        assert_eq!(report.records_rejected, 1); // Zero frequency rejected, not turned into fake data!
        assert_eq!(navaids[0].ident, "SFO");
        assert_eq!(navaids[0].frequency.to_mhz(), 115.8);
    }

    #[test]
    fn test_end_to_end_store_ingest() {
        let store = WorldStore::open_in_memory().unwrap();
        let dataset = FetchedDataset {
            provider_name: "OurAirports".to_string(),
            dataset_name: "airports".to_string(),
            source_uri: "https://davidmegginson.github.io/ourairports-data/airports.csv"
                .to_string(),
            content_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                .to_string(),
            retrieved_at: Utc::now(),
            provider_revision: Some("2026-08-12".to_string()),
            raw_content: SAMPLE_AIRPORTS_CSV.to_string(),
        };

        let report = OurAirportsImporter::ingest_dataset(&dataset, &store).unwrap();
        assert_eq!(report.records_accepted, 2);

        let status = store.status().unwrap();
        assert_eq!(status.total_airports, 2);
        assert_eq!(status.total_snapshots, 1);
    }
}
