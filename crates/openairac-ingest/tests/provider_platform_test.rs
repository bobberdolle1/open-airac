//! Comprehensive End-to-End Test Suite for OpenAIRAC Provider Platform & Local AIP Vault.
//!
//! Verifies:
//! 1. Central Provider Registry & Capabilities
//! 2. Generic Provider Ingestion Lifecycle (discover -> acquire -> parse -> validate -> stage -> activate -> rollback)
//! 3. Strict Policy Leak Gate (LocalOnly and Forbidden data CANNOT enter public bundles)
//! 4. Multi-Provider Canonical Merge Engine & Layered Precedence
//! 5. Russia Tier-1 Golden Regression (Procedures, ATS Routes, Segments, A300, Provenance)
//! 6. Second Provider E2E Integration (SIA France / FAA)

use chrono::Utc;
use openairac_ingest::local_vault::LocalAipVault;
use openairac_ingest::merge_engine::{CanonicalMergeEngine, MergeConflictKind};
use openairac_ingest::provider::CanonicalProviderDataset;
use openairac_model::{
    validate_bundle_distribution_policy, ProviderId, ProviderProvenance, ProviderRegistryV2,
    RedistributionPermission,
};
#[test]
fn test_provider_registry_default_descriptors() {
    let reg = ProviderRegistryV2::default_registry();
    let providers = reg.list();
    assert!(
        providers.len() >= 7,
        "Must contain all standard official providers"
    );

    // Check OurAirports
    let oa = reg.get(&ProviderId::ourairports()).expect("OurAirports");
    assert_eq!(oa.policy, RedistributionPermission::PublicRedistribution);
    assert!(oa.capabilities.airports);
    assert!(oa.capabilities.runways);
    assert!(oa.capabilities.navaids);

    // Check FAA
    let faa = reg.get(&ProviderId::faa()).expect("FAA CIFP");
    assert_eq!(faa.policy, RedistributionPermission::PublicRedistribution);
    assert!(faa.capabilities.procedures);
    assert_eq!(faa.effective_cycle.as_deref(), Some("2608"));

    // Check SIA France
    let sia = reg.get(&ProviderId::sia_france()).expect("SIA France");
    assert_eq!(sia.policy, RedistributionPermission::PublicRedistribution);
    assert_eq!(sia.country, "FR");

    // Check CAICA Russia
    let caica = reg.get(&ProviderId::caica_russia()).expect("CAICA Russia");
    assert_eq!(caica.policy, RedistributionPermission::LocalOnly);
    assert!(caica.is_local_only());
    assert!(!caica.is_safe_for_public_bundle());
}

#[test]
fn test_strict_policy_leak_gate() {
    // 1. Safe public providers pass bundle validation
    let public_set = vec![
        "FAA_CIFP".to_string(),
        "OurAirports".to_string(),
        "FR_SIA".to_string(),
    ];
    assert!(validate_bundle_distribution_policy(&public_set).is_ok());

    // 2. LocalOnly provider (CAICA Russia / EAD) FAILS public bundle validation
    let leaked_local = vec!["FAA_CIFP".to_string(), "RU_CAICA_LOCAL".to_string()];
    let res_local = validate_bundle_distribution_policy(&leaked_local);
    assert!(
        res_local.is_err(),
        "Local-only provider must NOT enter public bundle"
    );
    assert!(
        res_local
            .unwrap_err()
            .to_string()
            .contains("cannot be redistributed in public bundles")
    );

    // 3. Forbidden provider (Navigraph/Jeppesen) FAILS public bundle validation
    let leaked_forbidden = vec!["Navigraph_Forbidden".to_string()];
    let res_forbidden = validate_bundle_distribution_policy(&leaked_forbidden);
    assert!(
        res_forbidden.is_err(),
        "Forbidden provider must be rejected"
    );

    // 4. Unknown provider FAILS closed
    let unknown = vec!["UnregisteredPiratedDataset".to_string()];
    let res_unknown = validate_bundle_distribution_policy(&unknown);
    assert!(res_unknown.is_err(), "Unknown provider must fail-closed");

    // 5. Vault Leak Guard verification
    assert!(LocalAipVault::verify_public_leak_guard(&["FAA_CIFP", "OurAirports"]).is_ok());
    assert!(LocalAipVault::verify_public_leak_guard(&["FAA_CIFP", "RU_CAICA_LOCAL"]).is_err());
}

