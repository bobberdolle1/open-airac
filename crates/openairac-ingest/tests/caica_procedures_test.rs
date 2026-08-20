//! Comprehensive Integration Tests for Russian Federation CAICA Procedure Parsing,
//! Local AIP Vault, RSBN Radio Navigation, and Mixed AIRAC Cycle Provenance.

use chrono::Utc;
use openairac_ingest::caica_procedures::{
    CaicaAltitudeConstraint, CaicaProcedureIndex, CaicaProcedureProvider,
};
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
fn test_dynamic_caica_procedure_index_discovery() {
    let sample_index_html = r#"
    <html>
    <body>
    <a href="book/rus/uuee.htm">МОСКВА (ШЕРЕМЕТЬЕВО) / MOSCOW (SHEREMETYEVO) [UUEE]</a>
    <a href="book/rus/uers.htm">САСКЫЛАХ / SASKYLAKH [UERS]</a>
    <a href="book/rus/uhna.htm">АЯН (МУНУК) / AYAN (MUNUK) [UHNA]</a>
    <a href="book/rus/uhma.htm">АНАДЫРЬ (УГОЛЬНЫЙ) / ANADYR (UGOLNY) [UHMA]</a>
    <a href="book/rus/ustj.htm">ТОБОЛЬСК (РЕМИЗОВ) / TOBOLSK (REMIZOV) [USTJ]</a>
    </body>
    </html>
    "#;

    let mut index = CaicaProcedureIndex::new();
    let discovered = index.discover_from_index_text(sample_index_html);
    assert_eq!(discovered, 5);
    assert_eq!(index.airports.len(), 5);
    assert!(index.airports.iter().any(|a| a.icao == "UERS"));
    assert!(index.airports.iter().any(|a| a.icao == "UHNA"));
    assert!(index.airports.iter().any(|a| a.icao == "UHMA"));
    assert!(index.airports.iter().any(|a| a.icao == "USTJ"));
    assert!(index.airports.iter().any(|a| a.icao == "UUEE"));
}

#[test]
fn test_caica_index_semantic_classification_and_arithmetic() {
    let index_snippet = r#"
    <a href="book/rus/uuee.htm">МОСКВА (ШЕРЕМЕТЬЕВО) [UUEE]</a>
    <a href="book/rus/uldh.htm">ВЕРТОДРОМ АРКТИЧЕСКИЙ [ULDH]</a>
    <a href="book/rus/uldp.htm">МЛСП ПРИРАЗЛОМНАЯ [ULDP]</a>
    <a href="book/rus/uhsf.htm">МОЛИКПАК [UHSF]</a>
    <a href="book/rus/uhsd.htm">ЛУНСКОЕ-ОБТК [UHSD]</a>
    <a href="book/rus/uirs.htm">Р-111 [UIRS]</a>
    <a href="book/rus/urft.htm">ТЕРЛЕЦКАЯ [URFT]</a>
    <a href="book/rus/nav.htm">НАВИГАЦИЯ / INDEX [NAV]</a>
    "#;

    let mut index = CaicaProcedureIndex::new();
    index.discover_from_index_text(index_snippet);

    assert_eq!(index.total_entries(), 8);
    assert_eq!(index.airport_entries(), 1);
    assert_eq!(index.heliport_vertodrome_entries(), 1);
    assert_eq!(index.offshore_platform_entries(), 5);
    assert_eq!(index.navigation_entries(), 1);
    assert_eq!(index.total_aviation_objects(), 7);
    assert!(
        index.verify_arithmetic(),
        "Arithmetic: 1 + 1 + 5 = 7 aviation objects, 7 + 1 = 8 total"
    );
}

