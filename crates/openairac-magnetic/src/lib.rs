use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use thiserror::Error;
/// WMM2025 Spherical Harmonic Degree N=12
pub const WMM_MAX_DEGREE: usize = 12;
pub const WMM_EPOCH: f64 = 2025.0;
pub const WMM_VALID_FROM: f64 = 2025.0;
pub const WMM_VALID_UNTIL: f64 = 2030.0;

/// World Magnetic Model Metadata
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MagneticModelMetadata {
    pub model: &'static str,
    pub epoch: f64,
    pub valid_from_year: f64,
    pub valid_until_year: f64,
    pub source: &'static str,
}

pub fn wmm2025_metadata() -> MagneticModelMetadata {
    MagneticModelMetadata {
        model: "WMM2025",
        epoch: WMM_EPOCH,
        valid_from_year: WMM_VALID_FROM,
        valid_until_year: WMM_VALID_UNTIL,
        source: "NOAA/NCEI & BGS World Magnetic Model 2025",
    }
}

/// Errors occurring during WMM calculation and validation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
pub enum WmmError {
    #[error("Date {year:.2} is outside WMM2025 validity range ({min:.1}-{max:.1})")]
    DateOutsideValidityRange { year: f64, min: f64, max: f64 },
    #[error("Latitude {lat:.4}° is invalid (must be in range [-90.0, +90.0])")]
    InvalidLatitude { lat: f64 },
    #[error(
        "Longitude {lon:.4}° is invalid (must be in range [-360.0, +360.0]; 0..360°E accepted)"
    )]
    InvalidLongitude { lon: f64 },
    #[error("Altitude {alt_km:.2} km is invalid (must be in range [-1.0, 850.0] km)")]
    InvalidAltitude { alt_km: f64 },
    #[error("Invalid runway designator: {0}")]
    InvalidRunwayDesignator(String),
}

/// World Magnetic Model 2025 (WMM2025) Coefficient Entry
#[derive(Debug, Clone, Copy)]
pub struct WmmCoefficient {
    pub n: usize,
    pub m: usize,
    pub g: f64,  // nTesla
    pub h: f64,  // nTesla
    pub dg: f64, // nTesla / year (secular variation)
    pub dh: f64, // nTesla / year
}

