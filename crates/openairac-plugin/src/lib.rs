use std::ffi::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;

/// Native X-Plane 12 Plugin Export - XPluginStart
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStart(
    out_name: *mut c_char,
    out_sig: *mut c_char,
    out_desc: *mut c_char,
) -> c_int {
    unsafe {
        copy_c_str(out_name, "OpenAIRAC Status Bridge Plugin\0");
        copy_c_str(out_sig, "com.bobberdolle1.openairac\0");
        copy_c_str(out_desc, "OpenAIRAC status & telemetry bridge for X-Plane 12\0");
    }
    1
}

/// Native X-Plane 12 Plugin Export - XPluginStop
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginStop() {}

/// Native X-Plane 12 Plugin Export - XPluginEnable
#[unsafe(no_mangle)]
pub unsafe extern "C" fn XPluginEnable() -> c_int {
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

unsafe fn copy_c_str(dest: *mut c_char, src: &str) {
    let bytes = src.as_bytes();
    unsafe {
        for (i, &byte) in bytes.iter().enumerate() {
            *dest.add(i) = byte as c_char;
        }
    }
}
