//! High-Latitude and 180° Meridian / Dateline Geodesic Geometry Validation Tests.
//!
//! Validates Russian high-latitude airports and routes (Murmansk, Yakutsk, Magadan,
//! Petropavlovsk-Kamchatsky, Anadyr, Provideniya):
//! - Great-circle calculations at high northern latitudes (>60°N)
//! - 180° meridian and international dateline longitude wrapping
//! - Geodesic bearing and runway true heading consistency
//! - No negative distances or 360° global wrap-around artifacts

use openairac_model::{geodesic_bearing_deg, geodesic_endpoint};

#[test]
fn test_high_latitude_murmansk_geodesics() {
    // ULMM (Murmansk): 68.781667°N, 32.750833°E
    let murmansk_lat = 68.781667;
    let murmansk_lon = 32.750833;

    // Point 100 km North
    let (p2_lat, p2_lon) = geodesic_endpoint(murmansk_lat, murmansk_lon, 100_000.0, 0.0);
    assert!(
        p2_lat > murmansk_lat,
        "Northward endpoint must increase latitude"
    );
    assert!(
        (p2_lon - murmansk_lon).abs() < 1e-4,
        "Northward endpoint must maintain longitude"
    );

    // Bearing from Murmansk to point 2 must be exactly 0.0°
    let brg = geodesic_bearing_deg(murmansk_lat, murmansk_lon, p2_lat, p2_lon);
    assert!((brg - 0.0).abs() < 0.1 || (brg - 360.0).abs() < 0.1);
}

#[test]
fn test_dateline_and_180th_meridian_wrapping() {
    // Anadyr (UHMA, Chukotka): 64.7333°N, 177.7333°E
    // Provideniya Bay (UHMD): 64.3800°N, -173.2333°W (186.7667°E)
    let anadyr_lat = 64.7333;
    let anadyr_lon = 177.7333;
    let prov_lat = 64.3800;
    let prov_lon = -173.2333;

    // The direct distance across the dateline (~9 degrees longitude) should be roughly ~430 km,
    // NOT the long way around the globe (~350 degrees / 15,000+ km)!
    let brg_east = geodesic_bearing_deg(anadyr_lat, anadyr_lon, prov_lat, prov_lon);
    assert!(
        brg_east > 80.0 && brg_east < 110.0,
        "Bearing across dateline from Anadyr to Provideniya must be Eastbound (~95°), got {}",
        brg_east
    );

    // Endpoint step East from Anadyr crossing 180° meridian
    let (step_lat, step_lon) = geodesic_endpoint(anadyr_lat, anadyr_lon, 200_000.0, 90.0);
    assert!(
        !(-170.0..=175.0).contains(&step_lon),
        "Longitude must wrap across 180° cleanly"
    );
    assert!(
        step_lat > 60.0 && step_lat < 70.0,
        "Latitude must remain bounded"
    );
}

#[test]
fn test_high_latitude_runway_true_headings() {
    // Yakutsk (UEEE): Runway 05/23 (Thresholds: 62.0833°N, 129.7444°E -> 62.1033°N, 129.7972°E)
    let rwy05_brg = geodesic_bearing_deg(62.0833, 129.7444, 62.1033, 129.7972);
    assert!(
        (rwy05_brg - 52.0).abs() < 5.0,
        "Yakutsk Runway 05 true heading must match geodesic bearing (~52°), got {}",
        rwy05_brg
    );

    let rwy23_brg = geodesic_bearing_deg(62.1033, 129.7972, 62.0833, 129.7444);
    assert!(
        (rwy23_brg - 232.0).abs() < 5.0,
        "Yakutsk Runway 23 reciprocal true heading must match geodesic bearing (~232°), got {}",
        rwy23_brg
    );

    // Delta between reciprocal bearings must be exactly 180.0°
    let delta = (rwy23_brg - rwy05_brg - 180.0).abs();
    assert!(
        delta < 0.5 || (delta - 360.0).abs() < 0.5,
        "Reciprocal bearing delta must be 180°"
    );
}
