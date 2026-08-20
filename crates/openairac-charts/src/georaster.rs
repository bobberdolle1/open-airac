//! Geospatial Raster & Affine Georeferencing Engine.
//!
//! Provides metadata representations, 6-parameter affine coordinate transformations,
//! pixel-to-geographic projection mapping, round-trip validation, and GeoTIFF contracts.

use crate::model::GeoreferenceStatus;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Geographic bounding box in decimal degrees WGS84.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoBounds {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl GeoBounds {
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Result<Self> {
        if min_lon < -180.0 || max_lon > 180.0 || min_lat < -90.0 || max_lat > 90.0 {
            bail!(
                "geographic bounds outside valid WGS84 range: [{min_lon}, {min_lat}, {max_lon}, {max_lat}]"
            );
        }
        if min_lon > max_lon || min_lat > max_lat {
            bail!("inverted bounds: min must be <= max");
        }
        Ok(Self {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        })
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min_lon && lon <= self.max_lon && lat >= self.min_lat && lat <= self.max_lat
    }
}

/// 6-parameter 2D affine transformation matrix for raster georeferencing.
///
/// Maps raster pixel coordinates `(pixel_x, pixel_y)` to geographic coordinates:
/// ```text
/// X_geo = a * px + b * py + c
/// Y_geo = d * px + e * py + f
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AffineTransform {
    /// Pixel width / scale in X (degrees or projection units per pixel)
    pub a: f64,
    /// Rotation / shearing parameter in X
    pub b: f64,
    /// Origin X coordinate (top-left pixel center/corner)
    pub c: f64,
    /// Rotation / shearing parameter in Y
    pub d: f64,
    /// Pixel height / scale in Y (typically negative for north-up rasters)
    pub e: f64,
    /// Origin Y coordinate (top-left pixel center/corner)
    pub f: f64,
}

impl AffineTransform {
    pub fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Result<Self> {
        let det = a * e - b * d;
        if det.abs() < 1e-18 || det.is_nan() || det.is_infinite() {
            bail!("singular or non-invertible affine transform (determinant: {det})");
        }
        Ok(Self { a, b, c, d, e, f })
    }

    /// Calculate forward transformation: pixel `(px, py)` -> geographic `(lon, lat)`
    pub fn pixel_to_geo(&self, px: f64, py: f64) -> (f64, f64) {
        let x_geo = self.a * px + self.b * py + self.c;
        let y_geo = self.d * px + self.e * py + self.f;
        (x_geo, y_geo)
    }

    /// Calculate inverse transformation: geographic `(lon, lat)` -> pixel `(px, py)`
    pub fn geo_to_pixel(&self, lon: f64, lat: f64) -> Result<(f64, f64)> {
        let det = self.a * self.e - self.b * self.d;
        if det.abs() < 1e-18 {
            bail!("cannot invert singular affine transform");
        }

        let dx = lon - self.c;
        let dy = lat - self.f;

        let px = (self.e * dx - self.b * dy) / det;
        let py = (-self.d * dx + self.a * dy) / det;

        if px.is_nan() || py.is_nan() || px.is_infinite() || py.is_infinite() {
            bail!("computed pixel coordinates are invalid/non-finite");
        }

        Ok((px, py))
    }

    /// Validate mathematical invertible round-trip consistency.
    pub fn validate_round_trip(&self, test_pixels: &[(f64, f64)], tolerance_px: f64) -> Result<()> {
        for &(px, py) in test_pixels {
            let (lon, lat) = self.pixel_to_geo(px, py);
            let (inv_px, inv_py) = self
                .geo_to_pixel(lon, lat)
                .with_context(|| format!("inverting point ({px}, {py}) -> ({lon}, {lat})"))?;

            let err = ((inv_px - px).powi(2) + (inv_py - py).powi(2)).sqrt();
            if err > tolerance_px {
                bail!(
                    "affine transform round-trip error {err:.4} px exceeds tolerance {tolerance_px} px at ({px}, {py})"
                );
            }
        }
        Ok(())
    }
}

/// Metadata and georeferencing registration for raster aviation chart products.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoRasterAsset {
    pub id: String,
    pub provider: String,
    pub product_name: String,
    pub edition: String,
    pub effective_from: DateTime<Utc>,
    pub effective_to: Option<DateTime<Utc>>,
    pub crs_epsg: u32,
    pub bounds: GeoBounds,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub affine_transform: AffineTransform,
    pub sha256_hash: String,
    pub status: GeoreferenceStatus,
    pub source_url: Option<String>,
}

impl GeoRasterAsset {
    /// Verify that an aircraft or position is covered by this georeferenced raster.
    pub fn covers_position(&self, lon: f64, lat: f64) -> bool {
        self.bounds.contains(lon, lat)
    }

    /// Compute pixel location of an aircraft position if covered.
    pub fn ownship_pixel_position(&self, lon: f64, lat: f64) -> Option<(f64, f64)> {
        if !self.covers_position(lon, lat) || self.status != GeoreferenceStatus::Georeferenced {
            return None;
        }

        if let Ok((px, py)) = self.affine_transform.geo_to_pixel(lon, lat)
            && px >= 0.0
            && px <= self.pixel_width as f64
            && py >= 0.0
            && py <= self.pixel_height as f64
        {
            return Some((px, py));
        }
        None
    }
}
