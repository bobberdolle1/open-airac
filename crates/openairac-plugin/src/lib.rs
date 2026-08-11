use std::ffi::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;
use std::path::PathBuf;
use std::thread;

/// Native X-Plane 12 Plugin Export - XPluginStart
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStart(
    out_name: *mut c_char,
    out_sig: *mut c_char,
    out_desc: *mut c_char,
) -> c_int {
    unsafe {
        copy_c_str(out_name, "OpenAIRAC Auto-Sync Plugin\0");
        copy_c_str(out_sig, "com.bobberdolle1.openairac\0");
        copy_c_str(out_desc, "Zero-touch, math-driven magnetic navdata sync for X-Plane 12\0");
    }
    1
}

/// Native X-Plane 12 Plugin Export - XPluginStop
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStop() {}

/// Native X-Plane 12 Plugin Export - XPluginEnable
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginEnable() -> c_int {
    // Spawn background zero-touch auto-sync thread on sim launch
    thread::spawn(|| {
        let _ = perform_in_sim_auto_sync();
    });
    1
}

/// Native X-Plane 12 Plugin Export - XPluginDisable
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginDisable() {}

/// Native X-Plane 12 Plugin Export - XPluginReceiveMessage
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginReceiveMessage(
    _in_from_who: c_int,
    _in_message: c_int,
    _in_param: *mut c_void,
) {}

fn perform_in_sim_auto_sync() -> Result<(), Box<dyn std::error::Error>> {
    let custom_data_path = PathBuf::from("Custom Data");
    if !custom_data_path.exists() {
        let _ = std::fs::create_dir_all(&custom_data_path);
    }

    // Blocking HTTP fetch inside plugin background thread
    let navaid_url = "https://davidmegginson.github.io/ourairports-data/navaids.csv";
    let body = reqwest::blocking::get(navaid_url)?.text()?;

    let current_year = 2026.6;
    let navaids = openairac_core::OurAirportsParser::parse_navaids(body.as_bytes(), current_year)?;

    let nav_file = std::fs::File::create(custom_data_path.join("earth_nav.dat"))?;
    openairac_exporter::XPlane12Exporter::export_earth_nav(&navaids, nav_file)?;

    Ok(())
}

unsafe fn copy_c_str(dest: *mut c_char, src: &str) {
    let bytes = src.as_bytes();
    unsafe {
        for (i, &byte) in bytes.iter().enumerate() {
            *dest.add(i) = byte as c_char;
        }
    }
}
