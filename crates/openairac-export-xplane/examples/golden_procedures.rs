//! Procedure golden harness: compare the OpenAIRAC decoder's procedure
//! leg chains (fix sequences per procedure/transition) against the
//! Laminar reference converter (convert424toxplane) per-airport
//! procedure files, for a set of complex airports.
//!
//! Manual real-data tool; requires the converter binary and a CIFP
//! master file; never runs in CI.
//!
//! Usage:
//!   cargo run -p openairac-export-xplane --example golden_procedures -- \
//!     <cifp-file> <convert424toxplane.exe> <workdir> [airports...]
//!
//! Default airports: KSFO KDEN KJFK KLAX KORD.
//!
//! Comparison is (kind, procedure, transition) -> ordered leg chain:
//! sequence number, fix ident, path terminator. Only first-order
//! structure compares; leg geometry fields are decoder-specific.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LegKey {
    kind: String,
    procedure: String,
    transition: String,
    seq: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LegChain {
    fix: String,
    terminator: String,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        bail!(
            "usage: golden_procedures <cifp-file> <convert424toxplane.exe> <workdir> [airports...]"
        );
    }
    let cifp = PathBuf::from(&args[1]);
    let converter = PathBuf::from(&args[2]);
    let workdir = PathBuf::from(&args[3]);
    let airports: Vec<String> = if args.len() > 4 {
        args[4..].to_vec()
    } else {
        ["KSFO", "KDEN", "KJFK", "KLAX", "KORD"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    let effective: DateTime<Utc> = Utc
        .with_ymd_and_hms(2026, 8, 6, 9, 0, 0)
        .single()
        .expect("valid date");

    // 1. Converter run.
    std::fs::create_dir_all(&workdir)?;
    let converter_out = workdir.join("converter");
    std::fs::create_dir_all(&converter_out)?;
    let geoids_src = converter.parent().unwrap_or(Path::new(".")).join("geoids");
    if geoids_src.is_dir() && !converter_out.join("geoids").exists() {
        copy_dir(&geoids_src, &converter_out.join("geoids"))?;
    }
    if !converter_out.join("CIFP").exists() {
        println!("running converter...");
        let status = std::process::Command::new(&converter)
            .arg(&cifp)
            .arg("OpenAIRAC")
            .current_dir(&converter_out)
            .status()
            .context("running convert424toxplane")?;
        if !status.success() {
            bail!("convert424toxplane exited with {status}");
        }
    }

    // 2. Same CIFP through the OpenAIRAC decoder.
    let mut store = WorldStore::open_in_memory()?;
    let snapshot_id = SourceSnapshotId("faa_cifp:procs".to_string());
    store.insert_source_snapshot(&SourceSnapshot {
        id: snapshot_id.clone(),
        provider: "FAA_CIFP".to_string(),
        dataset: "FAACIFP18".to_string(),
        provider_revision: Some("2608".to_string()),
        airac_cycle: Some("2608".to_string()),
        effective_from: Some(effective),
        effective_until: None,
        retrieved_at: Utc::now(),
        source_uri: format!("file:{}", cifp.display()),
        content_sha256: "0".repeat(64),
        license_id: Some("US-GOV".to_string()),
        license_notes: Some("US Government work (public domain)".to_string()),
        parser_version: env!("CARGO_PKG_VERSION").to_string(),
    })?;
    store.insert_cycle(&AiracCycle {
        id: CycleId("2608".to_string()),
        effective_from: Some(effective),
        effective_until: None,
        status: CycleStatus::Active,
        source_uri: Some(format!("file:{}", cifp.display())),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        notes: Some("golden procedures harness".to_string()),
    })?;
    let content = std::fs::read_to_string(&cifp)?;
    let scan = FaaCifpAdapter::ingest_cifp(&content, &snapshot_id, effective, &mut store)?;
    println!(
        "ingested: {} procedure legs ({} unsupported, {} errors)",
        scan.procedure_legs_decoded, scan.unsupported_records, scan.decode_errors
    );

    // 3. Per-airport comparison.
    for airport in &airports {
        let path = converter_out.join("CIFP").join(format!("{airport}.dat"));
        if !path.exists() {
            println!("== {airport}: no converter file; skipping ==");
            continue;
        }
        compare_airport(&store, &path, airport, effective)?;
    }
    Ok(())
}

fn compare_airport(
    store: &WorldStore,
    converter_file: &Path,
    airport: &str,
    effective: DateTime<Utc>,
) -> Result<()> {
    let conv = parse_converter(converter_file)?;
    let ours = parse_ours(store, airport, effective)?;
    let keys: BTreeSet<LegKey> = conv.keys().chain(ours.keys()).cloned().collect();
    // Group by (kind, procedure, transition) for chain comparison.
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
    struct ChainKey {
        kind: String,
        procedure: String,
        transition: String,
    }
    let mut chains: BTreeMap<ChainKey, (Vec<LegChain>, Vec<LegChain>)> = BTreeMap::new();
    for key in keys {
        let ck = ChainKey {
            kind: key.kind.clone(),
            procedure: key.procedure.clone(),
            transition: key.transition.clone(),
        };
        let entry = chains.entry(ck).or_default();
        if let Some(c) = conv.get(&key) {
            entry.0.push(c.clone());
        }
        if let Some(o) = ours.get(&key) {
            entry.1.push(o.clone());
        }
    }
    let mut identical = 0usize;
    let mut differing = 0usize;
    let mut only_conv = 0usize;
    let mut only_ours = 0usize;
    let mut samples: Vec<String> = Vec::new();
    for (ck, (c, o)) in &chains {
        if c.is_empty() {
            only_ours += 1;
        } else if o.is_empty() {
            only_conv += 1;
        } else if c == o {
            identical += 1;
        } else {
            differing += 1;
            if samples.len() < 6 {
                samples.push(format!(
                    "  {}/{} trans '{}': conv {:?} ours {:?}",
                    ck.kind,
                    ck.procedure,
                    ck.transition,
                    c.iter()
                        .map(|l| (l.fix.clone(), l.terminator.clone()))
                        .collect::<Vec<_>>(),
                    o.iter()
                        .map(|l| (l.fix.clone(), l.terminator.clone()))
                        .collect::<Vec<_>>(),
                ));
            }
        }
    }
    println!("== {airport} ==");
    println!(
        "  chains: identical {identical} / differing {differing} / only-converter {only_conv} / only-ours {only_ours}"
    );
    for s in samples {
        println!("{s}");
    }
    Ok(())
}

/// Converter CIFP/<airport>.dat rows -> keyed legs.
fn parse_converter(path: &Path) -> Result<BTreeMap<LegKey, LegChain>> {
    let mut out = BTreeMap::new();
    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        let fields: Vec<&str> = line.split(',').collect();
        let Some((kind, seq)) = fields.first().and_then(|f| f.split_once(':')) else {
            continue;
        };
        let kind = match kind {
            "SID" => "D",
            "STAR" => "E",
            "APPCH" => "F",
            _ => continue,
        };
        let seq: u32 = seq.trim().parse().unwrap_or(0);
        if seq == 0 || fields.len() < 10 {
            continue;
        }
        let procedure = fields[2].trim().to_string();
        let transition = fields[3].trim().to_string();
        let fix = fields[4].trim().to_string();
        let terminator = fields[11].trim().to_string();
        if fix.is_empty() {
            continue;
        }
        let key = LegKey {
            kind: kind.to_string(),
            procedure: procedure.clone(),
            transition: transition.clone(),
            seq,
        };
        out.insert(key, LegChain { fix, terminator });
    }
    Ok(out)
}

/// Our decoded procedure legs -> keyed legs (same key space).
fn parse_ours(
    store: &WorldStore,
    airport: &str,
    effective: DateTime<Utc>,
) -> Result<BTreeMap<LegKey, LegChain>> {
    let mut out = BTreeMap::new();
    let legs = store.query_procedure_legs_at(effective)?;
    for leg in legs {
        if leg.airport_ident != airport {
            continue;
        }
        // Constraint-only legs (VA/VI without a fix) carry no fix
        // and compare structurally on both sides by their absence.
        if leg.fix_ident.is_empty() {
            continue;
        }
        let key = LegKey {
            kind: leg.procedure_kind.to_string(),
            procedure: leg.procedure_ident.clone(),
            transition: leg.transition_ident.clone(),
            seq: leg.sequence_number,
        };
        out.insert(
            key,
            LegChain {
                fix: leg.fix_ident.clone(),
                terminator: leg.path_terminator.clone(),
            },
        );
    }
    Ok(out)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
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