/// Official NOAA/NCEI WMM2025 Main Field (2025.0) & Secular Variation Coefficients (n=1..12)
/// Source: NOAA NCEI & BGS WMM2025.COF (Released Dec 17, 2024)
pub static WMM2025_COEFFICIENTS: &[WmmCoefficient] = &[
    WmmCoefficient {
        n: 1,
        m: 0,
        g: -29351.8,
        h: 0.0,
        dg: 12.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 1,
        m: 1,
        g: -1410.8,
        h: 4545.4,
        dg: 9.7,
        dh: -21.5,
    },
    WmmCoefficient {
        n: 2,
        m: 0,
        g: -2556.6,
        h: 0.0,
        dg: -11.6,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 2,
        m: 1,
        g: 2951.1,
        h: -3133.6,
        dg: -5.2,
        dh: -27.7,
    },
    WmmCoefficient {
        n: 2,
        m: 2,
        g: 1649.3,
        h: -815.1,
        dg: -8.0,
        dh: -12.1,
    },
    WmmCoefficient {
        n: 3,
        m: 0,
        g: 1361.0,
        h: 0.0,
        dg: -1.3,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 3,
        m: 1,
        g: -2404.1,
        h: -56.6,
        dg: -4.2,
        dh: 4.0,
    },
    WmmCoefficient {
        n: 3,
        m: 2,
        g: 1243.8,
        h: 237.5,
        dg: 0.4,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 3,
        m: 3,
        g: 453.6,
        h: -549.5,
        dg: -15.6,
        dh: -4.1,
    },
    WmmCoefficient {
        n: 4,
        m: 0,
        g: 895.0,
        h: 0.0,
        dg: -1.6,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 4,
        m: 1,
        g: 799.5,
        h: 278.6,
        dg: -2.4,
        dh: -1.1,
    },
    WmmCoefficient {
        n: 4,
        m: 2,
        g: 55.7,
        h: -133.9,
        dg: -6.0,
        dh: 4.1,
    },
    WmmCoefficient {
        n: 4,
        m: 3,
        g: -281.1,
        h: 212.0,
        dg: 5.6,
        dh: 1.6,
    },
    WmmCoefficient {
        n: 4,
        m: 4,
        g: 12.1,
        h: -375.6,
        dg: -7.0,
        dh: -4.4,
    },
    WmmCoefficient {
        n: 5,
        m: 0,
        g: -233.2,
        h: 0.0,
        dg: 0.6,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 5,
        m: 1,
        g: 368.9,
        h: 45.4,
        dg: 1.4,
        dh: -0.5,
    },
    WmmCoefficient {
        n: 5,
        m: 2,
        g: 187.2,
        h: 220.2,
        dg: 0.0,
        dh: 2.2,
    },
    WmmCoefficient {
        n: 5,
        m: 3,
        g: -138.7,
        h: -122.9,
        dg: 0.6,
        dh: 0.4,
    },
    WmmCoefficient {
        n: 5,
        m: 4,
        g: -142.0,
        h: 43.0,
        dg: 2.2,
        dh: 1.7,
    },
    WmmCoefficient {
        n: 5,
        m: 5,
        g: 20.9,
        h: 106.1,
        dg: 0.9,
        dh: 1.9,
    },
    WmmCoefficient {
        n: 6,
        m: 0,
        g: 64.4,
        h: 0.0,
        dg: -0.2,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 6,
        m: 1,
        g: 63.8,
        h: -18.4,
        dg: -0.4,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 6,
        m: 2,
        g: 76.9,
        h: 16.8,
        dg: 0.9,
        dh: -1.6,
    },
    WmmCoefficient {
        n: 6,
        m: 3,
        g: -115.7,
        h: 48.8,
        dg: 1.2,
        dh: -0.4,
    },
    WmmCoefficient {
        n: 6,
        m: 4,
        g: -40.9,
        h: -59.8,
        dg: -0.9,
        dh: 0.9,
    },
    WmmCoefficient {
        n: 6,
        m: 5,
        g: 14.9,
        h: 10.9,
        dg: 0.3,
        dh: 0.7,
    },
    WmmCoefficient {
        n: 6,
        m: 6,
        g: -60.7,
        h: 72.7,
        dg: 0.9,
        dh: 0.9,
    },
    WmmCoefficient {
        n: 7,
        m: 0,
        g: 79.5,
        h: 0.0,
        dg: -0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 7,
        m: 1,
        g: -77.0,
        h: -48.9,
        dg: -0.1,
        dh: 0.6,
    },
    WmmCoefficient {
        n: 7,
        m: 2,
        g: -8.8,
        h: -14.4,
        dg: -0.1,
        dh: 0.5,
    },
    WmmCoefficient {
        n: 7,
        m: 3,
        g: 59.3,
        h: -1.0,
        dg: 0.5,
        dh: -0.8,
    },
    WmmCoefficient {
        n: 7,
        m: 4,
        g: 15.8,
        h: 23.4,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 7,
        m: 5,
        g: 2.5,
        h: -7.4,
        dg: -0.8,
        dh: -1.0,
    },
    WmmCoefficient {
        n: 7,
        m: 6,
        g: -11.1,
        h: -25.1,
        dg: -0.8,
        dh: 0.6,
    },
    WmmCoefficient {
        n: 7,
        m: 7,
        g: 14.2,
        h: -2.3,
        dg: 0.8,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 8,
        m: 0,
        g: 23.2,
        h: 0.0,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 8,
        m: 1,
        g: 10.8,
        h: 7.1,
        dg: 0.2,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 8,
        m: 2,
        g: -17.5,
        h: -12.6,
        dg: 0.0,
        dh: 0.5,
    },
    WmmCoefficient {
        n: 8,
        m: 3,
        g: 2.0,
        h: 11.4,
        dg: 0.5,
        dh: -0.4,
    },
    WmmCoefficient {
        n: 8,
        m: 4,
        g: -21.7,
        h: -9.7,
        dg: -0.1,
        dh: 0.4,
    },
    WmmCoefficient {
        n: 8,
        m: 5,
        g: 16.9,
        h: 12.7,
        dg: 0.3,
        dh: -0.5,
    },
    WmmCoefficient {
        n: 8,
        m: 6,
        g: 15.0,
        h: 0.7,
        dg: 0.2,
        dh: -0.6,
    },
    WmmCoefficient {
        n: 8,
        m: 7,
        g: -16.8,
        h: -5.2,
        dg: -0.0,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 8,
        g: 0.9,
        h: 3.9,
        dg: 0.2,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 0,
        g: 4.6,
        h: 0.0,
        dg: -0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 9,
        m: 1,
        g: 7.8,
        h: -24.8,
        dg: -0.1,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 9,
        m: 2,
        g: 3.0,
        h: 12.2,
        dg: 0.1,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 9,
        m: 3,
        g: -0.2,
        h: 8.3,
        dg: 0.3,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 9,
        m: 4,
        g: -2.5,
        h: -3.3,
        dg: -0.3,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 9,
        m: 5,
        g: -13.1,
        h: -5.2,
        dg: 0.0,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 6,
        g: 2.4,
        h: 7.2,
        dg: 0.3,
        dh: -0.1,
    },
    WmmCoefficient {
        n: 9,
        m: 7,
        g: 8.6,
        h: -0.6,
        dg: -0.1,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 8,
        g: -8.7,
        h: 0.8,
        dg: 0.1,
        dh: 0.4,
    },
    WmmCoefficient {
        n: 9,
        m: 9,
        g: -12.9,
        h: 10.0,
        dg: -0.1,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 10,
        m: 0,
        g: -1.3,
        h: 0.0,
        dg: 0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 1,
        g: -6.4,
        h: 3.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 2,
        g: 0.2,
        h: 0.0,
        dg: 0.1,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 3,
        g: 2.0,
        h: 2.4,
        dg: 0.1,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 10,
        m: 4,
        g: -1.0,
        h: 5.3,
        dg: -0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 10,
        m: 5,
        g: -0.6,
        h: -9.1,
        dg: -0.3,
        dh: -0.1,
    },
    WmmCoefficient {
        n: 10,
        m: 6,
        g: -0.9,
        h: 0.4,
        dg: 0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 10,
        m: 7,
        g: 1.5,
        h: -4.2,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 8,
        g: 0.9,
        h: -3.8,
        dg: -0.1,
        dh: -0.1,
    },
    WmmCoefficient {
        n: 10,
        m: 9,
        g: -2.7,
        h: 0.9,
        dg: -0.0,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 10,
        m: 10,
        g: -3.9,
        h: -9.1,
        dg: -0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 0,
        g: 2.9,
        h: 0.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 1,
        g: -1.5,
        h: 0.0,
        dg: -0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 2,
        g: -2.5,
        h: 2.9,
        dg: 0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 11,
        m: 3,
        g: 2.4,
        h: -0.6,
        dg: 0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 4,
        g: -0.6,
        h: 0.2,
        dg: 0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 11,
        m: 5,
        g: -0.1,
        h: 0.5,
        dg: -0.1,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 6,
        g: -0.6,
        h: -0.3,
        dg: 0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 7,
        g: -0.1,
        h: -1.2,
        dg: -0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 11,
        m: 8,
        g: 1.1,
        h: -1.7,
        dg: -0.1,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 9,
        g: -1.0,
        h: -2.9,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 10,
        g: -0.2,
        h: -1.8,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 11,
        g: 2.6,
        h: -2.3,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 0,
        g: -2.0,
        h: 0.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 1,
        g: -0.2,
        h: -1.3,
        dg: 0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 2,
        g: 0.3,
        h: 0.7,
        dg: -0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 3,
        g: 1.2,
        h: 1.0,
        dg: -0.0,
        dh: -0.1,
    },
    WmmCoefficient {
        n: 12,
        m: 4,
        g: -1.3,
        h: -1.4,
        dg: -0.0,
        dh: 0.1,
    },
    WmmCoefficient {
        n: 12,
        m: 5,
        g: 0.6,
        h: -0.0,
        dg: -0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 6,
        g: 0.6,
        h: 0.6,
        dg: 0.1,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 7,
        g: 0.5,
        h: -0.1,
        dg: -0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 8,
        g: -0.1,
        h: 0.8,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 9,
        g: -0.4,
        h: 0.1,
        dg: 0.0,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 10,
        g: -0.2,
        h: -1.0,
        dg: -0.1,
        dh: -0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 11,
        g: -1.3,
        h: 0.1,
        dg: -0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 12,
        g: -0.7,
        h: 0.2,
        dg: -0.1,
        dh: -0.1,
    },
];

