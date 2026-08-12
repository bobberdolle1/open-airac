use anyhow::Result;
use clap::{Parser, Subcommand};
use openairac_magnetic::{Wmm2025, analyze_runway_magnetic_drift};

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
    /// Calculate WMM2025 magnetic variation for location
    Magvar {
        #[arg(short, long)]
        lat: f64,
        #[arg(short, long)]
        lon: f64,
        #[arg(short, long, default_value_t = 0.0)]
        alt_ft: f64,
        #[arg(short, long, default_value_t = 2026.0)]
        year: f64,
    },

    /// Inspect magnetic drift for official vs computed runway designators
    Magdrift {
        #[arg(short, long)]
        designator: String,
        #[arg(short, long)]
        heading: f64,
        #[arg(short, long)]
        lat: f64,
        #[arg(short, long)]
        lon: f64,
        #[arg(short, long, default_value_t = 2026.0)]
        year: f64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Magvar {
            lat,
            lon,
            alt_ft,
            year,
        } => {
            let res = Wmm2025::calculate(*lat, *lon, *alt_ft, *year);
            println!("WMM2025 Calculation Result:");
            println!("  Latitude: {:.4}°", lat);
            println!("  Longitude: {:.4}°", lon);
            println!("  Altitude: {:.0} ft", alt_ft);
            println!("  Year: {:.2}", year);
            println!("  Declination: {:.4}°", res.declination_deg);
            println!("  Inclination: {:.4}°", res.inclination_deg);
            println!("  Total Intensity: {:.1} nT", res.total_intensity_nt);
            println!(
                "  Horizontal Intensity: {:.1} nT",
                res.horizontal_intensity_nt
            );
        }
        Commands::Magdrift {
            designator,
            heading,
            lat,
            lon,
            year,
        } => {
            let analysis = analyze_runway_magnetic_drift(designator, *heading, *lat, *lon, *year);
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
            println!("  Drift Difference: {:.2}°", analysis.drift_difference_deg);
            println!(
                "  Redesignation Suggested: {}",
                analysis.is_redesignation_suggested
            );
        }
    }

    Ok(())
}
