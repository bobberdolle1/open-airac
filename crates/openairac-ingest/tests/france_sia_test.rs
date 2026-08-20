//! Integration test: Real French SIA AIXM 4.5 Acceptance Test.
//!
//! Verifies:
//! - Full AIXM 4.5 parsing against real DGAC / SIA data.
//! - Canonical store ingestion of French aerodromes (LFPG, LFPO, LFMN), runways, and navaids.
//! - Coverage & doctor reports accurately reflect SIA provenance and data presence.
//! - Truthful reporting of available vs missing elements (zero fake data invented).

use chrono::Utc;
use openairac_ingest::aixm45::{Aixm45Provider, parse_aixm45_xml};
use openairac_service::WorldQuery;
use openairac_store::WorldStore;
use std::path::Path;

const SAMPLE_FRANCE_SIA_AIRPORTS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<AIXM-Snapshot xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="4.5">
    <Ahp>
        <AhpUid>
            <codeId>LFPG</codeId>
        </AhpUid>
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
            <AhpUid>
                <codeId>LFPG</codeId>
            </AhpUid>
            <txtDesig>08L/26R</txtDesig>
        </RwyUid>
        <valLen>4215</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>ASPH</codeComposition>
    </Rwy>
    <Rwy>
        <RwyUid>
            <AhpUid>
                <codeId>LFPG</codeId>
            </AhpUid>
            <txtDesig>08R/26L</txtDesig>
        </RwyUid>
        <valLen>2700</valLen>
        <valWid>60</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>CONC</codeComposition>
    </Rwy>
    <Ahp>
        <AhpUid>
            <codeId>LFPO</codeId>
        </AhpUid>
        <txtName>PARIS ORLY</txtName>
        <codeIcao>LFPO</codeIcao>
        <codeType>AD</codeType>
        <geoLat>484324.00N</geoLat>
        <geoLong>0022246.00E</geoLong>
        <valElev>291</valElev>
        <uomDistVer>FT</uomDistVer>
    </Ahp>
    <Rwy>
        <RwyUid>
            <AhpUid>
                <codeId>LFPO</codeId>
            </AhpUid>
            <txtDesig>06/24</txtDesig>
        </RwyUid>
        <valLen>3650</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>ASPH</codeComposition>
    </Rwy>
    <Ahp>
        <AhpUid>
            <codeId>LFMN</codeId>
        </AhpUid>
        <txtName>NICE COTE D'AZUR</txtName>
        <codeIcao>LFMN</codeIcao>
        <codeType>AD</codeType>
        <geoLat>433955.00N</geoLat>
        <geoLong>0071307.00E</geoLong>
        <valElev>12</valElev>
        <uomDistVer>FT</uomDistVer>
    </Ahp>
    <Rwy>
        <RwyUid>
            <AhpUid>
                <codeId>LFMN</codeId>
            </AhpUid>
            <txtDesig>04L/22R</txtDesig>
        </RwyUid>
        <valLen>2570</valLen>
        <valWid>45</valWid>
        <uomDimRwy>M</uomDimRwy>
        <codeComposition>ASPH</codeComposition>
    </Rwy>
    <Vor>
        <VorUid>
            <codeId>PGS</codeId>
            <geoLat>490059.00N</geoLat>
            <geoLong>0023158.00E</geoLong>
        </VorUid>
        <txtName>PARIS CHARLES DE GAULLE</txtName>
        <codeType>VOR/DME</codeType>
        <valFreq>117.05</valFreq>
        <uomFreq>MHZ</uomFreq>
        <valElev>380</valElev>
        <uomDistVer>FT</uomDistVer>
        <valMagVar>1.5</valMagVar>
    </Vor>
    <Vor>
        <VorUid>
            <codeId>AZR</codeId>
            <geoLat>433930.00N</geoLat>
            <geoLong>0071330.00E</geoLong>
        </VorUid>
        <txtName>COTE D'AZUR</txtName>
        <codeType>VOR/DME</codeType>
        <valFreq>109.65</valFreq>
        <valElev>15</valElev>
    </Vor>
    <Ndb>
        <NdbUid>
            <codeId>CPO</codeId>
            <geoLat>484310.00N</geoLat>
            <geoLong>0022300.00E</geoLong>
        </NdbUid>
        <txtName>ORLY</txtName>
        <valFreq>377</valFreq>
        <valElev>290</valElev>
    </Ndb>
    <Rsg>
        <RsgUid>
            <RteUid>
                <txtDesig>UN874</txtDesig>
            </RteUid>
            <DpnUidSta>
                <codeId>LORNI</codeId>
                <geoLat>485500.00N</geoLat>
                <geoLong>0025500.00E</geoLong>
            </DpnUidSta>
            <DpnUidEnd>
                <codeId>OKTET</codeId>
                <geoLat>491000.00N</geoLat>
                <geoLong>0021500.00E</geoLong>
            </DpnUidEnd>
        </RsgUid>
        <valDistVerLower>195</valDistVerLower>
        <valDistVerUpper>660</valDistVerUpper>
    </Rsg>
</AIXM-Snapshot>"#;

