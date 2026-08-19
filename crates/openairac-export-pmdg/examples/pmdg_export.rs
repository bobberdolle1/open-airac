//! Real-data smoke: export PMDG classic navigation files.
//! Usage: cargo run -p openairac-export-pmdg --example pmdg_export -- <db> <out> [effective RFC3339]

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export::FormatExporter;
use openairac_export_pmdg::PmdgNavdataExporter;
use openairac_store::WorldStore;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/oa_2609.sqlite".into());
    let out = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "/tmp/pmdg_out".into());
    let effective: DateTime<Utc> = args
        .get(3)
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()?
        .unwrap_or_else(Utc::now);

    let store = WorldStore::open(&db)?;
    let set = PmdgNavdataExporter.export(&store, effective, std::path::Path::new(&out))?;
    println!("family: {}", set.family.as_str());
    println!("cycle: {}", set.cycle);
    for a in &set.artifacts {
        println!("  {} ({} bytes, {})", a.path, a.size, a.kind);
    }
    set.verify(std::path::Path::new(&out))?;
    println!("verification: PASS");
    Ok(())
}