/// Result of World Magnetic Model calculation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WmmResult {
    pub declination_deg: f64,
    pub inclination_deg: f64,
    pub total_intensity_nt: f64,
    pub horizontal_intensity_nt: f64,
    pub north_component_nt: f64,
    pub east_component_nt: f64,
    pub down_component_nt: f64,
}

/// Official WMM2025 Solver (Verbatim NOAA C Geomagnetism Library MAG_PcupLow algorithm)
pub struct Wmm2025;

impl Wmm2025 {
    /// Calculate Magnetic Field components & Declination with input validation
    pub fn calculate_checked(
        lat_deg: f64,
        lon_deg: f64,
        alt_ft: f64,
        year: f64,
    ) -> Result<WmmResult, WmmError> {
        let alt_km = alt_ft * 0.0003048;
        Self::calculate_km_checked(lat_deg, lon_deg, alt_km, year)
    }

    /// Calculate Magnetic Field components with altitude in km and strict parameter validation
    pub fn calculate_km_checked(
        lat_deg: f64,
        lon_deg: f64,
        alt_km: f64,
        year: f64,
    ) -> Result<WmmResult, WmmError> {
        if !(WMM_VALID_FROM..=WMM_VALID_UNTIL).contains(&year) {
            return Err(WmmError::DateOutsideValidityRange {
                year,
                min: WMM_VALID_FROM,
                max: WMM_VALID_UNTIL,
            });
        }
        if !(-90.0..=90.0).contains(&lat_deg) {
            return Err(WmmError::InvalidLatitude { lat: lat_deg });
        }
        if !(-360.0..=360.0).contains(&lon_deg) {
            return Err(WmmError::InvalidLongitude { lon: lon_deg });
        }
        if !(-1.0..=850.0).contains(&alt_km) {
            return Err(WmmError::InvalidAltitude { alt_km });
        }

        Ok(Self::calculate_km_unchecked(lat_deg, lon_deg, alt_km, year))
    }