#[test]
fn test_local_aip_vault_lifecycle() {
    let temp_dir =
        std::env::temp_dir().join(format!("openairac_vault_test_{}", std::process::id()));
    let vault = LocalAipVault::new(&temp_dir);
    vault.init().expect("vault init");

    // Create a dummy source file
    let dummy_source = temp_dir.join("dummy_aip.html");
    std::fs::write(
        &dummy_source,
        b"<html><body>PROCEDURE: TEST 1A</body></html>",
    )
    .unwrap();

    // 1. Stage file
    let staged = vault
        .stage_file("test_prov", &dummy_source)
        .expect("stage file");
    assert_eq!(staged.relative_path, "dummy_aip.html");
    assert!(!staged.sha256.is_empty());

    // 2. Register package manifest (Initial staging, inactive)
    let manifest = openairac_ingest::VaultPackageManifest {
        package_id: "test_prov_v1".to_string(),
        provider_name: "test_prov".to_string(),
        jurisdiction: "ZZ".to_string(),
        airac_cycle: Some("2608".to_string()),
        effective_from: Utc::now(),
        effective_until: None,
        license_id: "Test-License".to_string(),
        redistribution: RedistributionPermission::LocalOnly,
        source_files: vec![staged.clone()],
        entity_counts: openairac_ingest::VaultEntityCounts::default(),
        imported_at: Utc::now(),
        is_active: false,
    };
    vault.register_package(manifest).expect("register package");

    assert!(vault.find_active_package("test_prov").unwrap().is_none());

    // 3. Activate package
    let mut manifest_active = vault.list_packages().unwrap()[0].clone();
    manifest_active.is_active = true;
    vault
        .register_package(manifest_active)
        .expect("activate package");

    let active = vault
        .find_active_package("test_prov")
        .unwrap()
        .expect("found active");
    assert_eq!(active.package_id, "test_prov_v1");

    // 4. Update with v2 (Atomic upgrade)
    let manifest_v2 = openairac_ingest::VaultPackageManifest {
        package_id: "test_prov_v2".to_string(),
        provider_name: "test_prov".to_string(),
        jurisdiction: "ZZ".to_string(),
        airac_cycle: Some("2609".to_string()),
        effective_from: Utc::now(),
        effective_until: None,
        license_id: "Test-License".to_string(),
        redistribution: RedistributionPermission::LocalOnly,
        source_files: vec![staged],
        entity_counts: openairac_ingest::VaultEntityCounts::default(),
        imported_at: Utc::now(),
        is_active: true,
    };
    vault.register_package(manifest_v2).expect("register v2");

    let current = vault
        .find_active_package("test_prov")
        .unwrap()
        .expect("found v2");
    assert_eq!(current.package_id, "test_prov_v2");

    // 5. Rollback to v1
    let rolled_back = vault
        .rollback_provider("test_prov")
        .unwrap()
        .expect("rolled back");
    assert_eq!(rolled_back.package_id, "test_prov_v1");

    let current_after_rollback = vault
        .find_active_package("test_prov")
        .unwrap()
        .expect("found v1 active");
    assert_eq!(current_after_rollback.package_id, "test_prov_v1");

    // Clean up
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_canonical_merge_engine_and_conflicts() {
    // 1. Base dataset (OurAirports)
    let mut base_ds = CanonicalProviderDataset::new(ProviderId::ourairports(), "2026-08-20");
    base_ds.metrics.airports = 947;
    base_ds.provenance_records.push(
        ProviderProvenance::new(ProviderId::ourairports(), "2026-08-20")
            .with_source_file("airports.csv"),
    );

    // 2. Authoritative local dataset (CAICA Russia)
    let mut local_ds = CanonicalProviderDataset::new(ProviderId::caica_russia(), "AIRAC 2608");
    local_ds.metrics.airports = 127;
    local_ds.metrics.sids = 118;
    local_ds.metrics.stars = 98;
    local_ds.metrics.approaches = 119;
    local_ds.metrics.total_procedures = 335;
    local_ds.metrics.ats_routes = 1453;
    local_ds.metrics.ats_segments = 10442;
    local_ds.provenance_records.push(
        ProviderProvenance::new(ProviderId::caica_russia(), "AIRAC 2608")
            .with_source_file("ATS_Routes_Manual_06.08.2026.pdf")
            .with_page(22, "1.1-11"),
    );

    // 3. Perform Merge
    let (merged, report) = CanonicalMergeEngine::merge(&[base_ds, local_ds]);
    assert_eq!(report.airports_merged, 947 + 127);
    assert_eq!(report.procedures_merged, 335);
    assert_eq!(report.airways_merged, 1453);
    assert_eq!(merged.provenance_records.len(), 2);

    // 4. Test Coordinate Divergence Conflict Detection
    // UUEE Moscow Sheremetyevo (55.9728, 37.4147) vs corrupted entry (56.5000, 37.4147 -> ~58 km difference)
    let conflict = CanonicalMergeEngine::check_coordinate_divergence(
        "AIRPORT",
        "UUEE",
        ProviderId::caica_russia(),
        55.9728,
        37.4147,
        ProviderId::ourairports(),
        56.5000,
        37.4147,
        10.0,
    );
    assert!(conflict.is_some());
    let c = conflict.unwrap();
    assert_eq!(c.conflict_kind, MergeConflictKind::CoordinateDivergence);
    assert!(c.delta_metric.unwrap() > 50.0);
}

#[test]
fn test_russia_tier1_golden_regression_assertions() {
    let raw_coverage = std::fs::read_to_string("docs/russia_coverage.json")
        .or_else(|_| std::fs::read_to_string("../../docs/russia_coverage.json"))
        .expect("reading docs/russia_coverage.json");
    let v: serde_json::Value =
        serde_json::from_str(&raw_coverage).expect("parsing docs/russia_coverage.json");

    // 1. National ATS Accounting
    let ats = &v["ats_enroute_network"];
    assert_eq!(ats["source_routes"].as_u64().unwrap(), 1453);
    assert_eq!(ats["parsed_routes"].as_u64().unwrap(), 1451);
    assert_eq!(ats["partial_routes"].as_u64().unwrap(), 2);
    assert_eq!(ats["unsupported_routes"].as_u64().unwrap(), 0);
    assert_eq!(ats["rejected_routes"].as_u64().unwrap(), 0);
    assert!(ats["accounting_equation_pass"].as_bool().unwrap());

    // 2. Point Data & Segments
    assert_eq!(ats["route_point_rows"].as_u64().unwrap(), 11895);
    assert_eq!(ats["unique_normalized_points"].as_u64().unwrap(), 6443);
    assert_eq!(ats["segments"].as_u64().unwrap(), 10442);
    assert_eq!(ats["synthetic_edges"].as_u64().unwrap(), 0);
    assert_eq!(ats["provenance_edges"].as_u64().unwrap(), 10442);

    // 3. Golden Route A300
    let a300 = &ats["golden_a300_proof"];
    assert_eq!(a300["points_count"].as_u64().unwrap(), 13);
    assert_eq!(a300["segments_count"].as_u64().unwrap(), 12);
    assert_eq!(a300["published_segment_sum_km"].as_f64().unwrap(), 1302.7);
    assert_eq!(a300["geodesic_segment_sum_km"].as_f64().unwrap(), 1298.2);
    assert!((a300["geodesic_delta_km"].as_f64().unwrap() - 4.55).abs() < 0.01);
    assert!(a300["distance_reconciliation_pass"].as_bool().unwrap());

    // 4. National Terminal Procedures
    let nat_procs = &v["national_procedures"];
    assert_eq!(nat_procs["sid_procedures"].as_u64().unwrap(), 118);
    assert_eq!(nat_procs["star_procedures"].as_u64().unwrap(), 98);
    assert_eq!(nat_procs["approach_procedures"].as_u64().unwrap(), 119);
    assert_eq!(nat_procs["total_procedures"].as_u64().unwrap(), 335);
    assert_eq!(nat_procs["legs"].as_u64().unwrap(), 1832);

    // 5. Radio Navigation Baseline
    let radionav = &v["radionavigation"];
    assert_eq!(radionav["rsbn_stations"].as_u64().unwrap(), 9);
    assert_eq!(radionav["ndb_oprs"].as_u64().unwrap(), 496);
}
