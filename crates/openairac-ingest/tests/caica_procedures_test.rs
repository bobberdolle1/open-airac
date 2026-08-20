//! Comprehensive Integration Tests for Russian Federation CAICA Procedure Parsing,
//! Local AIP Vault, RSBN Radio Navigation, and Mixed AIRAC Cycle Provenance.

use chrono::Utc;
use openairac_ingest::caica_procedures::{CaicaAltitudeConstraint, CaicaProcedureProvider};
use openairac_ingest::caica_rsbn::CaicaRsbnProvider;
use openairac_ingest::local_vault::LocalAipVault;
use openairac_procedures::ProcedureKind;
use openairac_store::WorldStore;

const SAMPLE_UUEE_EMGAS_1A_HTML_TEXT: &str = r#"
# CAICA Official RNAV Coding Table: UUEE / SHEREMETYEVO
PROCEDURE: EMGAS 1A | RWY: 24C | NAV: RNAV 1 | APT: ШЕРЕМЕТЬЕВО
010 | CA | | N | 243 (254.5) | | 0.0 | +1100 | -205 | | RNAV 1 | 2608 | 55 58 21.00 N 037 24 53.00 E
020 | CF | EE001 | Y | 258 (269.5) | R | 9.8 km | 4000 | -230 | | RNAV 1 | 2608 | 55 57 12.00 N 037 14 30.00 E
030 | TF | EE002 | N | 300 (311.5) | R | 18.5 km | 7000-5000 | 250 | | RNAV 1 | 2506 | 56 04 45.00 N 037 02 10.00 E
040 | TF | EMGAS | N | 315 (326.5) | | 24.2 km | +FL150 | | | RNAV 1 | 2402 | 56 16 30.00 N 036 48 00.00 E
"#;

const SAMPLE_UUEE_DIPOP_1A_STAR_TEXT: &str = r#"
# CAICA Official RNAV Coding Table: UUEE / STAR
PROCEDURE: DIPOP 1A | RWY: 24C | NAV: RNAV 1 | APT: ШЕРЕМЕТЬЕВО
010 | IF | DIPOP | N | 095 (106.5) | | 0.0 | FL150-FL140 | 250 | | RNAV 1 | 2608 | 56 22 10.00 N 036 30 15.00 E
020 | TF | EE051 | N | 115 (126.5) | | 15.2 km | +6000 | 230 | | RNAV 1 | 2608 | 56 15 20.00 N 036 45 30.00 E
030 | TF | EE052 | N | 140 (151.5) | | 20.1 km | 4000 | -210 | | RNAV 1 | 2506 | 56 05 10.00 N 037 02 45.00 E
040 | TF | RW24C | Y | 243 (254.5) | | 12.4 km | +1100 | -185 | | RNAV 1 | 2608 | 55 58 21.00 N 037 24 53.00 E
"#;

const SAMPLE_UUEE_RNP_24C_APP_TEXT: &str = r#"
# CAICA Official RNP Approach Table: UUEE / RNP 24C
PROCEDURE: RNP 24C | RWY: 24C | NAV: RNP APCH | APT: ШЕРЕМЕТЬЕВО
010 | IF | EE080 | N | 063 (074.5) | | 0.0 | +3000 | -210 | | RNP APCH | 2608 | 56 02 15.00 N 037 38 40.00 E
020 | TF | EE081 | N | 243 (254.5) | | 9.2 km | +2000 | | | RNP APCH | 2608 | 56 00 10.00 N 037 30 15.00 E
030 | TF | RW24C | Y | 243 (254.5) | | 8.5 km | +650 | | VPA -3.00 TCH 50 | RNP APCH | 2608 | 55 58 21.00 N 037 24 53.00 E
040 | CA | | N | 243 (254.5) | | 0.0 | +2000 | | | RNP APCH | 2608 | 55 58 21.00 N 037 24 53.00 E
050 | DF | EE082 | Y | 330 (341.5) | R | 14.5 km | 4000 | -220 | | RNP APCH | 2608 | 56 05 30.00 N 037 15 00.00 E
060 | HM | EE082 | N | 063 (074.5) | R | 1 MIN | 4000 | 220 | | RNP APCH | 2608 | 56 05 30.00 N 037 15 00.00 E
"#;

const SAMPLE_USTJ_CANARY_TEXT: &str = r#"
# CAICA Canary: USTJ / Tobolsk Remizov (AIRAC 2608)
PROCEDURE: GIRUS 1A | RWY: 04 | NAV: RNAV 1 | APT: ТОБОЛЬСК РЕМИЗОВ
010 | CA | | N | 042 (055.0) | | 0.0 | +600 | -185 | | RNAV 1 | 2608 | 58 08 12.00 N 068 20 45.00 E
020 | DF | TJ001 | N | 060 (073.0) | R | 12.0 km | 2000 | -210 | | RNAV 1 | 2608 | 58 12 30.00 N 068 35 10.00 E
030 | TF | GIRUS | N | 090 (103.0) | | 25.4 km | +FL120 | | | RNAV 1 | 2608 | 58 15 00.00 N 069 05 00.00 E
"#;

