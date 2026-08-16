use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Package source: either a directory or a zip file.
pub enum PackageSource {
    Directory(PathBuf),
    Zip(PathBuf),
}

impl PackageSource {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let p = path.as_ref().to_path_buf();
        if p.is_dir() {
            Ok(PackageSource::Directory(p))
        } else if p.is_file() {
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext.eq_ignore_ascii_case("zip") {
                Ok(PackageSource::Zip(p))
            } else {
                bail!("path is a file but not a .zip: {:?}", p);
            }
        } else {
            bail!("path does not exist: {:?}", p);
        }
    }

    /// Read an entire file as a string if present.
    pub fn read_file(&self, relative_path: &str) -> Result<Option<String>> {
        let normalized = relative_path.replace('\\', "/");
        match self {
            PackageSource::Directory(base) => {
                let p = base.join(&normalized);
                if !p.is_file() {
                    return Ok(None);
                }
                let content =
                    std::fs::read_to_string(&p).with_context(|| format!("reading {:?}", p))?;
                Ok(Some(content))
            }
            PackageSource::Zip(zip_path) => {
                let file = File::open(zip_path)?;
                let mut archive = ZipArchive::new(file)?;
                // Try exact match or case-insensitive match
                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i)?;
                    let name = entry.name().replace('\\', "/");
                    if name.eq_ignore_ascii_case(&normalized) {
                        let mut buf = String::new();
                        entry.read_to_string(&mut buf)?;
                        return Ok(Some(buf));
                    }
                }
                Ok(None)
            }
        }
    }

    /// List all CIFP procedure files in the package.
    pub fn list_cifp_files(&self) -> Result<Vec<String>> {
        let mut list = Vec::new();
        match self {
            PackageSource::Directory(base) => {
                let cifp_dir = base.join("CIFP");
                if cifp_dir.is_dir() {
                    for entry in std::fs::read_dir(&cifp_dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_file()
                            && let Some(name) = path.file_name().and_then(|s| s.to_str())
                        {
                            list.push(format!("CIFP/{}", name));
                        }
                    }
                }
            }
            PackageSource::Zip(zip_path) => {
                let file = File::open(zip_path)?;
                let mut archive = ZipArchive::new(file)?;
                for i in 0..archive.len() {
                    let entry = archive.by_index(i)?;
                    let name = entry.name().replace('\\', "/");
                    if (name.starts_with("CIFP/") || name.starts_with("cifp/"))
                        && name.ends_with(".dat")
                    {
                        list.push(name);
                    }
                }
            }
        }
        list.sort();
        Ok(list)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FixRecord {
    pub latitude: f64,
    pub longitude: f64,
    pub ident: String,
    pub terminal_area: String,
    pub region: String,
    pub waypoint_type: Option<u32>,
    pub name: String,
    pub raw_line: String,
}

pub fn parse_earth_fix(content: &str) -> Vec<FixRecord> {
    let mut fixes = Vec::new();
    let mut lines = content.lines();
    // Skip 2 header lines
    for line in lines.by_ref().take(2) {
        let _ = line;
    }
    for line in lines {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed == "99" {
            continue;
        }
        let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let Ok(lat) = parts[0].parse::<f64>() else {
            continue;
        };
        let Ok(lon) = parts[1].parse::<f64>() else {
            continue;
        };
        let ident = parts[2].to_string();
        let terminal_area = parts[3].to_string();
        let region = parts[4].to_string();
        let waypoint_type = parts.get(5).and_then(|s| s.parse::<u32>().ok());
        let name = if parts.len() > 6 {
            parts[6..].join(" ")
        } else {
            ident.clone()
        };

        fixes.push(FixRecord {
            latitude: lat,
            longitude: lon,
            ident,
            terminal_area,
            region,
            waypoint_type,
            name,
            raw_line: line.to_string(),
        });
    }
    fixes
}

#[derive(Debug, Clone, PartialEq)]
pub struct NavRecord {
    pub row_code: u8,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_ft: i32,
    pub frequency_raw: i32,
    pub range_nm: f64,
    pub bearing_or_var: f64,
    pub ident: String,
    pub airport_or_enrt: String,
    pub region: String,
    pub runway_or_name: String,
    pub raw_line: String,
}

pub fn parse_earth_nav(content: &str) -> Vec<NavRecord> {
    let mut navaids = Vec::new();
    let mut lines = content.lines();
    for line in lines.by_ref().take(2) {
        let _ = line;
    }
    for line in lines {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed == "99" {
            continue;
        }
        let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        let Ok(row_code) = parts[0].parse::<u8>() else {
            continue;
        };
        let Ok(lat) = parts[1].parse::<f64>() else {
            continue;
        };
        let Ok(lon) = parts[2].parse::<f64>() else {
            continue;
        };
        let Ok(elevation_ft) = parts[3].parse::<i32>() else {
            continue;
        };
        let Ok(frequency_raw) = parts[4].parse::<i32>() else {
            continue;
        };
        let Ok(range_nm) = parts[5].parse::<f64>() else {
            continue;
        };
        let Ok(bearing_or_var) = parts[6].parse::<f64>() else {
            continue;
        };
        let ident = parts[7].to_string();
        let airport_or_enrt = parts[8].to_string();
        let region = parts[9].to_string();
        let runway_or_name = if parts.len() > 10 {
            parts[10..].join(" ")
        } else {
            String::new()
        };

        navaids.push(NavRecord {
            row_code,
            latitude: lat,
            longitude: lon,
            elevation_ft,
            frequency_raw,
            range_nm,
            bearing_or_var,
            ident,
            airport_or_enrt,
            region,
            runway_or_name,
            raw_line: line.to_string(),
        });
    }
    navaids
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirwayRecord {
    pub start_ident: String,
    pub start_region: String,
    pub start_type: u8,
    pub end_ident: String,
    pub end_region: String,
    pub end_type: u8,
    pub direction: char,
    pub level: char,
    pub base_fl: u32,
    pub top_fl: u32,
    pub names: Vec<String>,
    pub raw_line: String,
}

pub fn parse_earth_awy(content: &str) -> Vec<AirwayRecord> {
    let mut airways = Vec::new();
    let mut lines = content.lines();
    for line in lines.by_ref().take(2) {
        let _ = line;
    }
    for line in lines {
        let line_trimmed = line.trim();
        if line_trimmed.is_empty() || line_trimmed == "99" {
            continue;
        }
        let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
        if parts.len() < 11 {
            continue;
        }
        let start_ident = parts[0].to_string();
        let start_region = parts[1].to_string();
        let Ok(start_type) = parts[2].parse::<u8>() else {
            continue;
        };
        let end_ident = parts[3].to_string();
        let end_region = parts[4].to_string();
        let Ok(end_type) = parts[5].parse::<u8>() else {
            continue;
        };
        let direction = parts[6].chars().next().unwrap_or('1');
        let level_char = parts[7].chars().next().unwrap_or('1');
        let level = if level_char == '2' || level_char == 'H' {
            'H'
        } else {
            'L'
        };
        let Ok(base_fl) = parts[8].parse::<u32>() else {
            continue;
        };
        let Ok(top_fl) = parts[9].parse::<u32>() else {
            continue;
        };
        let raw_names = parts[10..].join(" ");
        let names: Vec<String> = raw_names
            .split('-')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        airways.push(AirwayRecord {
            start_ident,
            start_region,
            start_type,
            end_ident,
            end_region,
            end_type,
            direction,
            level,
            base_fl,
            top_fl,
            names,
            raw_line: line.to_string(),
        });
    }
    airways
}

#[derive(Debug, Clone, PartialEq)]
pub struct CifpProcedureLeg {
    pub raw_line: String,
    pub record_type: String, // SID, STAR, APPCH, RWY, PRDAT
    pub seq: String,
    pub trans_type: String,
    pub proc_ident: String,
    pub transition_ident: String,
    pub fix_ident: String,
    pub fix_region: String,
    pub path_terminator: String,
    pub turn_direction: String,
    pub rnp: String,
    pub alt_desc: String,
    pub alt1: String,
    pub alt2: String,
    pub speed_limit: String,
    pub vert_angle: String,
}

pub fn parse_cifp_file(content: &str) -> Vec<CifpProcedureLeg> {
    let mut legs = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (prefix, body) = match trimmed.split_once(':') {
            Some((p, b)) => (p.trim(), b.trim_end_matches(';')),
            None => continue,
        };
        let fields: Vec<&str> = body.split(',').map(|s| s.trim()).collect();

        let seq = fields.first().copied().unwrap_or("").to_string();
        let trans_type = fields.get(1).copied().unwrap_or("").to_string();
        let proc_ident = fields.get(2).copied().unwrap_or("").to_string();
        let transition_ident = fields.get(3).copied().unwrap_or("").to_string();
        let fix_ident = fields.get(4).copied().unwrap_or("").to_string();
        let fix_region = fields.get(5).copied().unwrap_or("").to_string();
        let path_terminator = fields.get(11).copied().unwrap_or("").to_string();
        let turn_direction = fields.get(9).copied().unwrap_or("").to_string();
        let rnp = fields.get(10).copied().unwrap_or("").to_string();
        let alt_desc = fields.get(21).copied().unwrap_or("").to_string();
        let alt1 = fields.get(22).copied().unwrap_or("").to_string();
        let alt2 = fields.get(23).copied().unwrap_or("").to_string();
        let speed_limit = fields.get(27).copied().unwrap_or("").to_string();
        let vert_angle = fields.get(28).copied().unwrap_or("").to_string();

        legs.push(CifpProcedureLeg {
            raw_line: line.to_string(),
            record_type: prefix.to_string(),
            seq,
            trans_type,
            proc_ident,
            transition_ident,
            fix_ident,
            fix_region,
            path_terminator,
            turn_direction,
            rnp,
            alt_desc,
            alt1,
            alt2,
            speed_limit,
            vert_angle,
        });
    }
    legs
}
