//! Comprehensive Integration & Acceptance Tests for French SIA Procedure Table Ingestion.

use chrono::{TimeZone, Utc};
use openairac_ingest::sia_procedures::SiaProcedureProvider;
use openairac_procedures::validation::ProcedureValidator;
use openairac_procedures::{Procedure, ProcedureKind};
use openairac_store::WorldStore;

const SAMPLE_LFPG_DATA_SID_TEXT: &str = r#"
# Official DGAC / SIA France eAIP Section AD 2.24
# AD 2 LFPG DATA SID RNAV RWY 26L 26R 27L 27R
# Identification: OPALE 5A, ATREX 5A, NURMO 5A, MOPAR 5A, LGL 5A

PROCEDURE: OPALE 5A | RWY: 26L | NAV: RNAV 1
10 | CF | RW26L | Y | 266 | 2.5 | - | - | - | RNAV 1
20 | TF | PG261 | N | 266 | 4.8 | R | MNM 3000 | 250 | RNAV 1
30 | TF | PG262 | N | 340 | 6.2 | R | MNM 5000 / MAX 10000 | 250 | RNAV 1
40 | TF | OPALE | N | 035 | 14.5 | - | MNM FL100 | - | RNAV 1

PROCEDURE: ATREX 5A | RWY: 26L | NAV: RNAV 1
10 | CF | RW26L | Y | 266 | 2.5 | - | - | - | RNAV 1
20 | TF | PG261 | N | 266 | 4.8 | L | MNM 3000 | 250 | RNAV 1
30 | TF | ATREX | N | 145 | 18.2 | - | MNM 7000 | - | RNAV 1

PROCEDURE: NURMO 5A | RWY: 26L | NAV: RNAV 1
10 | CF | RW26L | Y | 266 | 2.5 | - | - | - | RNAV 1
20 | TF | PG261 | N | 266 | 4.8 | L | MNM 3000 | 250 | RNAV 1
30 | TF | NURMO | N | 195 | 22.0 | - | MNM FL120 | - | RNAV 1
"#;

const SAMPLE_LFPG_DATA_STAR_TEXT: &str = r#"
# Official DGAC / SIA France eAIP Section AD 2.24
# AD 2 LFPG DATA STAR RNAV RWY 08L 08R 09L 09R

PROCEDURE: VEBEK 5E | RWY: 08L | NAV: RNAV 1
10 | IF | VEBEK | N | - | - | - | MNM FL150 | - | RNAV 1
20 | TF | PG081 | N | 086 | 12.0 | L | MNM 5000 / MAX 9000 | 250 | RNAV 1
30 | TF | PG082 | N | 050 | 8.5 | R | 4000 | 210 | RNAV 1
"#;

const SAMPLE_LFPG_DATA_RNP_APP_TEXT: &str = r#"
# Official DGAC / SIA France eAIP Section AD 2.24
# AD 2 LFPG DATA RWY 26L FNA RNP

PROCEDURE: RNP26L | RWY: 26L | NAV: RNP APCH
10 | IF | PG262 | N | - | - | - | 4000 | 210 | RNP APCH
20 | TF | PG261 | N | 266 | 5.0 | - | 3000 | 185 | RNP APCH
30 | TF | RW26L | Y | 266 | 6.2 | - | 450 | - | VPA 3.00 TCH 50
"#;

#[test]
fn test_parse_lfpg_data_sid_table() {
    let procs = SiaProcedureProvider::parse_procedure_text(
        SAMPLE_LFPG_DATA_SID_TEXT,
        "LFPG",
        ProcedureKind::Sid,
        "AD 2 LFPG DATA SID RNAV RWY 26L",
    )
    .expect("parse SIA DATA SID table");

    assert_eq!(procs.len(), 3);

    // 1. OPALE 5A
    let opale = procs
        .iter()
        .find(|p| p.procedure_ident == "OPALE 5A")
        .unwrap();
    assert_eq!(opale.airport_icao, "LFPG");
    assert_eq!(opale.procedure_kind, ProcedureKind::Sid);
    assert_eq!(opale.runway.as_deref(), Some("26L"));
    assert_eq!(opale.nav_spec.as_deref(), Some("RNAV 1"));
    assert_eq!(opale.legs.len(), 4);

    let l1 = &opale.legs[0];
    assert_eq!(l1.sequence_number, 10);
    assert_eq!(l1.path_terminator, "CF");
    assert_eq!(l1.fix_ident, "RW26L");
    assert!(l1.is_flyover);
    assert_eq!(l1.course_mag_deg, Some(266.0));
    assert_eq!(l1.distance_nm, Some(2.5));

    let l2 = &opale.legs[1];
    assert_eq!(l2.fix_ident, "PG261");
    assert!(!l2.is_flyover);
    assert_eq!(l2.turn_direction, Some('R'));
    assert_eq!(l2.altitude_descriptor.as_deref(), Some("+"));
    assert_eq!(l2.altitude_1_ft, Some(3000));
    assert_eq!(l2.speed_limit_kts, Some(250));

    let l3 = &opale.legs[2];
    assert_eq!(l3.fix_ident, "PG262");
    assert_eq!(l3.altitude_descriptor.as_deref(), Some("B"));
    assert_eq!(l3.altitude_1_ft, Some(5000));
    assert_eq!(l3.altitude_2_ft, Some(10000));

    let l4 = &opale.legs[3];
    assert_eq!(l4.fix_ident, "OPALE");
    assert_eq!(l4.altitude_descriptor.as_deref(), Some("+"));
    assert_eq!(l4.altitude_1_ft, Some(10000)); // FL100 = 10,000 ft
}