#[test]
fn test_parse_uers_saskylakh_arctic_procedures() {
    let baseline_text = include_str!("../tests/fixtures/caica_procedures_russian_baseline.txt");
    let procs = CaicaProcedureProvider::parse_procedure_text(
        baseline_text,
        "UUEE",
        ProcedureKind::Sid,
        "CAICA National Collection (AIRAC 2608)",
    )
    .expect("Must parse national baseline");

    let uers_procs: Vec<_> = procs.iter().filter(|p| p.airport_icao == "UERS").collect();
    assert!(
        !uers_procs.is_empty(),
        "UERS must be discovered in national baseline"
    );

    let sid = uers_procs
        .iter()
        .find(|p| p.procedure_ident == "LENA 1A")
        .expect("UERS LENA 1A");
    assert_eq!(sid.procedure_kind, ProcedureKind::Sid);
    assert_eq!(sid.airport_ru_name.as_deref(), Some("САСКЫЛАХ"));
    assert_eq!(sid.legs.len(), 3);

    let app = uers_procs
        .iter()
        .find(|p| p.procedure_ident == "RNP 04")
        .expect("UERS RNP 04");
    assert_eq!(app.procedure_kind, ProcedureKind::Approach);
    assert_eq!(app.legs[2].vertical_angle_deg, Some(-3.0));
    assert_eq!(app.legs[2].tch_ft, Some(50.0));
}

#[test]
fn test_parse_uhna_ayan_far_east_procedures() {
    let baseline_text = include_str!("../tests/fixtures/caica_procedures_russian_baseline.txt");
    let procs = CaicaProcedureProvider::parse_procedure_text(
        baseline_text,
        "UUEE",
        ProcedureKind::Sid,
        "CAICA National Collection (AIRAC 2608)",
    )
    .expect("Must parse national baseline");

    let uhna_procs: Vec<_> = procs.iter().filter(|p| p.airport_icao == "UHNA").collect();
    assert!(
        !uhna_procs.is_empty(),
        "UHNA must be discovered in national baseline"
    );

    let sid = uhna_procs
        .iter()
        .find(|p| p.procedure_ident == "AYAN 1A")
        .expect("UHNA AYAN 1A");
    assert_eq!(sid.procedure_kind, ProcedureKind::Sid);
    assert_eq!(sid.airport_ru_name.as_deref(), Some("АЯН МУНУК"));

    let stats = CaicaProcedureIndex::compute_national_statistics(&procs);
    assert!(stats.total_airports_discovered >= 10);
    assert!(stats.total_procedures >= 20);
    assert!(stats.total_legs >= 60);
    assert!(stats.path_terminator_histogram.contains_key("IF"));
    assert!(stats.path_terminator_histogram.contains_key("TF"));
    assert!(stats.path_terminator_histogram.contains_key("CF"));
    assert!(stats.path_terminator_histogram.contains_key("DF"));
    assert!(stats.path_terminator_histogram.contains_key("CA"));
}

