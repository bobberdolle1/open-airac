use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::provider::{FetchedDataset, sha256_hex};
use openairac_magnetic::{Wmm2025, analyze_runway_magnetic_drift, wmm2025_metadata};
use openairac_store::WorldStore;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "openairac")]
#[command(
    about = "OpenAIRAC — The open navigation data engine for flight simulation. Install once, navigate forever."
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
        #[arg(short = 'l', long, allow_negative_numbers = true)]
        lat: f64,
        #[arg(short = 'o', long, allow_negative_numbers = true)]
        lon: f64,
        #[arg(short, long, default_value_t = 0.0)]
        alt_ft: f64,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },

    /// Alias for magnetic command
    Magvar {
        #[arg(short = 'l', long, allow_negative_numbers = true)]
        lat: f64,
        #[arg(short = 'o', long, allow_negative_numbers = true)]
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
        #[arg(short = 't', long, allow_negative_numbers = true)]
        heading: f64,
        #[arg(short = 'l', long, allow_negative_numbers = true)]
        lat: f64,
        #[arg(short = 'o', long, allow_negative_numbers = true)]
        lon: f64,
        #[arg(long, default_value = "2026-08-12")]
        date: String,
    },

    /// Synchronize navigation data from a provider
    Sync {
        #[arg(short, long, default_value = "ourairports")]
        provider: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Use offline sample fixture content instead of live network
        #[arg(long, default_value_t = false)]
        fixture: bool,
        /// Comma-separated datasets (default: all supported by the provider)
        #[arg(long)]
        datasets: Option<String>,
        /// AIRAC cycle ident (required for cycle-aware providers like faa_cifp)
        #[arg(long)]
        cycle: Option<String>,
        /// Publication kind: baseline (full snapshot), differential
        /// (changes only; absence means nothing), correction
        /// (re-publishes/replaces publication state)
        #[arg(long, default_value = "baseline")]
        kind: String,
        /// Explicit publication identity (replay/conflict detection)
        #[arg(long)]
        publication: Option<String>,
    },

    /// AIRAC cycle catalog: discovery and inspection
    Cycle {
        #[command(subcommand)]
        cmd: CycleCmd,
    },

    /// Display local world database revision, entity counts, and status
    Status {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// Validate the canonical store's structural integrity
    Validate {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// Run multi-source entity reconciliation and report statistics
    Reconcile {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Reconcile the world valid at this instant (RFC3339); default: now
        #[arg(long)]
        as_of: Option<String>,
    },

    /// Versioned data bundles: build, inspect, verify, install
    Bundle {
        #[command(subcommand)]
        cmd: BundleCmd,
    },

    /// Local update channel: check and apply
    Update {
        #[command(subcommand)]
        cmd: UpdateCmd,
    },

    /// Coverage report per provider and country
    Coverage {
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
enum BundleCmd {
    /// Build a deterministic bundle from the canonical store
    Build {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./bundles")]
        out: PathBuf,
    },
    /// Inspect a bundle's manifest metadata
    Inspect {
        #[arg(short, long)]
        bundle: PathBuf,
    },
    /// Verify a bundle's integrity (fail-closed)
    Verify {
        #[arg(short, long)]
        bundle: PathBuf,
    },
    /// Install a bundle into a local root (staged, validated, swapped)
    Install {
        #[arg(short, long)]
        root: PathBuf,
        #[arg(short, long)]
        bundle: PathBuf,
    },
    /// List installed bundle state (current / next)
    List {
        #[arg(short, long)]
        root: PathBuf,
    },
    /// Roll back to the previous installed artifact
    Rollback {
        #[arg(short, long)]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum UpdateCmd {
    /// Compare installed state against a local channel
    Check {
        #[arg(short, long)]
        root: PathBuf,
        #[arg(short, long)]
        channel: PathBuf,
    },
    /// Verify and install the channel's latest bundle
    Apply {
        #[arg(short, long)]
        root: PathBuf,
        #[arg(short, long)]
        channel: PathBuf,
    },
}

#[derive(Subcommand)]
enum CycleCmd {
    /// Discover published cycles from the FAA CIFP directory (live network)
    /// and record them in the catalog. Effective dates stay unconfirmed
    /// until `cycle confirm` (future milestone).
    Discover {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// List the AIRAC cycle catalog
    List {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// Advance cycle bookkeeping to the current time: activate preloaded
    /// cycles whose effective date has passed (idempotent), supersede
    /// replaced cycles, mark expired windows.
    Observe {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },

    /// Roll an Active cycle back: re-publish the pre-cycle world state as
    /// new revisions (history is preserved, other providers untouched).
    Rollback {
        #[arg(short, long)]
        cycle: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Rollback instant (RFC3339); default: now
        #[arg(long)]
        at: Option<String>,
    },
}

#[derive(Subcommand)]
enum ExportTarget {
    /// Export X-Plane 12 dat files (earth_fix.dat, earth_nav.dat, earth_awy.dat)
    Xplane {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./dist/xplane")]
        out: PathBuf,
        /// Effective date for the export (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        date: Option<String>,
        /// Allow exporting an empty nav layer (DANGEROUS: overwrites the
        /// simulator's navaids/fixes with an empty file)
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
}

fn parse_iso_date_to_year_decimal(date_str: &str) -> Result<f64> {
    if let Ok(year_dec) = date_str.parse::<f64>() {
        return Ok(year_dec);
    }
    let d = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .with_context(|| format!("Invalid ISO date format '{date_str}' (expected YYYY-MM-DD)"))?;

    let year = d.year() as f64;
    let day_of_year = d.ordinal() as f64;
    let days_in_year = if d.leap_year() { 366.0 } else { 365.0 };

    Ok(year + (day_of_year - 1.0) / days_in_year)
}

fn parse_export_date(date: &Option<String>) -> Result<chrono::DateTime<Utc>> {
    match date {
        None => Ok(Utc::now()),
        Some(s) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Ok(dt.with_timezone(&Utc));
            }
            let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .with_context(|| format!("Invalid export date '{s}' (expected YYYY-MM-DD)"))?;
            let dt = d.and_hms_opt(9, 0, 0).context("building export datetime")?;
            Ok(chrono::DateTime::from_naive_utc_and_offset(dt, Utc))
        }
    }
}

fn sync_fixture(store: &mut WorldStore) -> Result<()> {
    let sample_airports = r#"id,ident,type,name,latitude_deg,longitude_deg,elevation_ft,iso_country,municipality
1,KSFO,large_airport,San Francisco International Airport,37.6188,-122.3750,13,US,San Francisco
2,KJFK,large_airport,John F Kennedy International Airport,40.6398,-73.7789,13,US,New York
3,BAD,,Airport With Bad Latitude,95.0,-122.3750,13,US,Nowhere
"#;
    let sample_runways = r#"id,airport_ref,airport_ident,length_ft,width_ft,surface,le_ident,le_latitude_deg,le_longitude_deg,le_elevation_ft,le_heading_degT,he_ident,he_latitude_deg,he_longitude_deg,he_elevation_ft
101,1,KSFO,11870,200,ASP,28R,37.6188,-122.3750,13,284.0,10L,37.6140,-122.3900,11
102,2,KJFK,14511,200,ASP,13L,40.6398,-73.7789,13,,31R,40.6200,-73.7500,11
"#;
    let sample_navaids = r#"id,filename,ident,name,type,frequency_khz,latitude_deg,longitude_deg,elevation_ft,associated_airport,magnetic_variation_deg
201,SFO.navaid,SFO,San Francisco VOR-DME,VOR-DME,115800,37.6195,-122.3739,13,KSFO,-13.0
202,JFK.navaid,JFK,Kennedy VOR-DME,VOR-DME,115900,40.6397,-73.7789,13,KJFK,-13.0
"#;

    for (dataset, content) in [
        ("airports", sample_airports),
        ("runways", sample_runways),
        ("navaids", sample_navaids),
    ] {
        let dataset = FetchedDataset {
            provider_name: "OurAirports".to_string(),
            dataset_name: dataset.to_string(),
            source_uri: "offline fixture".to_string(),
            content_sha256: sha256_hex(content.as_bytes()),
            retrieved_at: Utc::now(),
            provider_revision: Some("fixture".to_string()),
            airac_cycle: None,
            revision_kind: openairac_model::RevisionKind::Baseline,
            coverage: openairac_model::Coverage::FullSnapshot,
            valid_from: None,
            publication_id: None,
            raw_content: content.to_string(),
        };
        let report =
            openairac_ingest::ourairports::OurAirportsImporter::ingest_dataset(&dataset, store)?;
        println!(
            "  {}: accepted {}, unchanged {}, quarantined {}, rejected {}",
            report.dataset_name,
            report.records_accepted(),
            report.records_unchanged,
            report.records_quarantined,
            report.records_rejected
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Doctor { db } => {
            println!("OpenAIRAC System Doctor");
            println!("======================");
            println!("  CLI Version: {}", env!("CARGO_PKG_VERSION"));
            let meta = wmm2025_metadata();
            println!("  Magnetic Model: {} (Epoch {})", meta.model, meta.epoch);

            if !db.exists() {
                println!("  Database: NOT FOUND at {:?}", db);
                println!("  Run `openairac sync` to initialize local database.");
                std::process::exit(1);
            }

            match WorldStore::open(db) {
                Ok(store) => match store.status() {
                    Ok(status) => {
                        println!("  Database Open: OK ({:?})", db);
                        println!(
                            "  Integrity Check: {}",
                            if status.integrity_ok { "OK" } else { "FAILED" }
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
                        println!("  Airway Legs: {}", status.total_airway_legs);
                    }
                    Err(e) => {
                        println!("  Database Status Query: ERROR ({e})");
                        std::process::exit(1);
                    }
                },
                Err(e) => {
                    println!("  Database Connection: FAILED ({e})");
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
            println!("  Date: {date} (Decimal Year {year:.4})");
            println!("  Latitude: {lat:.4}°");
            println!("  Longitude: {lon:.4}°");
            println!("  Altitude: {alt_ft:.1} ft");
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
                    "YES (Candidate mismatch detected)"
                } else {
                    "NO"
                }
            );
        }

        Commands::Sync {
            provider,
            db,
            fixture,
            datasets,
            cycle,
            kind,
            publication,
        } => {
            println!("Synchronizing OpenAIRAC Navigation Data...");
            println!("  Provider: {provider}");
            println!("  Database: {:?}", db);

            let mut store = WorldStore::open(db)?;

            let known: Vec<&str> = openairac_ingest::registry::provider_constructors()
                .iter()
                .map(|(k, _)| *k)
                .collect();
            if !known.contains(&provider.as_str()) {
                anyhow::bail!(
                    "Unknown provider '{provider}' (supported: {})",
                    known.join(", ")
                );
            }

            if provider == "faa_cifp" {
                if *fixture {
                    anyhow::bail!("--fixture is not supported for faa_cifp");
                }
                let (revision_kind, coverage) = match kind.as_str() {
                    "baseline" => (
                        openairac_model::RevisionKind::Baseline,
                        openairac_model::Coverage::FullSnapshot,
                    ),
                    "differential" => (
                        openairac_model::RevisionKind::Baseline,
                        openairac_model::Coverage::Partial,
                    ),
                    "correction" => (
                        openairac_model::RevisionKind::Correction,
                        openairac_model::Coverage::FullSnapshot,
                    ),
                    other => anyhow::bail!(
                        "unknown --kind '{other}' (supported: baseline, differential, correction)"
                    ),
                };
                let Some(cycle_ident) = cycle.as_deref() else {
                    anyhow::bail!(
                        "--cycle <ident> is required for faa_cifp (discover cycles with `openairac cycle discover`)"
                    );
                };
                let catalog_cycle = store
                    .query_cycle(&openairac_model::CycleId(cycle_ident.to_string()))?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "cycle '{cycle_ident}' is not in the catalog; run `openairac cycle discover --db <db>` first"
                        )
                    })?;
                let Some(source_uri) = catalog_cycle.source_uri.as_deref() else {
                    anyhow::bail!("cycle '{cycle_ident}' has no source URI");
                };
                // Fail-closed: never sync/preload a cycle whose effective
                // date is unconfirmed — the data would land at the wrong
                // instant.
                let Some(effective_from) = catalog_cycle.effective_from else {
                    anyhow::bail!(
                        "cycle '{cycle_ident}' has UNCONFIRMED effective dates; \
                         confirm them before syncing/preloading"
                    );
                };
                let selector = openairac_ingest::provider::CycleSelector {
                    cycle_ident: cycle_ident.to_string(),
                    source_uri: source_uri.to_string(),
                    effective_from: Some(effective_from),
                };
                let provider = openairac_ingest::faa_cifp::CifpProvider;
                let requested: Vec<String> = datasets
                    .as_deref()
                    .map(|d| d.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["FAACIFP18".to_string()]);
                for dataset_name in requested {
                    println!("  Fetching {dataset_name} for cycle {cycle_ident}...");
                    let mut dataset = openairac_ingest::provider::DataProvider::fetch(
                        &provider,
                        &dataset_name,
                        Some(&selector),
                    )?;
                    dataset.revision_kind = revision_kind;
                    dataset.coverage = coverage;
                    dataset.publication_id = publication.clone();
                    println!(
                        "    fetched {} bytes from {}",
                        dataset.raw_content.len(),
                        dataset.source_uri
                    );
                    let report = openairac_ingest::provider::DataProvider::parse_and_ingest(
                        &provider, &dataset, &mut store,
                    )?;
                    println!(
                        "    {}: seen {}, accepted {}, unchanged {}, quarantined {}, rejected {}, {} ms",
                        report.dataset_name,
                        report.records_seen,
                        report.records_accepted(),
                        report.records_unchanged,
                        report.records_quarantined,
                        report.records_rejected,
                        report.duration_ms
                    );
                    for (kind, count) in &report.kind_counts {
                        println!("    {kind}: {count}");
                    }
                }
            } else if *fixture {
                println!("  Using offline fixture content.");
                sync_fixture(&mut store)?;
            } else {
                let importer =
                    openairac_ingest::registry::provider("ourairports").expect("registered");
                let requested: Vec<String> = datasets
                    .as_deref()
                    .map(|d| d.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_else(|| {
                        vec![
                            "airports".to_string(),
                            "runways".to_string(),
                            "navaids".to_string(),
                        ]
                    });
                for dataset_name in requested {
                    println!("  Fetching {dataset_name}...");
                    let dataset = importer.fetch(&dataset_name, None)?;
                    println!(
                        "    fetched {} bytes from {}",
                        dataset.raw_content.len(),
                        dataset.source_uri
                    );
                    let report = importer.parse_and_ingest(&dataset, &mut store)?;
                    println!(
                        "    {}: seen {}, accepted {}, unchanged {}, quarantined {}, rejected {}, {} ms",
                        report.dataset_name,
                        report.records_seen,
                        report.records_accepted(),
                        report.records_unchanged,
                        report.records_quarantined,
                        report.records_rejected,
                        report.duration_ms
                    );
                    for warning in report.warnings.iter().take(5) {
                        println!("    warning: {warning}");
                    }
                    if report.warnings.len() > 5 {
                        println!("    ... {} more warnings", report.warnings.len() - 5);
                    }
                    for error in &report.errors {
                        println!("    error: {error}");
                    }
                }
            }

            let status = store.status()?;
            println!("Synchronization completed.");
            println!("  Airports: {}", status.total_airports);
            println!("  Runways: {}", status.total_runways);
            println!("  Navaids: {}", status.total_navaids);
            println!("  Waypoints: {}", status.total_waypoints);
            println!("  Airway Legs: {}", status.total_airway_legs);
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
            println!("  Airway Legs: {}", status.total_airway_legs);
            println!(
                "  Magnetic Model: {} (Valid {:.1}-{:.1})",
                meta.model, meta.valid_from_year, meta.valid_until_year
            );
        }

        Commands::Coverage { db } => {
            let store = WorldStore::open(db)?;
            let service = openairac_service::WorldQuery::from_store(store);
            let report = service.coverage_report(chrono::Utc::now())?;
            println!("OpenAIRAC Coverage Report (as of {})", report.as_of);
            for p in &report.providers {
                println!(
                    "  {}: {} ({}, {})",
                    p.provider, p.coverage, p.temporal, p.update
                );
                println!(
                    "    airports {} runways {} navaids {} waypoints {} airways {} procedure legs {} snapshots {}",
                    p.airports,
                    p.runways,
                    p.navaids,
                    p.waypoints,
                    p.airway_legs,
                    p.procedure_legs,
                    p.snapshots
                );
            }
            let total: usize = report.airports_by_country.iter().map(|(_, n)| n).sum();
            println!(
                "  countries with airports: {} (total {})",
                report.airports_by_country.len(),
                total
            );
        }
        Commands::Reconcile { db, as_of } => {
            let as_of = match as_of {
                Some(s) => chrono::DateTime::parse_from_rfc3339(s)
                    .map(|t| t.with_timezone(&chrono::Utc))?,
                None => chrono::Utc::now(),
            };
            let store = WorldStore::open(db)?;
            let stats = openairac_reconcile::Reconciler::new(&store).reconcile(as_of)?;
            println!("OpenAIRAC Entity Reconciliation (as of {as_of})");
            println!("=============================================");
            println!("  Source entities considered: {}", stats.source_entities);
            println!("  Candidate pairs:            {}", stats.candidate_pairs);
            println!("  Exact matches:              {}", stats.exact_matches);
            println!("  Probable matches:           {}", stats.probable_matches);
            println!("  Ambiguous (no merge):       {}", stats.ambiguous);
            println!("  Conflicts:                  {}", stats.conflicts);
            println!("  Distinct/rejected:          {}", stats.distinct_rejected);
            let conflicts = store.query_reconciliation_conflicts()?;
            if !conflicts.is_empty() {
                println!("\nConflicts (first 10):");
                for c in conflicts.iter().take(10) {
                    println!(
                        "  [{}] {} {} <-> {}: {}",
                        c.severity.as_str(),
                        c.entity_table,
                        c.ref_a,
                        c.ref_b,
                        c.category
                    );
                }
            }
        }
        Commands::Validate { db } => {
            if !db.exists() {
                println!("Database not found at {:?}", db);
                std::process::exit(1);
            }
            let store = WorldStore::open(db)?;
            let issues = store.validate()?;
            if issues.is_empty() {
                println!("Canonical store is structurally valid.");
            } else {
                println!("Found {} structural issue(s):", issues.len());
                for issue in &issues {
                    println!(
                        "  [{}] {} {}: {}",
                        issue.severity, issue.table, issue.id, issue.message
                    );
                }
                std::process::exit(1);
            }
        }

        Commands::Bundle { cmd } => match cmd {
            BundleCmd::Build { db, out } => {
                let store = WorldStore::open(db)?;
                let (hash, dir) = openairac_bundle::build_bundle(&store, out, chrono::Utc::now())?;
                println!("Bundle built: {}", dir.display());
                println!("  bundle hash: {hash}");
            }
            BundleCmd::Inspect { bundle } => {
                let manifest = openairac_bundle::inspect_bundle(bundle)?;
                println!("Bundle: {}", manifest.bundle_hash);
                println!("  format version: {}", manifest.core.format_version);
                println!("  schema version: {}", manifest.core.schema_version);
                println!("  generated at:   {}", manifest.generated_at);
                println!("  effective from: {}", manifest.core.effective_from);
                println!(
                    "  AIRAC cycle:    {}",
                    manifest.core.airac_cycle.as_deref().unwrap_or("-")
                );
                println!("  providers:      {}", manifest.core.providers.join(", "));
                println!("  publications:   {}", manifest.core.publications.len());
                println!(
                    "  provenance:     {} snapshots",
                    manifest.core.provenance.len()
                );
                println!(
                    "  reconciliation: {} canonical, {} memberships, {} conflicts",
                    manifest.core.reconciliation.canonical_entities,
                    manifest.core.reconciliation.memberships,
                    manifest.core.reconciliation.conflicts
                );
                println!("  authenticity:   {}", manifest.core.authenticity);
                for file in &manifest.core.files {
                    println!(
                        "  file: {} ({} bytes, sha256 {})",
                        file.path, file.size, file.sha256
                    );
                }
            }
            BundleCmd::Verify { bundle } => {
                let report = openairac_bundle::verify_bundle(bundle)?;
                println!(
                    "Bundle verified: {} ({} file(s), {})",
                    report.bundle_hash,
                    report.files,
                    report.authenticity.as_str()
                );
            }
            BundleCmd::Install { root, bundle } => {
                let report = openairac_bundle::install_bundle(root, bundle, chrono::Utc::now())?;
                if report.preloaded {
                    println!(
                        "Bundle preloaded as NEXT (effective {})",
                        report.effective_from
                    );
                } else {
                    println!(
                        "Bundle installed as CURRENT (effective {})",
                        report.effective_from
                    );
                }
                println!("  bundle hash: {}", report.bundle_hash);
            }
            BundleCmd::List { root } => match openairac_bundle::load_installed(root) {
                Ok(state) => {
                    println!("Installed bundles:");
                    match &state.current {
                        Some(c) => println!(
                            "  current: {} (effective {})",
                            c.bundle_hash, c.effective_from
                        ),
                        None => println!("  current: (none)"),
                    }
                    match &state.next {
                        Some(n) => println!(
                            "  next:    {} (effective {})",
                            n.bundle_hash, n.effective_from
                        ),
                        None => println!("  next:    (none)"),
                    }
                }
                Err(_) => println!("No installed state at {}.", root.display()),
            },
            BundleCmd::Rollback { root } => {
                let hash = openairac_bundle::rollback_bundle(root, chrono::Utc::now())?;
                println!("Rolled back to previous artifact: {hash}");
            }
        },
        Commands::Update { cmd } => match cmd {
            UpdateCmd::Check { root, channel } => {
                let index = openairac_bundle::read_channel(channel)?;
                let installed = openairac_bundle::load_installed(root).unwrap_or_default();
                let schema = WorldStore::open_in_memory()?.migration_version()?;
                let decision = openairac_bundle::decide_update(
                    &installed,
                    &index,
                    channel,
                    schema,
                    chrono::Utc::now(),
                );
                println!("Update decision: {decision:?}");
                println!(
                    "  latest: {} (effective {})",
                    index.latest.bundle_hash, index.latest.effective_from
                );
            }
            UpdateCmd::Apply { root, channel } => {
                let decision = openairac_bundle::update_apply(root, channel, chrono::Utc::now())?;
                println!("Update applied: {decision:?}");
            }
        },
        Commands::Cycle { cmd } => match cmd {
            CycleCmd::Discover { db } => {
                println!("Discovering FAA CIFP cycles...");
                let store = WorldStore::open(db)?;
                let discovered = openairac_ingest::cifp_discovery::discover_cifp_cycles()?;
                let mut new_count = 0usize;
                for cycle in &discovered {
                    let id = openairac_model::CycleId(cycle.ident.clone());
                    if store.query_cycle(&id)?.is_some() {
                        println!("  cycle {} already in catalog (skipped)", cycle.ident);
                        continue;
                    }
                    let now = chrono::Utc::now();
                    store.insert_cycle(&openairac_model::AiracCycle {
                        id,
                        effective_from: cycle.effective_from,
                        effective_until: cycle.effective_until,
                        status: openairac_model::CycleStatus::Discovered,
                        source_uri: Some(cycle.source_uri.clone()),
                        created_at: now,
                        updated_at: now,
                        notes: Some("effective dates unconfirmed".to_string()),
                    })?;
                    new_count += 1;
                    println!(
                        "  discovered {} ({}) — effective dates UNCONFIRMED",
                        cycle.ident, cycle.source_uri
                    );
                }
                println!(
                    "Discovery complete: {} new cycle(s), {} total in catalog.",
                    new_count,
                    store.query_cycles()?.len()
                );
            }
            CycleCmd::Observe { db } => {
                let mut store = WorldStore::open(db)?;
                let now = chrono::Utc::now();
                let report = store.observe_cycles(now)?;
                for cycle in &report.activated {
                    println!("Activated cycle {}", cycle.0);
                }
                for cycle in &report.superseded {
                    println!("  superseded {}", cycle.0);
                }
                for cycle in &report.expired {
                    println!("  expired {}", cycle.0);
                }
                println!("Cycle bookkeeping is up to date.");
            }
            CycleCmd::Rollback { cycle, db, at } => {
                let at = match at {
                    Some(s) => chrono::DateTime::parse_from_rfc3339(s)
                        .map(|t| t.with_timezone(&chrono::Utc))?,
                    None => chrono::Utc::now(),
                };
                let mut store = WorldStore::open(db)?;
                let report = store.rollback_cycle(&openairac_model::CycleId(cycle.clone()), at)?;
                if report.noop {
                    println!("Cycle {} was already rolled back (no-op).", cycle);
                } else {
                    println!("Rolled back cycle {} at {at}.", cycle);
                    println!(
                        "  restored: {}",
                        report
                            .restored_cycle_id
                            .as_ref()
                            .map(|c| c.0.as_str())
                            .unwrap_or("(no earlier cycle)")
                    );
                    println!("  added entities closed: {}", report.added_closed);
                    println!(
                        "  changed entities re-published: {}",
                        report.changed_republished
                    );
                    println!(
                        "  removed entities re-published: {}",
                        report.removed_republished
                    );
                }
            }
            CycleCmd::List { db } => {
                let store = WorldStore::open(db)?;
                let cycles = store.query_cycles()?;
                if cycles.is_empty() {
                    println!("Cycle catalog is empty. Run `openairac cycle discover`.");
                    return Ok(());
                }
                println!("AIRAC Cycle Catalog");
                println!("===================");
                for cycle in &cycles {
                    println!(
                        "  {}  status={}  effective_from={}  effective_until={}  source={}",
                        cycle.id.0,
                        cycle.status.as_str(),
                        cycle
                            .effective_from
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_else(|| "UNCONFIRMED".to_string()),
                        cycle
                            .effective_until
                            .map(|t| t.to_rfc3339())
                            .unwrap_or_else(|| "-".to_string()),
                        cycle.source_uri.as_deref().unwrap_or("-"),
                    );
                }
            }
        },
        Commands::Export { target } => match target {
            ExportTarget::Xplane {
                db,
                out,
                date,
                allow_empty,
            } => {
                let export_date = parse_export_date(date)?;
                println!("Exporting X-Plane 12 Navigation Data...");
                println!("  Database: {:?}", db);
                println!("  Output Directory: {:?}", out);
                println!(
                    "  Effective Date: {}",
                    export_date.format("%Y-%m-%d %H:%M UTC")
                );

                let store = WorldStore::open(db)?;
                let report =
                    XPlane12Exporter::export_from_db(&store, export_date, out, *allow_empty)?;

                println!(
                    "  Exported {} waypoints to earth_fix.dat",
                    report.fixes_written
                );
                println!(
                    "  Exported {} navaids to earth_nav.dat",
                    report.navaids_written
                );
                println!(
                    "  Skipped {} fixes, {} navaids",
                    report.fixes_skipped, report.navaids_skipped
                );
                for diagnostic in report.diagnostics.iter().take(20) {
                    println!("  diagnostic: {diagnostic}");
                }
                if report.diagnostics.len() > 20 {
                    println!("  ... {} more diagnostics", report.diagnostics.len() - 20);
                }
                println!("X-Plane 12 export complete.");
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

    #[test]
    fn test_parse_export_date() {
        let dt = parse_export_date(&Some("2026-08-06".to_string())).unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-08-06");
        let dt = parse_export_date(&Some("2026-08-06T12:00:00Z".to_string())).unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2026-08-06");
        assert!(parse_export_date(&Some("garbage".to_string())).is_err());
    }
}
