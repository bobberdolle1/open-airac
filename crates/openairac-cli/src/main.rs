use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::ourairports::OurAirportsImporter;
use openairac_ingest::provider::FetchedDataset;
use openairac_magnetic::{Wmm2025, analyze_runway_magnetic_drift, wmm2025_metadata};
use openairac_store::WorldStore;
use std::path::PathBuf;
#[derive(Parser)]
#[command(name = "openairac")]
#[command(
    about = "✈️ OpenAIRAC — The open navigation data engine for flight simulation. Install once, navigate forever.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Perform system & database health check
    Doctor {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// Calculate WMM2025 magnetic field & variation for location and date
    Magnetic {
        #[arg(short = 'l', long)]
        lat: f64,
        #[arg(short = 'o', long)]
        lon: f64,
        #[arg(short, long, default_value_t = 0.0)]
        alt_ft: f64,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },

    /// Alias for magnetic command
    Magvar {
        #[arg(short = 'l', long)]
        lat: f64,
        #[arg(short = 'o', long)]
        lon: f64,
        #[arg(short, long, default_value_t = 0.0)]
        alt_ft: f64,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },

    /// Inspect magnetic drift for official vs computed runway designators
    Magdrift {
        #[arg(short, long)]
        designator: String,
        #[arg(short = 't', long)]
        heading: f64,
        #[arg(short = 'l', long)]
        lat: f64,
        #[arg(short = 'o', long)]
        lon: f64,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },
    Sync {
        #[arg(short, long, default_value = "ourairports")]
        provider: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Use offline sample fixture content instead of live network
        #[arg(long, default_value_t = false)]
        fixture: bool,
    },

    /// Display local world database revision, entity counts, and status
    Status {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },
    /// Export canonical navigation data into simulator format
    Export {
        #[command(subcommand)]
        target: ExportTarget,
    },
}

#[derive(Subcommand)]
enum ExportTarget {
    /// Export X-Plane 12 dat files (earth_fix.dat, earth_nav.dat)
    Xplane {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./dist/xplane")]
        out: PathBuf,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },
}

