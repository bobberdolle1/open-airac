use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use magnetic::{analyze_runway_magnetic_drift, Wmm2025};
use openairac_core::OurAirportsParser;
use openairac_exporter::XPlane12Exporter;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

#[derive(Parser)]
#[command(name = "openairac")]
#[command(
    about = "✈️ OpenAIRAC — The open navigation data engine for flight simulation. Install once, stay current automatically.",
    long_about = None
)]
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

    /// Create an automatic auto-sync launcher script for X-Plane 12
    InstallLauncher {
        /// Path to X-Plane 12 installation directory
        #[arg(short, long)]
        path: String,
    },

    /// Calculate genuine NOAA WMM2025 magnetic variation for location
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

    /// Inspect magnetic drift for official vs computed runway designators
    Magdrift {
        /// Official runway designator (e.g. "09")
        #[arg(short, long)]
        designator: String,

        /// True heading of runway in degrees
        #[arg(short, long)]
        heading: f64,

        /// Latitude
        #[arg(short, long)]
        lat: f64,

        /// Longitude
        #[arg(short, long)]
        lon: f64,

        /// Year (e.g. 2026.0)
        #[arg(short, long, default_value_t = 2026.0)]
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

                println!("⚙️ Parsing navaids & computing NOAA WMM2025 magnetic variations...");
                let navaids = OurAirportsParser::parse_navaids(response.as_bytes(), *year)?;
                println!("✅ Processed {} canonical navaids.", navaids.len());

                let nav_file_path = custom_data_dir.join("earth_nav.dat");
                let file = File::create(&nav_file_path)?;
                XPlane12Exporter::export_earth_nav(&navaids, file)?;
                println!("💾 Written native X-Plane 12 file: {:?}", nav_file_path);

                println!("🎉 OpenAIRAC sync completed successfully! Restart X-Plane 12 to apply.");
            } else {
                println!("⚠️ Simulator '{}' packaging is under active development.", sim);
            }
        }
        Commands::InstallLauncher { path } => {
            let xp_path = Path::new(path);
            let exe_path = std::env::current_exe()?;
            let bat_path = xp_path.join("Start_XPlane12_OpenAIRAC.bat");

            let bat_content = format!(
                "@echo off\n\
                 title OpenAIRAC Auto-Sync Launcher\n\
                 echo [OpenAIRAC] Checking and updating navigation data before launch...\n\
                 \"{}\" sync --sim xp12 --path \"{}\"\n\
                 echo [OpenAIRAC] Launching X-Plane 12...\n\
                 start \"\" \"{}\\X-Plane.exe\"\n",
                exe_path.display(),
                xp_path.display(),
                xp_path.display()
            );

            let mut file = File::create(&bat_path)?;
            file.write_all(bat_content.as_bytes())?;
            println!("✅ Auto-sync launcher created at: {:?}", bat_path);
        }
        Commands::Magvar { lat, lon, year } => {
            let wmm = Wmm2025::calculate(*lat, *lon, 0.0, *year);
            println!("🧭 Lat: {}, Lon: {}, Year: {}", lat, lon, year);
            println!("📍 Magnetic Declination (Variation): {:.2}°", wmm.declination_deg);
            println!("📍 Dip Angle (Inclination): {:.2}°", wmm.inclination_deg);
            println!("📍 Total Intensity: {:.1} nT", wmm.total_intensity_nt);
        }
        Commands::Magdrift {
            designator,
            heading,
            lat,
            lon,
            year,
        } => {
            let analysis = analyze_runway_magnetic_drift(designator, *heading, *lat, *lon, *year);
            println!("✈️ Runway Drift Analysis for RWY {}", analysis.official_designator);
            println!("   True Heading: {:.1}°", analysis.true_heading_deg);
            println!("   WMM2025 MagVar: {:.2}°", analysis.wmm_magvar_deg);
            println!("   Computed Magnetic Heading: {:.1}°", analysis.computed_magnetic_heading_deg);
            println!("   Computed Magnetic Candidate: {}", analysis.computed_magnetic_designator);

            if analysis.is_redesignation_suggested {
                println!(
                    "⚠️  WARNING: Magnetic drift threshold exceeded! Official: {}, Computed: {} (Difference: {:.1}°)",
                    analysis.official_designator, analysis.computed_magnetic_designator, analysis.drift_difference_deg
                );
            } else {
                println!("✅ Runway designator aligns with official charts.");
            }
        }
    }

    Ok(())
}
