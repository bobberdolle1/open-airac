use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use openairac_core::{OurAirportsParser, WmmCalculator};
use openairac_exporter::XPlane12Exporter;
use std::fs::{self, File};
use std::io::Write;
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

    /// Create an automatic auto-sync launcher script for X-Plane 12
    InstallLauncher {
        /// Path to X-Plane 12 installation directory
        #[arg(short, long)]
        path: String,
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
            println!("💡 Run 'Start_XPlane12_OpenAIRAC.bat' or set it as Steam Launch Option!");
        }
        Commands::Magvar { lat, lon, year } => {
            let declination = WmmCalculator::calculate_declination(*lat, *lon, 0.0, *year);
            println!("🧭 Lat: {}, Lon: {}, Year: {}", lat, lon, year);
            println!("📍 Magnetic Declination (Variation): {:.1}°", declination);
        }
    }

    Ok(())
}