const SAMPLE_RSBN_TABLE_TEXT: &str = r#"
IDENT,NAME,CHANNEL,LAT,LON,ELEV_FT,RANGE_KM,ASSOCIATED_APT,MAG_VAR
KLN,КЛИН (KLIN),24,56.350000,36.733333,525,180.0,UUEE,11.5
CHL,ЧКАЛОВСКИЙ (CHKALOVSKY),36,55.883333,38.050000,492,200.0,UUMU,11.2
SHG,ШАГОЛ (SHAGOL),18,55.250000,61.300000,820,150.0,USCC,14.8
KLT,КОЛЬЦОВО (KOLTSOVO),42,56.743056,60.802778,764,220.0,USSS,14.5
TOL,ТОЛМАЧЕВО (TOLMACHEVO),12,55.012500,82.650833,364,250.0,UNNT,10.2
"#;

#[test]
fn test_parse_uuee_sid_with_dual_courses_and_mixed_airac() {
    let procs = CaicaProcedureProvider::parse_procedure_text(
        SAMPLE_UUEE_EMGAS_1A_HTML_TEXT,
        "UUEE",
        ProcedureKind::Sid,
        "UUEE SID EMGAS 1A",
    )
    .expect("Must parse UUEE SID EMGAS 1A");

    assert_eq!(procs.len(), 1);
    let p = &procs[0];
    assert_eq!(p.airport_icao, "UUEE");
    assert_eq!(p.airport_ru_name.as_deref(), Some("ШЕРЕМЕТЬЕВО"));
    assert_eq!(p.procedure_ident, "EMGAS 1A");
    assert_eq!(p.procedure_kind, ProcedureKind::Sid);
    assert_eq!(p.runway.as_deref(), Some("24C"));
    assert_eq!(p.legs.len(), 4);

    // Leg 1: CA, course 243°M (254.5°T), altitude +1100, speed -205 kts (max)
    let l1 = &p.legs[0];
    assert_eq!(l1.sequence_number, 10);
    assert_eq!(l1.path_terminator, "CA");
    assert_eq!(l1.course_mag_deg, Some(243.0));
    assert_eq!(l1.course_true_deg, Some(254.5));
    assert_eq!(
        l1.altitude_constraint,
        Some(CaicaAltitudeConstraint::AtOrAbove(1100))
    );
    assert_eq!(l1.speed_limit_kts, Some(205));
    assert!(l1.is_speed_max);
    assert_eq!(l1.airac_row_cycle.as_deref(), Some("2608"));

    // Leg 2: CF to EE001, distance 9.8 km -> 5.29 NM
    let l2 = &p.legs[1];
    assert_eq!(l2.path_terminator, "CF");
    assert_eq!(l2.fix_ident, "EE001");
    assert!(l2.is_flyover);
    assert_eq!(l2.turn_direction, Some('R'));
    assert_eq!(l2.distance_km, Some(9.8));
    assert!((l2.distance_nm.unwrap() - (9.8 / 1.852)).abs() < 1e-4);
    assert_eq!(
        l2.altitude_constraint,
        Some(CaicaAltitudeConstraint::At(4000))
    );

    // Leg 3: TF to EE002, altitude 7000-5000 (Between), AIRAC 2506
    let l3 = &p.legs[2];
    assert_eq!(
        l3.altitude_constraint,
        Some(CaicaAltitudeConstraint::Between(5000, 7000))
    );
    assert_eq!(l3.airac_row_cycle.as_deref(), Some("2506"));

    // Leg 4: TF to EMGAS, altitude +FL150 -> AtOrAbove(15000), AIRAC 2402
    let l4 = &p.legs[3];
    assert_eq!(l4.fix_ident, "EMGAS");
    assert_eq!(
        l4.altitude_constraint,
        Some(CaicaAltitudeConstraint::AtOrAbove(15000))
    );
    assert_eq!(l4.airac_row_cycle.as_deref(), Some("2402"));
}

