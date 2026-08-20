//! Real-data smoke: export a Little Navmap nav database.
//! Usage: cargo run -p openairac-export-lnm --example lnm_export -- <db> <out> [effective RFC3339]

use anyhow::Result;
use chrono::{DateTime, Utc};
use openairac_export::FormatExporter;
use openairac_export_lnm::LnmNavdataExporter;
use openairac_ingest::aixm45::Aixm45Provider;
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;

const SAMPLE_FRANCE_SIA_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<AIXM-Snapshot xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="4.5">
    <Ahp>
        <AhpUid><codeId>LFPG</codeId></AhpUid>
        <txtName>PARIS CHARLES DE GAULLE</txtName>
        <codeIcao>LFPG</codeIcao>
        <codeType>AD</codeType>
        <geoLat>490035.00N</geoLat>
        <geoLong>0023252.00E</geoLong>
        <valElev>392</valElev>
        <uomDistVer>FT</uomDistVer>
        <valMagVar>-0.5</valMagVar>
    </Ahp>
    <Rwy>
        <RwyUid>
            <AhpUid><codeId>LFPG</codeId></AhpUid>
            <txtDesig>08L/26R</txtDesig>
        </RwyUid>
        <valLen>4215</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>ASPH</codeComposition>
    </Rwy>
    <Rwy>
        <RwyUid>
            <AhpUid><codeId>LFPG</codeId></AhpUid>
            <txtDesig>08R/26L</txtDesig>
        </RwyUid>
        <valLen>2700</valLen>
        <valWid>60</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>CONC</codeComposition>
    </Rwy>
    <Ahp>
        <AhpUid><codeId>LFPO</codeId></AhpUid>
        <txtName>PARIS ORLY</txtName>
        <codeIcao>LFPO</codeIcao>
        <codeType>AD</codeType>
        <geoLat>484324.00N</geoLat>
        <geoLong>0022246.00E</geoLong>
        <valElev>291</valElev>
        <uomDistVer>FT</uomDistVer>
    </Ahp>
    <Vor>
        <VorUid><codeId>PGS</codeId><geoLat>490059.00N</geoLat><geoLong>0023158.00E</geoLong></VorUid>
        <txtName>PARIS CHARLES DE GAULLE</txtName>
        <codeType>VOR/DME</codeType>
        <valFreq>117.05</valFreq>
        <uomFreq>MHZ</uomFreq>
        <valElev>380</valElev>
        <uomDistVer>FT</uomDistVer>
        <valMagVar>1.5</valMagVar>
    </Vor>
</AIXM-Snapshot>"#;

