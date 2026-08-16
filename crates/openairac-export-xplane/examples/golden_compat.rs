//! Golden compatibility harness: run the Laminar reference converter
//! (convert424toxplane) on a CIFP file and compare its
//! earth_fix/earth_nav/earth_awy output against the OpenAIRAC exporter
//! fed from the SAME CIFP via the canonical store.
//!
//! This is a manual, real-data cross-check — it requires the converter
//! binary and a CIFP master file; it never runs in CI.
//!
//! Usage:
//!   cargo run -p openairac-export-xplane --example golden_compat -- \
//!     <cifp-file> <convert424toxplane.exe> <workdir> [effective RFC3339]
//!
//! The converter must find `geoids/` in its working directory; the
//! harness copies it from next to the converter executable when present.
//!
//! Comparison is identifier-keyed (first whitespace token of each
//! record), because the two exporters target different format versions
//! (converter: XP11 layouts; OpenAIRAC: XP12 1200/1101 headers) and a
//! line-level diff would be noise. For every common identifier:
//!   - identical line  -> counted as identical
//!   - different line  -> counted as differing (coordinate deltas
//!     sampled for the first N records)
//!   - identifier in only one output -> counted and sampled

use anyhow::{Context, Result, bail};
use chrono::{DateTime, TimeZone, Utc};
use openairac_export_xplane::XPlane12Exporter;
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{AiracCycle, CycleId, CycleStatus, SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        bail!("usage: golden_compat <cifp-file> <convert424toxplane.exe> <workdir> [effective]");
    }
    let cifp = PathBuf::from(&args[1]);
    let converter = PathBuf::from(&args[2]);
    let workdir = PathBuf::from(&args[3]);
    let effective: DateTime<Utc> = args
        .get(4)
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()?
        .unwrap_or_else(|| {
            Utc.with_ymd_and_hms(2026, 8, 6, 9, 0, 0)
                .single()
                .expect("valid date")
        });

    std::fs::create_dir_all(&workdir)?;
    let converter_out = workdir.join("converter");
    std::fs::create_dir_all(&converter_out)?;
    let geoids_src = converter.parent().unwrap_or(Path::new(".")).join("geoids");
    if geoids_src.is_dir() && !converter_out.join("geoids").exists() {
        copy_dir(&geoids_src, &converter_out.join("geoids"))?;
        println!("copied geoids/ into converter workdir");
    }

    // 1. Reference converter run.
    println!("running converter (this can take a minute)...");
    let status = std::process::Command::new(&converter)
        .arg(&cifp)
        .arg("OpenAIRAC")
        .current_dir(&converter_out)
        .status()
        .context("running convert424toxplane")?;
    if !status.success() {
        bail!("convert424toxplane exited with {status}");
    }

    // 2. Same CIFP through the OpenAIRAC pipeline.
    let mut store = WorldStore::open_in_memory()?;
    let snapshot_id = SourceSnapshotId("faa_cifp:golden".to_string());
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
        notes: Some("golden compat harness".to_string()),
    })?;
    let content = std::fs::read_to_string(&cifp)?;
    let scan = FaaCifpAdapter::ingest_cifp(&content, &snapshot_id, effective, &mut store)?;
    println!(
        "ingested: {} waypoints, {} navaids, {} airway legs ({} unsupported, {} errors)",
        scan.waypoints_decoded,
        scan.navaids_decoded,
        scan.airway_legs_decoded,
        scan.unsupported_records,
        scan.decode_errors
    );

    let ours_out = workdir.join("ours");
    let report = XPlane12Exporter::export_from_db(&store, effective, &ours_out, true)?;
    println!(
        "exported: {} fixes, {} navaids, {} airway rows",
        report.fixes_written, report.navaids_written, report.airway_rows_written
    );

    // 3. Keyed diff per file. Each file type keys on its identifier
    // field and compares values (not column padding).
    println!();
    diff_fix(
        &converter_out.join("earth_fix.dat"),
        &ours_out.join("earth_fix.dat"),
    )?;
    diff_nav(
        &converter_out.join("earth_nav.dat"),
        &ours_out.join("earth_nav.dat"),
    )?;
    diff_awy(
        &converter_out.join("earth_awy.dat"),
        &ours_out.join("earth_awy.dat"),
    )?;
    Ok(())
}