#[test]
fn test_caica_ats_route_table_parsing_and_graph_analysis() {
    let ats_csv = r#"
ROUTE,SEQ,START_FIX,START_LAT,START_LON,END_FIX,END_LAT,END_LON,DIR,MIN_FL,MAX_FL,MEA,NAV_SPEC,FIR
A300,10,RANET,55.3933,38.7900,ROTLI,55.3333,39.4167,BOTH,140,660,14000,RNAV 5,UUWV
A300,20,ROTLI,55.3333,39.4167,ADONI,55.2400,40.3100,BOTH,140,660,14000,RNAV 5,UUWV
A300,30,ADONI,55.2400,40.3100,BINBA,55.1333,41.2500,BOTH,140,660,14000,RNAV 5,UUWV
A300,40,BINBA,55.1333,41.2500,LEDVA,55.0417,42.0833,BOTH,140,660,14000,RNAV 5,UUWV
A300,50,LEDVA,55.0417,42.0833,KOSOT,54.9333,43.0000,BOTH,140,660,14000,RNAV 5,UWWW
A300,60,KOSOT,54.9333,43.0000,UWKD_FIX,54.8000,44.1667,BOTH,140,660,14000,RNAV 5,UWKD
B210,10,RANET,55.3933,38.7900,ROTLI,55.3333,39.4167,BOTH,180,660,18000,RNAV 5,UUWV
B210,20,ROTLI,55.3333,39.4167,NALEK,55.5000,43.2000,BOTH,180,660,18000,RNAV 5,UUWV
B210,30,NALEK,55.5000,43.2000,LUSAL,55.8000,48.5000,BOTH,180,660,18000,RNAV 5,UWWW
B210,40,LUSAL,55.8000,48.5000,TIKNA,56.2000,54.8000,BOTH,180,660,18000,RNAV 5,USSS
B210,50,TIKNA,56.2000,54.8000,KLT,56.7431,60.8028,BOTH,180,660,18000,RNAV 5,USSS
G370,10,KLT,56.7431,60.8028,TOL,55.0125,82.6508,BOTH,180,660,18000,RNAV 5,UNNT
N869,10,TOL,55.0125,82.6508,KUBEK,55.6000,87.5000,BOTH,180,660,18000,RNAV 5,UNNT
N869,20,KUBEK,55.6000,87.5000,KEM,56.1728,92.4831,BOTH,180,660,18000,RNAV 5,UNKL
N869,30,KEM,56.1728,92.4831,TAIGA,55.1000,96.8000,BOTH,180,660,18000,RNAV 5,UNKL
N869,40,TAIGA,55.1000,96.8000,TULUN,54.5000,100.5000,BOTH,180,660,18000,RNAV 5,UIII
N869,50,TULUN,54.5000,100.5000,IRK,52.2681,104.3889,BOTH,180,660,18000,RNAV 5,UIII
T562,10,IRK,52.2681,104.3889,KHB,48.5281,135.1883,BOTH,180,660,18000,RNAV 5,UHHH
T562,20,KHB,48.5281,135.1883,UHWW,43.3989,132.1481,BOTH,180,660,18000,RNAV 5,UHWW
W31,10,MR,55.9726,37.4146,VALDA,53.8000,38.2000,BOTH,180,660,18000,RNAV 5,UUWV
W31,20,VALDA,53.8000,38.2000,MOROZ,50.5000,39.1000,BOTH,180,660,18000,RNAV 5,URRV
W31,30,MOROZ,50.5000,39.1000,ROSTV,47.2500,39.8000,BOTH,180,660,18000,RNAV 5,URRV
W31,40,ROSTV,47.2500,39.8000,SCH,43.4499,39.9566,BOTH,180,660,18000,RNAV 5,URRV
M864,10,MR,55.9726,37.4146,KLN,56.3500,36.7333,BOTH,180,660,18000,RNAV 5,UUWV
M864,20,KLN,56.3500,36.7333,TVER,56.8500,35.9000,BOTH,180,660,18000,RNAV 5,UUWV
M864,30,TVER,56.8500,35.9000,VALDA,57.9800,33.2500,BOTH,180,660,18000,RNAV 5,ULLL
M864,40,VALDA,57.9800,33.2500,LUKIR,59.2000,31.4000,BOTH,180,660,18000,RNAV 5,ULLL
M864,50,LUKIR,59.2000,31.4000,SPB,59.8003,30.2625,BOTH,180,660,18000,RNAV 5,ULLL
B200,10,UWKD_FIX,54.8000,44.1667,UWWW_FIX,53.5047,50.1644,BOTH,140,660,14000,RNAV 5,UWWW
G495,10,UWWW_FIX,53.5047,50.1644,KLT,56.7431,60.8028,BOTH,180,660,18000,RNAV 5,USSS
L870,10,MR,55.9726,37.4146,URMM_FIX,44.2250,43.0819,BOTH,180,660,18000,RNAV 5,URRV
P865,10,KLT,56.7431,60.8028,USRR_FIX,61.3439,73.4019,BOTH,140,660,14000,RNAV 5,USRR
P865,20,USRR_FIX,61.3439,73.4019,USNN_FIX,60.9497,76.4883,BOTH,140,660,14000,RNAV 5,USNN
R30,10,TOL,55.0125,82.6508,UNOO_FIX,54.9669,73.3106,BOTH,140,660,14000,RNAV 5,UNOO
T500,10,IRK,52.2681,104.3889,UEEE_FIX,62.0933,129.7708,BOTH,180,660,18000,RNAV 5,UEEE
UM864,10,SPB,59.8003,30.2625,ULMM_FIX,68.7817,32.7508,BOTH,180,660,18000,RNAV 5,ULMM
W12,10,IRK,52.2681,104.3889,UIBB_FIX,56.3706,101.6989,BOTH,120,660,12000,RNAV 5,UIBB
W18,10,KHB,48.5281,135.1883,UHPP_FIX,53.1678,158.4539,BOTH,180,660,18000,RNAV 5,UHPP
W24,10,KHB,48.5281,135.1883,UHSS_FIX,46.8886,142.7175,BOTH,140,660,14000,RNAV 5,UHSS
W35,10,UEEE_FIX,62.0933,129.7708,UHMM_FIX,59.9108,150.7206,BOTH,180,660,18000,RNAV 5,UHMM
W42,10,UHMM_FIX,59.9108,150.7206,UHMA_FIX,64.7333,177.7333,BOTH,180,660,18000,RNAV 5,UHMA
W55,10,KEM,56.1728,92.4831,UOHH_FIX,71.9700,102.4900,BOTH,140,660,14000,RNAV 5,UOHH
W68,10,TOL,55.0125,82.6508,USTJ_FIX,58.1367,68.3458,BOTH,120,660,12000,RNAV 5,USTJ
W74,10,UEEE_FIX,62.0933,129.7708,UERS_FIX,71.9800,114.0800,BOTH,120,660,12000,RNAV 5,UEEE
W82,10,KHB,48.5281,135.1883,UHNA_FIX,56.4500,138.1600,BOTH,120,660,12000,RNAV 5,UHNA
"#;

    let segments = openairac_ingest::caica_ats::CaicaAtsProvider::parse_ats_table(ats_csv)
        .expect("Must parse ATS table");
    assert_eq!(segments.len(), 45);

    let summary = openairac_ingest::caica_ats::CaicaAtsProvider::analyze_graph(&segments);
    assert_eq!(summary.total_routes, 23);
    assert_eq!(summary.total_segments, 45);
    assert_eq!(summary.unique_nodes, 44);
    assert_eq!(summary.bidirectional_segments, 45);
    assert_eq!(summary.one_way_segments, 0);
    assert!(
        summary.validation_errors.is_empty(),
        "Zero graph validation errors"
    );

    // Golden A300 route verification: must contain RANET, ROTLI, ADONI, BINBA
    let a300_segs: Vec<_> = segments
        .iter()
        .filter(|s| s.route_designator == "A300")
        .collect();
    assert_eq!(a300_segs.len(), 6);
    assert!(
        a300_segs
            .iter()
            .any(|s| s.start_fix == "RANET" && s.end_fix == "ROTLI")
    );
    assert!(
        a300_segs
            .iter()
            .any(|s| s.start_fix == "ROTLI" && s.end_fix == "ADONI")
    );
    assert!(
        a300_segs
            .iter()
            .any(|s| s.start_fix == "ADONI" && s.end_fix == "BINBA")
    );
    assert!(
        a300_segs
            .iter()
            .any(|s| s.start_fix == "BINBA" && s.end_fix == "LEDVA")
    );
}

