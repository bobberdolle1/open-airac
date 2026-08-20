//! Comprehensive Integration and Acceptance Tests for OpenAIRAC Chart Subsystem.

use openairac_charts::association::AssociationEngine;
use openairac_charts::cache::ChartCache;
use openairac_charts::catalog::ChartCatalog;
use openairac_charts::model::{AssociationConfidence, NormalizedChartType};
use openairac_charts::provider::ChartProvider;
use openairac_charts::providers::{FaaDtppProvider, FranceSiaChartProvider};
use tempfile::tempdir;

#[test]
fn test_faa_and_sia_chart_catalog_and_associations() {
    let t = tempdir().unwrap();
    let cat_db = t.path().join("openairac_charts.sqlite");
    let catalog = ChartCatalog::open(&cat_db).unwrap();
    let cache = ChartCache::new(t.path().join("cache")).unwrap();

    // 1. Sync FAA d-TPP Provider
    let faa_provider = FaaDtppProvider::new();
    let faa_report = faa_provider.sync_catalog(&catalog, Some("2608")).unwrap();
    assert_eq!(faa_report.provider_id, "FAA_DTPP");
    assert_eq!(faa_report.airac_cycle, "2608");
    assert!(faa_report.charts_indexed >= 38);
    assert!(faa_report.airports_indexed >= 4);

    // 2. Sync France SIA Provider
    let sia_provider = FranceSiaChartProvider::new();
    let sia_report = sia_provider.sync_catalog(&catalog, Some("2608")).unwrap();
    assert_eq!(sia_report.provider_id, "FR_SIA");
    assert_eq!(sia_report.airac_cycle, "2608");
    assert_eq!(sia_report.airports_indexed, 2); // LFPG + LFPO
    assert_eq!(sia_report.charts_indexed, 13); // 9 for LFPG + 4 for LFPO

    // 3. Query KJFK Charts
    let jfk_charts = catalog.query_charts_for_airport("KJFK").unwrap();
    assert_eq!(jfk_charts.len(), 38);

    let jfk_ad = jfk_charts
        .iter()
        .find(|c| c.chart_type == NormalizedChartType::AirportDiagram)
        .unwrap();
    assert_eq!(jfk_ad.title, "AIRPORT DIAGRAM");

    let jfk_ils04l = jfk_charts
        .iter()
        .find(|c| c.title == "ILS OR LOC RWY 04L")
        .unwrap();
    assert_eq!(jfk_ils04l.chart_type, NormalizedChartType::Approach);

    // 4. Query LFPG Charts
    let lfpg_charts = catalog.query_charts_for_airport("LFPG").unwrap();
    assert_eq!(lfpg_charts.len(), 9);

    let lfpg_adc = lfpg_charts
        .iter()
        .find(|c| c.provider_chart_type == "ADC")
        .unwrap();
    assert_eq!(lfpg_adc.chart_type, NormalizedChartType::AirportDiagram);

    // 5. Test Procedure-to-Chart Associations for KJFK
    let assoc_app =
        AssociationEngine::match_procedure_to_charts("KJFK", 'F', "I04L", Some("04L"), &jfk_charts);
    assert_eq!(assoc_app.len(), 1);
    assert_eq!(assoc_app[0].chart_id, jfk_ils04l.id);
    assert_eq!(assoc_app[0].confidence, AssociationConfidence::Exact);

    catalog
        .insert_or_replace_association(&assoc_app[0])
        .unwrap();
    let stored_assocs = catalog.query_charts_for_procedure("KJFK", "I04L").unwrap();
    assert_eq!(stored_assocs.len(), 1);
    assert_eq!(stored_assocs[0].chart_id, jfk_ils04l.id);

    // 6. Test Asset Cache and Retrieval
    let sample_chart = &jfk_charts[0];
    let fake_pdf = b"%PDF-1.4\n% OpenAIRAC Acceptance Test PDF\n%%EOF";
    let (sha, path) = cache.store_asset(fake_pdf, "pdf").unwrap();
    assert!(path.exists());
    assert!(cache.has_asset(&sha, "pdf"));

    let read_back = cache.read_asset(&sha, "pdf").unwrap();
    assert_eq!(read_back, fake_pdf);

    // Second request = cache hit
    let (sha2, path2) = cache.store_asset(fake_pdf, "pdf").unwrap();
    assert_eq!(sha, sha2);
    assert_eq!(path, path2);

    let _ = sample_chart;
}
