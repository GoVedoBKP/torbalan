use std::ptr;
use libc::{c_int, size_t, sysctlbyname};
use std::ffi::CString;

pub struct SysctlInfo {
    pub name: String,
    pub value: String,
    pub description: String,
}

pub fn get_sysctl_value(name: &str) -> Option<String> {
    let c_name = CString::new(name).ok()?;
    let mut size: size_t = 0;

    // First call to get the required buffer size
    unsafe {
        if sysctlbyname(c_name.as_ptr(), ptr::null_mut(), &mut size, ptr::null_mut(), 0) != 0 {
            return None;
        }
    }

    let mut buffer = vec![0u8; size];
    unsafe {
        if sysctlbyname(c_name.as_ptr(), buffer.as_mut_ptr() as *mut _, &mut size, ptr::null_mut(), 0) != 0 {
            return None;
        }
    }

    // Convert buffer to string (handle different types later, for now assume string or int)
    // Simple heuristic: if it looks like a string, treat as string
    String::from_utf8(buffer)
        .ok()
        .map(|s| s.trim_matches(char::from(0)).to_string())
        .or_else(|| {
            // If it's 4 bytes, maybe it's an int?
            // This is a bit simplified
            None
        })
}

pub fn list_sysctls(filter: &str) -> Vec<String> {
    // For now, let's use the command to get the list of names
    // because iterating the MIB tree in pure Rust/libc is quite involved
    let output = match std::process::Command::new("sysctl")
        .arg("-aN")
        .output() {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines()
        .filter(|line| line.contains(filter))
        .map(|s| s.to_string())
        .collect()
}
