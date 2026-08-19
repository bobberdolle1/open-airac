//! Real-data smoke: export a Little Navmap nav database.
//! Usage: cargo run -p openairac-export-lnm --example lnm_export -- <db> <out> [effective RFC3339]

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export::FormatExporter;
use openairac_export_lnm::LnmNavdataExporter;
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
        .unwrap_or_else(|| "/tmp/lnm_out".into());
    let effective: DateTime<Utc> = args
        .get(3)
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()?
        .unwrap_or_else(Utc::now);
    let store = WorldStore::open(&db)?;
    let set = LnmNavdataExporter.export(&store, effective, std::path::Path::new(&out))?;
    println!("family: {}", set.family.as_str());
    println!("cycle: {}", set.cycle);
    for a in &set.artifacts {
        println!("  {} ({} bytes)", a.path, a.size);
    }
    set.verify(std::path::Path::new(&out))?;
    println!("verification: PASS");
    let conn =
        rusqlite::Connection::open(std::path::Path::new(&out).join("little_navmap_openairac.db"))?;
    for (label, table) in [
        ("airports", "airport"),
        ("runways", "runway"),
        ("waypoints", "waypoint"),
        ("vor", "vor"),
        ("ndb", "ndb"),
        ("ils", "ils"),
        ("airways", "airway"),
    ] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        println!("{label}: {n}");
    }
    Ok(())
}