    fn calculate_km_unchecked(lat_deg: f64, lon_deg: f64, alt_km: f64, year: f64) -> WmmResult {
        let dt = year - WMM_EPOCH;

        // WGS84 Ellipsoid constants
        let a = 6378.137; // km
        let b = 6356.7523142; // km
        let re = 6371.2; // Reference radius km

        let lat_rad = lat_deg * PI / 180.0;
        let lon_rad = lon_deg * PI / 180.0;

        let sin_lat = lat_rad.sin();
        let cos_lat = lat_rad.cos();

        // Geodetic to Geocentric conversion
        let e2 = 1.0 - (b * b) / (a * a);
        let n_lat = a / (1.0 - e2 * sin_lat * sin_lat).sqrt();

        let rho = (n_lat + alt_km) * cos_lat;
        let z_geod = (n_lat * (1.0 - e2) + alt_km) * sin_lat;

        let r = (rho * rho + z_geod * z_geod).sqrt();

        // Geocentric latitude in radians and parameters
        let phig_rad = (z_geod / r).asin();
        let psi_rad = phig_rad - lat_rad;

        // NOAA MAG_PcupLow variables: x = sin(phig), z = cos(phig)
        let x = phig_rad.sin();
        let z = phig_rad.cos();

        let mut g = [[0.0f64; 13]; 13];
        let mut h = [[0.0f64; 13]; 13];

        for c in WMM2025_COEFFICIENTS {
            g[c.n][c.m] = c.g + dt * c.dg;
            h[c.n][c.m] = c.h + dt * c.dh;
        }

        let mut pcup = [[0.0f64; 13]; 13];
        let mut dpcup = [[0.0f64; 13]; 13];

        pcup[0][0] = 1.0;
        dpcup[0][0] = 0.0;

        for n in 1..=WMM_MAX_DEGREE {
            for m in 0..=n {
                if n == m {
                    pcup[n][n] = z * pcup[n - 1][n - 1];
                    dpcup[n][n] = z * dpcup[n - 1][n - 1] + x * pcup[n - 1][n - 1];
                } else if n == 1 && m == 0 {
                    pcup[1][0] = x * pcup[0][0];
                    dpcup[1][0] = x * dpcup[0][0] - z * pcup[0][0];
                } else if n > 1 && n != m {
                    if m > n - 2 {
                        pcup[n][m] = x * pcup[n - 1][m];
                        dpcup[n][m] = x * dpcup[n - 1][m] - z * pcup[n - 1][m];
                    } else {
                        let n_f = n as f64;
                        let m_f = m as f64;
                        let k = ((n_f - 1.0) * (n_f - 1.0) - m_f * m_f)
                            / ((2.0 * n_f - 1.0) * (2.0 * n_f - 3.0));
                        pcup[n][m] = x * pcup[n - 1][m] - k * pcup[n - 2][m];
                        dpcup[n][m] =
                            x * dpcup[n - 1][m] - z * pcup[n - 1][m] - k * dpcup[n - 2][m];
                    }
                }
            }
        }

        // Schmidt quasi-normalization array
        let mut schmidt = [[0.0f64; 13]; 13];
        schmidt[0][0] = 1.0;
        for n in 1..=WMM_MAX_DEGREE {
            let n_f = n as f64;
            schmidt[n][0] = schmidt[n - 1][0] * (2.0 * n_f - 1.0) / n_f;
            for m in 1..=n {
                let m_f = m as f64;
                let k = if m == 1 { 2.0 } else { 1.0 };
                schmidt[n][m] = schmidt[n][m - 1] * (((n_f - m_f + 1.0) * k) / (n_f + m_f)).sqrt();
            }
        }

        // Apply Schmidt quasi-normalization
        for n in 1..=WMM_MAX_DEGREE {
            for m in 0..=n {
                pcup[n][m] *= schmidt[n][m];
                dpcup[n][m] *= schmidt[n][m];
            }
        }

        let mut bx_sph = 0.0;
        let mut by_sph = 0.0;
        let mut bz_sph = 0.0;

        let ratio = re / r;
        let mut ratio_n = ratio * ratio;

        for n in 1..=WMM_MAX_DEGREE {
            ratio_n *= ratio;
            let n_f = n as f64;
            for m in 0..=n {
                let m_f = m as f64;
                let cos_m_lon = (m_f * lon_rad).cos();
                let sin_m_lon = (m_f * lon_rad).sin();

                let g_m = g[n][m];
                let h_m = h[n][m];

                let g_cos_h_sin = g_m * cos_m_lon + h_m * sin_m_lon;
                let g_sin_h_cos = g_m * sin_m_lon - h_m * cos_m_lon;

                // NOAA MAG_Summation equations
                bz_sph -= ratio_n * g_cos_h_sin * (n_f + 1.0) * pcup[n][m];
                by_sph += ratio_n * g_sin_h_cos * m_f * pcup[n][m];
                bx_sph += ratio_n * g_cos_h_sin * dpcup[n][m];
            }
        }

        // NOAA MAG_Summation: divide by_sph by cos(phig)
        if z.abs() > 1e-10 {
            by_sph /= z;
        }

        // NOAA MAG_RotateMagneticVector: map spherical magnetic vector to geodetic coordinates
        let x_geo = bx_sph * psi_rad.cos() - bz_sph * psi_rad.sin();
        let y_geo = by_sph;
        let z_geo = bx_sph * psi_rad.sin() + bz_sph * psi_rad.cos();

        let h_int = (x_geo * x_geo + y_geo * y_geo).sqrt();
        let f_int = (h_int * h_int + z_geo * z_geo).sqrt();
        let decl = y_geo.atan2(x_geo) * 180.0 / PI;
        let incl = z_geo.atan2(h_int) * 180.0 / PI;

        WmmResult {
            declination_deg: decl,
            inclination_deg: incl,
            total_intensity_nt: f_int,
            horizontal_intensity_nt: h_int,
            north_component_nt: x_geo,
            east_component_nt: y_geo,
            down_component_nt: z_geo,
        }
    }
}