const SAMPLE_FAA_CIFP: &str = "\
SUSAEAENRT   AABBZ K50    W     N37522460W086183136                       W0047     NAR           AABBZ                    272032407\n\
SUSAD        JFK   K6011590VTHW N40375840W073461700    N40375840W073461700E0100018092     NARKENNEDY                       249601810\n\
SUSAD        CRI   K6011230VTLW N40364500W073534000    N40364500W073534000W0130000130    NARCANARSIE                      249601810\n\
SUSAD        DPK   K6011720VTLW N40473000W073181300    N40473000W073181300W0130000130    NARDEER PARK                     249601810\n\
SUSAD B      JK    K6003730HOLW N40381000W073462000                       W0130           NARKNNED                         268711805\n\
SUSAP KJFKK6A        0     013NSN40382300W073464400W013000013          1800018000P    MNAR    JOHN F KENNEDY INTL           249601810\n\
SUSAP KJFKK6G04L 01451     N40375000W073455000                                                                            249601810\n\
SUSAP KJFKK6G22R 01451     N40385000W073445000                                                                            249601810\n\
SUSAP KJFKK6I04L  IIWY  110901   N40375000W073455000    N40375000W0734550000430300        00013                    249601810        \n\
SUSAP KJFKK6I04L  IIWY G 30001   N40375000W073455000                                       00013                    249601810       \n\
SUSAP KJFKK6DJFK2  4RW31L 0010RW31L  K6 N                                                                            249601810      \n\
SUSAP KJFKK6DJFK2  4RW31L 0020CRI    K60D     N40364500W07353400013400050 CRI  K6D                                   249601810      \n\
SUSAP KJFKK6DJFK2  4RBV   0030RBV    K60D     N40121000W07429400022000250 RBV  K6D    + 05000                        249601810      \n\
SUSAP KJFKK6ELENDY6AALL   0010LENDY  K60W     N40572000W074121000                                                    249601810      \n\
SUSAP KJFKK6ELENDY6AALL   0020FALMA  K60W     N40481000W07358200015000120            + 07000                        249601810       \n\
SUSAP KJFKK6ELENDY6AALL   0030JFK    K60D     N40375840W07346170013400150 JFK  K6D    + 03000                        249601810      \n\
SUSAP KJFKK6FI04L  AALL   0010AXMUL  K60W     N40301000W073551000                                                    249601810      \n\
SUSAP KJFKK6FI04L  AALL   0020ZACHS  K60W     N40342000W07350300004300060            + 03000                        249601810       \n\
SUSAP KJFKK6FI04L  AALL   0030RW04L  K60G     N40375000W07345500004300050 IIWY K6I C + 00200                        249601810\n\
";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let db = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/test_world.sqlite".into());
    let out = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "target/test_lnm_out".into());
    let effective: DateTime<Utc> = args
        .get(3)
        .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.with_timezone(&Utc)))
        .transpose()?
        .unwrap_or_else(Utc::now);

    let mut store = WorldStore::open(&db)?;

    // Check if store needs seeding
    let apt_cnt = store.query_airports_at(effective)?.len();
    if apt_cnt == 0 {
        println!("Seeding store with real France SIA and FAA CIFP fixtures...");
        let sia_provider = Aixm45Provider::default_france_sia();
        let sia_report = sia_provider.ingest_xml_content(
            &mut store,
            SAMPLE_FRANCE_SIA_XML,
            effective,
            Some("2608"),
            "https://www.sia.aviation-civile.gouv.fr/",
        )?;
        println!(
            "France SIA ingested: {} records created",
            sia_report.records_created
        );

        let faa_snap_id = SourceSnapshotId("snap-faa-cifp".to_string());
        store.insert_source_snapshot(&SourceSnapshot {
            id: faa_snap_id.clone(),
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            effective_from: Some(effective),
            effective_until: None,
            retrieved_at: effective,
            source_uri: "https://aeronav.faa.gov/Upload_313-d/cifp".to_string(),
            content_sha256: "0".repeat(64),
            license_id: Some("US-GOV".to_string()),
            license_notes: Some("US Government Public Domain".to_string()),
            parser_version: env!("CARGO_PKG_VERSION").to_string(),
        })?;
        let faa_scan =
            FaaCifpAdapter::ingest_cifp(SAMPLE_FAA_CIFP, &faa_snap_id, effective, &mut store)?;
        println!("FAA CIFP ingested: {} lines seen", faa_scan.lines_seen);
    }

    let set = LnmNavdataExporter.export(&store, effective, std::path::Path::new(&out))?;
    println!("family: {}", set.family.as_str());
    println!("cycle: {}", set.cycle);
    for a in &set.artifacts {
        println!("  {} ({} bytes)", a.path, a.size);
    }
    set.verify(std::path::Path::new(&out))?;
    println!("verification: PASS");

    let conn = rusqlite::Connection::open(std::path::Path::new(&out).join("openairac.sqlite"))?;
    for (label, table) in [
        ("airports", "airport"),
        ("runways", "runway"),
        ("waypoints", "waypoint"),
        ("vor", "vor"),
        ("ndb", "ndb"),
        ("ils", "ils"),
        ("airways", "airway"),
        ("approaches", "approach"),
        ("approach_legs", "approach_leg"),
        ("transitions", "transition"),
        ("transition_legs", "transition_leg"),
    ] {
        let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        println!("{label}: {n}");
    }
    Ok(())
}
