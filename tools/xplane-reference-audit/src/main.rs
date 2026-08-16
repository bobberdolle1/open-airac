mod airport_report;
mod compare_airways;
mod compare_fixes;
mod compare_nav;
mod compare_procedures;
mod diff_summary;
mod ingest_export;
mod parser;

use anyhow::Result;
use clap::{Parser, Subcommand};
use parser::PackageSource;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "xplane-reference-audit")]
#[command(about = "Generic X-Plane navdata forensic & differential audit utility")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Inspect and print package inventory
    Inventory {
        /// Path to navdata directory or .zip archive
        path: PathBuf,
    },
    /// Ingest a CIFP master file and export X-Plane 12 dat files
    IngestExport {
        /// Path to CIFP master file (e.g. FAACIFP18)
        cifp: PathBuf,
        /// Path to output SQLite db
        #[arg(long, default_value = "./target/world_audit_2608.sqlite")]
        db: PathBuf,
        /// Output directory for exported X-Plane 12 dat files
        #[arg(long, default_value = "./target/openairac_exported_2608")]
        out_dir: PathBuf,
    },
    /// Compare fixes (earth_fix.dat) between two packages
    CompareFixes {
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
        #[arg(long, default_value = "50")]
        max_samples: usize,
    },
    /// Compare navaids (earth_nav.dat) between two packages
    CompareNav {
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
        #[arg(long, default_value = "100")]
        max_discrepancies: usize,
    },
    /// Compare airways (earth_awy.dat) between two packages
    CompareAirways {
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
        #[arg(long, default_value = "50")]
        max_samples: usize,
    },
    /// Compare procedures across airports between two packages
    CompareProcedures {
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
        #[arg(long)]
        max_airports: Option<usize>,
        #[arg(long, default_value = "50")]
        max_samples: usize,
    },
    /// Generate deep airport discrepancy report (e.g. KSFO, KDEN, KJFK, KLAX, KORD)
    AirportReport {
        /// ICAO airport ident (e.g. KSFO)
        airport: String,
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
    },
    /// Run comprehensive differential summary across all layers
    DiffSummary {
        /// Path to Package A (reference / baseline)
        pkg_a: PathBuf,
        /// Path to Package B (target / test)
        pkg_b: PathBuf,
        #[arg(long)]
        max_airports: Option<usize>,
        #[arg(long, default_value = "50")]
        max_samples: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Inventory { path } => {
            let pkg = PackageSource::open(&path)?;
            println!("Package source: {:?}", path);
            let cifp_files = pkg.list_cifp_files()?;
            println!("CIFP procedure files found: {}", cifp_files.len());

            for standard_file in &[
                "earth_fix.dat",
                "earth_nav.dat",
                "earth_awy.dat",
                "earth_hold.dat",
                "earth_msa.dat",
                "earth_mora.dat",
                "earth_aptmeta.dat",
                "cycle_info.txt",
                "cycle.json",
            ] {
                match pkg.read_file(standard_file)? {
                    Some(content) => {
                        let lines = content.lines().count();
                        let header = content.lines().take(2).collect::<Vec<_>>().join(" | ");
                        println!(
                            "  {:20} -> {:8} bytes, {:6} lines | Header: {}",
                            standard_file,
                            content.len(),
                            lines,
                            header
                        );
                    }
                    None => {
                        println!("  {:20} -> [NOT PRESENT]", standard_file);
                    }
                }
            }
        }
        Commands::IngestExport { cifp, db, out_dir } => {
            use chrono::TimeZone;
            let effective = chrono::Utc
                .with_ymd_and_hms(2026, 8, 6, 9, 0, 0)
                .single()
                .unwrap();
            ingest_export::run_ingest_and_export(&cifp, &db, &out_dir, effective)?;
        }
        Commands::CompareFixes {
            pkg_a,
            pkg_b,
            max_samples,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let report = compare_fixes::compare_fixes(&pa, &pb, max_samples)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::CompareNav {
            pkg_a,
            pkg_b,
            max_discrepancies,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let report = compare_nav::compare_nav(&pa, &pb, max_discrepancies)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::CompareAirways {
            pkg_a,
            pkg_b,
            max_samples,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let report = compare_airways::compare_airways(&pa, &pb, max_samples)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::CompareProcedures {
            pkg_a,
            pkg_b,
            max_airports,
            max_samples,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let report =
                compare_procedures::compare_procedures_global(&pa, &pb, max_airports, max_samples)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::AirportReport {
            airport,
            pkg_a,
            pkg_b,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let report = airport_report::generate_airport_report(&airport, &pa, &pb)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Commands::DiffSummary {
            pkg_a,
            pkg_b,
            max_airports,
            max_samples,
        } => {
            let pa = PackageSource::open(&pkg_a)?;
            let pb = PackageSource::open(&pkg_b)?;
            let summary = diff_summary::run_diff_summary(&pa, &pb, max_airports, max_samples)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
    }

    Ok(())
}