/// Dual Runway Magnetic Drift Analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunwayMagneticAnalysis {
    pub official_designator: String,
    pub true_heading_deg: f64,
    pub wmm_magvar_deg: f64,
    pub computed_magnetic_heading_deg: f64,
    pub computed_magnetic_designator: String,
    pub reciprocal_official_designator: String,
    pub reciprocal_computed_designator: String,
    pub drift_difference_deg: f64,
    pub is_redesignation_suggested: bool,
}

pub fn analyze_runway_magnetic_drift(
    official_designator: &str,
    true_heading_deg: f64,
    lat: f64,
    lon: f64,
    year: f64,
) -> Result<RunwayMagneticAnalysis, WmmError> {
    let wmm = Wmm2025::calculate_checked(lat, lon, 0.0, year)?;
    let mag_heading = (true_heading_deg - wmm.declination_deg + 360.0) % 360.0;

    let suffix: String = official_designator
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect();

    let digits_part: String = official_designator
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits_part.is_empty() {
        return Err(WmmError::InvalidRunwayDesignator(
            official_designator.to_string(),
        ));
    }
    let official_num: u32 = digits_part
        .parse()
        .map_err(|_| WmmError::InvalidRunwayDesignator(official_designator.to_string()))?;
    if official_num == 0 || official_num > 36 {
        return Err(WmmError::InvalidRunwayDesignator(
            official_designator.to_string(),
        ));
    }

    let rwy_num = ((mag_heading / 10.0).round() as u32) % 36;
    let final_num = if rwy_num == 0 { 36 } else { rwy_num };
    let computed_designator = format!("{:02}{}", final_num, suffix);

    let reciprocal_heading = (mag_heading + 180.0) % 360.0;
    let recip_num = ((reciprocal_heading / 10.0).round() as u32) % 36;
    let final_recip_num = if recip_num == 0 { 36 } else { recip_num };

    let reciprocal_suffix = match suffix.as_str() {
        "L" => "R",
        "R" => "L",
        "C" => "C",
        other => other,
    };
    let recip_computed_designator = format!("{:02}{}", final_recip_num, reciprocal_suffix);

    let reciprocal_official_num = if official_num > 18 {
        official_num - 18
    } else {
        official_num + 18
    };
    let recip_official_designator = format!("{:02}{}", reciprocal_official_num, reciprocal_suffix);

    let official_heading = (official_num * 10) as f64;
    let raw_drift = (mag_heading - official_heading).abs();
    let drift = if raw_drift > 180.0 {
        360.0 - raw_drift
    } else {
        raw_drift
    };

    Ok(RunwayMagneticAnalysis {
        official_designator: official_designator.to_string(),
        true_heading_deg,
        wmm_magvar_deg: wmm.declination_deg,
        computed_magnetic_heading_deg: mag_heading,
        computed_magnetic_designator: computed_designator.clone(),
        reciprocal_official_designator: recip_official_designator,
        reciprocal_computed_designator: recip_computed_designator,
        drift_difference_deg: drift,
        is_redesignation_suggested: computed_designator != official_designator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Golden test against the official NOAA/NCEI WMM2025 test values
    /// (https://www.ncei.noaa.gov/sites/default/files/2025-02/WMM2025_TEST_VALUES.txt).
    /// Every vector checks date, altitude, latitude, longitude and all field components.
    /// Official values are published to 0.1 nT / 0.01 deg; we allow 0.3 nT and 0.02 deg.
    struct GoldenVector {
        year: f64,
        alt_km: f64,
        lat: f64,
        lon: f64,
        x: f64,
        y: f64,
        z: f64,
        h: f64,
        f: f64,
        inc: f64,
        dec: f64,
    }
    impl GoldenVector {
        fn check(&self) {
            let v = Wmm2025::calculate_km_checked(self.lat, self.lon, self.alt_km, self.year)
                .unwrap_or_else(|e| {
                    panic!(
                        "WMM failed for ({}, {}, {}, {}): {e}",
                        self.lat, self.lon, self.alt_km, self.year
                    )
                });
            assert!(
                (v.north_component_nt - self.x).abs() < 0.3,
                "X mismatch: got {:.2}, expected {:.1}",
                v.north_component_nt,
                self.x
            );
            assert!(
                (v.east_component_nt - self.y).abs() < 0.3,
                "Y mismatch: got {:.2}, expected {:.1}",
                v.east_component_nt,
                self.y
            );
            assert!(
                (v.down_component_nt - self.z).abs() < 0.3,
                "Z mismatch: got {:.2}, expected {:.1}",
                v.down_component_nt,
                self.z
            );
            assert!(
                (v.horizontal_intensity_nt - self.h).abs() < 0.3,
                "H mismatch: got {:.2}, expected {:.1}",
                v.horizontal_intensity_nt,
                self.h
            );
            assert!(
                (v.total_intensity_nt - self.f).abs() < 0.3,
                "F mismatch: got {:.2}, expected {:.1}",
                v.total_intensity_nt,
                self.f
            );
            assert!(
                (v.inclination_deg - self.inc).abs() < 0.02,
                "I mismatch: got {:.3}, expected {:.2}",
                v.inclination_deg,
                self.inc
            );
            assert!(
                (v.declination_deg - self.dec).abs() < 0.02,
                "D mismatch: got {:.3}, expected {:.2}",
                v.declination_deg,
                self.dec
            );
        }
    }

    #[test]
    fn test_noaa_wmm2025_reference_vectors() {
        // Official NOAA/NCEI WMM2025 test values, transcribed verbatim.
        let vectors = [
            // 2025.0, 0 km
            GoldenVector {
                year: 2025.0,
                alt_km: 0.0,
                lat: 80.0,
                lon: 0.0,
                x: 6521.6,
                y: 145.9,
                z: 54791.5,
                h: 6523.2,
                f: 55178.5,
                inc: 83.21,
                dec: 1.28,
            },
            GoldenVector {
                year: 2025.0,
                alt_km: 0.0,
                lat: 0.0,
                lon: 120.0,
                x: 39677.8,
                y: -109.6,
                z: -10580.2,
                h: 39677.9,
                f: 41064.3,
                inc: -14.93,
                dec: -0.16,
            },
            GoldenVector {
                year: 2025.0,
                alt_km: 0.0,
                lat: -80.0,
                lon: 240.0,
                x: 6117.5,
                y: 15751.9,
                z: -52022.5,
                h: 16898.1,
                f: 54698.2,
                inc: -72.00,
                dec: 68.78,
            },
            // 2025.0, 100 km
            GoldenVector {
                year: 2025.0,
                alt_km: 100.0,
                lat: 80.0,
                lon: 0.0,
                x: 6216.0,
                y: 92.4,
                z: 52598.8,
                h: 6216.7,
                f: 52964.9,
                inc: 83.26,
                dec: 0.85,
            },
            GoldenVector {
                year: 2025.0,
                alt_km: 100.0,
                lat: 0.0,
                lon: 120.0,
                x: 37688.6,
                y: -96.2,
                z: -10152.1,
                h: 37688.7,
                f: 39032.1,
                inc: -15.08,
                dec: -0.15,
            },
            GoldenVector {
                year: 2025.0,
                alt_km: 100.0,
                lat: -80.0,
                lon: 240.0,
                x: 5907.6,
                y: 14780.3,
                z: -49540.7,
                h: 15917.1,
                f: 52035.0,
                inc: -72.19,
                dec: 68.21,
            },
            // 2027.5, 0 km
            GoldenVector {
                year: 2027.5,
                alt_km: 0.0,
                lat: 80.0,
                lon: 0.0,
                x: 6500.8,
                y: 294.5,
                z: 54869.4,
                h: 6507.5,
                f: 55253.9,
                inc: 83.24,
                dec: 2.59,
            },
            GoldenVector {
                year: 2027.5,
                alt_km: 0.0,
                lat: 0.0,
                lon: 120.0,
                x: 39701.6,
                y: -167.4,
                z: -10381.8,
                h: 39702.0,
                f: 41036.9,
                inc: -14.65,
                dec: -0.24,
            },
            GoldenVector {
                year: 2027.5,
                alt_km: 0.0,
                lat: -80.0,
                lon: 240.0,
                x: 6200.7,
                y: 15730.3,
                z: -51783.7,
                h: 16908.3,
                f: 54474.2,
                inc: -71.92,
                dec: 68.49,
            },
            // 2027.5, 100 km
            GoldenVector {
                year: 2027.5,
                alt_km: 100.0,
                lat: 80.0,
                lon: 0.0,
                x: 6196.7,
                y: 233.8,
                z: 52670.5,
                h: 6201.1,
                f: 53034.3,
                inc: 83.29,
                dec: 2.16,
            },
            GoldenVector {
                year: 2027.5,
                alt_km: 100.0,
                lat: 0.0,
                lon: 120.0,
                x: 37711.5,
                y: -148.7,
                z: -9969.8,
                h: 37711.8,
                f: 39007.4,
                inc: -14.81,
                dec: -0.23,
            },
            GoldenVector {
                year: 2027.5,
                alt_km: 100.0,
                lat: -80.0,
                lon: 240.0,
                x: 5984.0,
                y: 14760.1,
                z: -49317.7,
                h: 15927.0,
                f: 51825.7,
                inc: -72.10,
                dec: 67.93,
            },
        ];
        for v in &vectors {
            v.check();
        }
    }

    #[test]
    fn test_wmm_validation_errors() {
        assert!(Wmm2025::calculate_km_checked(80.0, 0.0, 0.0, 2047.0).is_err());
        assert!(Wmm2025::calculate_km_checked(95.0, 0.0, 0.0, 2025.0).is_err());
        assert!(Wmm2025::calculate_km_checked(80.0, 400.0, 0.0, 2025.0).is_err());
        assert!(Wmm2025::calculate_km_checked(80.0, 0.0, 1000.0, 2025.0).is_err());
    }

    #[test]
    fn test_runway_magnetic_drift_detector() {
        let analysis = analyze_runway_magnetic_drift("09", 96.7, 55.97, 37.41, 2026.0).unwrap();
        assert_eq!(analysis.official_designator, "09");
        assert!(analysis.computed_magnetic_heading_deg > 0.0);
    }

    #[test]
    fn test_runway_designator_validation() {
        assert!(analyze_runway_magnetic_drift("invalid", 90.0, 0.0, 0.0, 2025.0).is_err());
        assert!(analyze_runway_magnetic_drift("00L", 90.0, 0.0, 0.0, 2025.0).is_err());
        assert!(analyze_runway_magnetic_drift("37R", 90.0, 0.0, 0.0, 2025.0).is_err());
    }

    #[test]
    fn test_runway_36_18_wrap_and_reciprocal() {
        let analysis = analyze_runway_magnetic_drift("36L", 356.0, 0.0, 0.0, 2025.0).unwrap();
        assert_eq!(analysis.computed_magnetic_designator, "36L");
        assert_eq!(analysis.reciprocal_computed_designator, "18R");
    }
}