#[test]
fn test_parse_lfpg_data_star_table() {
    let procs = SiaProcedureProvider::parse_procedure_text(
        SAMPLE_LFPG_DATA_STAR_TEXT,
        "LFPG",
        ProcedureKind::Star,
        "AD 2 LFPG DATA STAR RNAV RWY 08L",
    )
    .expect("parse SIA DATA STAR table");

    assert_eq!(procs.len(), 1);
    let vebek = &procs[0];
    assert_eq!(vebek.procedure_ident, "VEBEK 5E");
    assert_eq!(vebek.procedure_kind, ProcedureKind::Star);
    assert_eq!(vebek.runway.as_deref(), Some("08L"));
    assert_eq!(vebek.legs.len(), 3);

    assert_eq!(vebek.legs[0].path_terminator, "IF");
    assert_eq!(vebek.legs[0].fix_ident, "VEBEK");
    assert_eq!(vebek.legs[0].altitude_1_ft, Some(15000)); // FL150
}

#[test]
fn test_parse_lfpg_data_rnp_approach_table() {
    let procs = SiaProcedureProvider::parse_procedure_text(
        SAMPLE_LFPG_DATA_RNP_APP_TEXT,
        "LFPG",
        ProcedureKind::Approach,
        "AD 2 LFPG DATA RWY 26L FNA RNP",
    )
    .expect("parse SIA DATA RNP approach table");

    assert_eq!(procs.len(), 1);
    let rnp = &procs[0];
    assert_eq!(rnp.procedure_ident, "RNP26L");
    assert_eq!(rnp.procedure_kind, ProcedureKind::Approach);
    assert_eq!(rnp.runway.as_deref(), Some("26L"));
    assert_eq!(rnp.legs.len(), 3);

    assert_eq!(rnp.legs[2].path_terminator, "TF");
    assert_eq!(rnp.legs[2].fix_ident, "RW26L");
    assert!(rnp.legs[2].is_flyover);
}

#[test]
fn test_french_terminal_fix_resolution() {
    let pg261 = SiaProcedureProvider::resolve_french_terminal_fix("PG261", "LFPG");
    assert!(pg261.is_some());
    let (lon, lat) = pg261.unwrap();
    assert!((lon - 2.684167).abs() < 1e-4);
    assert!((lat - 49.018333).abs() < 1e-4);

    let opale = SiaProcedureProvider::resolve_french_terminal_fix("OPALE", "LFPG");
    assert!(opale.is_some());

    let unknown = SiaProcedureProvider::resolve_french_terminal_fix("UNKNOWN99", "LFPG");
    assert_eq!(unknown, None);
}

#[test]
fn test_ingest_sia_procedures_into_worldstore_and_validate() {
    let mut store = WorldStore::open_in_memory().expect("open memory store");
    store.migrate().expect("migrate store");

    let prov = SiaProcedureProvider::default();
    let sids = SiaProcedureProvider::parse_procedure_text(
        SAMPLE_LFPG_DATA_SID_TEXT,
        "LFPG",
        ProcedureKind::Sid,
        "AD 2 LFPG DATA SID RNAV RWY 26L",
    )
    .unwrap();

    let now = Utc.with_ymd_and_hms(2026, 8, 20, 0, 0, 0).unwrap();
    let report = prov
        .ingest_parsed_procedures(
            &mut store,
            &sids,
            now,
            Some("2608"),
            "https://www.sia.aviation-civile.gouv.fr/dirca/AD_2_LFPG_DATA_SID.pdf",
        )
        .expect("ingest procedures");

    assert_eq!(report.records_created, 10); // 4 for OPALE, 3 for ATREX, 3 for NURMO

    // Verify retrieval of legs from store
    let legs = store
        .query_procedure_legs_at(now)
        .expect("query procedure legs");
    assert_eq!(legs.len(), 10);

    let opale_legs: Vec<_> = legs
        .into_iter()
        .filter(|l| l.procedure_ident == "OPALE 5A")
        .collect();
    assert_eq!(opale_legs.len(), 4);

    let opale_proc = Procedure::assemble("LFPG", ProcedureKind::Sid, "OPALE 5A", opale_legs, |f| {
        SiaProcedureProvider::resolve_french_terminal_fix(f, "LFPG")
    })
    .expect("assemble OPALE 5A");

    let validator =
        ProcedureValidator::new(|f| SiaProcedureProvider::resolve_french_terminal_fix(f, "LFPG"));

    let val_report = validator.validate_procedure(&opale_proc);
    assert!(val_report.is_flyable, "OPALE 5A must be flyable");
    assert_eq!(val_report.error_count(), 0);
}
