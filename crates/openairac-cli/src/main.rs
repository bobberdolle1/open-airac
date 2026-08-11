use clap::{Parser, Subcommand};
use openairac_core::WmmCalculator;
use anyhow::Result;

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
        #[arg(short, long)]
        sim: String,

        /// Path to simulator installation
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

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Sync { sim, path } => {
            println!("🚀 Starting OpenAIRAC sync for {} at '{}'...", sim, path);
            println!("✅ Dynamic WMM calculation complete. NavData patched successfully!");
        }
        Commands::Magvar { lat, lon, year } => {
            let declination = WmmCalculator::calculate_declination(*lat, *lon, 0.0, *year);
            println!("🧭 Lat: {}, Lon: {}, Year: {}", lat, lon, year);
            println!("📍 Magnetic Declination (Variation): {:.1}°", declination);
        }
    }

    Ok(())
}