fn diff_fix(converter: &Path, ours: &Path) -> Result<()> {
    // XPFIX rows: `lat lon ident ...` — key = ident (tokens[2]).
    // Values are compared whitespace-normalized (the two exporters pad
    // columns differently; a value difference is a real divergence).
    let conv = keyed_rows(converter, |t| t.get(2).map(|s| s.to_string()))?;
    let our = keyed_rows(ours, |t| t.get(2).map(|s| s.to_string()))?;
    let keys: BTreeSet<&str> = conv.keys().chain(our.keys()).map(|k| k.as_str()).collect();
    let mut identical = 0usize;
    let mut differing = 0usize;
    let mut samples = Vec::new();
    for key in &keys {
        let (Some(c), Some(o)) = (conv.get(*key), our.get(*key)) else {
            continue;
        };
        let nc: Vec<&str> = c.split_whitespace().collect();
        let no: Vec<&str> = o.split_whitespace().collect();
        if nc == no {
            identical += 1;
        } else {
            differing += 1;
            if samples.len() < 8 {
                samples.push(format!(
                    "  {key}: conv {:?} vs ours {:?}",
                    &nc[..6],
                    &no[..6]
                ));
            }
        }
    }
    let only_conv = conv.keys().filter(|k| !our.contains_key(*k)).count();
    let only_ours = our.keys().filter(|k| !conv.contains_key(*k)).count();
    println!("== earth_fix.dat ==");
    println!(
        "  converter {} / ours {} / common value-identical {} / value-differing {}",
        conv.len(),
        our.len(),
        identical,
        differing
    );
    println!("  only in converter: {only_conv} / only in ours: {only_ours}");
    for s in samples {
        println!("{s}");
    }
    println!("  (ours deliberately skips rows missing required fields; converter defaults them)");
    Ok(())
}

fn diff_nav(converter: &Path, ours: &Path) -> Result<()> {
    // XPNAV1200 rows: ident = tokens[7]; several rows can share an
    // ident (ILS localizer + DME pairs), so key by ident + row code.
    let conv = keyed_rows(converter, |t| t.get(7).map(|s| format!("{}#{}", s, t[0])))?;
    let our = keyed_rows(ours, |t| t.get(7).map(|s| format!("{}#{}", s, t[0])))?;
    let keys: BTreeSet<&str> = conv.keys().chain(our.keys()).map(|k| k.as_str()).collect();
    // The first 8 tokens are the data fields (row code, lat, lon,
    // elevation, frequency, class, magvar, ident); the trailing fields
    // are region/name where the converter applies cosmetic rewrites
    // (e.g. stripping a redundant "NAR" name prefix). Compare data
    // fields for value equality and report name diffs separately.
    let mut identical = 0usize;
    let mut name_only = 0usize;
    let mut row_code_only = 0usize;
    let mut differing = 0usize;
    let mut samples = Vec::new();
    for key in &keys {
        let (Some(c), Some(o)) = (conv.get(*key), our.get(*key)) else {
            continue;
        };
        let nc: Vec<&str> = c.split_whitespace().collect();
        let no: Vec<&str> = o.split_whitespace().collect();
        if nc == no {
            identical += 1;
        } else if nc.len() >= 8 && no.len() >= 8 && nc[..8] == no[..8] {
            name_only += 1;
        } else if nc.len() >= 8 && no.len() >= 8 {
            // -0.000 vs 0.000 is a formatting artifact, and row-code
            // differences (12 vs 13 for ILS DME components) are a
            // known, deliberate spec-vs-converter divergence.
            let norm = |v: &str| -> String {
                match v.parse::<f64>() {
                    Ok(x) if x.abs() < 1e-12 => "0".to_string(),
                    Ok(x) => x.to_string(),
                    Err(_) => v.to_string(),
                }
            };
            let data_eq = nc[1..8]
                .iter()
                .zip(no[1..8].iter())
                .all(|(a, b)| norm(a) == norm(b));
            if data_eq && nc[0] != no[0] {
                row_code_only += 1;
            } else if data_eq {
                // e.g. -0.000 vs 0.000 formatting.
                name_only += 1;
            } else {
                differing += 1;
                if samples.len() < 8 {
                    samples.push(format!(
                        "  {key}: conv {:?} vs ours {:?}",
                        &nc[..8],
                        &no[..8]
                    ));
                }
            }
        }
    }
    let only_conv = conv.keys().filter(|k| !our.contains_key(*k)).count();
    let only_ours = our.keys().filter(|k| !conv.contains_key(*k)).count();
    println!("== earth_nav.dat ==");
    println!(
        "  converter {} / ours {} / data value-identical {} / name-only {} / row-code-only {} / data-differing {}",
        conv.len(),
        our.len(),
        identical,
        name_only,
        row_code_only,
        differing
    );
    println!("  only in converter: {only_conv} / only in ours: {only_ours}");
    for s in samples {
        println!("{s}");
    }
    println!(
        "  (converter rewrites names cosmetically — e.g. stripping a redundant 'NAR' \
         prefix — and defaults unknown fields; ours writes source values verbatim)"
    );
    println!(
        "  (remaining data diffs: class for standalone DME facilities with \
         'U' = undetermined class — converter writes 150 for some, 125 for \
         others with no published discriminator; ours is a documented \
         deterministic default of 125)"
    );
    Ok(())
}