#[test]
fn test_russia_coverage_json_internal_consistency_self_check() {
    let report_str = include_str!("../../../docs/russia_coverage.json");
    let v: serde_json::Value = serde_json::from_str(report_str).expect("Valid JSON report");

    // 1. Aviation Object Category Sum: airports + vertodromes + offshore == aviation_objects
    let airports = v["source_index"]["airports"].as_u64().unwrap();
    let vertodromes = v["source_index"]["vertodromes"].as_u64().unwrap();
    let offshore = v["source_index"]["offshore"].as_u64().unwrap();
    let aviation_objects = v["source_index"]["aviation_objects"].as_u64().unwrap();
    assert_eq!(
        airports + vertodromes + offshore,
        aviation_objects,
        "Aviation objects sum mismatch: {} + {} + {} != {}",
        airports,
        vertodromes,
        offshore,
        aviation_objects
    );

    // 2. Total Href Sum: aviation_objects + navigation == total_href
    let navigation = v["source_index"]["navigation"].as_u64().unwrap();
    let total_href = v["source_index"]["total_href"].as_u64().unwrap();
    assert_eq!(
        aviation_objects + navigation,
        total_href,
        "Total href sum mismatch: {} + {} != {}",
        aviation_objects,
        navigation,
        total_href
    );

    // 3. Procedure Pages Accounting: pages_with_procedures + pages_no_procedure_data == pages_attempted
    let procs_pages = v["national_procedures"]["pages_with_any_structured_data"]
        .as_u64()
        .unwrap();
    let no_procs_pages = v["national_procedures"]["pages_without_procedure_tables"]
        .as_u64()
        .unwrap();
    let attempted = v["national_procedures"]["pages_attempted"]
        .as_u64()
        .unwrap();
    assert_eq!(
        procs_pages + no_procs_pages,
        attempted,
        "Pages accounting mismatch: {} + {} != {}",
        procs_pages,
        no_procs_pages,
        attempted
    );

    // 4. VOR Arithmetic: vor_family_total == vor_dme + vor_standalone + vortac
    let vor_total = v["radionavigation"]["vor_family_total"].as_u64().unwrap();
    let vor_dme = v["radionavigation"]["vor_dme"].as_u64().unwrap();
    let vor_std = v["radionavigation"]["vor_standalone"].as_u64().unwrap();
    let vortac = v["radionavigation"]["vortac"].as_u64().unwrap();
    assert_eq!(
        vor_dme + vor_std + vortac,
        vor_total,
        "VOR arithmetic mismatch: {} + {} + {} != {}",
        vor_dme,
        vor_std,
        vortac,
        vor_total
    );

    // 5. ILS Systems Consistency: russian_loc_components == russian_ils_systems
    let ils_systems = v["ils"]["russian_ils_systems"].as_u64().unwrap();
    let loc_comps = v["ils"]["russian_loc_components"].as_u64().unwrap();
    assert_eq!(
        ils_systems, loc_comps,
        "ILS systems and LOC count must match"
    );

    // 6. ATS Unique Points List Consistency: declared_count == unique_points_list.len()
    let declared_pts = v["ats_enroute_network"]["unique_route_points"]
        .as_u64()
        .unwrap() as usize;
    let list_len = v["ats_enroute_network"]["unique_points_list"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(
        declared_pts, list_len,
        "Declared ATS unique points count must equal list length"
    );
    assert_eq!(
        v["ats_enroute_network"]["graph"]["nodes"].as_u64().unwrap() as usize,
        list_len
    );

    // 7. Procedure Category Sum: sid + star + app == total_procedures
    let sid_p = v["national_procedures"]["sid_procedures"].as_u64().unwrap();
    let star_p = v["national_procedures"]["star_procedures"]
        .as_u64()
        .unwrap();
    let app_p = v["national_procedures"]["approach_procedures"]
        .as_u64()
        .unwrap();
    let total_p = v["national_procedures"]["total_procedures"]
        .as_u64()
        .unwrap();
    assert_eq!(
        sid_p + star_p + app_p,
        total_p,
        "Procedures sum mismatch: {} + {} + {} != {}",
        sid_p,
        star_p,
        app_p,
        total_p
    );

    // 8. Consistency Errors list must be empty
    assert!(
        v["consistency_errors"].as_array().unwrap().is_empty(),
        "Machine report must have zero consistency errors"
    );
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
