#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, c_char};
use std::os::raw::{c_int, c_void};
use std::path::Path;

/// Native X-Plane 12 Plugin Export - XPluginStart
///
/// # Safety
/// Dest pointers must be valid C string pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStart(
    out_name: *mut c_char,
    out_sig: *mut c_char,
    out_desc: *mut c_char,
) -> c_int {
    unsafe {
        copy_c_str(out_name, "OpenAIRAC Status Bridge Plugin\0");
        copy_c_str(out_sig, "com.bobberdolle1.openairac\0");
        copy_c_str(
            out_desc,
            "OpenAIRAC status & local SQLite telemetry bridge for X-Plane 12\0",
        );
    }
    1
}

/// Native X-Plane 12 Plugin Export - XPluginStop
///
/// # Safety
/// X-Plane plugin API callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStop() {}

/// Native X-Plane 12 Plugin Export - XPluginEnable
///
/// # Safety
/// X-Plane plugin API callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginEnable() -> c_int {
    1
}

/// Native X-Plane 12 Plugin Export - XPluginDisable
///
/// # Safety
/// X-Plane plugin API callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginDisable() {}

/// Native X-Plane 12 Plugin Export - XPluginReceiveMessage
///
/// # Safety
/// X-Plane plugin API callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginReceiveMessage(
    _in_from_who: c_int,
    _in_message: c_int,
    _in_param: *mut c_void,
) {
}

/// C-ABI Bridge: Query local OpenAIRAC SQLite database status without simulator launch mutation
///
/// # Safety
/// db_path_c must be a valid C string pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn OpenAIRAC_QueryWorldStatus(
    db_path_c: *const c_char,
    out_airports: *mut c_int,
    out_navaids: *mut c_int,
) -> c_int {
    if db_path_c.is_null() {
        return 0;
    }
    let c_str = unsafe { CStr::from_ptr(db_path_c) };
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let path = Path::new(path_str);
    if !path.exists() {
        return 0;
    }

    match openairac_store::WorldStore::open(path) {
        Ok(store) => match store.status() {
            Ok(status) => {
                if !out_airports.is_null() {
                    unsafe { *out_airports = status.total_airports as c_int };
                }
                if !out_navaids.is_null() {
                    unsafe { *out_navaids = status.total_navaids as c_int };
                }
                1
            }
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

unsafe fn copy_c_str(dest: *mut c_char, src: &str) {
    let bytes = src.as_bytes();
    unsafe {
        for (i, &byte) in bytes.iter().enumerate() {
            *dest.add(i) = byte as c_char;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_query_world_status_null() {
        let res = unsafe {
            OpenAIRAC_QueryWorldStatus(std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut())
        };
        assert_eq!(res, 0);
    }
}
