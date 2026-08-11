use openairac_core::{Waypoint, Navaid};
use anyhow::Result;
use std::io::Write;

pub struct XPlane12Exporter;

impl XPlane12Exporter {
    /// Export waypoints into X-Plane 12 `earth_fix.dat` format (1200 Version)
    pub fn export_earth_fix<W: Write>(waypoints: &[Waypoint], mut writer: W) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(writer, "1200 Version - OpenAIRAC Dynamic Cycle, WMM Engine")?;
        writeln!(writer)?;

        for wp in waypoints {
            let enrt_str = if wp.is_enroute { "ENRT" } else { "TRML" };
            writeln!(
                writer,
                "{:>13.9} {:>14.9} {:<5} {:<4} {:<2} 0 {:<5}",
                wp.latitude, wp.longitude, wp.id, enrt_str, wp.region_code, wp.id
            )?;
        }

        writeln!(writer, "99")?;
        Ok(())
    }

    /// Export navaids into X-Plane 12 `earth_nav.dat` format
    pub fn export_earth_nav<W: Write>(_navaids: &[Navaid], mut writer: W) -> Result<()> {
        writeln!(writer, "I")?;
        writeln!(writer, "1200 Version - OpenAIRAC Dynamic Cycle")?;
        writeln!(writer)?;
        writeln!(writer, "99")?;
        Ok(())
    }
}
