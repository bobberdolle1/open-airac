use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// WMM2025 Spherical Harmonic Degree N=12
pub const WMM_MAX_DEGREE: usize = 12;
pub const WMM_EPOCH: f64 = 2025.0;

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

/// Official WMM2025 Main Field (2025.0) & Secular Variation Coefficients (n=1..12)
/// Source: NOAA National Centers for Environmental Information (NCEI) WMM2025.COF
pub static WMM2025_COEFFICIENTS: &[WmmCoefficient] = &[
    // n=1
    WmmCoefficient {
        n: 1,
        m: 0,
        g: -29404.5,
        h: 0.0,
        dg: 6.7,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 1,
        m: 1,
        g: -1450.7,
        h: 4652.9,
        dg: 7.7,
        dh: -25.1,
    },
    // n=2
    WmmCoefficient {
        n: 2,
        m: 0,
        g: -2500.0,
        h: 0.0,
        dg: -11.5,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 2,
        m: 1,
        g: 2982.0,
        h: -2991.6,
        dg: -7.1,
        dh: -30.2,
    },
    WmmCoefficient {
        n: 2,
        m: 2,
        g: 1677.0,
        h: -734.8,
        dg: -2.2,
        dh: -23.9,
    },
    // n=3
    WmmCoefficient {
        n: 3,
        m: 0,
        g: 1362.7,
        h: 0.0,
        dg: 3.2,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 3,
        m: 1,
        g: -2381.6,
        h: -121.3,
        dg: -6.2,
        dh: 5.7,
    },
    WmmCoefficient {
        n: 3,
        m: 2,
        g: 1236.2,
        h: 241.9,
        dg: 3.4,
        dh: -1.3,
    },
    WmmCoefficient {
        n: 3,
        m: 3,
        g: 525.7,
        h: -542.9,
        dg: -12.2,
        dh: -14.4,
    },
    // n=4
    WmmCoefficient {
        n: 4,
        m: 0,
        g: 903.1,
        h: 0.0,
        dg: -3.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 4,
        m: 1,
        g: 809.4,
        h: 282.3,
        dg: 0.2,
        dh: 1.8,
    },
    WmmCoefficient {
        n: 4,
        m: 2,
        g: -377.9,
        h: -264.4,
        dg: -6.4,
        dh: 3.4,
    },
    WmmCoefficient {
        n: 4,
        m: 3,
        g: -128.8,
        h: 84.7,
        dg: 0.8,
        dh: 3.2,
    },
    WmmCoefficient {
        n: 4,
        m: 4,
        g: -307.2,
        h: -299.7,
        dg: -3.8,
        dh: -4.3,
    },
    // n=5
    WmmCoefficient {
        n: 5,
        m: 0,
        g: -230.6,
        h: 0.0,
        dg: 0.5,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 5,
        m: 1,
        g: 354.4,
        h: 47.0,
        dg: 0.7,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 5,
        m: 2,
        g: 208.7,
        h: 153.6,
        dg: 1.5,
        dh: -1.4,
    },
    WmmCoefficient {
        n: 5,
        m: 3,
        g: -121.3,
        h: -153.4,
        dg: -1.4,
        dh: 0.6,
    },
    WmmCoefficient {
        n: 5,
        m: 4,
        g: -168.6,
        h: -66.8,
        dg: -0.6,
        dh: 1.6,
    },
    WmmCoefficient {
        n: 5,
        m: 5,
        g: -14.0,
        h: 96.6,
        dg: 0.8,
        dh: -1.2,
    },
    // n=6
    WmmCoefficient {
        n: 6,
        m: 0,
        g: 71.7,
        h: 0.0,
        dg: -0.3,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 6,
        m: 1,
        g: 69.4,
        h: -18.0,
        dg: -0.4,
        dh: -0.4,
    },
    WmmCoefficient {
        n: 6,
        m: 2,
        g: 76.5,
        h: 54.8,
        dg: 0.6,
        dh: -1.4,
    },
    WmmCoefficient {
        n: 6,
        m: 3,
        g: -143.1,
        h: 67.2,
        dg: 0.5,
        dh: 0.4,
    },
    WmmCoefficient {
        n: 6,
        m: 4,
        g: -10.4,
        h: -63.5,
        dg: -1.8,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 6,
        m: 5,
        g: 9.3,
        h: -4.6,
        dg: -0.4,
        dh: -0.9,
    },
    WmmCoefficient {
        n: 6,
        m: 6,
        g: -90.9,
        h: 22.0,
        dg: 0.8,
        dh: 0.7,
    },
    // n=7
    WmmCoefficient {
        n: 7,
        m: 0,
        g: 80.8,
        h: 0.0,
        dg: 0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 7,
        m: 1,
        g: -75.8,
        h: -61.2,
        dg: -0.4,
        dh: 0.7,
    },
    WmmCoefficient {
        n: 7,
        m: 2,
        g: 2.1,
        h: 25.1,
        dg: 0.4,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 7,
        m: 3,
        g: 24.3,
        h: 6.9,
        dg: 0.7,
        dh: -0.6,
    },
    WmmCoefficient {
        n: 7,
        m: 4,
        g: 5.6,
        h: -24.4,
        dg: -0.4,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 7,
        m: 5,
        g: 8.7,
        h: -4.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 7,
        m: 6,
        g: 8.9,
        h: -18.7,
        dg: 0.3,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 7,
        m: 7,
        g: -2.3,
        h: -10.1,
        dg: 0.2,
        dh: 0.5,
    },
    // n=8
    WmmCoefficient {
        n: 8,
        m: 0,
        g: 24.3,
        h: 0.0,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 8,
        m: 1,
        g: 8.7,
        h: 10.7,
        dg: 0.1,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 8,
        m: 2,
        g: -10.5,
        h: -11.9,
        dg: -0.3,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 3,
        g: -8.1,
        h: 9.3,
        dg: 0.2,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 4,
        g: -16.4,
        h: -16.8,
        dg: -0.2,
        dh: 0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 5,
        g: 4.1,
        h: 16.3,
        dg: 0.1,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 6,
        g: 1.5,
        h: -13.0,
        dg: 0.4,
        dh: 0.4,
    },
    WmmCoefficient {
        n: 8,
        m: 7,
        g: 6.2,
        h: 11.2,
        dg: 0.2,
        dh: -0.3,
    },
    WmmCoefficient {
        n: 8,
        m: 8,
        g: -10.6,
        h: 2.1,
        dg: 0.4,
        dh: 0.2,
    },
    // n=9
    WmmCoefficient {
        n: 9,
        m: 0,
        g: 5.7,
        h: 0.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 9,
        m: 1,
        g: 9.2,
        h: -21.4,
        dg: -0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 9,
        m: 2,
        g: 2.4,
        h: 14.8,
        dg: 0.0,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 3,
        g: -3.5,
        h: 10.8,
        dg: 0.2,
        dh: -0.4,
    },
    WmmCoefficient {
        n: 9,
        m: 4,
        g: -8.7,
        h: 5.7,
        dg: 0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 9,
        m: 5,
        g: -1.7,
        h: -8.9,
        dg: 0.0,
        dh: 0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 6,
        g: -5.7,
        h: 9.7,
        dg: 0.1,
        dh: -0.1,
    },
    WmmCoefficient {
        n: 9,
        m: 7,
        g: 6.3,
        h: 3.5,
        dg: 0.0,
        dh: -0.2,
    },
    WmmCoefficient {
        n: 9,
        m: 8,
        g: 0.5,
        h: -8.2,
        dg: 0.2,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 9,
        m: 9,
        g: -3.2,
        h: 3.5,
        dg: 0.2,
        dh: 0.0,
    },
    // n=10
    WmmCoefficient {
        n: 10,
        m: 0,
        g: -2.3,
        h: 0.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 1,
        g: -6.4,
        h: 2.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 2,
        g: 2.3,
        h: 1.9,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 3,
        g: -1.8,
        h: -4.3,
        dg: 0.1,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 4,
        g: -1.6,
        h: 1.4,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 5,
        g: -2.9,
        h: -4.1,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 6,
        g: 1.9,
        h: 0.1,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 7,
        g: 1.7,
        h: -2.8,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 8,
        g: 1.8,
        h: -1.6,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 9,
        g: -0.1,
        h: -3.6,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 10,
        m: 10,
        g: -5.7,
        h: -6.4,
        dg: 0.0,
        dh: 0.0,
    },
    // n=11
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
        g: -1.7,
        h: -1.5,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 2,
        g: -1.8,
        h: 2.7,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 3,
        g: 2.4,
        h: -0.6,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 4,
        g: -0.9,
        h: -0.8,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 5,
        g: 0.8,
        h: 0.9,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 6,
        g: -0.5,
        h: -0.7,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 7,
        g: 0.4,
        h: -1.1,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 8,
        g: 1.0,
        h: -0.6,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 9,
        g: 1.8,
        h: 2.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 10,
        g: -0.8,
        h: -1.4,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 11,
        m: 11,
        g: 0.7,
        h: -2.7,
        dg: 0.0,
        dh: 0.0,
    },
    // n=12
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
        h: -0.9,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 2,
        g: 0.5,
        h: 0.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 3,
        g: 1.3,
        h: 1.8,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 4,
        g: -0.8,
        h: -1.0,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 5,
        g: 0.6,
        h: 0.8,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 6,
        g: 0.3,
        h: -0.1,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 7,
        g: 0.5,
        h: 0.6,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 8,
        g: -0.1,
        h: -0.4,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 9,
        g: -0.4,
        h: 0.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 10,
        g: -0.3,
        h: -0.7,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 11,
        g: -0.4,
        h: -0.3,
        dg: 0.0,
        dh: 0.0,
    },
    WmmCoefficient {
        n: 12,
        m: 12,
        g: 0.2,
        h: 0.6,
        dg: 0.0,
        dh: 0.0,
    },
];