fn diff_awy(converter: &Path, ours: &Path) -> Result<()> {
    // XPAWY rows: converter consolidates contiguous legs into chain
    // rows (multiple airway ids per row), so whole-line comparison is
    // noise. Compare first-segment pairs: ident -> (next, level, base,
    // airway id).
    let pairs = |path: &Path| -> Result<BTreeMap<String, Vec<String>>> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let content = std::fs::read_to_string(path)?;
        for line in content.lines() {
            if line.contains("Version") {
                continue;
            }
            let t: Vec<&str> = line.split_whitespace().collect();
            if t.len() < 10 {
                continue;
            }
            // XPAWY1101: ident, region, type, next-ident, region,
            // type, direction, level, base, top, name. Unordered pair
            // key: orientation differs between the two exporters
            // (converter follows the EA chain direction, we follow the
            // ER sequence), and N-direction segments are semantically
            // undirected anyway.
            let (a, b) = if t[0] <= t[3] {
                (t[0], t[3])
            } else {
                (t[3], t[0])
            };
            let key = format!("{a}>{b}");
            map.entry(key)
                .or_default()
                .push(format!("{}/{}/{}", t[7], t[8], t[9]));
        }
        Ok(map)
    };
    let conv = pairs(converter)?;
    let our = pairs(ours)?;
    let keys: BTreeSet<&str> = conv.keys().chain(our.keys()).map(|k| k.as_str()).collect();
    let mut both = 0usize;
    let mut differing = 0usize;
    let mut samples = Vec::new();
    for key in &keys {
        let (Some(c), Some(o)) = (conv.get(*key), our.get(*key)) else {
            continue;
        };
        both += 1;
        // Same pair may appear on multiple airways (or a chain row);
        // compare as sets of (level/base/airway) descriptors.
        let cs: BTreeSet<&String> = c.iter().collect();
        let os: BTreeSet<&String> = o.iter().collect();
        if cs != os {
            differing += 1;
            if samples.len() < 8 {
                samples.push(format!("  {key}: conv {:?} ours {:?}", cs, os));
            }
        }
    }
    let only_conv = conv.keys().filter(|k| !our.contains_key(*k)).count();
    let only_ours = our.keys().filter(|k| !conv.contains_key(*k)).count();
    println!("== earth_awy.dat ==");
    println!(
        "  first-segment pairs: converter {} / ours {} / common {} (identical {} / differing {})",
        conv.len(),
        our.len(),
        both,
        both.saturating_sub(differing),
        differing
    );
    println!("  only in converter: {only_conv} / only in ours: {only_ours}");
    for s in samples {
        println!("{s}");
    }
    println!(
        "  (converter consolidates contiguous legs into chain rows; only first segments compare)"
    );
    Ok(())
}

/// Token-keyed rows: `key(&tokens)` -> Some(key) maps the line; None
/// skips it (headers). Last line wins per key.
fn keyed_rows<F>(path: &Path, key: F) -> Result<BTreeMap<String, String>>
where
    F: Fn(&[&str]) -> Option<String>,
{
    let mut rows = BTreeMap::new();
    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        if line.contains("Version") {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        if let Some(k) = key(&tokens) {
            rows.insert(k, line.to_string());
        }
    }
    Ok(rows)
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
