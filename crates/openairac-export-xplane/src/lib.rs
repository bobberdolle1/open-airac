use anyhow::Result;
use openairac_model::{CanonicalNavaid, CanonicalWaypoint, NavaidKind};
use std::io::Write;

pub struct XPlane12Exporter;

impl XPlane12Exporter {
    /// Export waypoints into X-Plane 12 `earth_fix.dat` format (1200 Version)
    pub fn export_earth_fix<W: Write>(
        waypoints: &[CanonicalWaypoint],
        mut writer: W,
    ) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - OpenAIRAC Canonical World, NOAA WMM2025"
        )?;
        writeln!(writer)?;

        for wp in waypoints {
            let enrt_str = if wp.is_enroute { "ENRT" } else { "TRML" };
            writeln!(
                writer,
                "{:>13.9} {:>14.9} {:<5} {:<4} {:<2} 0 {:<5}",
                wp.latitude, wp.longitude, wp.ident, enrt_str, wp.region_code, wp.ident
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Export navaids into X-Plane 12 `earth_nav.dat` format (1200 Version)
    pub fn export_earth_nav<W: Write>(navaids: &[CanonicalNavaid], mut writer: W) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(
            writer,
            "1200 Version - OpenAIRAC Canonical World, NOAA WMM2025 Engine"
        )?;
        writeln!(writer)?;

        for nav in navaids {
            let type_code = match nav.kind {
                NavaidKind::Vor => 3,
                NavaidKind::Vordme | NavaidKind::Vortac => 13,
                NavaidKind::Ndb => 2,
                NavaidKind::IlsLocalizer => 4,
                NavaidKind::IlsGlidepath => 5,
            };

            let freq = nav.frequency_khz / 10;

            writeln!(
                writer,
                "{:<2} {:>13.9} {:>14.9} {:>6} {:>5} {:>5} {:>6.2} {:<5} {}",
                type_code,
                nav.latitude,
                nav.longitude,
                nav.elevation_ft,
                freq,
                130,
                nav.computed_wmm_magvar_deg,
                nav.ident,
                nav.name
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }
}
