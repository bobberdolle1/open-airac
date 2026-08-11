use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use openairac_core::{OurAirportsParser, WmmCalculator};
use openairac_exporter::XPlane12Exporter;
use std::fs::{self, File};
use std::path::Path;

#[derive(Parser)]
#[command(name = "openairac")]
#[command(about = "✈️ OpenAIRAC CLI - Math-Driven Navigation Engine for Flight Simulators", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Synchronize open navigation data with X-Plane 12 or MSFS 2024
    Sync {
        /// Simulator type (xp12, msfs)
        #[arg(short, long, default_value = "xp12")]
        sim: String,

        /// Path to simulator installation
        #[arg(short, long)]
        path: String,

        /// Target year for dynamic magnetic variation (e.g. 2026.6)
        #[arg(short, long, default_value_t = 2026.6)]
        year: f64,
    },

    /// Calculate dynamic World Magnetic Model variation for location
    Magvar {
        /// Latitude
        #[arg(short, long)]
        lat: f64,

        /// Longitude
        #[arg(short, long)]
        lon: f64,

        /// Year (e.g. 2026.6)
        #[arg(short, long, default_value_t = 2026.6)]
        year: f64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Sync { sim, path, year } => {
            println!("🚀 Starting OpenAIRAC sync for {} at '{}' (Year: {})...", sim, path, year);

            if sim == "xp12" {
                let custom_data_dir = Path::new(path).join("Custom Data");
                if !custom_data_dir.exists() {
                    fs::create_dir_all(&custom_data_dir)
                        .context("Failed to create Custom Data directory")?;
                }

                println!("📡 Fetching open navaids data from OurAirports...");
                let navaid_url = "https://davidmegginson.github.io/ourairports-data/navaids.csv";
                let response = reqwest::get(navaid_url).await?.text().await?;

                println!("⚙️ Parsing navaids & computing dynamic WMM magnetic variations...");
                let navaids = OurAirportsParser::parse_navaids(response.as_bytes(), *year)?;
                println!("✅ Processed {} navaids.", navaids.len());

                let nav_file_path = custom_data_dir.join("earth_nav.dat");
                let file = File::create(&nav_file_path)?;
                XPlane12Exporter::export_earth_nav(&navaids, file)?;
                println!("💾 Written native X-Plane 12 file: {:?}", nav_file_path);

                println!("🎉 OpenAIRAC sync completed successfully! Restart X-Plane 12 to apply.");
            } else {
                println!("⚠️ Simulator '{}' packaging is under active development.", sim);
            }
        }
        Commands::Magvar { lat, lon, year } => {
            let declination = WmmCalculator::calculate_declination(*lat, *lon, 0.0, *year);
            println!("🧭 Lat: {}, Lon: {}, Year: {}", lat, lon, year);
            println!("📍 Magnetic Declination (Variation): {:.1}°", declination);
        }
    }

    Ok(())
}