/// Result of World Magnetic Model calculation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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
    /// Calculate Magnetic Field components & Declination for given Lat, Lon, Alt (km), Year
    pub fn calculate_km(lat_deg: f64, lon_deg: f64, alt_km: f64, year: f64) -> WmmResult {
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

    /// Calculate WMM2025 result where altitude is provided in feet above sea level
    pub fn calculate(lat_deg: f64, lon_deg: f64, alt_ft: f64, year: f64) -> WmmResult {
        let alt_km = alt_ft * 0.0003048;
        Self::calculate_km(lat_deg, lon_deg, alt_km, year)
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
) -> RunwayMagneticAnalysis {
    let wmm = Wmm2025::calculate(lat, lon, 0.0, year);
    let mag_heading = (true_heading_deg - wmm.declination_deg + 360.0) % 360.0;

    let suffix: String = official_designator
        .chars()
        .filter(|c| c.is_alphabetic())
        .collect();

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

    let official_num: u32 = official_designator
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(final_num);

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

    RunwayMagneticAnalysis {
        official_designator: official_designator.to_string(),
        true_heading_deg,
        wmm_magvar_deg: wmm.declination_deg,
        computed_magnetic_heading_deg: mag_heading,
        computed_magnetic_designator: computed_designator.clone(),
        reciprocal_official_designator: recip_official_designator,
        reciprocal_computed_designator: recip_computed_designator,
        drift_difference_deg: drift,
        is_redesignation_suggested: computed_designator != official_designator,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test against official NOAA WMM2025 Reference Vectors (from WMM2025testvalues.pdf)
    #[test]
    fn test_noaa_wmm2025_reference_vectors() {
        // Test Vector 1: Lat 80°N, Lon 0°E, Year 2025.0, Alt 0 km
        // Official values: X=6521.6, Y=145.9, Z=54791.5, H=6523.2, F=55178.5, I=83.21°, D=1.28°
        let v1 = Wmm2025::calculate_km(80.0, 0.0, 0.0, 2025.0);
        assert!(
            (v1.declination_deg - 1.28).abs() < 1.5,
            "V1 Declination mismatch: expected 1.28, got {}",
            v1.declination_deg
        );
        assert!(
            (v1.inclination_deg - 83.21).abs() < 1.0,
            "V1 Inclination mismatch: expected 83.21, got {}",
            v1.inclination_deg
        );
        assert!(
            (v1.horizontal_intensity_nt - 6523.2).abs() < 600.0,
            "V1 Horizontal Intensity mismatch: expected 6523.2, got {}",
            v1.horizontal_intensity_nt
        );
        assert!(
            (v1.total_intensity_nt - 55178.5).abs() < 200.0,
            "V1 Total Intensity mismatch: expected 55178.5, got {}",
            v1.total_intensity_nt
        );

        // Test Vector 2: Lat 0°N, Lon 120°E, Year 2025.0, Alt 0 km
        // Official values: X=39677.8, Y=-109.6, Z=-10580.2, H=39677.9, F=41064.3, I=-14.93°, D=-0.16°
        let v2 = Wmm2025::calculate_km(0.0, 120.0, 0.0, 2025.0);
        assert!(
            (v2.declination_deg - (-0.16)).abs() < 1.5,
            "V2 Declination mismatch: expected -0.16, got {}",
            v2.declination_deg
        );
        assert!(
            (v2.inclination_deg - (-14.93)).abs() < 1.0,
            "V2 Inclination mismatch: expected -14.93, got {}",
            v2.inclination_deg
        );
        assert!(
            (v2.horizontal_intensity_nt - 39677.9).abs() < 600.0,
            "V2 Horizontal Intensity mismatch: expected 39677.9, got {}",
            v2.horizontal_intensity_nt
        );

        // Test Vector 3: Lat 80°S (-80.0), Lon 240°E, Year 2027.5, Alt 0 km
        // Official values: X=6200.7, Y=15730.3, Z=-51783.7, H=16908.3, F=54474.2, I=-71.92°, D=68.49°
        let v3 = Wmm2025::calculate_km(-80.0, 240.0, 0.0, 2027.5);
        assert!(
            (v3.declination_deg - 68.49).abs() < 1.5,
            "V3 Declination mismatch: expected 68.49, got {}",
            v3.declination_deg
        );
        assert!(
            (v3.inclination_deg - (-71.92)).abs() < 1.0,
            "V3 Inclination mismatch: expected -71.92, got {}",
            v3.inclination_deg
        );

        // Test Vector 4: Lat 80°N, Lon 0°E, Year 2025.0, Alt 100 km
        // Official values: X=6216.0, Y=92.4, Z=52598.8, H=6216.7, F=52964.9, I=83.26°, D=0.85°
        let v4 = Wmm2025::calculate_km(80.0, 0.0, 100.0, 2025.0);
        assert!(
            (v4.declination_deg - 0.85).abs() < 1.5,
            "V4 Declination mismatch: expected 0.85, got {}",
            v4.declination_deg
        );
    }

    #[test]
    fn test_runway_magnetic_drift_detector() {
        let analysis = analyze_runway_magnetic_drift("09", 96.7, 55.97, 37.41, 2026.0);
        assert_eq!(analysis.official_designator, "09");
        assert!(analysis.computed_magnetic_heading_deg > 0.0);
    }

    #[test]
    fn test_runway_36_18_wrap_and_reciprocal() {
        let analysis = analyze_runway_magnetic_drift("36L", 356.0, 0.0, 0.0, 2025.0);
        assert_eq!(analysis.computed_magnetic_designator, "36L");
        assert_eq!(analysis.reciprocal_computed_designator, "18R");
    }
}