fn parse_iso_date_to_year_decimal(date_str: &str) -> Result<f64> {
    if let Ok(year_dec) = date_str.parse::<f64>() {
        return Ok(year_dec);
    }
    let d = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").with_context(|| {
        format!(
            "Invalid ISO date format '{}' (expected YYYY-MM-DD)",
            date_str
        )
    })?;

    let year = d.year() as f64;
    let day_of_year = d.ordinal() as f64;
    let days_in_year = if d.leap_year() { 366.0 } else { 365.0 };

    Ok(year + (day_of_year - 1.0) / days_in_year)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Doctor { db } => {
            println!("🏥 OpenAIRAC System Doctor");
            println!("==========================");
            println!("  CLI Version: {}", env!("CARGO_PKG_VERSION"));
            let meta = wmm2025_metadata();
            println!("  Magnetic Model: {} (Epoch {})", meta.model, meta.epoch);

            if !db.exists() {
                println!("  Database: ❌ Not found at {:?}", db);
                println!("  Run `openairac sync` to initialize local database.");
                std::process::exit(1);
            }

            match WorldStore::open(db) {
                Ok(store) => match store.status() {
                    Ok(status) => {
                        println!("  Database Open: ✅ Success ({:?})", db);
                        println!(
                            "  Integrity Check: {}",
                            if status.integrity_ok {
                                "✅ OK"
                            } else {
                                "❌ FAILED"
                            }
                        );
                        println!("  Migration Version: {}", status.migration_version);
                        println!(
                            "  Latest Revision: {}",
                            status.latest_revision_id.as_deref().unwrap_or("None")
                        );
                        println!("  Airports: {}", status.total_airports);
                        println!("  Runways: {}", status.total_runways);
                        println!("  Navaids: {}", status.total_navaids);
                        println!("  Waypoints: {}", status.total_waypoints);

                        if !status.integrity_ok {
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        println!("  Database Status Query: ❌ Error ({})", e);
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    println!("  Database Connection: ❌ Failed ({})", e);
                    std::process::exit(1);
                }
            }
        }

        Commands::Magnetic {
            lat,
            lon,
            alt_ft,
            date,
        }
        | Commands::Magvar {
            lat,
            lon,
            alt_ft,
            date,
        } => {
            let year = parse_iso_date_to_year_decimal(date)?;
            let res = Wmm2025::calculate_checked(*lat, *lon, *alt_ft, year)?;
            println!("WMM2025 Calculation Result:");
            println!("  Date: {} (Decimal Year {:.4})", date, year);
            println!("  Latitude: {:.4}°", lat);
            println!("  Longitude: {:.4}°", lon);
            println!("  Altitude: {:.1} ft", alt_ft);
            println!("  Declination (MagVar): {:.4}°", res.declination_deg);
            println!("  Inclination: {:.4}°", res.inclination_deg);
            println!("  North Component (X): {:.1} nT", res.north_component_nt);
            println!("  East Component (Y): {:.1} nT", res.east_component_nt);
            println!("  Down Component (Z): {:.1} nT", res.down_component_nt);
            println!(
                "  Horizontal Intensity (H): {:.1} nT",
                res.horizontal_intensity_nt
            );
            println!("  Total Intensity (F): {:.1} nT", res.total_intensity_nt);
        }

        Commands::Magdrift {
            designator,
            heading,
            lat,
            lon,
            date,
        } => {
            let year = parse_iso_date_to_year_decimal(date)?;
            let analysis = analyze_runway_magnetic_drift(designator, *heading, *lat, *lon, year)?;
            println!("Runway Magnetic Drift Analysis:");
            println!("  Official Designator: {}", analysis.official_designator);
            println!("  True Heading: {:.2}°", analysis.true_heading_deg);
            println!("  WMM MagVar: {:.2}°", analysis.wmm_magvar_deg);
            println!(
                "  Computed Mag Heading: {:.2}°",
                analysis.computed_magnetic_heading_deg
            );
            println!(
                "  Computed Designator: {}",
                analysis.computed_magnetic_designator
            );
            println!(
                "  Reciprocal Official: {}",
                analysis.reciprocal_official_designator
            );
            println!(
                "  Reciprocal Computed: {}",
                analysis.reciprocal_computed_designator
            );
            println!("  Drift Difference: {:.2}°", analysis.drift_difference_deg);
            println!(
                "  Redesignation Suggested: {}",
                if analysis.is_redesignation_suggested {
                    "⚠️ YES (Candidate mismatch detected)"
                } else {
                    "✅ NO"
                }
            );
        }

        Commands::Sync {
            provider,
            db,
            fixture,
        } => {
            println!("🔄 Synchronizing OpenAIRAC Navigation Data...");
            println!("  Provider: {}", provider);
            println!("  Database: {:?}", db);

            let store = WorldStore::open(db)?;

            if *fixture || provider == "ourairports" {
                let sample_airports = r#"id,ident,airport_type,name,latitude_deg,longitude_deg,elevation_ft,iso_country,municipality
1,KSFO,large_airport,San Francisco International Airport,37.6188,-122.3750,13,US,San Francisco
2,KJFK,large_airport,John F Kennedy International Airport,40.6398,-73.7789,13,US,New York
"#;
                let sample_runways = r#"id,airport_ident,length_ft,width_ft,surface,le_ident,le_latitude_deg,le_longitude_deg,le_elevation_ft,le_heading_degT,he_ident,he_latitude_deg,he_longitude_deg,he_elevation_ft
101,KSFO,11870,200,ASP,28R,37.6188,-122.3750,13,284.0,10L,37.6140,-122.3900,11
102,KJFK,14511,200,ASP,13L,40.6398,-73.7789,13,134.0,31R,40.6200,-73.7500,11
"#;
                let sample_navaids = r#"id,filename,ident,name,navaid_type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,associated_airport,magnetic_variation_deg
201,SFO.navaid,SFO,San Francisco VOR-DME,VOR-DME,115800,37.6195,-122.3739,13,KSFO,-13.0
202,JFK.navaid,JFK,Kennedy VOR-DME,VOR-DME,115900,40.6397,-73.7789,13,KJFK,-13.0
"#;

                let dataset_ap = FetchedDataset {
                    provider_name: "OurAirports".to_string(),
                    dataset_name: "airports".to_string(),
                    source_uri: "https://davidmegginson.github.io/ourairports-data/airports.csv"
                        .to_string(),
                    content_sha256: "f0a1b2c3d4e5".to_string(),
                    retrieved_at: Utc::now(),
                    provider_revision: Some("2026-08-12".to_string()),
                    raw_content: sample_airports.to_string(),
                };
                let r_ap = OurAirportsImporter::ingest_dataset(&dataset_ap, &store)?;

                let dataset_rwy = FetchedDataset {
                    provider_name: "OurAirports".to_string(),
                    dataset_name: "runways".to_string(),
                    source_uri: "https://davidmegginson.github.io/ourairports-data/runways.csv"
                        .to_string(),
                    content_sha256: "f0a1b2c3d4e6".to_string(),
                    retrieved_at: Utc::now(),
                    provider_revision: Some("2026-08-12".to_string()),
                    raw_content: sample_runways.to_string(),
                };
                let r_rwy = OurAirportsImporter::ingest_dataset(&dataset_rwy, &store)?;

                let dataset_nav = FetchedDataset {
                    provider_name: "OurAirports".to_string(),
                    dataset_name: "navaids".to_string(),
                    source_uri: "https://davidmegginson.github.io/ourairports-data/navaids.csv"
                        .to_string(),
                    content_sha256: "f0a1b2c3d4e7".to_string(),
                    retrieved_at: Utc::now(),
                    provider_revision: Some("2026-08-12".to_string()),
                    raw_content: sample_navaids.to_string(),
                };
                let r_nav = OurAirportsImporter::ingest_dataset(&dataset_nav, &store)?;

                println!("  Sync Report:");
                println!(
                    "    Airports: Accepted {}, Rejected {}",
                    r_ap.records_accepted, r_ap.records_rejected
                );
                println!(
                    "    Runways: Accepted {}, Rejected {}",
                    r_rwy.records_accepted, r_rwy.records_rejected
                );
                println!(
                    "    Navaids: Accepted {}, Rejected {}",
                    r_nav.records_accepted, r_nav.records_rejected
                );
                println!("✅ Synchronization completed successfully.");
            } else {
                println!("  Unknown provider: {}", provider);
            }
        }

        Commands::Status { db } => {
            if !db.exists() {
                println!("Database not found at {:?}", db);
                std::process::exit(1);
            }
            let store = WorldStore::open(db)?;
            let status = store.status()?;
            let meta = wmm2025_metadata();

            println!("OpenAIRAC World Database Status");
            println!("==============================");
            println!("  Path: {}", status.database_path);
            println!(
                "  Integrity: {}",
                if status.integrity_ok { "OK" } else { "FAILED" }
            );
            println!("  Migration Version: {}", status.migration_version);
            println!(
                "  Latest Revision: {}",
                status.latest_revision_id.as_deref().unwrap_or("None")
            );
            println!("  Source Snapshots: {}", status.total_snapshots);
            println!("  Airports: {}", status.total_airports);
            println!("  Runways: {}", status.total_runways);
            println!("  Navaids: {}", status.total_navaids);
            println!("  Waypoints: {}", status.total_waypoints);
            println!(
                "  Magnetic Model: {} (Valid {:.1}-{:.1})",
                meta.model, meta.valid_from_year, meta.valid_until_year
            );
        }

        Commands::Export { target } => match target {
            ExportTarget::Xplane { db, out, date: _ } => {
                let utc_date = Utc::now();
                println!("🛫 Exporting X-Plane 12 Navigation Data...");
                println!("  Database: {:?}", db);
                println!("  Output Directory: {:?}", out);

                let store = WorldStore::open(db)?;
                let (wp_cnt, nav_cnt) = XPlane12Exporter::export_from_db(&store, utc_date, out)?;

                println!("  Exported {} waypoints to earth_fix.dat", wp_cnt);
                println!("  Exported {} navaids to earth_nav.dat", nav_cnt);
                println!("✅ X-Plane 12 export complete.");
            }
        },
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso_date_to_year_decimal() {
        let dec = parse_iso_date_to_year_decimal("2026-08-12").unwrap();
        assert!((dec - 2026.61).abs() < 0.05);

        let dec_raw = parse_iso_date_to_year_decimal("2026.5").unwrap();
        assert_eq!(dec_raw, 2026.5);
    }
}
