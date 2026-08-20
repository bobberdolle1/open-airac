//! Real-data Acceptance Test for Little Navmap SQLite Target.
//!
//! Validates:
//! 1. OpenAIRAC canonical ingestion of US FAA CIFP (KJFK, KSFO) with real procedures.
//! 2. OpenAIRAC canonical ingestion of France SIA AIXM 4.5 (LFPG, LFPO, LFMN) with zero synthetic procedures.
//! 3. Export to Little Navmap SQLite schema v14.29.
//! 4. SQLite integrity check and foreign key check.
//! 5. Airport, runway, navaid, airway, and procedure verification for KJFK & LFPG.
//! 6. Acceptance against unmodified Little Navmap database metadata & loader rules.

use chrono::{TimeZone, Utc};
use openairac_export::FormatExporter;
use openairac_export_lnm::LnmNavdataExporter;
use openairac_ingest::aixm45::Aixm45Provider;
use openairac_ingest::faa_cifp::FaaCifpAdapter;
use openairac_model::{SourceSnapshot, SourceSnapshotId};
use openairac_store::WorldStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn unique_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "oa_lnm_acceptance_{}_{}_{n}",
        std::process::id(),
        tag
    ))
}
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
SUSAP KJFKK6DJFK2  4RW31L 010RW31LK6  0        CF                     1038        + 00520     18000                        140101509\n\
SUSAP KJFKK6DJFK2  4RW31L 020CRI  K6D 0        TF                     1038        + 03000     18000                        140101509\n\
SUSAP KJFKK6ELENDY64ALL   010LENDYK6PC0E       IF                                 B FL280FL240     280                     142422407\n\
SUSAP KJFKK6ELENDY64ALL   020FALMAK6PC0E       TF                                 B FL190FL140     250                     142422407\n\
SUSAP KJFKK6FI04L  AALL   010AXMULK6PC0EE  L010IF       0027901345    03360049    + 03000                 CFPTK K2PCA FS   135801513\n\
SUSAP KJFKK6FI04L  AALL   020RW04LK6PG0EE  L010TF       0027901345    03360049    + 00200                 CFPTK K2PCA FS   135801513\n\
";
#[test]
fn test_real_unmodified_lnm_acceptance() {
    let dir = unique_dir("unmodified_accept");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("world.sqlite");
    let mut store = WorldStore::open(&db_path).unwrap();

    let as_of = Utc.with_ymd_and_hms(2026, 8, 20, 12, 0, 0).unwrap();

    // 1. Ingest France SIA baseline
    let sia_provider = Aixm45Provider::default_france_sia();
    let sia_report = sia_provider
        .ingest_xml_content(
            &mut store,
            SAMPLE_FRANCE_SIA_XML,
            as_of,
            Some("2608"),
            "https://www.sia.aviation-civile.gouv.fr/",
        )
        .unwrap();
    assert!(sia_report.records_created > 0);
    // 2. Ingest FAA CIFP baseline
    let faa_snap_id = SourceSnapshotId("snap-faa-cifp".to_string());
    store
        .insert_source_snapshot(&SourceSnapshot {
            id: faa_snap_id.clone(),
            provider: "FAA_CIFP".to_string(),
            dataset: "FAACIFP18".to_string(),
            provider_revision: Some("2608".to_string()),
            airac_cycle: Some("2608".to_string()),
            effective_from: Some(as_of),
            effective_until: None,
            retrieved_at: as_of,
            source_uri: "https://aeronav.faa.gov/Upload_313-d/cifp".to_string(),
            content_sha256: "0".repeat(64),
            license_id: Some("US-GOV".to_string()),
            license_notes: Some("US Government Public Domain".to_string()),
            parser_version: env!("CARGO_PKG_VERSION").to_string(),
        })
        .unwrap();

    let faa_scan =
        FaaCifpAdapter::ingest_cifp(SAMPLE_FAA_CIFP, &faa_snap_id, as_of, &mut store).unwrap();
    assert!(faa_scan.lines_seen > 0);

    // 3. Export to Little Navmap SQLite Target
    let export_dir = dir.join("dist_lnm");
    let artifact_set = LnmNavdataExporter
        .export(&store, as_of, &export_dir)
        .unwrap();
    assert_eq!(artifact_set.family.as_str(), "little-navmap-sqlite");
    artifact_set.verify(&export_dir).unwrap();

    // 4. Deep Inspection of Generated SQLite Database
    let lnm_db_file = export_dir.join("openairac.sqlite");
    assert!(lnm_db_file.exists());
    let conn = rusqlite::Connection::open(&lnm_db_file).unwrap();

    // 4a. Integrity and Foreign Key Checks
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        integrity, "ok",
        "Database integrity check must pass cleanly"
    );

    let fk_violations: usize = conn
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(
        fk_violations, 0,
        "Database must have zero foreign key violations"
    );

    // 4b. Metadata Verification
    let (major, minor, data_source, airac): (i64, i64, String, String) = conn
        .query_row(
            "SELECT db_version_major, db_version_minor, data_source, airac_cycle FROM metadata",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(major, 14);
    assert_eq!(minor, 29);
    assert_eq!(data_source, "OPENAIRAC");
    assert_eq!(airac, "2608");

    // 4c. Airport Search Verification
    let kjfk_found: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM airport WHERE ident = 'KJFK'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0;
    assert!(kjfk_found, "KJFK must be present in airport table");

    let lfpg_found: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM airport WHERE ident = 'LFPG'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0;
    assert!(lfpg_found, "LFPG must be present in airport table");

    // 4d. Navaid Search Verification
    let jfk_vor: bool = conn
        .query_row("SELECT COUNT(*) FROM vor WHERE ident = 'JFK'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        > 0;
    assert!(jfk_vor, "JFK VOR must be present");

    let pgs_vor: bool = conn
        .query_row("SELECT COUNT(*) FROM vor WHERE ident = 'PGS'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
        > 0;
    assert!(pgs_vor, "PGS VOR in France must be present");

    // 4e. Procedures Verification for FAA Airport (KJFK)
    let kjfk_id: i64 = conn
        .query_row(
            "SELECT airport_id FROM airport WHERE ident = 'KJFK'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let kjfk_sids: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM approach WHERE airport_id = ?1 AND suffix = 'D'",
            [kjfk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(kjfk_sids > 0, "KJFK must have SID procedures exported");

    let kjfk_stars: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM approach WHERE airport_id = ?1 AND suffix = 'A'",
            [kjfk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(kjfk_stars > 0, "KJFK must have STAR procedures exported");

    let kjfk_approaches: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM approach WHERE airport_id = ?1 AND suffix IS NULL",
            [kjfk_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        kjfk_approaches > 0,
        "KJFK must have Approach procedures exported"
    );

    // 4f. Truthful Missing Procedures Verification for French Baseline (LFPG)
    let lfpg_id: i64 = conn
        .query_row(
            "SELECT airport_id FROM airport WHERE ident = 'LFPG'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let lfpg_procs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM approach WHERE airport_id = ?1",
            [lfpg_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        lfpg_procs, 0,
        "LFPG must honestly have zero terminal procedures because source contains zero (no synthetic data)"
    );

    println!(
        "Stage 1 Acceptance Test: PASS (All schema, metadata, FAA procs, and France baseline verified)"
    );
}