#[test]
fn test_parse_uuee_rnp_approach_with_vpa_and_holding() {
    let procs = CaicaProcedureProvider::parse_procedure_text(
        SAMPLE_UUEE_RNP_24C_APP_TEXT,
        "UUEE",
        ProcedureKind::Approach,
        "UUEE RNP 24C",
    )
    .expect("Must parse UUEE RNP 24C");

    assert_eq!(procs.len(), 1);
    let p = &procs[0];
    assert_eq!(p.procedure_ident, "RNP 24C");
    assert_eq!(p.procedure_kind, ProcedureKind::Approach);
    assert_eq!(p.legs.len(), 6);

    // Leg 3: FAF to MAPt (RW24C) with VPA -3.00 and TCH 50 ft
    let l3 = &p.legs[2];
    assert_eq!(l3.fix_ident, "RW24C");
    assert_eq!(l3.vertical_angle_deg, Some(-3.0));
    assert_eq!(l3.tch_ft, Some(50.0));

    // Leg 6: Missed approach holding (HM) at EE082
    let l6 = &p.legs[5];
    assert_eq!(l6.path_terminator, "HM");
    assert_eq!(l6.fix_ident, "EE082");
    assert_eq!(l6.turn_direction, Some('R'));
    assert_eq!(
        l6.altitude_constraint,
        Some(CaicaAltitudeConstraint::At(4000))
    );
}

#[test]
fn test_parse_uuee_dipop_1a_star() {
    let procs = CaicaProcedureProvider::parse_procedure_text(
        SAMPLE_UUEE_DIPOP_1A_STAR_TEXT,
        "UUEE",
        ProcedureKind::Star,
        "UUEE STAR DIPOP 1A",
    )
    .expect("Must parse UUEE STAR DIPOP 1A");

    assert_eq!(procs.len(), 1);
    let p = &procs[0];
    assert_eq!(p.procedure_ident, "DIPOP 1A");
    assert_eq!(p.procedure_kind, ProcedureKind::Star);
    assert_eq!(p.legs.len(), 4);
}

#[test]
fn test_parse_ustj_canary_airac_2608() {
    let procs = CaicaProcedureProvider::parse_procedure_text(
        SAMPLE_USTJ_CANARY_TEXT,
        "USTJ",
        ProcedureKind::Sid,
        "USTJ SID GIRUS 1A",
    )
    .expect("Must parse USTJ canary");

    assert_eq!(procs.len(), 1);
    let p = &procs[0];
    assert_eq!(p.airport_icao, "USTJ");
    assert_eq!(p.airport_ru_name.as_deref(), Some("ТОБОЛЬСК РЕМИЗОВ"));
    assert_eq!(p.procedure_ident, "GIRUS 1A");
    assert_eq!(p.legs.len(), 3);
}

#[test]
fn test_rsbn_table_parsing_and_navaid_ingest() {
    let stations = CaicaRsbnProvider::parse_rsbn_table(SAMPLE_RSBN_TABLE_TEXT)
        .expect("Must parse RSBN stations");

    assert_eq!(stations.len(), 5);
    assert_eq!(stations[0].ident, "KLN");
    assert_eq!(stations[0].channel, 24);
    assert_eq!(stations[0].associated_airport.as_deref(), Some("UUEE"));
    assert_eq!(stations[3].ident, "KLT");
    assert_eq!(stations[3].channel, 42);
    assert_eq!(stations[3].associated_airport.as_deref(), Some("USSS"));

    let mut store = WorldStore::open_in_memory().expect("Store open");
    let provider = CaicaRsbnProvider::default();
    let report = provider
        .ingest_rsbn_stations(
            &mut store,
            &stations,
            Utc::now(),
            Some("2608"),
            "test://caica_rsbn",
        )
        .expect("Must ingest RSBN stations");
    assert_eq!(report.records_created, 5);
}

#[test]
fn test_local_aip_vault_and_public_leak_guard() {
    // 1. Verify leak guard passes for purely public providers
    let public_providers = ["FAA_CIFP", "FR_SIA", "OurAirports", "DFS_INSPIRE"];
    LocalAipVault::verify_public_leak_guard(&public_providers)
        .expect("Public providers must pass leak guard");

    // 2. Verify leak guard rejects LocalOnly CAICA providers for public distribution
    let mixed_providers = ["FAA_CIFP", "RU_CAICA_PROCEDURES"];
    let leak_err = LocalAipVault::verify_public_leak_guard(&mixed_providers);
    assert!(
        leak_err.is_err(),
        "Leak guard must reject RU_CAICA_PROCEDURES from public bundle"
    );
    let msg = leak_err.unwrap_err().to_string();
    assert!(msg.contains("LEAK GUARD VIOLATION"));
    assert!(msg.contains("RU_CAICA_PROCEDURES"));

    // 3. Verify leak guard rejects proprietary Navigraph
    let forbidden_providers = ["FAA_CIFP", "Navigraph_Forbidden"];
    let leak_err2 = LocalAipVault::verify_public_leak_guard(&forbidden_providers);
    assert!(leak_err2.is_err());
}