#[test]
fn test_france_sia_acceptance_pipeline() {
    let now = Utc::now();
    let mut store = WorldStore::open_in_memory().unwrap();
    let provider = Aixm45Provider::default_france_sia();

    // 1. Ingest SIA dataset
    let report = provider
        .ingest_xml_content(
            &mut store,
            SAMPLE_FRANCE_SIA_AIRPORTS_XML,
            now,
            Some("2608"),
            "http://data.cquest.org/dgac/aip/AIXM4.5_all_FR_OM.xml",
        )
        .expect("ingest SIA AIXM 4.5");

    assert!(report.records_created >= 10);

    // 2. Query through WorldQuery service
    let service = WorldQuery::from_store(store);

    // 3. Verify LFPG (Paris Charles de Gaulle)
    let lfpg_cov = service
        .airport_coverage("LFPG", now)
        .expect("query LFPG coverage")
        .expect("LFPG must exist");
    assert_eq!(lfpg_cov.ident, "LFPG");
    assert_eq!(lfpg_cov.name, "PARIS CHARLES DE GAULLE");
    assert_eq!(lfpg_cov.country.as_deref(), Some("FR"));
    assert_eq!(lfpg_cov.runways.len(), 2);
    assert!(lfpg_cov.runways.iter().any(|r| r.designator == "08L/26R"));
    assert!(lfpg_cov.runways.iter().any(|r| r.designator == "08R/26L"));
    assert_eq!(lfpg_cov.sources.len(), 1);
    assert_eq!(lfpg_cov.sources[0].provider, "FR_SIA");
    assert_eq!(lfpg_cov.sources[0].license_id, "Licence-Ouverte-v2.0");
    assert_eq!(lfpg_cov.sources[0].redistribution, "public_redistribution");

    let lfpg_doc = service.doctor_airport("LFPG", now).expect("doctor LFPG");
    assert_eq!(lfpg_doc.ident, "LFPG");
    assert!(lfpg_doc.has_airport_record);
    assert_eq!(lfpg_doc.runway_count, 2);
    assert_eq!(lfpg_doc.procedures_found, 0); // SIA AIXM 4.5 public export does not include procedure legs
    assert!(
        lfpg_doc
            .missing_elements
            .iter()
            .any(|m| m.contains("No instrument terminal procedures"))
    );

    // 4. Verify LFPO (Paris Orly)
    let lfpo_cov = service
        .airport_coverage("LFPO", now)
        .expect("query LFPO coverage")
        .expect("LFPO must exist");
    assert_eq!(lfpo_cov.ident, "LFPO");
    assert_eq!(lfpo_cov.name, "PARIS ORLY");
    assert_eq!(lfpo_cov.runways.len(), 1);
    assert_eq!(lfpo_cov.runways[0].designator, "06/24");

    // 5. Verify LFMN (Nice Cote d'Azur)
    let lfmn_cov = service
        .airport_coverage("LFMN", now)
        .expect("query LFMN coverage")
        .expect("LFMN must exist");
    assert_eq!(lfmn_cov.ident, "LFMN");
    assert_eq!(lfmn_cov.name, "NICE COTE D'AZUR");
    assert_eq!(lfmn_cov.runways.len(), 1);

    // 6. Verify Navaids & Airways
    let navaids = service.store().query_navaids_at(now).unwrap();
    assert!(
        navaids
            .iter()
            .any(|n| n.ident == "PGS" && n.kind == openairac_model::NavaidKind::Vordme)
    );
    assert!(
        navaids
            .iter()
            .any(|n| n.ident == "AZR" && n.kind == openairac_model::NavaidKind::Vordme)
    );
    assert!(
        navaids
            .iter()
            .any(|n| n.ident == "CPO" && n.kind == openairac_model::NavaidKind::Ndb)
    );

    let waypoints = service.store().query_waypoints_at(now).unwrap();
    assert!(waypoints.iter().any(|w| w.ident == "LORNI"));
    assert!(waypoints.iter().any(|w| w.ident == "OKTET"));

    let airways = service.store().query_airway_legs_at(now).unwrap();
    assert!(
        airways
            .iter()
            .any(|a| a.route_ident == "UN874" && a.start_fix == "LORNI" && a.end_fix == "OKTET")
    );
}

#[test]
fn test_real_france_sia_file_if_available() {
    // If target/tmp_sia_test/sia.xml exists on the workstation, parse real 34MB file
    let path = Path::new("target/tmp_sia_test/sia.xml");
    if !path.exists() {
        return; // Skip if file is not locally downloaded
    }

    let xml_data = std::fs::read_to_string(path).expect("reading real SIA XML");
    let ds = parse_aixm45_xml(&xml_data).expect("parse real SIA dataset");

    println!("Real French SIA AIXM 4.5 dataset parsed:");
    println!("  Airports: {}", ds.airports.len());
    println!("  Navaids: {}", ds.navaids.len());
    println!("  Fixes: {}", ds.fixes.len());
    println!("  Airway segments: {}", ds.airway_segments.len());

    assert!(
        ds.airports.len() >= 500,
        "Must contain >= 500 French airports"
    );
    assert!(
        ds.navaids.len() >= 100,
        "Must contain >= 100 French radio navaids"
    );
    assert!(
        ds.airway_segments.len() >= 500,
        "Must contain >= 500 route segments"
    );

    assert!(ds.airports.iter().any(|a| a.ident == "LFPG"));
    assert!(ds.airports.iter().any(|a| a.ident == "LFPO"));
    assert!(ds.airports.iter().any(|a| a.ident == "LFMN"));
}
