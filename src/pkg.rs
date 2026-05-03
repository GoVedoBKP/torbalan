use std::{collections::HashSet, process::Command};

const PKG: &str = "/usr/local/sbin/pkg";

#[derive(Clone, Debug)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,   // installed version, or available version if not installed
    pub comment: String,
    pub is_installed: bool,
}

pub fn list_installed() -> Vec<PackageEntry> {
    let output = Command::new(PKG)
        .arg("query")
        .arg("%n|%v|%o|%c")
        .output()
        .ok();

    let mut packages = Vec::new();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() >= 4 {
                packages.push(PackageEntry {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    comment: parts[3].to_string(),
                    is_installed: true,
                });
            }
        }
    }
    packages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    packages
}

/// Search the pkg catalog, merging results with installed status.
/// Returns uninstalled catalog hits; installed packages matching the
/// query are filtered on the caller's side from the installed list.
pub fn search_catalog(query: &str) -> Vec<PackageEntry> {
    let installed: HashSet<String> = list_installed()
        .into_iter()
        .map(|p| p.name)
        .collect();

    let output = Command::new(PKG)
        .arg("search")
        .arg("-q")
        .arg(query)
        .output()
        .ok();

    let mut results = Vec::new();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for name_ver in stdout.lines() {
            let (name, version) = split_name_version(name_ver);
            if !installed.contains(name) {
                results.push(PackageEntry {
                    name: name.to_string(),
                    version: version.to_string(),
                    comment: String::new(),
                    is_installed: false,
                });
            }
        }
    }
    results
}

/// Split "name-1.2.3_4" into ("name", "1.2.3_4").
/// The version is the suffix after the last '-' that starts with a digit.
fn split_name_version(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    for i in (0..s.len()).rev() {
        if bytes[i] == b'-' {
            let after = &s[i + 1..];
            if after.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                return (&s[..i], after);
            }
        }
    }
    (s, "")
}

pub fn install(name: &str) -> bool {
    Command::new(PKG)
        .arg("install")
        .arg("-y")
        .arg(name)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn remove(name: &str) -> bool {
    Command::new(PKG)
        .arg("delete")
        .arg("-y")
        .arg(name)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
