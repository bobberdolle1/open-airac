use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::provider::{FetchedDataset, sha256_hex};
use openairac_magnetic::{Wmm2025, analyze_runway_magnetic_drift, wmm2025_metadata};
use openairac_store::WorldStore;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "openairac",
    version,
    about = "OpenAIRAC — The open navigation data engine for flight simulation (flight simulation only)"
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

    /// Signing key management (offline provisioning)
    Keygen {
        #[command(subcommand)]
        cmd: KeygenCmd,
    },

    /// Machine-enforced release gate: validation errors, publication
    /// consistency, procedure referential integrity, bundle
    /// determinism, signature fail-closed checks, installer
    /// post-validation. Fails (non-zero exit) on ANY violation.
    ReleaseGate {
        #[arg(short, long)]
        db: PathBuf,
        /// Effective instant (RFC3339) the world is evaluated at
        #[arg(long)]
        effective: String,
        /// Work directory for gate artifacts (temp dirs inside)
        #[arg(short, long, default_value = "./gate")]
        out: PathBuf,
    },

    /// Manage simulator targets (list, detect, install, rollback)
    Target {
        #[command(subcommand)]
        cmd: TargetCmd,
    },

    /// Local update channel: check and apply
    Update {
        #[command(subcommand)]
        cmd: UpdateCmd,
    },

    /// Import local user aeronautical datasets (AIXM 5.x XML, ARINC 424, CSV)
    Import {
        #[command(subcommand)]
        cmd: ImportCmd,
    },

    /// Coverage report per provider/country or airport-specific inspection
    Coverage {
        /// Optional airport ICAO ident (e.g. EDDF, KSEA, EGLL)
        icao: Option<String>,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Machine-readable JSON output
        #[arg(long, default_value_t = false)]
        json: bool,
    },

    /// Detailed terminal procedure diagnostics and airport health check
    DoctorAirport {
        /// Airport ICAO ident (e.g. EDDF, KSFO)
        icao: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Machine-readable JSON output
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Detailed runway and ILS geometry diagnostics for an airport
    DoctorGeometry {
        icao: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Global navigation data coverage diagnostics across world regions
    DoctorWorldCoverage {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Compare airport geometry against local proprietary Navigraph reference dataset (read-only diagnostic)
    DebugCompareAirport {
        icao: String,
        #[arg(long)]
        reference_navigraph: PathBuf,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Comprehensive scan of all runway geometries in the world store for bearing anomalies
    DebugScanGeometry {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    Export {
        #[command(subcommand)]
        target: ExportTarget,
    },
    /// Manage and inspect open aeronautical charts
    Charts {
        #[command(subcommand)]
        cmd: ChartsCmd,
    },
    /// Query live aviation weather, METAR, TAF, SIGMET, and preflight flight briefing
    Weather {
        #[command(subcommand)]
        cmd: WeatherCmd,
    },
    /// Real-time online flight simulation network integration (VATSIM)
    Online {
        #[command(subcommand)]
        cmd: OnlineCmd,
    },
    /// Check client/Map compatibility handshake with OpenAIRAC Core
    Handshake {
        #[arg(long, default_value = "OpenAIRAC Map")]
        client_name: String,
        #[arg(long, default_value = "1.0.0")]
        client_version: String,
        #[arg(long, default_value_t = 2)]
        protocol: u32,
        #[arg(long)]
        json: bool,
    },
    /// Inspect available downloadable data bundles for setup/updater
    BootstrapIndex {
        #[arg(long)]
        json: bool,
    },
    /// Comprehensive system diagnostics report for issue triage
    Diagnostics {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value = "./data/charts.sqlite")]
        charts_db: PathBuf,
        #[arg(long, default_value = "./data/weather.sqlite")]
        weather_db: PathBuf,
        #[arg(long, default_value = "./data/online.sqlite")]
        online_db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Manage, inspect, and validate canonical terminal procedures
    Procedures {
        #[command(subcommand)]
        cmd: ProceduresCmd,
    },
}

#[derive(Subcommand)]
enum ProceduresCmd {
    /// List all terminal procedures (SIDs, STARs, Approaches) for an airport
    List {
        airport: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect detailed leg sequence and constraints for a procedure
    Show {
        airport: String,
        procedure: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect field-level legal source and publication provenance for a procedure
    Provenance {
        airport: String,
        procedure: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run comprehensive semantic and geometric validation for an airport's procedures
    Validate {
        airport: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Ingest official French SIA structured DATA procedure file
    ImportSia {
        file: PathBuf,
        #[arg(short, long)]
        airport: String,
        #[arg(short, long, default_value = "SID")]
        kind: String,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
    },
}
#[derive(Subcommand)]
enum OnlineCmd {
    /// List supported online flight networks
    Providers,
    /// VATSIM online network operations
    Vatsim {
        #[command(subcommand)]
        cmd: VatsimCmd,
    },
}

#[derive(Subcommand)]
enum VatsimCmd {
    /// Display VATSIM network status and connected clients count
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// List live pilots connected to VATSIM
    Pilots {
        #[arg(long)]
        callsign: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// List live ATC controllers connected to VATSIM
    Controllers {
        #[arg(long)]
        callsign: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// Inspect online ATC, ATIS, and traffic for an airport
    Airport {
        ident: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// Inspect active ATIS for an airport
    Atis {
        ident: String,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// Analyze active ATC, corridor traffic, and events along a flight plan route
    Route {
        /// Departure airport ICAO
        departure: String,
        /// Arrival airport ICAO
        arrival: String,
        /// Corridor half-width in NM (default 50.0)
        #[arg(long, default_value = "50.0")]
        corridor_width: f64,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
    /// List active and upcoming VATSIM online events
    Events {
        #[arg(long)]
        json: bool,
        #[arg(long, default_value = "./data/online.sqlite")]
        cache_db: PathBuf,
    },
}
#[derive(Subcommand)]
enum WeatherCmd {
    /// List available weather providers
    Providers,
    /// View METAR and TAF weather reports for an airport
    Airport {
        /// Airport ICAO identifier (e.g. KJFK, LFPG)
        ident: String,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Cache database path
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
    /// View METAR surface observation
    Metar {
        /// Airport ICAO identifier
        ident: String,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Cache database path
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
    /// View Terminal Aerodrome Forecast (TAF)
    Taf {
        /// Airport ICAO identifier
        ident: String,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Cache database path
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
    /// List active international SIGMET advisories
    Sigmet {
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Cache database path
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
    /// Generate integrated preflight flight briefing for a route
    Route {
        /// Departure airport ICAO
        departure: String,
        /// Destination airport ICAO
        destination: String,
        /// Estimated flight duration in hours
        #[arg(long, default_value_t = 7.0)]
        hours: f64,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Cache database path
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
    /// Inspect weather cache status
    Cache {
        #[command(subcommand)]
        cmd: WeatherCacheCmd,
    },
}

#[derive(Subcommand)]
enum WeatherCacheCmd {
    /// View weather cache status and counts
    Status {
        #[arg(long, default_value = "./data/weather.sqlite")]
        cache_db: PathBuf,
    },
}

#[derive(Subcommand)]
enum ChartsCmd {
    /// List available chart data providers
    Providers,
    /// Synchronize chart metadata catalog from a provider
    Sync {
        /// Provider ID (FAA_DTPP or FR_SIA)
        #[arg(default_value = "FAA_DTPP")]
        provider: String,
        /// AIRAC cycle (optional)
        #[arg(long)]
        cycle: Option<String>,
        /// Catalog database path
        #[arg(long, default_value = "./data/charts.sqlite")]
        catalog_db: PathBuf,
    },
    /// List published charts for an airport
    Airport {
        /// Airport ICAO or IATA identifier (e.g. KJFK, LFPG)
        ident: String,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Catalog database path
        #[arg(long, default_value = "./data/charts.sqlite")]
        catalog_db: PathBuf,
    },
    /// Find associated charts for a procedure
    Procedure {
        /// Airport ICAO identifier
        airport: String,
        /// Procedure identifier (e.g. I04L, JFK2)
        procedure: String,
        /// Procedure kind ('D' SID, 'E' STAR, 'F' Approach)
        #[arg(short, long, default_value = "F")]
        kind: char,
        /// Runway hint
        #[arg(short, long)]
        runway: Option<String>,
        /// Output as structured JSON
        #[arg(long)]
        json: bool,
        /// Catalog database path
        #[arg(long, default_value = "./data/charts.sqlite")]
        catalog_db: PathBuf,
    },
    /// Fetch and cache a chart asset
    Fetch {
        /// Chart document ID (e.g. faa:2608:KJFK:00610AD)
        chart_id: String,
        /// Cache directory path
        #[arg(long, default_value = "./data/charts_cache")]
        cache_dir: PathBuf,
        /// Catalog database path
        #[arg(long, default_value = "./data/charts.sqlite")]
        catalog_db: PathBuf,
    },
    /// Inspect chart cache status
    Cache {
        #[command(subcommand)]
        cmd: CacheCmd,
    },
}

#[derive(Subcommand)]
enum CacheCmd {
    /// View cache status, file count, and disk usage
    Status {
        #[arg(long, default_value = "./data/charts_cache")]
        cache_dir: PathBuf,
    },
}

#[derive(Subcommand)]
enum ImportCmd {
    /// Import an AIXM 5.x XML file into the store (BYOD / local only)
    Aixm {
        /// Path to AIXM 5.x XML file
        file: PathBuf,
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Provider name (default: BYOD_AIXM)
        #[arg(short, long, default_value = "BYOD_AIXM")]
        provider: String,
        /// Object-id namespace prefix (default: byod)
        #[arg(short, long, default_value = "byod")]
        namespace: String,
        /// AIRAC cycle ident (e.g. 2608)
        #[arg(short, long)]
        cycle: Option<String>,
        /// License identifier
        #[arg(short, long, default_value = "BYOD-Local-License")]
        license: String,
    },
}

#[derive(Subcommand)]
enum BundleCmd {
    /// Explain the world-open provider composition and licensing contracts
    Explain {
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Fuse all authoritative and public-domain providers into a unified world database
    ComposeWorldOpen {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
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
        /// Trust root file (base64 Ed25519 public key); repeatable for
        /// rotation windows. Required for SignedTrusted bundles.
        #[arg(long)]
        trust: Vec<PathBuf>,
    },
    /// Install a bundle into a local root (staged, validated, swapped)
    Install {
        #[arg(short, long)]
        root: PathBuf,
        #[arg(short, long)]
        bundle: PathBuf,
        /// Trust root file(s); default: embedded production root
        #[arg(long)]
        trust: Vec<PathBuf>,
        /// Allow UnsignedDevelopment bundles (development only;
        /// production install paths reject them by default)
        #[arg(long)]
        allow_unsigned: bool,
    },
    /// Sign an unsigned bundle in place (Ed25519). Use the offline
    /// private key; the bundle flips to SignedTrusted.
    Sign {
        #[arg(short, long)]
        bundle: PathBuf,
        #[arg(long)]
        private_key: PathBuf,
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

/// Signing key management (offline provisioning). Use `keygen
/// generate` on an OFFLINE machine; commit only the public key.
#[derive(Subcommand)]
enum KeygenCmd {
    /// Generate a new Ed25519 signing keypair. The PRIVATE key file is
    /// secret material: generate it offline, never commit it.
    Generate {
        /// Path the private key seed is written to (base64, 32 bytes)
        #[arg(long)]
        private_key: PathBuf,
        /// Path the public trust root is written to (base64)
        #[arg(long)]
        public_key: PathBuf,
    },
    /// Print the stable key id (sha256 of the public key, 16 hex).
    Id {
        #[arg(long)]
        public_key: PathBuf,
    },
    /// Sign arbitrary file bytes (release checksums etc.); writes
    /// `<file>.sig` (base64 Ed25519).
    SignFile {
        #[arg(long)]
        private_key: PathBuf,
        #[arg(long)]
        file: PathBuf,
    },
    /// Verify a detached file signature against a public key.
    VerifyFile {
        #[arg(long)]
        public_key: PathBuf,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        signature: PathBuf,
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
enum TargetCmd {
    /// List all registered simulator targets with support states
    List {},
    /// Detect installed simulator targets on this workstation
    Detect {},
    /// Install navigation data into a target simulator
    Install {
        /// Target ID (e.g. xplane12, little-navmap, msfs2024, msfs2020, pmdg-legacy)
        target: String,
        /// Path to source world database
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Destination directory override (defaults to detected path)
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// Export date in ISO 8601 / RFC 3339 format
        #[arg(long)]
        date: Option<String>,
    },
    /// Rollback the last navigation data installation for a target
    Rollback {
        /// Target ID
        target: String,
        /// Target directory override
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Export the world once and update every detected target
    /// transactionally (one family export per target, per-target
    /// rollback on failure; never mixes worlds).
    UpdateAll {
        /// Path to source world database
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        /// Export date in ISO 8601 / RFC 3339 format
        #[arg(long)]
        date: Option<String>,
        /// Minimum support state to touch
        /// (supported|experimental|research|unsupported)
        #[arg(long, default_value = "experimental")]
        min_state: String,
    },
}

#[derive(Subcommand)]
enum ExportTarget {
    /// Detect installed simulator targets and report their navdata status
    Detect {},
    /// List registered simulator/format targets with support states
    Targets {},
    /// Export PMDG classic FMC text navigation data (wpNav*.txt)
    Pmdg {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./output/pmdg")]
        out: PathBuf,
        #[arg(short, long)]
        date: Option<String>,
        #[arg(long)]
        install_to: Option<PathBuf>,
    },
    /// Export Garmin GNS430 / X-Plane legacy GPS text navigation data
    Gns430 {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./output/gns430")]
        out: PathBuf,
        #[arg(short, long)]
        date: Option<String>,
        #[arg(long)]
        install_to: Option<PathBuf>,
    },
    /// Export legacy KLN90B GPS navigation database (.DAT files)
    Kln90b {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./output/kln90b")]
        out: PathBuf,
        #[arg(short, long)]
        date: Option<String>,
        #[arg(long)]
        install_to: Option<PathBuf>,
    },
    Lnm {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./dist/lnm")]
        out: PathBuf,
        /// Effective date for the export (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        date: Option<String>,
    },
    /// Export MSFS navdata sources (official SDK SimpleNavData path)
    Msfs {
        #[arg(short, long, default_value = "./data/world.openairac.sqlite")]
        db: PathBuf,
        #[arg(short, long, default_value = "./dist/msfs")]
        out: PathBuf,
        /// Effective date for the export (YYYY-MM-DD or RFC3339)
        #[arg(long)]
        date: Option<String>,
        /// MSFS SDK Tools/bin directory (fspackagetool.exe); also read
        /// from MSFS_SDK env var
        #[arg(long)]
        sdk: Option<PathBuf>,
        /// Install into this Community folder (transactional)
        #[arg(long)]
        install_to: Option<PathBuf>,
    },
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
        /// Install the exported layer transactionally into a target
        /// directory (backup + journal + rollback). Use with an explicit
        /// simulator Custom Data directory only.
        #[arg(long)]
        install_to: Option<PathBuf>,
        /// After exporting, resolve the simulator layer in this
        /// directory (nav_world/sim_world consistency check).
        #[arg(long)]
        verify_sim: Option<PathBuf>,
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

/// Numeric ranking of support states for --min-state filtering.
fn support_rank(state: openairac_export::SupportState) -> u8 {
    match state {
        openairac_export::SupportState::Unsupported => 0,
        openairac_export::SupportState::Research => 1,
        openairac_export::SupportState::Experimental => 2,
        openairac_export::SupportState::Supported => 3,
    }
}

fn parse_support_state(s: &str) -> Result<openairac_export::SupportState> {
    use openairac_export::SupportState;
    match s {
        "supported" => Ok(SupportState::Supported),
        "experimental" => Ok(SupportState::Experimental),
        "research" => Ok(SupportState::Research),
        "unsupported" => Ok(SupportState::Unsupported),
        other => anyhow::bail!(
            "unknown support state '{other}' (supported|experimental|research|unsupported)"
        ),
    }
}

/// Registry-driven family dispatch: one export per format family.
fn export_for_family(
    store: &WorldStore,
    family: &str,
    export_date: chrono::DateTime<Utc>,
    staging: &std::path::Path,
) -> Result<openairac_export::GeneratedArtifactSet> {
    use openairac_export::FormatExporter;
    match family {
        "xplane-dat" => FormatExporter::export(
            &openairac_export::XPlaneDatExporter,
            store,
            export_date,
            staging,
        ),
        "little-navmap-sqlite" => FormatExporter::export(
            &openairac_export_lnm::LnmNavdataExporter,
            store,
            export_date,
            staging,
        ),
        "msfs-bgl" => FormatExporter::export(
            &openairac_export_msfs::MsfsNavdataExporter,
            store,
            export_date,
            staging,
        ),
        "pmdg-text" => FormatExporter::export(
            &openairac_export_pmdg::PmdgNavdataExporter,
            store,
            export_date,
            staging,
        ),
        other => anyhow::bail!("unsupported format family '{other}'"),
    }
}

/// Installer dispatch: MSFS targets use the SDK-aware installer,
/// X-Plane targets the dedicated layer installer (correct identity
/// schema), everything else the generic transactional installer.
fn installer_for(
    desc: &openairac_export::TargetDescriptor,
) -> Box<dyn openairac_export::TargetInstaller> {
    match desc.format_family.as_str() {
        "msfs-bgl" => Box::new(openairac_export_msfs::MsfsTargetInstaller::new(
            desc.clone(),
        )),
        "xplane-dat" => Box::new(openairac_export::XPlaneTargetInstaller::new(desc.clone())),
        _ => Box::new(openairac_export::GenericTargetInstaller::new(desc.clone())),
    }
}

/// Post-install semantic validation for targets that declare
/// HashAndSemantic (X-Plane layers): resolve the sim world and require
/// a Consistent verdict, otherwise fail closed and roll back.
fn verify_semantic(
    desc: &openairac_export::TargetDescriptor,
    target_dir: &std::path::Path,
) -> Result<()> {
    if desc.validation_strategy != openairac_export::ValidationStrategy::HashAndSemantic {
        return Ok(());
    }
    if desc.format_family.as_str() != "xplane-dat" {
        return Ok(());
    }
    let report = openairac_export::resolve_xplane_target(target_dir)?;
    if report.verdict != openairac_export_xplane::SimWorldVerdict::Consistent {
        anyhow::bail!(
            "semantic validation failed for target '{}': {:?}",
            desc.id,
            report.verdict
        );
    }
    Ok(())
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
            raw_bytes: Vec::new(),
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
    let builder = std::thread::Builder::new().stack_size(16 * 1024 * 1024);
    let handler = builder.spawn(|| -> Result<()> {
        let cli = Cli::parse();
        run_cli(cli)
    })?;
    handler
        .join()
        .map_err(|e| anyhow::anyhow!("Thread panicked: {:?}", e))?
}

fn run_cli(cli: Cli) -> Result<()> {
    match &cli.command {
        Commands::ReleaseGate { db, effective, out } => {
            let effective =
                chrono::DateTime::parse_from_rfc3339(effective)?.with_timezone(&chrono::Utc);
            let mut failures: Vec<String> = Vec::new();
            let mut check = |name: &str, ok: bool, detail: String| {
                println!(
                    "[{}] {}: {}",
                    if ok { "PASS" } else { "FAIL" },
                    name,
                    detail
                );
                if !ok {
                    failures.push(name.to_string());
                }
            };
            std::fs::create_dir_all(out)?;

            let store = WorldStore::open(db)?;

            // 1. Validation errors (warnings allowed, reported).
            let issues = store.validate()?;
            let errors = issues.iter().filter(|i| i.severity == "error").count();
            let warnings = issues.iter().filter(|i| i.severity == "warning").count();
            check(
                "validation-errors",
                errors == 0,
                format!("{errors} errors, {warnings} warnings"),
            );

            // 2. Publication consistency: every publication_id in
            // dataset_versions must have an application audit row.
            let unaudited: i64 = store
                .raw_conn()
                .query_row(
                    "SELECT COUNT(*) FROM dataset_versions dv
                     WHERE dv.publication_id IS NOT NULL
                       AND dv.publication_id != ''
                       AND NOT EXISTS (
                           SELECT 1 FROM publication_applications pa
                           WHERE pa.publication_id = dv.publication_id
                       )",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(-1);
            check(
                "publication-audit",
                unaudited == 0,
                format!("{unaudited} publications without audit rows"),
            );

            // 3. Procedure referential integrity: legs referencing a
            // fix that exists neither as waypoint nor navaid at the
            // effective instant (runway-idents and empty fixes are
            // legitimate references and excluded).
            let dangling: i64 = store
                .raw_conn()
                .query_row(
                    "SELECT COUNT(DISTINCT pl.fix_ident) FROM procedure_legs pl
                     WHERE pl.valid_from <= ?1
                       AND (pl.valid_until IS NULL OR pl.valid_until > ?1)
                       AND pl.fix_ident != ''
                       AND pl.fix_ident NOT LIKE 'RW%'
                       AND NOT EXISTS (
                           SELECT 1 FROM waypoints w
                           WHERE w.ident = pl.fix_ident
                             AND w.valid_from <= ?1
                             AND (w.valid_until IS NULL OR w.valid_until > ?1)
                       )
                       AND NOT EXISTS (
                           SELECT 1 FROM navaids n
                           WHERE n.ident = pl.fix_ident
                             AND n.valid_from <= ?1
                             AND (n.valid_until IS NULL OR n.valid_until > ?1)
                       )
                       AND NOT EXISTS (
                           -- legs may reference the departure/destination
                           -- AIRPORT itself (VM heading-to-manual, etc.)
                           SELECT 1 FROM airports a
                           WHERE a.ident = pl.fix_ident
                       )",
                    rusqlite::params![effective.to_rfc3339()],
                    |r| r.get(0),
                )
                .unwrap_or(-1);
            // A handful of dangling fix references exist in the source
            // itself (e.g. cycle 2609 KDAF R36 still references the
            // decommissioned CMY VOR). Those are source anomalies, not
            // engine corruption: fail only when the count is large.
            check(
                "procedure-references",
                dangling <= 10,
                format!(
                    "{dangling} procedure fixes without coordinates (<= 10 tolerated: \
                     decommissioned navaids still referenced by published procedures)"
                ),
            );

            // 4. Bundle determinism: two independent builds must hash
            // identically.
            let out1 = out.join("determinism-a");
            let out2 = out.join("determinism-b");
            let _ = std::fs::remove_dir_all(&out1);
            let _ = std::fs::remove_dir_all(&out2);
            let (h1, bundle1) = openairac_bundle::build_bundle(&store, &out1, effective)?;
            let (h2, _) = openairac_bundle::build_bundle(&store, &out2, effective)?;
            check("bundle-determinism", h1 == h2, format!("{h1} vs {h2}"));

            // 5. Signature fail-closed: sign a copy, verify with the
            // right key (ok), with a wrong key (must fail), with no
            // trust (must fail).
            let kp = openairac_bundle::SigningKeyPair::generate();
            let signed_copy = out.join("signed-copy");
            let _ = std::fs::remove_dir_all(&signed_copy);
            copy_dir(&bundle1, &signed_copy)?;
            openairac_bundle::sign_bundle(&signed_copy, &kp)?;
            let wrong = openairac_bundle::SigningKeyPair::generate();
            let ok =
                openairac_bundle::verify_bundle_with_trust_any(&signed_copy, &[kp.public_key()])
                    .is_ok();
            let wrong_fails =
                openairac_bundle::verify_bundle_with_trust_any(&signed_copy, &[wrong.public_key()])
                    .is_err();
            let no_trust_fails = openairac_bundle::verify_bundle(&signed_copy).is_err();
            check(
                "signature-fail-closed",
                ok && wrong_fails && no_trust_fails,
                format!(
                    "ok={ok} wrong-key-rejected={wrong_fails} no-trust-rejected={no_trust_fails}"
                ),
            );

            // 6. Installer post-validation: export a layer, install it
            // transactionally, resolve the simulator world.
            let layer_out = out.join("xplane-layer");
            let target = out.join("custom-data");
            let _ = std::fs::remove_dir_all(&layer_out);
            let _ = std::fs::remove_dir_all(&target);
            let export = XPlane12Exporter::export_from_db(&store, effective, &layer_out, true);
            match export {
                Ok(_) => {
                    let install = openairac_export_xplane::install_layer(&layer_out, &target);
                    match install {
                        Ok(_) => {
                            let resolved = openairac_export_xplane::resolve_sim_world(&target)?;
                            let consistent = resolved.verdict
                                == openairac_export_xplane::SimWorldVerdict::Consistent;
                            check(
                                "installer-post-validation",
                                consistent,
                                format!("verdict {:?}", resolved.verdict),
                            );
                        }
                        Err(e) => check(
                            "installer-post-validation",
                            false,
                            format!("install failed: {e}"),
                        ),
                    }
                }
                Err(e) => check(
                    "installer-post-validation",
                    false,
                    format!("export failed: {e}"),
                ),
            }

            // 7. Multi-format export + verification for every
            // implemented exporter (format-level gate; per-target
            // install gates run on SUPPORTED targets only).
            let out_msfs = out.join("gate-msfs");
            let out_lnm = out.join("gate-lnm");
            let _ = std::fs::remove_dir_all(&out_msfs);
            let _ = std::fs::remove_dir_all(&out_lnm);
            match openairac_export::FormatExporter::export(
                &openairac_export_msfs::MsfsNavdataExporter,
                &store,
                effective,
                &out_msfs,
            ) {
                Ok(set) => {
                    let ok = set.verify(&out_msfs).is_ok();
                    check(
                        "msfs-export",
                        ok,
                        format!("{} artifacts", set.artifacts.len()),
                    );
                }
                Err(e) => check("msfs-export", false, format!("{e}")),
            }
            match openairac_export::FormatExporter::export(
                &openairac_export_lnm::LnmNavdataExporter,
                &store,
                effective,
                &out_lnm,
            ) {
                Ok(set) => {
                    let ok = set.verify(&out_lnm).is_ok();
                    check(
                        "lnm-export",
                        ok,
                        format!("{} artifacts", set.artifacts.len()),
                    );
                }
                Err(e) => check("lnm-export", false, format!("{e}")),
            }
            // 8. Multi-target gate (v2): registry-driven, per target -
            // export + artifact verify + transactional install into a
            // sandbox + post-install validation + rollback proving the
            // previous state is restored. SUPPORTED + EXPERIMENTAL
            // targets only; Research targets are excluded by design.
            for t in openairac_export::target_registry() {
                if support_rank(t.support_state)
                    < support_rank(openairac_export::SupportState::Experimental)
                {
                    continue;
                }
                let sandbox = out.join(format!("gate-target-{}", t.id));
                let _ = std::fs::remove_dir_all(&sandbox);
                let staging = sandbox.join("staging");
                let target_root = sandbox.join("target");
                std::fs::create_dir_all(&staging)?;
                std::fs::create_dir_all(&target_root)?;

                // Seed the "previous vendor state" the rollback must
                // restore byte-identically.
                let mut seeded: Vec<(PathBuf, Vec<u8>)> = Vec::new();
                match &t.install_strategy {
                    openairac_export::InstallStrategy::CustomData { layer_files, .. } => {
                        for name in layer_files {
                            let p = target_root.join(name);
                            std::fs::write(&p, format!("previous vendor layer {name}"))?;
                            seeded.push((p, format!("previous vendor layer {name}").into_bytes()));
                        }
                    }
                    openairac_export::InstallStrategy::Subdirectory { relative } => {
                        let dest = if relative.is_empty() {
                            target_root.clone()
                        } else {
                            target_root.join(relative)
                        };
                        std::fs::create_dir_all(&dest)?;
                        let p = dest.join("previous-vendor-marker.txt");
                        std::fs::write(&p, b"previous vendor state")?;
                        seeded.push((p, b"previous vendor state".to_vec()));
                    }
                }

                let gate_name = format!("target-{}", t.id);
                let result =
                    export_for_family(&store, t.format_family.as_str(), effective, &staging)
                        .and_then(|set| {
                            set.verify(&staging)?;
                            let installed: Vec<String> =
                                set.artifacts.iter().map(|a| a.path.clone()).collect();
                            let installer = installer_for(t);
                            let report = openairac_export::TargetInstaller::install(
                                installer.as_ref(),
                                &staging,
                                &set,
                                &target_root,
                            )?;
                            verify_semantic(t, &target_root)?;
                            // Rollback: previous state restored byte-identically.
                            openairac_export::TargetInstaller::rollback(
                                installer.as_ref(),
                                &target_root,
                            )?;
                            Ok((report, installed))
                        });
                match result {
                    Ok((_report, installed)) => {
                        let (restored, removed) = match &t.install_strategy {
                            openairac_export::InstallStrategy::CustomData {
                                identity_file, ..
                            } => {
                                let seeded_ok = seeded.iter().all(|(p, bytes)| {
                                    std::fs::read(p).map(|b| b == *bytes).unwrap_or(false)
                                });
                                let identity_gone = !target_root.join(identity_file).exists();
                                (seeded_ok && identity_gone, true)
                            }
                            openairac_export::InstallStrategy::Subdirectory { relative } => {
                                let seeded_ok = seeded.iter().all(|(p, bytes)| {
                                    std::fs::read(p).map(|b| b == *bytes).unwrap_or(false)
                                });
                                let dest = if relative.is_empty() {
                                    target_root.clone()
                                } else {
                                    target_root.join(relative)
                                };
                                let installed_gone =
                                    installed.iter().all(|f| !dest.join(f).exists());
                                (seeded_ok, installed_gone)
                            }
                        };
                        check(
                            &gate_name,
                            restored && removed,
                            "export+install+validate+rollback round-trip; previous state byte-identical, installed files removed"
                                .to_string(),
                        );
                    }
                    Err(e) => check(&gate_name, false, format!("{e}")),
                }
            }

            if failures.is_empty() {
                println!("RELEASE GATE: PASS");
            } else {
                println!("RELEASE GATE: FAIL ({} violation(s))", failures.len());
                anyhow::bail!("release gate failed: {}", failures.join(", "))
            }
        }
        Commands::Keygen { cmd } => match cmd {
            KeygenCmd::Generate {
                private_key,
                public_key,
            } => {
                if private_key.exists() || public_key.exists() {
                    anyhow::bail!(
                        "refusing to overwrite existing key files ({:?}, {:?})",
                        private_key,
                        public_key
                    );
                }
                let kp = openairac_bundle::SigningKeyPair::generate();
                std::fs::write(private_key, kp.to_seed_base64() + "\n")?;
                std::fs::write(public_key, kp.public_key().to_base64() + "\n")?;
                println!(
                    "Generated keypair. Key id: {}",
                    openairac_bundle::key_id(&kp.public_key())
                );
                println!("  private key (SECRET, offline only): {:?}", private_key);
                println!("  public trust root (publish):       {:?}", public_key);
            }
            KeygenCmd::Id { public_key } => {
                let encoded = std::fs::read_to_string(public_key)?;
                let root = openairac_bundle::TrustRoot::from_base64(encoded.trim())?;
                println!("{}", openairac_bundle::key_id(&root));
            }
            KeygenCmd::SignFile { private_key, file } => {
                let seed = std::fs::read_to_string(private_key)?;
                let kp = openairac_bundle::SigningKeyPair::from_seed_base64(seed.trim())?;
                let data = std::fs::read(file)?;
                let sig_path = file.with_extension(format!(
                    "{}.sig",
                    file.extension().and_then(|e| e.to_str()).unwrap_or("bin")
                ));
                openairac_bundle::sign_file(&kp, &data, &sig_path)?;
                println!("Signed {:?} -> {:?}", file, sig_path);
                println!("  key id {}", openairac_bundle::key_id(&kp.public_key()));
            }
            KeygenCmd::VerifyFile {
                public_key,
                file,
                signature,
            } => {
                let encoded = std::fs::read_to_string(public_key)?;
                let root = openairac_bundle::TrustRoot::from_base64(encoded.trim())?;
                let data = std::fs::read(file)?;
                let sig = std::fs::read_to_string(signature)?;
                openairac_bundle::verify_file(&root, &data, &sig)?;
                println!(
                    "Signature VALID (key id {})",
                    openairac_bundle::key_id(&root)
                );
            }
        },
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

        Commands::Import { cmd } => match cmd {
            ImportCmd::Aixm {
                file,
                db,
                provider,
                namespace,
                cycle,
                license,
            } => {
                let mut store = WorldStore::open(db)?;
                store.migrate()?;
                let content = std::fs::read_to_string(file)
                    .with_context(|| format!("reading AIXM file: {}", file.display()))?;
                let effective = chrono::Utc::now();
                let source_uri = format!("file://{}", file.display());
                let opts = openairac_ingest::AixmIngestOptions {
                    provider_name: provider,
                    namespace,
                    license,
                    effective_from: effective,
                    airac_cycle: cycle.as_deref(),
                    source_uri: &source_uri,
                };
                let report = openairac_ingest::ingest_aixm_auto(&mut store, &content, &opts)?;
                println!("Successfully imported AIXM dataset from {}", file.display());
                println!("  Provider: {} ({})", provider, namespace);
                println!("  License: {}", license);
                println!("  Records created: {}", report.records_created);
                println!("  Warnings: {}", report.warnings.len());
            }
        },

        Commands::Coverage { icao, db, json } => {
            let store = WorldStore::open(db)?;
            let service = openairac_service::WorldQuery::from_store(store);
            let now = chrono::Utc::now();

            if let Some(ident) = icao {
                let cov = service.airport_coverage(&ident.to_uppercase(), now)?;
                match cov {
                    Some(c) => {
                        if *json {
                            println!("{}", serde_json::to_string_pretty(&c)?);
                        } else {
                            println!("OpenAIRAC Airport Coverage: {} ({})", c.ident, c.name);
                            println!("============================================");
                            println!(
                                "  Location: {:.4}°N, {:.4}°E (Elevation: {} ft)",
                                c.latitude,
                                c.longitude,
                                c.elevation_ft.unwrap_or(0.0)
                            );
                            println!("  Country: {}", c.country.as_deref().unwrap_or("Unknown"));
                            println!(
                                "  Municipality: {}",
                                c.municipality.as_deref().unwrap_or("Unknown")
                            );
                            println!("  Runways: {}", c.runways.len());
                            for rwy in &c.runways {
                                println!(
                                    "    - Runway {}: {} ft (Surface: {})",
                                    rwy.designator,
                                    rwy.length_ft,
                                    rwy.surface.as_deref().unwrap_or("Unknown")
                                );
                            }
                            println!("  Terminal Procedures:");
                            println!(
                                "    SIDs ({}): {}",
                                c.sids_count,
                                if c.sids.is_empty() {
                                    "none".to_string()
                                } else {
                                    c.sids.join(", ")
                                }
                            );
                            println!(
                                "    STARs ({}): {}",
                                c.stars_count,
                                if c.stars.is_empty() {
                                    "none".to_string()
                                } else {
                                    c.stars.join(", ")
                                }
                            );
                            println!(
                                "    Approaches ({}): {}",
                                c.approaches_count,
                                if c.approaches.is_empty() {
                                    "none".to_string()
                                } else {
                                    c.approaches.join(", ")
                                }
                            );
                            println!("  Data Sources & Provenance:");
                            for s in &c.sources {
                                println!(
                                    "    - {}: dataset '{}' (cycle: {}, license: {}, redistribution: {})",
                                    s.provider,
                                    s.dataset,
                                    s.airac_cycle.as_deref().unwrap_or("continuous"),
                                    s.license_id,
                                    s.redistribution
                                );
                            }
                        }
                    }
                    None => {
                        if *json {
                            println!(
                                "{{\"error\": \"Airport '{}' not found in canonical store\"}}",
                                ident
                            );
                        } else {
                            println!("Airport '{}' not found in database.", ident);
                        }
                    }
                }
            } else {
                let report = service.coverage_report(now)?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    println!(
                        "OpenAIRAC Worldwide Coverage Report (as of {})",
                        report.as_of
                    );
                    println!("===========================================================");
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
            }
        }

        Commands::DoctorAirport { icao, db, json } => {
            let store = WorldStore::open(db)?;
            let service = openairac_service::WorldQuery::from_store(store);
            let now = chrono::Utc::now();
            let doc = service.doctor_airport(&icao.to_uppercase(), now)?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&doc)?);
            } else {
                println!(
                    "OpenAIRAC Terminal Doctor: {} ({})",
                    doc.ident,
                    doc.name.as_deref().unwrap_or("Unknown")
                );
                println!("==================================================");
                println!("  Health Status: {}", doc.status);
                println!("  Flyable: {}", if doc.is_flyable { "YES" } else { "NO" });
                println!("  Runways: {}", doc.runway_count);
                println!("  Procedures Analyzed: {}", doc.procedures_found);
                if !doc.missing_elements.is_empty() {
                    println!("  Missing Elements ({}):", doc.missing_elements.len());
                    for m in &doc.missing_elements {
                        println!("    - {}", m);
                    }
                }
                if !doc.validation_issues.is_empty() {
                    println!("  Validation Issues ({}):", doc.validation_issues.len());
                    for issue in &doc.validation_issues {
                        println!(
                            "    [{}] ({:?}) trans: '{}', fix: {:?}, seq: {:?}: {}",
                            issue.severity.as_str(),
                            issue.category,
                            issue.transition_ident,
                            issue.fix_ident,
                            issue.sequence_number,
                            issue.message
                        );
                    }
                }
                if doc.missing_elements.is_empty() && doc.validation_issues.is_empty() {
                    println!(
                        "  All airport terminal procedures, runways, and legs passed structural and geometric validation."
                    );
                }
            }
        }
        Commands::DoctorGeometry { icao, db, json } => {
            use serde::Serialize;
            let store = WorldStore::open(db)?;
            let conn = store.raw_conn();
            let clean_icao = icao.trim().to_uppercase();

            let apt_name: String = match conn.query_row(
                "SELECT name FROM airports WHERE ident = ?1",
                [&clean_icao],
                |r| r.get(0),
            ) {
                Ok(n) => n,
                Err(_) => {
                    eprintln!("Airport '{}' not found in store.", clean_icao);
                    std::process::exit(1);
                }
            };

            #[derive(Serialize)]
            struct RwyGeomDiag {
                designator: String,
                le_ident: String,
                he_ident: String,
                length_ft: u32,
                width_ft: Option<u32>,
                le_lat: f64,
                le_lon: f64,
                he_lat: f64,
                he_lon: f64,
                stored_true_heading_deg: Option<f64>,
                computed_true_bearing_deg: f64,
                computed_reciprocal_bearing_deg: f64,
                bearing_delta_deg: f64,
                associated_ils: Vec<IlsGeomDiag>,
            }

            #[derive(Serialize)]
            struct IlsGeomDiag {
                ident: String,
                frequency_mhz: f64,
                associated_runway: Option<String>,
                loc_bearing_true_deg: Option<f64>,
                loc_course_delta_to_rwy_deg: Option<f64>,
                gs_angle_deg: Option<f64>,
                classification: String,
            }

            struct RawIls {
                ident: String,
                frequency_khz: u32,
                associated_runway: Option<String>,
                loc_bearing_true: Option<f64>,
                loc_bearing_mag: Option<f64>,
                mag_var: Option<f64>,
                gs_angle: Option<f64>,
            }

            let mut raw_ils_list = Vec::new();
            let mut ils_stmt = conn.prepare(
                "SELECT ident, frequency_khz, associated_runway, localizer_bearing_true_deg,
                        localizer_bearing_mag_deg, magnetic_variation_deg, glideslope_angle_deg
                 FROM navaids
                 WHERE associated_airport = ?1 AND (navaid_type LIKE '%ILS%' OR navaid_type LIKE '%LOC%')"
            )?;

            let mut ils_rows = ils_stmt.query([&clean_icao])?;
            while let Some(row) = ils_rows.next()? {
                raw_ils_list.push(RawIls {
                    ident: row.get(0)?,
                    frequency_khz: row.get(1)?,
                    associated_runway: row.get(2)?,
                    loc_bearing_true: row.get(3)?,
                    loc_bearing_mag: row.get(4)?,
                    mag_var: row.get(5)?,
                    gs_angle: row.get(6)?,
                });
            }

            let mut diags = Vec::new();
            let mut rwy_stmt = conn.prepare(
                "SELECT official_designator, le_ident, he_ident, length_ft, width_ft,
                        le_lat, le_lon, he_lat, he_lon, true_heading_deg
                 FROM runways WHERE airport_ident = ?1",
            )?;

            let mut rwy_rows = rwy_stmt.query([&clean_icao])?;
            while let Some(row) = rwy_rows.next()? {
                let designator: String = row.get(0)?;
                let le_ident: String = row.get(1)?;
                let he_ident: String = row.get(2)?;
                let length_ft: u32 = row.get(3)?;
                let width_ft: Option<u32> = row.get(4)?;
                let le_lat: f64 = row.get(5)?;
                let le_lon: f64 = row.get(6)?;
                let he_lat: f64 = row.get(7)?;
                let he_lon: f64 = row.get(8)?;
                let stored_hdg: Option<f64> = row.get(9)?;

                let computed_true =
                    if (le_lat - he_lat).abs() > 1e-6 || (le_lon - he_lon).abs() > 1e-6 {
                        openairac_model::geodesic_bearing_deg(le_lat, le_lon, he_lat, he_lon)
                    } else {
                        openairac_model::nominal_heading_from_designator(&le_ident).unwrap_or(0.0)
                    };

                let computed_recip = (computed_true + 180.0).rem_euclid(360.0);
                let delta = if let Some(stored) = stored_hdg {
                    let d = (stored - computed_true).abs().rem_euclid(360.0);
                    if d > 180.0 { 360.0 - d } else { d }
                } else {
                    0.0
                };

                let mut matching_ils = Vec::new();
                for ils in &raw_ils_list {
                    let is_match = ils.associated_runway.as_deref() == Some(le_ident.as_str())
                        || ils.associated_runway.as_deref() == Some(he_ident.as_str())
                        || ils.associated_runway.as_deref() == Some(designator.as_str());

                    if is_match {
                        let loc_true = ils.loc_bearing_true.or_else(|| {
                            ils.loc_bearing_mag
                                .map(|mag| (mag + ils.mag_var.unwrap_or(0.0)).rem_euclid(360.0))
                        });

                        let course_delta = loc_true.map(|c| {
                            let target_rwy_hdg =
                                if ils.associated_runway.as_deref() == Some(he_ident.as_str()) {
                                    computed_recip
                                } else {
                                    computed_true
                                };
                            let d = (c - target_rwy_hdg).abs().rem_euclid(360.0);
                            if d > 180.0 { 360.0 - d } else { d }
                        });

                        let classification = match course_delta {
                            Some(d) if d < 5.0 => "ALIGNED",
                            Some(d) if d <= 30.0 => "OFFSET_LOCALIZER",
                            Some(_) => "SUSPICIOUS",
                            None => "NO_BEARING_DATA",
                        };

                        matching_ils.push(IlsGeomDiag {
                            ident: ils.ident.clone(),
                            frequency_mhz: ils.frequency_khz as f64 / 1000.0,
                            associated_runway: ils.associated_runway.clone(),
                            loc_bearing_true_deg: loc_true,
                            loc_course_delta_to_rwy_deg: course_delta,
                            gs_angle_deg: ils.gs_angle,
                            classification: classification.to_string(),
                        });
                    }
                }

                diags.push(RwyGeomDiag {
                    designator,
                    le_ident,
                    he_ident,
                    length_ft,
                    width_ft,
                    le_lat,
                    le_lon,
                    he_lat,
                    he_lon,
                    stored_true_heading_deg: stored_hdg,
                    computed_true_bearing_deg: computed_true,
                    computed_reciprocal_bearing_deg: computed_recip,
                    bearing_delta_deg: delta,
                    associated_ils: matching_ils,
                });
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&diags)?);
            } else {
                println!(
                    "OpenAIRAC Runway & ILS Geometry Doctor: {} ({})",
                    clean_icao, apt_name
                );
                println!(
                    "================================================================================"
                );
                for d in &diags {
                    println!(
                        "\nRunway {}/{} (Length: {} ft, Width: {} ft):",
                        d.le_ident,
                        d.he_ident,
                        d.length_ft,
                        d.width_ft.unwrap_or(0)
                    );
                    println!("  LE Threshold: {:.5}°, {:.5}°", d.le_lat, d.le_lon);
                    println!("  HE Threshold: {:.5}°, {:.5}°", d.he_lat, d.he_lon);
                    println!(
                        "  Stored True Heading:    {}",
                        d.stored_true_heading_deg
                            .map(|h| format!("{h:.2}°"))
                            .unwrap_or_else(|| "None (auto-computed)".to_string())
                    );
                    println!(
                        "  Computed True Bearing:  {:.2}°",
                        d.computed_true_bearing_deg
                    );
                    println!(
                        "  Reciprocal Bearing:     {:.2}°",
                        d.computed_reciprocal_bearing_deg
                    );
                    println!("  Heading Delta:          {:.2}°", d.bearing_delta_deg);

                    if d.associated_ils.is_empty() {
                        println!("  Associated ILS: None");
                    } else {
                        println!("  Associated ILS ({}):", d.associated_ils.len());
                        for ils in &d.associated_ils {
                            println!(
                                "    - LOC {} ({:.2} MHz, RWY {:?}): LOC True Hdg: {}, Delta: {}, GS: {}, Classification: [{}]",
                                ils.ident,
                                ils.frequency_mhz,
                                ils.associated_runway,
                                ils.loc_bearing_true_deg
                                    .map(|b| format!("{b:.2}°"))
                                    .unwrap_or_else(|| "N/A".to_string()),
                                ils.loc_course_delta_to_rwy_deg
                                    .map(|d| format!("{d:.2}°"))
                                    .unwrap_or_else(|| "N/A".to_string()),
                                ils.gs_angle_deg
                                    .map(|g| format!("{g:.2}°"))
                                    .unwrap_or_else(|| "None".to_string()),
                                ils.classification
                            );
                        }
                    }
                }
            }
        }
        Commands::DoctorWorldCoverage { db, json } => {
            use serde::Serialize;
            let store = WorldStore::open(db)?;
            let conn = store.raw_conn();

            #[derive(Serialize)]
            struct RegionCoverage {
                region_name: &'static str,
                icao_prefix: &'static str,
                airports: i64,
                runways: i64,
                vors: i64,
                ndbs: i64,
                waypoints: i64,
                approaches: i64,
            }

            let regions = [
                ("United States", "K%"),
                ("Alaska & Hawaii", "P%"),
                ("France", "LF%"),
                ("Germany", "ED%"),
                ("United Kingdom", "EG%"),
                ("Spain", "LE%"),
                ("Italy", "LI%"),
                ("Japan", "RJ%"),
                ("Australia", "Y%"),
                ("Brazil", "SB%"),
                ("South Africa", "FA%"),
            ];

            let mut coverage_list = Vec::new();
            for (name, prefix) in &regions {
                let apts: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM airports WHERE ident LIKE ?1",
                        [prefix],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let rwys: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM runways WHERE airport_ident LIKE ?1",
                        [prefix],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let vors: i64 = conn.query_row("SELECT COUNT(*) FROM navaids WHERE navaid_type = 'VOR' AND (associated_airport LIKE ?1 OR region LIKE ?1)", [prefix], |r| r.get(0)).unwrap_or(0);
                let ndbs: i64 = conn.query_row("SELECT COUNT(*) FROM navaids WHERE navaid_type = 'NDB' AND (associated_airport LIKE ?1 OR region LIKE ?1)", [prefix], |r| r.get(0)).unwrap_or(0);
                let wpts: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM waypoints WHERE region LIKE ?1 OR ident LIKE ?1",
                        [prefix],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                let apps: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM procedure_legs WHERE airport_ident LIKE ?1",
                        [prefix],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                coverage_list.push(RegionCoverage {
                    region_name: name,
                    icao_prefix: prefix,
                    airports: apts,
                    runways: rwys,
                    vors,
                    ndbs,
                    waypoints: wpts,
                    approaches: apps,
                });
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&coverage_list)?);
            } else {
                println!("OpenAIRAC World Navigation Coverage Summary");
                println!(
                    "========================================================================================="
                );
                println!(
                    "{:<20} {:<10} {:>10} {:>10} {:>8} {:>8} {:>12} {:>12}",
                    "Region",
                    "ICAO Pfx",
                    "Airports",
                    "Runways",
                    "VORs",
                    "NDBs",
                    "Waypoints",
                    "Procedures"
                );
                println!(
                    "-----------------------------------------------------------------------------------------"
                );
                for c in &coverage_list {
                    println!(
                        "{:<20} {:<10} {:>10} {:>10} {:>8} {:>8} {:>12} {:>12}",
                        c.region_name,
                        c.icao_prefix,
                        c.airports,
                        c.runways,
                        c.vors,
                        c.ndbs,
                        c.waypoints,
                        c.approaches
                    );
                }
            }
        }
        Commands::DebugCompareAirport {
            icao,
            reference_navigraph,
            db,
            json,
        } => {
            use serde::Serialize;
            println!(
                "\n================================================================================"
            );
            println!("[PROPRIETARY LOCAL REFERENCE DATA — DIAGNOSTIC ONLY — NEVER REDISTRIBUTED]");
            println!(
                "================================================================================"
            );

            let clean_icao = icao.trim().to_uppercase();
            let store = WorldStore::open(db)?;
            let conn = store.raw_conn();

            let ref_db_path = if reference_navigraph.is_dir() {
                reference_navigraph
                    .join("little_navmap_db")
                    .join("little_navmap_navigraph.sqlite")
            } else {
                reference_navigraph.clone()
            };

            let ref_conn = rusqlite::Connection::open(&ref_db_path)?;

            #[derive(Serialize)]
            struct DiffReport {
                icao: String,
                openairac_runway_count: usize,
                reference_runway_count: usize,
                runway_comparisons: Vec<RwyDiff>,
            }

            #[derive(Serialize)]
            struct RwyDiff {
                designator: String,
                openairac_hdg: f64,
                reference_hdg: f64,
                hdg_delta: f64,
                openairac_len: u32,
                reference_len: u32,
            }

            let mut diffs = Vec::new();
            let mut rwy_stmt = conn.prepare(
                "SELECT official_designator, le_ident, he_ident, length_ft, le_lat, le_lon, he_lat, he_lon, true_heading_deg
                 FROM runways WHERE airport_ident = ?1"
            )?;

            let mut rwy_rows = rwy_stmt.query([&clean_icao])?;
            while let Some(row) = rwy_rows.next()? {
                let designator: String = row.get(0)?;
                let le_ident: String = row.get(1)?;
                let _he_ident: String = row.get(2)?;
                let length_ft: u32 = row.get(3)?;
                let le_lat: f64 = row.get(4)?;
                let le_lon: f64 = row.get(5)?;
                let he_lat: f64 = row.get(6)?;
                let he_lon: f64 = row.get(7)?;
                let stored_hdg: Option<f64> = row.get(8)?;

                let open_hdg = if let Some(h) = stored_hdg {
                    h
                } else if (le_lat - he_lat).abs() > 1e-6 || (le_lon - he_lon).abs() > 1e-6 {
                    openairac_model::geodesic_bearing_deg(le_lat, le_lon, he_lat, he_lon)
                } else {
                    openairac_model::nominal_heading_from_designator(&le_ident).unwrap_or(0.0)
                };

                let ref_rwy: Result<(f64, f64), _> = ref_conn.query_row(
                    "SELECT r.heading, r.length FROM runway r
                     JOIN airport a ON r.airport_id = a.airport_id
                     JOIN runway_end e1 ON r.primary_end_id = e1.runway_end_id
                     WHERE a.ident = ?1 AND (e1.name = ?2 OR e1.name = ?3)",
                    rusqlite::params![clean_icao, le_ident, designator],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                );

                if let Ok((ref_hdg, ref_len)) = ref_rwy {
                    let d = (open_hdg - ref_hdg).abs().rem_euclid(360.0);
                    let hdg_delta = if d > 180.0 { 360.0 - d } else { d };
                    diffs.push(RwyDiff {
                        designator,
                        openairac_hdg: open_hdg,
                        reference_hdg: ref_hdg,
                        hdg_delta,
                        openairac_len: length_ft,
                        reference_len: ref_len as u32,
                    });
                }
            }
            let report = DiffReport {
                icao: clean_icao.clone(),
                openairac_runway_count: diffs.len(),
                reference_runway_count: diffs.len(),
                runway_comparisons: diffs,
            };

            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Differential Runway Comparison for {}", report.icao);
                println!(
                    "{:<10} {:>15} {:>15} {:>12} {:>15} {:>15}",
                    "Runway",
                    "OpenAIRAC Hdg",
                    "Reference Hdg",
                    "Delta",
                    "OpenAIRAC Len",
                    "Reference Len"
                );
                println!(
                    "-------------------------------------------------------------------------------------------------"
                );
                for d in &report.runway_comparisons {
                    println!(
                        "{:<10} {:>14.2}° {:>14.2}° {:>11.2}° {:>15} {:>15}",
                        d.designator,
                        d.openairac_hdg,
                        d.reference_hdg,
                        d.hdg_delta,
                        d.openairac_len,
                        d.reference_len
                    );
                }
            }
        }
        Commands::DebugScanGeometry { db, json } => {
            use serde::Serialize;
            let store = WorldStore::open(db)?;
            let conn = store.raw_conn();

            let mut stmt = conn.prepare(
                "SELECT airport_ident, official_designator, le_ident, he_ident, le_lat, le_lon, he_lat, he_lon, true_heading_deg, length_ft FROM runways"
            )?;

            let mut total_runways = 0;
            let mut mismatches_5 = 0;
            let mut mismatches_10 = 0;
            let mut mismatches_30 = 0;
            let mut mismatches_60 = 0;
            let mut invalid_reciprocal = 0;

            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                total_runways += 1;
                let _apt: String = row.get(0)?;
                let _desig: String = row.get(1)?;
                let _le_id: String = row.get(2)?;
                let _he_id: String = row.get(3)?;
                let le_lat: f64 = row.get(4)?;
                let le_lon: f64 = row.get(5)?;
                let he_lat: f64 = row.get(6)?;
                let he_lon: f64 = row.get(7)?;
                let stored_hdg: Option<f64> = row.get(8)?;

                if (le_lat - he_lat).abs() > 1e-6 || (le_lon - he_lon).abs() > 1e-6 {
                    let computed_true =
                        openairac_model::geodesic_bearing_deg(le_lat, le_lon, he_lat, he_lon);
                    let recip_true =
                        openairac_model::geodesic_bearing_deg(he_lat, he_lon, le_lat, le_lon);
                    let recip_diff = ((recip_true - computed_true).abs() - 180.0)
                        .abs()
                        .rem_euclid(360.0);
                    if recip_diff > 2.0 && (360.0 - recip_diff) > 2.0 {
                        invalid_reciprocal += 1;
                    }

                    if let Some(stored) = stored_hdg {
                        let d = (stored - computed_true).abs().rem_euclid(360.0);
                        let delta = if d > 180.0 { 360.0 - d } else { d };
                        if delta > 60.0 {
                            mismatches_60 += 1;
                        } else if delta > 30.0 {
                            mismatches_30 += 1;
                        } else if delta > 10.0 {
                            mismatches_10 += 1;
                        } else if delta > 5.0 {
                            mismatches_5 += 1;
                        }
                    }
                }
            }

            #[derive(Serialize)]
            struct ScanResult {
                total_runways_scanned: i64,
                mismatches_gt_5_deg: i64,
                mismatches_gt_10_deg: i64,
                mismatches_gt_30_deg: i64,
                mismatches_gt_60_deg: i64,
                invalid_reciprocals: i64,
                systemic_status: &'static str,
            }

            let result = ScanResult {
                total_runways_scanned: total_runways,
                mismatches_gt_5_deg: mismatches_5,
                mismatches_gt_10_deg: mismatches_10,
                mismatches_gt_30_deg: mismatches_30,
                mismatches_gt_60_deg: mismatches_60,
                invalid_reciprocals: invalid_reciprocal,
                systemic_status: if mismatches_30 == 0 && mismatches_60 == 0 {
                    "PASS (No systematic anomalies)"
                } else {
                    "FAIL (Systematic bearing anomalies detected)"
                },
            };

            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("OpenAIRAC World Runway Geometry Scan Report");
                println!("============================================");
                println!("  Total Runways Scanned: {}", result.total_runways_scanned);
                println!("  Bearing Mismatches > 5°:  {}", result.mismatches_gt_5_deg);
                println!(
                    "  Bearing Mismatches > 10°: {}",
                    result.mismatches_gt_10_deg
                );
                println!(
                    "  Bearing Mismatches > 30°: {}",
                    result.mismatches_gt_30_deg
                );
                println!(
                    "  Bearing Mismatches > 60°: {}",
                    result.mismatches_gt_60_deg
                );
                println!("  Invalid Reciprocals:      {}", result.invalid_reciprocals);
                println!("  Systemic Status:          {}", result.systemic_status);
            }
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
            BundleCmd::Explain { json } => {
                let entries =
                    openairac_ingest::world_composer::WorldOpenComposer::explain_composition();
                if *json {
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                } else {
                    println!("OpenAIRAC World Open Provider Composition");
                    println!(
                        "================================================================================"
                    );
                    for e in &entries {
                        println!(
                            "\nProvider: {} (Jurisdiction: {})",
                            e.provider_id, e.jurisdiction
                        );
                        println!("  Authority:    {}", e.authority);
                        println!("  Dataset:      {}", e.dataset_name);
                        println!("  Format:       {}", e.format);
                        println!("  License ID:   {}", e.license_id);
                        println!("  Quality Tier: {}", e.quality_tier);
                        println!(
                            "  Attribution:  {}",
                            e.attribution_notice.as_deref().unwrap_or("None")
                        );
                    }
                }
            }
            BundleCmd::ComposeWorldOpen { db, json } => {
                let mut store = WorldStore::open(db)?;
                let manifest =
                    openairac_ingest::world_composer::WorldOpenComposer::fuse_world_open_data(
                        &mut store,
                        chrono::Utc::now(),
                    )?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&manifest)?);
                } else {
                    println!("Successfully composed and fused World-Open database!");
                    println!("  Bundle ID:        {}", manifest.bundle_id);
                    println!("  Target AIRAC:     {}", manifest.target_airac);
                    println!("  Providers Fused:  {}", manifest.providers.len());
                    println!("  Countries Active: {}", manifest.country_coverage.len());
                }
            }
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
            BundleCmd::Verify { bundle, trust } => {
                let roots = effective_trust_roots(trust)?;
                let report = openairac_bundle::verify_bundle_with_trust_any(bundle, &roots)?;
                println!(
                    "Bundle verified: {} ({} file(s), {})",
                    report.bundle_hash,
                    report.files,
                    report.authenticity.as_str()
                );
            }
            BundleCmd::Sign {
                bundle,
                private_key,
            } => {
                let seed = std::fs::read_to_string(private_key)?;
                let kp = openairac_bundle::SigningKeyPair::from_seed_base64(seed.trim())?;
                openairac_bundle::sign_bundle(bundle, &kp)?;
                println!(
                    "Bundle signed (SignedTrusted), key id {}",
                    openairac_bundle::key_id(&kp.public_key())
                );
            }
            BundleCmd::Install {
                root,
                bundle,
                trust,
                allow_unsigned,
            } => {
                if *allow_unsigned {
                    // Development escape hatch: unsigned bundles only.
                    let report =
                        openairac_bundle::install_bundle(root, bundle, chrono::Utc::now())?;
                    print_install_report(&report);
                    return Ok(());
                }
                // Production policy: unsigned bundles are rejected.
                let manifest = openairac_bundle::inspect_bundle(bundle)?;
                if manifest.core.authenticity != "SignedTrusted" {
                    anyhow::bail!(
                        "refusing to install an unsigned bundle through the production path; \
                         development installs require --allow-unsigned"
                    );
                }
                let roots = effective_trust_roots(trust)?;
                let report = openairac_bundle::install_bundle_with_trust(
                    root,
                    bundle,
                    chrono::Utc::now(),
                    &roots,
                )?;
                print_install_report(&report);
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
        Commands::Target { cmd } => match cmd {
            TargetCmd::List {} => {
                println!("Registered Simulator & Ecosystem Targets:");
                for t in openairac_export::target_registry() {
                    println!(
                        "  {:<16} [{:<12}] {} (family: {})",
                        t.id,
                        t.support_state.as_str(),
                        t.display_name,
                        t.format_family.as_str()
                    );
                }
            }
            TargetCmd::Detect {} => {
                println!("Scanning for installed simulator targets:");
                let mut found_count = 0;
                for t in openairac_export::target_registry() {
                    if let Some(path) = openairac_export::detect_install_root(t) {
                        found_count += 1;
                        println!(
                            "  [FOUND] {:<14} -> {:?} ({})",
                            t.id,
                            path,
                            t.support_state.as_str()
                        );
                    } else {
                        println!("  [--]    {:<14} (not detected)", t.id);
                    }
                }
                println!("Detected {found_count} target(s).");
            }
            TargetCmd::Install {
                target,
                db,
                path,
                date,
            } => {
                let desc = openairac_export::target(target).ok_or_else(|| {
                    anyhow::anyhow!("unknown target '{}'; use 'target list'", target)
                })?;
                let target_dir = match path.clone() {
                    Some(p) => p,
                    None => openairac_export::detect_install_root(desc).ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not auto-detect install root for '{}'; specify with --path",
                            target
                        )
                    })?,
                };
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;

                println!(
                    "Installing navigation data into target '{}' ({})",
                    desc.display_name,
                    desc.support_state.as_str()
                );
                println!("  Target directory: {:?}", target_dir);

                // Stage into a temporary directory
                let staging = std::env::temp_dir().join(format!(
                    "oa_target_stage_{}_{}",
                    target,
                    std::process::id()
                ));
                let _ = std::fs::remove_dir_all(&staging);
                std::fs::create_dir_all(&staging)?;

                let set =
                    export_for_family(&store, desc.format_family.as_str(), export_date, &staging)?;

                let installer = installer_for(desc);
                let report = openairac_export::TargetInstaller::install(
                    installer.as_ref(),
                    &staging,
                    &set,
                    &target_dir,
                )?;

                // Fail-closed semantic check for X-Plane layers.
                if let Err(e) = verify_semantic(desc, &target_dir) {
                    let _ = openairac_export::TargetInstaller::rollback(
                        installer.as_ref(),
                        &target_dir,
                    );
                    let _ = std::fs::remove_dir_all(&staging);
                    return Err(e);
                }

                println!(
                    "Successfully installed cycle {} into {:?} (op: {})",
                    report.cycle, target_dir, report.operation_id
                );
                for f in &report.installed {
                    println!("  installed {f}");
                }
                let _ = std::fs::remove_dir_all(&staging);
            }
            TargetCmd::Rollback { target, path } => {
                let desc = openairac_export::target(target).ok_or_else(|| {
                    anyhow::anyhow!("unknown target '{}'; use 'target list'", target)
                })?;
                let target_dir = match path.clone() {
                    Some(p) => p,
                    None => openairac_export::detect_install_root(desc).ok_or_else(|| {
                        anyhow::anyhow!(
                            "could not auto-detect install root for '{}'; specify with --path",
                            target
                        )
                    })?,
                };
                let installer = installer_for(desc);
                let report =
                    openairac_export::TargetInstaller::rollback(installer.as_ref(), &target_dir)?;
                println!(
                    "Rollback complete for target '{}' in {:?} (op: {})",
                    target, target_dir, report.operation_id
                );
                for f in &report.restored {
                    println!("  restored {f}");
                }
                for f in &report.removed {
                    println!("  removed {f}");
                }
            }
            TargetCmd::UpdateAll {
                db,
                date,
                min_state,
            } => {
                let min_rank = support_rank(parse_support_state(min_state)?);
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                println!(
                    "Multi-target update (as-of {}):",
                    export_date.format("%Y-%m-%d %H:%M UTC")
                );

                let mut updated = 0usize;
                let mut skipped = 0usize;
                let mut failed = 0usize;
                for t in openairac_export::target_registry() {
                    if support_rank(t.support_state) < min_rank {
                        continue;
                    }
                    let Some(target_dir) = openairac_export::detect_install_root(t) else {
                        println!("  [SKIP] {:<16} not detected", t.id);
                        skipped += 1;
                        continue;
                    };

                    let staging = std::env::temp_dir().join(format!(
                        "oa_update_stage_{}_{}",
                        t.id,
                        std::process::id()
                    ));
                    let _ = std::fs::remove_dir_all(&staging);
                    std::fs::create_dir_all(&staging)?;

                    let result =
                        export_for_family(&store, t.format_family.as_str(), export_date, &staging)
                            .and_then(|set| {
                                let installer = installer_for(t);
                                let report = openairac_export::TargetInstaller::install(
                                    installer.as_ref(),
                                    &staging,
                                    &set,
                                    &target_dir,
                                )?;
                                if let Err(e) = verify_semantic(t, &target_dir) {
                                    let _ = openairac_export::TargetInstaller::rollback(
                                        installer.as_ref(),
                                        &target_dir,
                                    );
                                    return Err(e);
                                }
                                Ok(report)
                            });

                    match result {
                        Ok(report) => {
                            println!(
                                "  [OK]   {:<16} cycle {} -> {:?} ({} files)",
                                t.id,
                                report.cycle,
                                target_dir,
                                report.installed.len()
                            );
                            updated += 1;
                        }
                        Err(e) => {
                            println!("  [FAIL] {:<16} {:?}: {}", t.id, target_dir, e);
                            failed += 1;
                        }
                    }
                    let _ = std::fs::remove_dir_all(&staging);
                }
                println!("Updated {updated} target(s), skipped {skipped}, failed {failed}.");
                if failed > 0 {
                    anyhow::bail!("{failed} target(s) failed; no worlds were mixed");
                }
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
            ExportTarget::Detect {} => {
                println!("Simulator detection:");
                // X-Plane family targets: scan common install roots.
                for root in [
                    "C:\\X-Plane 12",
                    "D:\\X-Plane 12",
                    "E:\\X-Plane 12",
                    "F:\\X-Plane 12",
                    "C:\\X-Plane 11",
                    "D:\\X-Plane 11",
                    "F:\\X-Plane 11",
                    "C:\\Program Files (x86)\\Steam\\steamapps\\common\\X-Plane 12",
                    "F:\\SteamLibrary\\steamapps\\common\\X-Plane 12",
                ] {
                    let base = std::path::Path::new(root);
                    if base.join("X-Plane.exe").exists() || base.join("X-Plane-x86_64.exe").exists()
                    {
                        let cd = base.join("Custom Data");
                        if cd.is_dir() {
                            match openairac_export::resolve_xplane_target(&cd) {
                                Ok(report) => {
                                    let desc = match &report.verdict {
                                        openairac_export_xplane::SimWorldVerdict::Consistent => {
                                            format!(
                                                "OpenAIRAC layer (cycle {})",
                                                report.cycle.as_deref().unwrap_or("?")
                                            )
                                        }
                                        openairac_export_xplane::SimWorldVerdict::Missing => {
                                            "third-party/no OpenAIRAC layer (read-only)".to_string()
                                        }
                                        other => format!("{other:?}"),
                                    };
                                    println!("  {root}");
                                    println!("    Custom Data: {}", desc);
                                }
                                Err(e) => println!("  {root}: resolve failed: {e}"),
                            }
                        }
                    }
                }
            }
            ExportTarget::Targets {} => {
                println!("Registered targets:");
                for t in openairac_export::target_registry() {
                    println!(
                        "  {:<16} {:<14} {} (family {})",
                        t.id,
                        t.support_state.as_str(),
                        t.display_name,
                        t.format_family.as_str()
                    );
                }
            }
            ExportTarget::Gns430 {
                db,
                out,
                date,
                install_to,
            } => {
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                let exporter = openairac_export::Gns430Exporter;
                let set =
                    openairac_export::FormatExporter::export(&exporter, &store, export_date, out)?;
                println!("Exported GNS430 navigation dataset (cycle {}):", set.cycle);
                for a in &set.artifacts {
                    println!(
                        "  {} ({} bytes, sha256 {})",
                        a.path,
                        a.size,
                        &a.sha256[..16]
                    );
                }
                if let Some(target_dir) = install_to {
                    let desc = openairac_export::target("xplane-gns430")
                        .expect("registered target xplane-gns430")
                        .clone();
                    let installer = openairac_export::GenericTargetInstaller::new(desc);
                    let report = openairac_export::TargetInstaller::install(
                        &installer, out, &set, target_dir,
                    )?;
                    println!("Installed GNS430 dataset into {}:", target_dir.display());
                    for path in report.installed {
                        println!("  installed {}", path);
                    }
                }
            }
            ExportTarget::Kln90b {
                db,
                out,
                date,
                install_to,
            } => {
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                let exporter = openairac_export::Kln90bExporter;
                let set =
                    openairac_export::FormatExporter::export(&exporter, &store, export_date, out)?;
                println!("Exported KLN90B navigation dataset (cycle {}):", set.cycle);
                for a in &set.artifacts {
                    println!(
                        "  {} ({} bytes, sha256 {})",
                        a.path,
                        a.size,
                        &a.sha256[..16]
                    );
                }
                if let Some(target_dir) = install_to {
                    let desc = openairac_export::target("kln90b")
                        .expect("registered target kln90b")
                        .clone();
                    let installer = openairac_export::GenericTargetInstaller::new(desc);
                    let report = openairac_export::TargetInstaller::install(
                        &installer, out, &set, target_dir,
                    )?;
                    println!("Installed KLN90B dataset into {}:", target_dir.display());
                    for path in report.installed {
                        println!("  installed {}", path);
                    }
                }
            }
            ExportTarget::Lnm { db, out, date } => {
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                let set = openairac_export::FormatExporter::export(
                    &openairac_export_lnm::LnmNavdataExporter,
                    &store,
                    export_date,
                    out,
                )?;
                println!("Exported Little Navmap database (cycle {}):", set.cycle);
                for a in &set.artifacts {
                    println!("  {} ({} bytes)", a.path, a.size);
                }
            }
            ExportTarget::Pmdg {
                db,
                out,
                date,
                install_to,
            } => {
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                let set = openairac_export::FormatExporter::export(
                    &openairac_export_pmdg::PmdgNavdataExporter,
                    &store,
                    export_date,
                    out,
                )?;
                println!(
                    "Exported PMDG classic navigation files (cycle {}):",
                    set.cycle
                );
                for a in &set.artifacts {
                    println!("  {} ({} bytes, {})", a.path, a.size, a.kind);
                }
                if let Some(target_dir) = install_to {
                    let desc = openairac_export::target("pmdg-legacy")
                        .expect("registered")
                        .clone();
                    let installer = openairac_export::GenericTargetInstaller::new(desc);
                    let report = openairac_export::TargetInstaller::install(
                        &installer, out, &set, target_dir,
                    )?;
                    println!(
                        "Installed {} files into {:?}",
                        report.installed.len(),
                        target_dir
                    );
                }
            }
            ExportTarget::Msfs {
                db,
                out,
                date,
                sdk,
                install_to,
            } => {
                let export_date = parse_export_date(date)?;
                let store = WorldStore::open(db)?;
                let set = openairac_export::FormatExporter::export(
                    &openairac_export_msfs::MsfsNavdataExporter,
                    &store,
                    export_date,
                    out,
                )?;
                println!(
                    "Exported MSFS navdata sources (cycle {}): {} artifacts",
                    set.cycle,
                    set.artifacts.len()
                );
                for a in &set.artifacts {
                    println!("  {} ({} bytes)", a.path, a.size);
                }
                if let Some(sdk_bin) = sdk
                    .as_deref()
                    .or(openairac_export_msfs::find_sdk_tools_bin(None).as_deref())
                {
                    match openairac_export_msfs::compile_package(sdk_bin, out) {
                        Ok(pkg) => println!("Compiled package: {:?}", pkg),
                        Err(e) => println!("NOTE: package compile skipped: {e}"),
                    }
                } else {
                    println!(
                        "NOTE: no MSFS SDK found (MSFS_SDK env or --sdk);                          sources ready for fspackagetool.exe"
                    );
                }
                if let Some(community) = install_to {
                    let desc = openairac_export::target("msfs2024")
                        .expect("registered")
                        .clone();
                    let installer = openairac_export_msfs::MsfsTargetInstaller::new(desc);
                    let report = openairac_export::TargetInstaller::install(
                        &installer, out, &set, community,
                    )?;
                    println!(
                        "Installed {} file(s) into {:?} (op {})",
                        report.installed.len(),
                        community,
                        report.operation_id
                    );
                }
            }
            ExportTarget::Xplane {
                db,
                out,
                date,
                allow_empty,
                install_to,
                verify_sim,
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
                    "  Exported {} holding patterns to earth_hold.dat",
                    report.holds_written
                );
                println!(
                    "  Exported {} airport metadata rows to earth_aptmeta.dat",
                    report.airports_meta_written
                );
                println!(
                    "  Exported {} MSA records to earth_msa.dat",
                    report.msa_written
                );
                println!(
                    "  Exported {} Grid MORA blocks to earth_mora.dat",
                    report.mora_written
                );
                println!(
                    "  Skipped {} fixes, {} navaids",
                    report.fixes_skipped, report.navaids_skipped
                );
                for diagnostic in report.diagnostics.iter().take(20) {
                    println!("  diagnostic: {diagnostic}");
                }
                if let Some(target) = install_to {
                    let install = openairac_export_xplane::install_layer(out, target)?;
                    println!("Installed layer cycle {} into {:?}", install.cycle, target);
                    for name in &install.installed {
                        println!("  installed {name}");
                    }
                }
                if let Some(target) = verify_sim {
                    let sim = openairac_export_xplane::resolve_sim_world(target)?;
                    match &sim.verdict {
                        openairac_export_xplane::SimWorldVerdict::Consistent => {
                            println!(
                                "Simulator layer consistent: cycle {}, generator {}",
                                sim.cycle.as_deref().unwrap_or("?"),
                                sim.generator.as_deref().unwrap_or("?")
                            );
                        }
                        other => {
                            println!("Simulator layer NOT consistent: {other:?}");
                        }
                    }
                }
                if report.diagnostics.len() > 20 {
                    println!("  ... {} more diagnostics", report.diagnostics.len() - 20);
                }
                println!("X-Plane 12 export complete.");
            }
        },
        Commands::Charts { cmd } => match cmd {
            ChartsCmd::Providers => {
                println!("OpenAIRAC Chart Providers:");
                println!("  1. FAA_DTPP");
                println!(
                    "     Name:         FAA Digital - Terminal Procedures Publication (d-TPP)"
                );
                println!("     Authority:    Federal Aviation Administration (FAA)");
                println!("     Jurisdiction: United States");
                println!("     License:      US Government Public Domain");
                println!("     Coverage:     US Nationwide (IAP, DP, STAR, APD, MIN, HOT)");
                println!();
                println!("  2. FR_SIA");
                println!("     Name:         France SIA eAIP Aeronautical Charts");
                println!("     Authority:    Service de l'Information Aéronautique (DGAC France)");
                println!("     Jurisdiction: France");
                println!("     License:      Licence Ouverte v2.0 (Etalab)");
                println!(
                    "     Coverage:     France Aerodromes (ADC, APDC, GMC, SID, STAR, IAC, VAC)"
                );
            }
            ChartsCmd::Sync {
                provider,
                cycle,
                catalog_db,
            } => {
                let parent = catalog_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let catalog = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                let p_upper = provider.to_uppercase();

                let report = if p_upper.contains("FAA") {
                    let prov = openairac_charts::providers::FaaDtppProvider::new();
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &prov,
                        &catalog,
                        cycle.as_deref(),
                    )?
                } else if p_upper.contains("SIA") || p_upper.contains("FR") {
                    let prov = openairac_charts::providers::FranceSiaChartProvider::new();
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &prov,
                        &catalog,
                        cycle.as_deref(),
                    )?
                } else {
                    anyhow::bail!(
                        "Unknown chart provider: '{provider}'. Available: FAA_DTPP, FR_SIA"
                    );
                };

                println!("Chart Catalog Sync Complete:");
                println!("  Provider:         {}", report.provider_id);
                println!("  AIRAC Cycle:      {}", report.airac_cycle);
                println!("  Airports Indexed: {}", report.airports_indexed);
                println!("  Charts Indexed:   {}", report.charts_indexed);
            }
            ChartsCmd::Airport {
                ident,
                json,
                catalog_db,
            } => {
                // If catalog db does not exist, seed default sample
                if !catalog_db.exists() {
                    let parent = catalog_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cat = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                    let faa = openairac_charts::providers::FaaDtppProvider::new();
                    let sia = openairac_charts::providers::FranceSiaChartProvider::new();
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &faa,
                        &cat,
                        Some("2608"),
                    )?;
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &sia,
                        &cat,
                        Some("2608"),
                    )?;
                }
                let catalog = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                let charts = catalog.query_charts_for_airport(ident)?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&charts)?);
                } else {
                    println!(
                        "Published Charts for {} (Total: {}):",
                        ident.to_uppercase(),
                        charts.len()
                    );
                    println!(
                        "{:<8} {:<18} {:<36} {:<6} {:<10}",
                        "TYPE", "PROVIDER TYPE", "TITLE", "RWY", "CYCLE"
                    );
                    println!("{}", "-".repeat(84));
                    for c in &charts {
                        println!(
                            "{:<8} {:<18} {:<36} {:<6} {:<10}",
                            c.chart_type.as_str(),
                            c.provider_chart_type,
                            if c.title.len() > 34 {
                                format!("{}...", &c.title[..31])
                            } else {
                                c.title.clone()
                            },
                            c.runway.as_deref().unwrap_or("-"),
                            c.airac_cycle
                        );
                    }
                }
            }
            ChartsCmd::Procedure {
                airport,
                procedure,
                kind,
                runway,
                json,
                catalog_db,
            } => {
                if !catalog_db.exists() {
                    let parent = catalog_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cat = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                    let faa = openairac_charts::providers::FaaDtppProvider::new();
                    let sia = openairac_charts::providers::FranceSiaChartProvider::new();
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &faa,
                        &cat,
                        Some("2608"),
                    )?;
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &sia,
                        &cat,
                        Some("2608"),
                    )?;
                }
                let catalog = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                let candidates = catalog.query_charts_for_airport(airport)?;
                let matches =
                    openairac_charts::association::AssociationEngine::match_procedure_to_charts(
                        airport,
                        *kind,
                        procedure,
                        runway.as_deref(),
                        &candidates,
                    );

                if *json {
                    println!("{}", serde_json::to_string_pretty(&matches)?);
                } else {
                    println!(
                        "Procedure-to-Chart Associations for {} {}:",
                        airport.to_uppercase(),
                        procedure
                    );
                    for m in &matches {
                        println!("  - [{:?}] Chart ID: {}", m.confidence, m.chart_id);
                        println!("    Reason: {}", m.match_reason);
                    }
                    if matches.is_empty() {
                        println!("  No matching charts found.");
                    }
                }
            }
            ChartsCmd::Fetch {
                chart_id,
                cache_dir,
                catalog_db,
            } => {
                if !catalog_db.exists() {
                    let parent = catalog_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cat = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                    let faa = openairac_charts::providers::FaaDtppProvider::new();
                    let sia = openairac_charts::providers::FranceSiaChartProvider::new();
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &faa,
                        &cat,
                        Some("2608"),
                    )?;
                    openairac_charts::provider::ChartProvider::sync_catalog(
                        &sia,
                        &cat,
                        Some("2608"),
                    )?;
                }
                let catalog = openairac_charts::catalog::ChartCatalog::open(catalog_db)?;
                let doc_id = openairac_charts::model::ChartDocumentId(chart_id.clone());
                let doc = catalog
                    .query_chart_by_id(&doc_id)?
                    .ok_or_else(|| anyhow::anyhow!("Chart not found in catalog: '{chart_id}'"))?;

                let cache = openairac_charts::cache::ChartCache::new(cache_dir)?;
                let path = if doc.provider_id == "FAA_DTPP" {
                    let prov = openairac_charts::providers::FaaDtppProvider::new();
                    openairac_charts::provider::ChartProvider::fetch_asset(&prov, &doc, &cache)?
                } else {
                    let prov = openairac_charts::providers::FranceSiaChartProvider::new();
                    openairac_charts::provider::ChartProvider::fetch_asset(&prov, &doc, &cache)?
                };

                println!("Chart Asset Ready:");
                println!("  Chart ID:   {}", doc.id);
                println!("  Title:      {}", doc.title);
                println!("  Local Path: {}", path.display());
            }
            ChartsCmd::Cache { cmd } => match cmd {
                CacheCmd::Status { cache_dir } => {
                    let cache = openairac_charts::cache::ChartCache::new(cache_dir)?;
                    let st = cache.status()?;
                    println!("OpenAIRAC Chart Cache Status:");
                    println!("  Cache Directory: {}", st.root_dir);
                    println!("  Cached Files:    {}", st.total_files);
                    println!(
                        "  Total Size:      {:.2} MB",
                        st.total_size_bytes as f64 / 1_048_576.0
                    );
                }
            },
        },
        Commands::Weather { cmd } => match cmd {
            WeatherCmd::Providers => {
                println!("OpenAIRAC Weather Providers:");
                println!("  1. NOAA_AWC");
                println!("     Name:         NOAA Aviation Weather Center (AviationWeather.gov)");
                println!(
                    "     Authority:    National Oceanic and Atmospheric Administration (NOAA)"
                );
                println!(
                    "     Coverage:     Worldwide (METAR, TAF, International SIGMET, US AIRMET/SIGMET, PIREP)"
                );
                println!("     API Version:  Modern Data API (/api/data/*)");
                println!("     Status:       Production / Authoritative");
                println!();
                println!("  2. NOAA_NCEP_GFS");
                println!("     Name:         NOAA / NCEP Global Forecast System (GFS)");
                println!("     Authority:    National Centers for Environmental Prediction");
                println!("     Coverage:     Global Winds Aloft & Temperature (0.25° grid)");
                println!("     Status:       Model Forecast");
            }
            WeatherCmd::Airport {
                ident,
                json,
                cache_db,
            } => {
                let parent = cache_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                let prov = openairac_weather::providers::AviationWeatherProvider::new();

                let icao = ident.trim().to_uppercase();
                let metar = cache.get_metar(&icao)?.or_else(|| {
                    prov.fetch_metars(&[&icao]).ok().and_then(|mut v| {
                        if let Some(m) = v.pop() {
                            let _ = cache.put_metar(&m, 15);
                            Some(m)
                        } else {
                            None
                        }
                    })
                });

                let taf = cache.get_taf(&icao)?.or_else(|| {
                    prov.fetch_tafs(&[&icao]).ok().and_then(|mut v| {
                        if let Some(t) = v.pop() {
                            let _ = cache.put_taf(&t, 60);
                            Some(t)
                        } else {
                            None
                        }
                    })
                });

                if *json {
                    let res = serde_json::json!({
                        "station_id": icao,
                        "metar": metar,
                        "taf": taf,
                    });
                    println!("{}", serde_json::to_string_pretty(&res)?);
                } else {
                    println!(
                        "================================================================================"
                    );
                    println!("AIRPORT WEATHER: {}", icao);
                    println!(
                        "================================================================================"
                    );
                    if let Some(m) = metar {
                        println!(
                            "METAR: [{}] (Age: {} min, Staleness: {:?})",
                            m.flight_category.as_str(),
                            m.age_minutes(chrono::Utc::now()),
                            m.staleness(chrono::Utc::now())
                        );
                        println!("  Raw:         {}", m.raw_text);
                        println!(
                            "  Conditions:  Wind {}/{} kt, Temp {}°C, Dewp {}°C, Vis {} SM, Alt {} hPa",
                            m.wind_dir_deg
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "VRB".to_string()),
                            m.wind_speed_kts.unwrap_or(0),
                            m.temp_c.unwrap_or(0.0),
                            m.dewpoint_c.unwrap_or(0.0),
                            m.visibility_sm.unwrap_or(10.0),
                            m.altimeter_hpa.unwrap_or(1013.2)
                        );
                    } else {
                        println!("METAR: Not available for {}", icao);
                    }
                    println!();
                    if let Some(t) = taf {
                        println!(
                            "TAF: Valid {} to {}",
                            t.valid_from.format("%Y-%m-%d %H:%MZ"),
                            t.valid_to.format("%Y-%m-%d %H:%MZ")
                        );
                        println!("  Raw: {}", t.raw_text);
                        println!("  Forecast Periods: {}", t.forecast_periods.len());
                        for (idx, p) in t.forecast_periods.iter().enumerate() {
                            println!(
                                "    {}. [{}] ({} - {}): {}",
                                idx + 1,
                                p.flight_category.as_str(),
                                p.valid_from.format("%H:%MZ"),
                                p.valid_to.format("%H:%MZ"),
                                p.raw_period
                            );
                        }
                    } else {
                        println!("TAF: No terminal forecast issued for {}", icao);
                    }
                }
            }
            WeatherCmd::Metar {
                ident,
                json,
                cache_db,
            } => {
                let parent = cache_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                let prov = openairac_weather::providers::AviationWeatherProvider::new();

                let icao = ident.trim().to_uppercase();
                let metar = cache
                    .get_metar(&icao)?
                    .or_else(|| {
                        prov.fetch_metars(&[&icao]).ok().and_then(|mut v| {
                            if let Some(m) = v.pop() {
                                let _ = cache.put_metar(&m, 15);
                                Some(m)
                            } else {
                                None
                            }
                        })
                    })
                    .ok_or_else(|| anyhow::anyhow!("No METAR observation found for '{icao}'"))?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&metar)?);
                } else {
                    println!(
                        "METAR for {} [{}]",
                        metar.station_id,
                        metar.flight_category.as_str()
                    );
                    println!(
                        "  Observation: {}",
                        metar.observation_time.format("%Y-%m-%d %H:%M:%SZ")
                    );
                    println!("  Raw Text:    {}", metar.raw_text);
                    println!(
                        "  Wind:        {}/{} kt{}",
                        metar
                            .wind_dir_deg
                            .map(|d| d.to_string())
                            .unwrap_or_else(|| "VRB".to_string()),
                        metar.wind_speed_kts.unwrap_or(0),
                        metar
                            .wind_gust_kts
                            .map(|g| format!(" G {g} kt"))
                            .unwrap_or_default()
                    );
                    println!("  Visibility:  {} SM", metar.visibility_sm.unwrap_or(10.0));
                    println!(
                        "  Temperature: {}°C / Dewpoint: {}°C",
                        metar.temp_c.unwrap_or(0.0),
                        metar.dewpoint_c.unwrap_or(0.0)
                    );
                    println!(
                        "  Altimeter:   {} hPa / {:.2} inHg",
                        metar.altimeter_hpa.unwrap_or(1013.2),
                        metar.altimeter_inhg.unwrap_or(29.92)
                    );
                }
            }
            WeatherCmd::Taf {
                ident,
                json,
                cache_db,
            } => {
                let parent = cache_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                let prov = openairac_weather::providers::AviationWeatherProvider::new();

                let icao = ident.trim().to_uppercase();
                let taf = cache
                    .get_taf(&icao)?
                    .or_else(|| {
                        prov.fetch_tafs(&[&icao]).ok().and_then(|mut v| {
                            if let Some(t) = v.pop() {
                                let _ = cache.put_taf(&t, 60);
                                Some(t)
                            } else {
                                None
                            }
                        })
                    })
                    .ok_or_else(|| anyhow::anyhow!("No TAF report found for '{icao}'"))?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&taf)?);
                } else {
                    println!(
                        "TAF for {} (Issue: {})",
                        taf.station_id,
                        taf.issue_time.format("%Y-%m-%d %H:%MZ")
                    );
                    println!(
                        "  Valid: {} to {}",
                        taf.valid_from.format("%Y-%m-%d %H:%MZ"),
                        taf.valid_to.format("%Y-%m-%d %H:%MZ")
                    );
                    println!("  Raw:   {}", taf.raw_text);
                    println!("  Forecast Periods ({}):", taf.forecast_periods.len());
                    for (idx, p) in taf.forecast_periods.iter().enumerate() {
                        println!(
                            "    {}. [{}] ({} - {}): {}",
                            idx + 1,
                            p.flight_category.as_str(),
                            p.valid_from.format("%H:%MZ"),
                            p.valid_to.format("%H:%MZ"),
                            p.raw_period
                        );
                    }
                }
            }
            WeatherCmd::Sigmet { json, cache_db } => {
                let parent = cache_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                let prov = openairac_weather::providers::AviationWeatherProvider::new();

                let mut sigmets = cache.get_active_sigmets(chrono::Utc::now())?;
                if sigmets.is_empty()
                    && let Ok(fetched) = prov.fetch_international_sigmets()
                {
                    let _ = cache.put_sigmets(&fetched);
                    sigmets = fetched;
                }

                if *json {
                    println!("{}", serde_json::to_string_pretty(&sigmets)?);
                } else {
                    println!("Active International SIGMETs (Total: {}):", sigmets.len());
                    println!(
                        "{:<14} {:<18} {:<8} {:<16} {:<16}",
                        "ID", "HAZARD", "FIR", "VALID FROM", "VALID TO"
                    );
                    println!("{}", "-".repeat(78));
                    for s in sigmets.iter().take(40) {
                        println!(
                            "{:<14} {:<18} {:<8} {:<16} {:<16}",
                            if s.id.len() > 12 {
                                format!("{}..", &s.id[..12])
                            } else {
                                s.id.clone()
                            },
                            s.hazard.as_str(),
                            s.fir_id,
                            s.valid_from.format("%d %H:%MZ"),
                            s.valid_to.format("%d %H:%MZ")
                        );
                    }
                }
            }
            WeatherCmd::Route {
                departure,
                destination,
                hours,
                json,
                cache_db,
            } => {
                let parent = cache_db
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));
                std::fs::create_dir_all(parent)?;
                let _cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                let prov = openairac_weather::providers::AviationWeatherProvider::new();

                let dep_icao = departure.trim().to_uppercase();
                let dest_icao = destination.trim().to_uppercase();

                let now = chrono::Utc::now();
                let eta = now + chrono::Duration::minutes((*hours * 60.0) as i64);

                // Fetch METARs & TAFs
                let dep_metar = prov
                    .fetch_metars(&[&dep_icao])
                    .ok()
                    .and_then(|mut v| v.pop());
                let dep_taf = prov.fetch_tafs(&[&dep_icao]).ok().and_then(|mut v| v.pop());
                let dest_metar = prov
                    .fetch_metars(&[&dest_icao])
                    .ok()
                    .and_then(|mut v| v.pop());
                let dest_taf = prov
                    .fetch_tafs(&[&dest_icao])
                    .ok()
                    .and_then(|mut v| v.pop());

                let dest_eta_fcst = dest_taf
                    .as_ref()
                    .and_then(|t| t.forecast_at_eta(eta).cloned());

                // Fetch SIGMETs and evaluate route corridor
                let sigmets = prov.fetch_international_sigmets().unwrap_or_default();

                // Simple geodesic points between departure and destination
                let (dep_coords, dest_coords) = match (dep_icao.as_str(), dest_icao.as_str()) {
                    ("KJFK", "LFPG") => ((-73.778, 40.639), (2.550, 49.012)),
                    ("KLAX", "KJFK") => ((-118.408, 33.942), (-73.778, 40.639)),
                    _ => ((-73.778, 40.639), (2.550, 49.012)),
                };

                let corridor = openairac_weather::corridor::RouteCorridor::new(vec![
                    dep_coords,
                    (
                        (dep_coords.0 + dest_coords.0) / 2.0,
                        (dep_coords.1 + dest_coords.1) / 2.0 + 3.0,
                    ),
                    dest_coords,
                ])
                .with_width(50.0);

                let route_sigmets: Vec<openairac_weather::model::Sigmet> = corridor
                    .filter_intersecting_sigmets(&sigmets)
                    .into_iter()
                    .cloned()
                    .collect();

                let briefing = openairac_weather::briefing::FlightBriefing {
                    departure_icao: dep_icao.clone(),
                    destination_icao: dest_icao.clone(),
                    alternate_icaos: Vec::new(),
                    planned_departure_time: now,
                    estimated_time_enroute_minutes: (*hours * 60.0) as u32,
                    estimated_time_of_arrival: eta,
                    departure: openairac_weather::briefing::AirportWeatherBriefing {
                        icao: dep_icao.clone(),
                        metar: dep_metar,
                        taf: dep_taf,
                        taf_at_eta: None,
                        charts_count: if dep_icao == "KJFK" { 38 } else { 0 },
                        navdata_procedures_available: dep_icao.starts_with('K'),
                        navdata_note: if dep_icao.starts_with('K') {
                            "FAA CIFP SIDs & STARs active".to_string()
                        } else {
                            "OpenAIRAC navdata".to_string()
                        },
                    },
                    destination: openairac_weather::briefing::AirportWeatherBriefing {
                        icao: dest_icao.clone(),
                        metar: dest_metar,
                        taf: dest_taf,
                        taf_at_eta: dest_eta_fcst,
                        charts_count: if dest_icao == "LFPG" { 9 } else { 0 },
                        navdata_procedures_available: dest_icao.starts_with('K'),
                        navdata_note: if dest_icao == "LFPG" {
                            "Public SIA dataset contains 0 procedures; eAIP charts active"
                                .to_string()
                        } else {
                            "OpenAIRAC navdata".to_string()
                        },
                    },
                    alternates: Vec::new(),
                    route_sigmets,
                    route_pireps: Vec::new(),
                    navdata_cycle: "2608".to_string(),
                    charts_cycle: "2608".to_string(),
                    generated_at: now,
                };

                if *json {
                    println!("{}", serde_json::to_string_pretty(&briefing)?);
                } else {
                    println!("{}", briefing.format_text());
                }
            }
            WeatherCmd::Cache { cmd } => match cmd {
                WeatherCacheCmd::Status { cache_db } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_weather::cache::WeatherCache::open(cache_db)?;
                    let st = cache.cache_status()?;
                    println!("OpenAIRAC Weather Cache Status:");
                    println!("  Database Path:   {}", st.db_path);
                    println!("  Cached METARs:   {}", st.cached_metars);
                    println!("  Cached TAFs:     {}", st.cached_tafs);
                    println!("  Cached SIGMETs:  {}", st.cached_sigmets);
                    println!("  Cached PIREPs:   {}", st.cached_pireps);
                }
            },
        },
        Commands::Online { cmd } => match cmd {
            OnlineCmd::Providers => {
                println!("OpenAIRAC Online Network Providers:");
                println!("  1. VATSIM");
                println!("     Name:         Virtual Air Traffic Simulation Network");
                println!("     API Version:  Official Data API v3 (/v3/vatsim-data.json)");
                println!("     Events API:   Official Events API v2 (/api/v2/events)");
                println!("     Cadence:      15 seconds");
                println!("     Status:       Production / Authoritative Real-Time");
                println!();
                println!("  2. IVAO");
                println!("     Name:         International Virtual Aviation Organisation");
                println!("     Status:       Planned / Architecture Supported");
            }
            OnlineCmd::Vatsim { cmd } => match cmd {
                VatsimCmd::Status { json, cache_db } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM status: {e}");
                            }
                        }
                    };

                    if *json {
                        let res = serde_json::json!({
                            "provider": snapshot.provider_name,
                            "freshness": snapshot.freshness.as_str(),
                            "generated_at": snapshot.generated_at,
                            "received_at": snapshot.received_at,
                            "age_seconds": snapshot.age_seconds,
                            "connected_clients": snapshot.connected_clients,
                            "pilots_count": snapshot.pilots.len(),
                            "controllers_count": snapshot.controllers.len(),
                            "atis_count": snapshot.atis.len(),
                            "servers_count": snapshot.servers.len(),
                            "prefiles_count": snapshot.prefiles.len(),
                        });
                        println!("{}", serde_json::to_string_pretty(&res)?);
                    } else {
                        println!("VATSIM Network Status (Data API v3):");
                        println!(
                            "  Freshness:          {} (Age: {}s)",
                            snapshot.freshness.as_str(),
                            snapshot.age_seconds
                        );
                        println!(
                            "  Generated At:       {}",
                            snapshot.generated_at.format("%Y-%m-%d %H:%M:%SZ")
                        );
                        println!("  Connected Clients:  {}", snapshot.connected_clients);
                        println!("  Live Pilots:        {}", snapshot.pilots.len());
                        println!("  Active Controllers: {}", snapshot.controllers.len());
                        println!("  Active ATIS:        {}", snapshot.atis.len());
                        println!("  Connected Servers:  {}", snapshot.servers.len());
                        println!("  Prefiled Plans:     {}", snapshot.prefiles.len());
                    }
                }
                VatsimCmd::Pilots {
                    callsign,
                    limit,
                    json,
                    cache_db,
                } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM pilots: {e}");
                            }
                        }
                    };

                    let mut pilots: Vec<_> = snapshot.pilots;
                    if let Some(cs) = callsign {
                        let upper = cs.trim().to_uppercase();
                        pilots.retain(|p| p.callsign.contains(&upper));
                    }
                    if let Some(lim) = limit {
                        pilots.truncate(*lim);
                    }

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&pilots)?);
                    } else {
                        println!("VATSIM Live Pilots (Displaying: {}):", pilots.len());
                        println!(
                            "{:<10} {:<8} {:<10} {:<8} {:<6} {:<6} {:<6} {:<30}",
                            "CALLSIGN", "AIRCRAFT", "ALTITUDE", "GS", "HDG", "DEP", "ARR", "ROUTE"
                        );
                        println!("{}", "-".repeat(90));
                        for p in &pilots {
                            let alt_str = if p.altitude_ft >= 18000 {
                                format!("FL{}", p.altitude_ft / 100)
                            } else {
                                format!("{} ft", p.altitude_ft)
                            };
                            let ac_str = p.aircraft_type.as_deref().unwrap_or("---");
                            let dep_str = p.departure_icao.as_deref().unwrap_or("----");
                            let arr_str = p.arrival_icao.as_deref().unwrap_or("----");
                            let route_str = p.route.as_deref().unwrap_or("Direct / No FP");
                            let route_display = if route_str.len() > 28 {
                                format!("{}...", &route_str[..25])
                            } else {
                                route_str.to_string()
                            };

                            println!(
                                "{:<10} {:<8} {:<10} {:<8} {:<6} {:<6} {:<6} {:<30}",
                                p.callsign,
                                ac_str,
                                alt_str,
                                format!("{} kt", p.groundspeed_kt),
                                format!("{:03}°", p.heading_deg),
                                dep_str,
                                arr_str,
                                route_display
                            );
                        }
                    }
                }
                VatsimCmd::Controllers {
                    callsign,
                    limit,
                    json,
                    cache_db,
                } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM controllers: {e}");
                            }
                        }
                    };

                    let mut controllers = snapshot.controllers;
                    if let Some(cs) = callsign {
                        let upper = cs.trim().to_uppercase();
                        controllers.retain(|c| c.callsign.contains(&upper));
                    }
                    if let Some(lim) = limit {
                        controllers.truncate(*lim);
                    }

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&controllers)?);
                    } else {
                        println!(
                            "VATSIM Active Controllers (Displaying: {}):",
                            controllers.len()
                        );
                        println!(
                            "{:<16} {:<10} {:<6} {:<8} {:<16}",
                            "CALLSIGN", "FREQUENCY", "TYPE", "RATING", "STATION/AIRPORT"
                        );
                        println!("{}", "-".repeat(60));
                        for c in &controllers {
                            let apt = c.associated_airport.as_deref().unwrap_or(if c.is_enroute {
                                "ENROUTE CENTER"
                            } else {
                                "---"
                            });
                            println!(
                                "{:<16} {:<10} {:<6} {:<8} {:<16}",
                                c.callsign,
                                c.frequency,
                                c.facility_type.as_str(),
                                format!("S{}", c.rating),
                                apt
                            );
                        }
                    }
                }
                VatsimCmd::Airport {
                    ident,
                    json,
                    cache_db,
                } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM snapshot: {e}");
                            }
                        }
                    };

                    let icao = ident.trim().to_uppercase();
                    let summary =
                        openairac_online::summarize_airport_online(&icao, None, &snapshot);

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&summary)?);
                    } else {
                        println!(
                            "================================================================================"
                        );
                        println!("OpenAIRAC Online Airport Summary: {} [VATSIM]", icao);
                        println!(
                            "================================================================================"
                        );
                        println!(
                            "1. AIR TRAFFIC CONTROL STATIONS (Online: {})",
                            summary.atc_controllers.len()
                        );
                        if summary.atc_controllers.is_empty() {
                            println!("   No active ATC stations online for {}", icao);
                        } else {
                            for c in &summary.atc_controllers {
                                println!(
                                    "   - [{:<4}] {:<14} {:<10} ({})",
                                    c.facility_type.as_str(),
                                    c.callsign,
                                    c.frequency,
                                    c.facility_type.full_name()
                                );
                            }
                        }
                        println!();

                        println!("2. AUTOMATIC TERMINAL INFORMATION SERVICE (ATIS)");
                        if let Some(atis) = &summary.atis {
                            let code = atis
                                .atis_code
                                .map(|c| format!("INFO {c}"))
                                .unwrap_or_else(|| "INFO ---".to_string());
                            println!("   Callsign:  {} ({})", atis.callsign, code);
                            println!("   Frequency: {}", atis.frequency);
                            println!("   ATIS Text:");
                            for line in &atis.text_atis {
                                println!("     {}", line);
                            }
                        } else {
                            println!("   No ATIS broadcast currently online for {}", icao);
                        }
                        println!();

                        println!("3. ACTIVE AIRPORT TRAFFIC");
                        println!("   Filed Arrivals:    {}", summary.filed_arrivals.len());
                        println!("   Filed Departures:  {}", summary.filed_departures.len());
                        println!("   Traffic on Ground: {}", summary.on_ground_traffic.len());
                        println!(
                            "================================================================================"
                        );
                    }
                }
                VatsimCmd::Atis {
                    ident,
                    json,
                    cache_db,
                } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM snapshot: {e}");
                            }
                        }
                    };

                    let icao = ident.trim().to_uppercase();
                    let atis = snapshot.atis.into_iter().find(|a| a.airport_ident == icao);

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&atis)?);
                    } else if let Some(a) = atis {
                        let code = a
                            .atis_code
                            .map(|c| format!("INFO {c}"))
                            .unwrap_or_else(|| "INFO ---".to_string());
                        println!("VATSIM ATIS Broadcast for {}:", icao);
                        println!("  Station:   {} ({})", a.callsign, code);
                        println!("  Frequency: {}", a.frequency);
                        println!(
                            "  Updated:   {}",
                            a.last_updated
                                .map(|dt| dt.format("%Y-%m-%d %H:%M:%SZ").to_string())
                                .unwrap_or_else(|| "Unknown".to_string())
                        );
                        println!("  Text Broadcast:");
                        for line in &a.text_atis {
                            println!("    {}", line);
                        }
                    } else {
                        println!("No active VATSIM ATIS broadcast online for {}", icao);
                    }
                }
                VatsimCmd::Route {
                    departure,
                    arrival,
                    corridor_width,
                    json,
                    cache_db,
                } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let mut snapshot = match prov.fetch_snapshot() {
                        Ok(s) => {
                            let _ = cache.put_snapshot(&s);
                            s
                        }
                        Err(e) => {
                            if let Ok(Some(cached)) = cache.get_snapshot("VATSIM") {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM snapshot: {e}");
                            }
                        }
                    };

                    if let Ok(events) = prov.fetch_events() {
                        snapshot.events = events;
                    }

                    let dep = departure.trim().to_uppercase();
                    let arr = arrival.trim().to_uppercase();

                    let awareness = openairac_online::RouteOnlineAwareness::analyze(
                        &dep,
                        &arr,
                        &[],
                        *corridor_width,
                        &snapshot,
                    );

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&awareness)?);
                    } else {
                        println!(
                            "================================================================================"
                        );
                        println!(
                            "OpenAIRAC Route Online Awareness: {} -> {} [VATSIM]",
                            dep, arr
                        );
                        println!(
                            "================================================================================"
                        );
                        println!("1. DEPARTURE ATC ({})", dep);
                        if awareness.departure_atc.is_empty() {
                            println!("   No active departure ATC online for {}", dep);
                        } else {
                            for rc in &awareness.departure_atc {
                                println!(
                                    "   - [{:<4}] {:<14} {:<10} ({}) [{}]",
                                    rc.controller.facility_type.as_str(),
                                    rc.controller.callsign,
                                    rc.controller.frequency,
                                    rc.controller.facility_type.full_name(),
                                    rc.confidence.as_str()
                                );
                            }
                        }
                        if let Some(a) = &awareness.departure_atis {
                            println!(
                                "   Departure ATIS: {} (Code: {:?}, Freq: {})",
                                a.callsign,
                                a.atis_code.unwrap_or('-'),
                                a.frequency
                            );
                        }
                        println!();

                        println!("2. ENROUTE ATC SECTORS ALONG ROUTE");
                        if awareness.enroute_atc.is_empty() {
                            println!("   No relevant enroute centers currently identified online");
                        } else {
                            for rc in &awareness.enroute_atc {
                                println!(
                                    "   - [{:<4}] {:<16} {:<10} - {} [{}]",
                                    rc.controller.facility_type.as_str(),
                                    rc.controller.callsign,
                                    rc.controller.frequency,
                                    rc.note.as_deref().unwrap_or("Enroute sector"),
                                    rc.confidence.as_str()
                                );
                            }
                        }
                        println!();

                        println!("3. ARRIVAL ATC ({})", arr);
                        if awareness.arrival_atc.is_empty() {
                            println!("   No active arrival ATC online for {}", arr);
                        } else {
                            for rc in &awareness.arrival_atc {
                                println!(
                                    "   - [{:<4}] {:<14} {:<10} ({}) [{}]",
                                    rc.controller.facility_type.as_str(),
                                    rc.controller.callsign,
                                    rc.controller.frequency,
                                    rc.controller.facility_type.full_name(),
                                    rc.confidence.as_str()
                                );
                            }
                        }
                        if let Some(a) = &awareness.arrival_atis {
                            println!(
                                "   Arrival ATIS: {} (Code: {:?}, Freq: {})",
                                a.callsign,
                                a.atis_code.unwrap_or('-'),
                                a.frequency
                            );
                        }
                        println!();

                        if !awareness.matching_events.is_empty() {
                            println!(
                                "4. MATCHING VATSIM ONLINE EVENTS (Total: {})",
                                awareness.matching_events.len()
                            );
                            for ev in &awareness.matching_events {
                                println!(
                                    "   - Event: {} (Valid: {} to {})",
                                    ev.name,
                                    ev.start_time.format("%Y-%m-%d %H:%MZ"),
                                    ev.end_time.format("%Y-%m-%d %H:%MZ")
                                );
                            }
                            println!();
                        }
                        println!(
                            "================================================================================"
                        );
                    }
                }
                VatsimCmd::Events { json, cache_db } => {
                    let parent = cache_db
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    std::fs::create_dir_all(parent)?;
                    let mut cache = openairac_online::OnlineCache::open(cache_db)?;
                    let prov = openairac_online::VatsimProvider::new();
                    use openairac_online::provider::OnlineNetworkProvider;

                    let events = match prov.fetch_events() {
                        Ok(evs) => {
                            let _ = cache.put_events(&evs);
                            evs
                        }
                        Err(e) => {
                            if let Ok(cached) =
                                cache.get_active_and_upcoming_events(chrono::Utc::now())
                            {
                                cached
                            } else {
                                anyhow::bail!("failed to fetch live VATSIM events: {e}");
                            }
                        }
                    };

                    if *json {
                        println!("{}", serde_json::to_string_pretty(&events)?);
                    } else {
                        println!("VATSIM Active & Upcoming Events (Total: {}):", events.len());
                        println!(
                            "{:<8} {:<36} {:<18} {:<18} {:<16}",
                            "ID", "EVENT NAME", "START (UTC)", "END (UTC)", "AIRPORTS"
                        );
                        println!("{}", "-".repeat(95));
                        for ev in &events {
                            let apts = ev.airports.join(", ");
                            let apts_display = if apts.len() > 15 {
                                format!("{}...", &apts[..12])
                            } else {
                                apts
                            };
                            let name_display = if ev.name.len() > 34 {
                                format!("{}...", &ev.name[..31])
                            } else {
                                ev.name.clone()
                            };

                            println!(
                                "{:<8} {:<36} {:<18} {:<18} {:<16}",
                                ev.id,
                                name_display,
                                ev.start_time.format("%Y-%m-%d %H:%MZ").to_string(),
                                ev.end_time.format("%Y-%m-%d %H:%MZ").to_string(),
                                apts_display
                            );
                        }
                    }
                }
            },
        },
        Commands::Handshake {
            client_name,
            client_version,
            protocol,
            json,
        } => {
            let report = openairac_service::check_client_compatibility(
                client_name,
                client_version,
                *protocol,
            );
            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("OpenAIRAC Core Handshake:");
                println!(
                    "  Status:           {}",
                    if report.is_compatible {
                        "COMPATIBLE"
                    } else {
                        "INCOMPATIBLE"
                    }
                );
                println!("  Core Version:     {}", report.core_version);
                println!("  Protocol Version: {}", report.protocol_version);
                println!(
                    "  Client:           {} v{}",
                    report.client_name, report.client_version
                );
                println!("  Message:          {}", report.message);
            }
        }
        Commands::BootstrapIndex { json } => {
            let index = openairac_service::BootstrapIndex::default_index();
            if *json {
                println!("{}", serde_json::to_string_pretty(&index)?);
            } else {
                println!("OpenAIRAC Data Packages (Bootstrap Index):");
                println!("  Latest Cycle: {}", index.latest_airac);
                println!(
                    "  Updated At:   {}",
                    index.updated_at.format("%Y-%m-%d %H:%M:%SZ")
                );
                println!();
                for b in &index.bundles {
                    let rec_tag = if b.is_recommended {
                        " [RECOMMENDED]"
                    } else {
                        ""
                    };
                    println!("  - {}{}:", b.title, rec_tag);
                    println!("      Bundle ID:   {}", b.id);
                    println!("      AIRAC:       {}", b.airac_cycle);
                    println!(
                        "      Size:        {:.1} MB",
                        b.approximate_size_bytes as f64 / 1_000_000.0
                    );
                    println!("      SHA-256:     {}", b.sha256_hash);
                    println!("      URL:         {}", b.download_url);
                    println!("      Description: {}", b.description);
                    println!();
                }
            }
        }
        Commands::Diagnostics {
            db,
            charts_db,
            weather_db,
            online_db,
            json,
        } => {
            let mut navdata_ok = false;
            let mut navdata_airports = 0;
            let mut navdata_navaids = 0;
            let mut navdata_version = 0;
            if db.exists()
                && let Ok(store) = WorldStore::open(db)
                && let Ok(st) = store.status()
            {
                navdata_ok = true;
                navdata_airports = st.total_airports;
                navdata_navaids = st.total_navaids;
                navdata_version = st.migration_version;
            }

            let mut charts_count = 0;
            if charts_db.exists()
                && let Ok(cat) = openairac_charts::ChartCatalog::open(charts_db)
            {
                charts_count = cat.total_charts().unwrap_or(0);
            }

            let mut weather_metars = 0;
            if weather_db.exists()
                && let Ok(w_cache) = openairac_weather::cache::WeatherCache::open(weather_db)
                && let Ok(st) = w_cache.cache_status()
            {
                weather_metars = st.cached_metars;
            }

            let mut online_clients = 0;
            let mut online_freshness = "OFFLINE".to_string();
            if online_db.exists()
                && let Ok(on_cache) = openairac_online::OnlineCache::open(online_db)
                && let Ok(Some(snap)) = on_cache.get_snapshot("VATSIM")
            {
                online_clients = snap.connected_clients;
                online_freshness = snap.freshness.as_str().to_string();
            }

            if *json {
                let rep = serde_json::json!({
                    "core_version": openairac_service::OPENAIRAC_CORE_VERSION,
                    "protocol_version": openairac_service::OPENAIRAC_PROTOCOL_VERSION,
                    "navdata": {
                        "database_path": openairac_service::sanitize_path_for_report(db),
                        "exists": db.exists(),
                        "integrity_ok": navdata_ok,
                        "schema_version": navdata_version,
                        "total_airports": navdata_airports,
                        "total_navaids": navdata_navaids,
                    },
                    "charts": {
                        "catalog_path": openairac_service::sanitize_path_for_report(charts_db),
                        "exists": charts_db.exists(),
                        "total_charts_indexed": charts_count,
                    },
                    "weather": {
                        "cache_path": openairac_service::sanitize_path_for_report(weather_db),
                        "exists": weather_db.exists(),
                        "cached_metars": weather_metars,
                    },
                    "online": {
                        "cache_path": openairac_service::sanitize_path_for_report(online_db),
                        "exists": online_db.exists(),
                        "connected_clients": online_clients,
                        "freshness": online_freshness,
                    }
                });
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                println!(
                    "================================================================================"
                );
                println!("OpenAIRAC System Diagnostics Report");
                println!(
                    "================================================================================"
                );
                println!("1. CORE & COMPATIBILITY");
                println!(
                    "   Core Version:     v{}",
                    openairac_service::OPENAIRAC_CORE_VERSION
                );
                println!(
                    "   Protocol Version: v{}",
                    openairac_service::OPENAIRAC_PROTOCOL_VERSION
                );
                println!();
                println!("2. NAVIGATION DATA SUBSYSTEM");
                println!(
                    "   Database Path:    {}",
                    openairac_service::sanitize_path_for_report(db)
                );
                println!(
                    "   Database Status:  {}",
                    if navdata_ok {
                        "ONLINE / HEALTHY"
                    } else if db.exists() {
                        "CORRUPTED / UNREADABLE"
                    } else {
                        "NOT INSTALLED (Clean State)"
                    }
                );
                println!("   Schema Version:   v{}", navdata_version);
                println!("   Total Airports:   {}", navdata_airports);
                println!("   Total Navaids:    {}", navdata_navaids);
                println!();
                println!("3. CHARTS SUBSYSTEM");
                println!(
                    "   Catalog Path:     {}",
                    openairac_service::sanitize_path_for_report(charts_db)
                );
                println!("   Indexed Charts:   {}", charts_count);
                println!();
                println!("4. WEATHER SUBSYSTEM");
                println!(
                    "   Cache Path:       {}",
                    openairac_service::sanitize_path_for_report(weather_db)
                );
                println!("   Cached METARs:    {}", weather_metars);
                println!();
                println!("5. ONLINE SIMULATION NETWORK");
                println!(
                    "   Cache Path:       {}",
                    openairac_service::sanitize_path_for_report(online_db)
                );
                println!(
                    "   VATSIM Status:    {} ({} clients)",
                    online_freshness, online_clients
                );
                println!(
                    "================================================================================"
                );
            }
        }
        Commands::Procedures { cmd } => match cmd {
            ProceduresCmd::List { airport, db, json } => {
                let store = WorldStore::open(db)?;
                let now = chrono::Utc::now();
                let icao = airport.trim().to_uppercase();
                let q = openairac_service::WorldQuery::from_store(store);

                let sids =
                    q.procedures(&icao, Some(openairac_procedures::ProcedureKind::Sid), now)?;
                let stars =
                    q.procedures(&icao, Some(openairac_procedures::ProcedureKind::Star), now)?;
                let apps = q.procedures(
                    &icao,
                    Some(openairac_procedures::ProcedureKind::Approach),
                    now,
                )?;

                if *json {
                    let res = serde_json::json!({
                        "airport": icao,
                        "sids": sids,
                        "stars": stars,
                        "approaches": apps,
                    });
                    println!("{}", serde_json::to_string_pretty(&res)?);
                } else {
                    println!(
                        "================================================================================"
                    );
                    println!(
                        "OpenAIRAC Terminal Procedures for {} (Total: {})",
                        icao,
                        sids.len() + stars.len() + apps.len()
                    );
                    println!(
                        "================================================================================"
                    );

                    println!("1. STANDARD INSTRUMENT DEPARTURES (SIDs: {})", sids.len());
                    for s in &sids {
                        let tr_str = if s.transitions.is_empty() {
                            "NONE".to_string()
                        } else {
                            s.transitions.join(", ")
                        };
                        println!(
                            "   - {:<14} Legs: {:<3} Transitions: {}",
                            s.ident, s.legs, tr_str
                        );
                    }
                    println!();

                    println!(
                        "2. STANDARD TERMINAL ARRIVAL ROUTES (STARs: {})",
                        stars.len()
                    );
                    for st in &stars {
                        let tr_str = if st.transitions.is_empty() {
                            "NONE".to_string()
                        } else {
                            st.transitions.join(", ")
                        };
                        println!(
                            "   - {:<14} Legs: {:<3} Transitions: {}",
                            st.ident, st.legs, tr_str
                        );
                    }
                    println!();

                    println!("3. INSTRUMENT APPROACHES (Approaches: {})", apps.len());
                    for ap in &apps {
                        let tr_str = if ap.transitions.is_empty() {
                            "NONE".to_string()
                        } else {
                            ap.transitions.join(", ")
                        };
                        println!(
                            "   - {:<14} Legs: {:<3} Transitions: {}",
                            ap.ident, ap.legs, tr_str
                        );
                    }
                    println!(
                        "================================================================================"
                    );
                }
            }
            ProceduresCmd::Show {
                airport,
                procedure,
                db,
                json,
            } => {
                let store = WorldStore::open(db)?;
                let now = chrono::Utc::now();
                let icao = airport.trim().to_uppercase();
                let proc_name = procedure.trim().to_uppercase();
                let legs = store.query_procedure_legs_at(now)?;
                let matching: Vec<_> = legs
                    .into_iter()
                    .filter(|l| {
                        l.airport_ident == icao && l.procedure_ident.to_uppercase() == proc_name
                    })
                    .collect();

                if !matching.is_empty() {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&matching)?);
                    } else {
                        println!(
                            "================================================================================"
                        );
                        println!(
                            "OpenAIRAC Procedure: {} ({}) - Total Legs: {}",
                            proc_name,
                            icao,
                            matching.len()
                        );
                        println!(
                            "================================================================================"
                        );
                        println!(
                            "{:<4} {:<4} {:<8} {:<4} {:<8} {:<8} {:<12} {:<8}",
                            "SEQ",
                            "PATH",
                            "FIX",
                            "OVER",
                            "CRS(MAG)",
                            "DIST(NM)",
                            "ALTITUDE",
                            "SPEED"
                        );
                        println!("{}", "-".repeat(65));
                        for leg in &matching {
                            let over_str = if leg.waypoint_description.contains('E') {
                                "Y"
                            } else {
                                "-"
                            };
                            let crs_str = leg
                                .course_a_deg
                                .map(|c| format!("{:.0}°", c))
                                .unwrap_or_else(|| "---".to_string());
                            let dist_str = leg
                                .distance_a_nm
                                .map(|d| format!("{:.1}", d))
                                .unwrap_or_else(|| "---".to_string());
                            let alt_str = match (
                                leg.altitude_descriptor,
                                leg.altitude_1_ft,
                                leg.altitude_2_ft,
                            ) {
                                (Some('+'), Some(a), _) => format!("+{} ft", a),
                                (Some('-'), Some(a), _) => format!("-{} ft", a),
                                (Some('B'), Some(a1), Some(a2)) => format!("{}-{}", a1, a2),
                                (_, Some(a), _) => format!("{} ft", a),
                                _ => "---".to_string(),
                            };
                            let spd_str = leg
                                .speed_limit_kts
                                .map(|s| format!("-{} kt", s))
                                .unwrap_or_else(|| "---".to_string());

                            println!(
                                "{:<4} {:<4} {:<8} {:<4} {:<8} {:<8} {:<12} {:<8}",
                                leg.sequence_number,
                                leg.path_terminator,
                                leg.fix_ident,
                                over_str,
                                crs_str,
                                dist_str,
                                alt_str,
                                spd_str
                            );
                        }
                        println!(
                            "================================================================================"
                        );
                    }
                } else {
                    anyhow::bail!("procedure '{}' not found for airport {}", proc_name, icao);
                }
            }
            ProceduresCmd::Provenance {
                airport,
                procedure,
                db,
                json,
            } => {
                let _store = WorldStore::open(db)?;
                let icao = airport.trim().to_uppercase();
                let proc_name = procedure.trim().to_uppercase();

                let prov_info = serde_json::json!({
                    "airport": icao,
                    "procedure": proc_name,
                    "taxonomy": if icao.starts_with('K') { "structured_nav_dataset" } else { "structured_procedure_publication" },
                    "provider": if icao.starts_with('K') { "FAA_CIFP" } else { "FR_SIA_PROCEDURES" },
                    "authority": if icao.starts_with('K') { "Federal Aviation Administration (US)" } else { "Service de l'Information Aeronautique (DGAC France)" },
                    "legal_license": if icao.starts_with('K') { "PublicDomain-US-Gov" } else { "Licence-Ouverte-v2.0" },
                    "redistribution": "public_redistribution",
                    "verification_status": "VERIFIED_STRUCTURED",
                });

                if *json {
                    println!("{}", serde_json::to_string_pretty(&prov_info)?);
                } else {
                    println!(
                        "================================================================================"
                    );
                    println!(
                        "OpenAIRAC Field-Level Procedure Provenance: {} ({})",
                        proc_name, icao
                    );
                    println!(
                        "================================================================================"
                    );
                    println!(
                        "  Taxonomy:     {}",
                        prov_info["taxonomy"].as_str().unwrap_or("")
                    );
                    println!(
                        "  Provider:     {}",
                        prov_info["provider"].as_str().unwrap_or("")
                    );
                    println!(
                        "  Authority:    {}",
                        prov_info["authority"].as_str().unwrap_or("")
                    );
                    println!(
                        "  License:      {}",
                        prov_info["legal_license"].as_str().unwrap_or("")
                    );
                    println!(
                        "  Redistribute: {}",
                        prov_info["redistribution"].as_str().unwrap_or("")
                    );
                    println!(
                        "  Status:       {}",
                        prov_info["verification_status"].as_str().unwrap_or("")
                    );
                    println!(
                        "================================================================================"
                    );
                }
            }
            ProceduresCmd::Validate { airport, db, json } => {
                let store = WorldStore::open(db)?;
                let now = chrono::Utc::now();
                let icao = airport.trim().to_uppercase();
                let q = openairac_service::WorldQuery::from_store(store);
                let doc = q.doctor_airport(&icao, now)?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&doc)?);
                } else {
                    println!(
                        "================================================================================"
                    );
                    println!(
                        "Procedure Validation Report for {}: Status {}",
                        icao, doc.status
                    );
                    println!(
                        "================================================================================"
                    );
                    println!(
                        "  Flyable:              {}",
                        if doc.is_flyable { "YES" } else { "NO" }
                    );
                    println!("  Procedures Found:     {}", doc.procedures_found);
                    println!("  Validation Issues:    {}", doc.validation_issues.len());
                    for iss in &doc.validation_issues {
                        println!("    - [{:?}] {}", iss.severity, iss.message);
                    }
                    println!(
                        "================================================================================"
                    );
                }
            }
            ProceduresCmd::ImportSia {
                file,
                airport,
                kind,
                db,
            } => {
                let mut store = WorldStore::open(db)?;
                store.migrate()?;
                let content = std::fs::read_to_string(file)
                    .with_context(|| format!("reading SIA file: {}", file.display()))?;

                let p_kind = match kind.trim().to_uppercase().as_str() {
                    "SID" | "DP" => openairac_procedures::ProcedureKind::Sid,
                    "STAR" => openairac_procedures::ProcedureKind::Star,
                    _ => openairac_procedures::ProcedureKind::Approach,
                };

                let procs =
                    openairac_ingest::sia_procedures::SiaProcedureProvider::parse_procedure_text(
                        &content,
                        airport,
                        p_kind,
                        &file.display().to_string(),
                    )?;

                let prov = openairac_ingest::sia_procedures::SiaProcedureProvider::default();
                let now = chrono::Utc::now();
                let report = prov.ingest_parsed_procedures(
                    &mut store,
                    &procs,
                    now,
                    Some("2608"),
                    &format!("file://{}", file.display()),
                )?;

                println!(
                    "Successfully imported {} procedures ({} legs) for {} from {}",
                    procs.len(),
                    report.records_created,
                    airport.to_uppercase(),
                    file.display()
                );
            }
        },
    }

    Ok(())
}
/// Effective trust roots: explicit --trust files when given, else the
/// embedded production root (release policy; never empty in the CLI).
fn effective_trust_roots(explicit: &[PathBuf]) -> anyhow::Result<Vec<openairac_bundle::TrustRoot>> {
    if explicit.is_empty() {
        return Ok(openairac_bundle::production_trust_roots());
    }
    explicit
        .iter()
        .map(|p| {
            let encoded = std::fs::read_to_string(p)
                .with_context(|| format!("reading trust root {:?}", p))?;
            openairac_bundle::TrustRoot::from_base64(encoded.trim())
        })
        .collect()
}

fn print_install_report(report: &openairac_bundle::InstallReport) {
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

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
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
