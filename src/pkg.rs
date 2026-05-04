use std::{collections::HashMap, process::Command};

const PKG: &str = "/usr/local/sbin/pkg";

#[derive(Clone, Debug)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub comment: String,
    pub origin: String,   // e.g. "www/curl"  — category is the first component
    pub is_installed: bool,
}

impl PackageEntry {
    pub fn category(&self) -> &str {
        self.origin.split('/').next().unwrap_or("misc")
    }
}

/// All packages from the repository, merged with local install state.
/// Sorted by category then name.
pub fn list_all_packages() -> Vec<PackageEntry> {
    // Collect installed packages into a map keyed by name.
    let installed: HashMap<String, PackageEntry> = query_installed()
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect();

    // Query the full remote catalog.
    let output = Command::new(PKG)
        .args(["rquery", "-a", "%n|%v|%o|%c"])
        .output()
        .ok();

    let mut packages: Vec<PackageEntry> = Vec::new();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() >= 4 {
                let name = parts[0];
                let is_installed = installed.contains_key(name);
                let version = if is_installed {
                    installed[name].version.clone()
                } else {
                    parts[1].to_string()
                };
                packages.push(PackageEntry {
                    name: name.to_string(),
                    version,
                    comment: parts[3].to_string(),
                    origin: parts[2].to_string(),
                    is_installed,
                });
            }
        }
    }

    // Fall back to installed-only if the repo catalog is empty.
    if packages.is_empty() {
        let mut fallback: Vec<PackageEntry> = installed.into_values().collect();
        fallback.sort_by(|a, b| {
            a.origin.cmp(&b.origin).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        return fallback;
    }

    packages.sort_by(|a, b| {
        a.origin.cmp(&b.origin).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    packages
}

/// Installed packages only (used for post-action refresh and search base).
pub fn list_installed() -> Vec<PackageEntry> {
    let mut packages = query_installed();
    packages.sort_by(|a, b| {
        a.origin.cmp(&b.origin).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    packages
}

fn query_installed() -> Vec<PackageEntry> {
    let output = Command::new(PKG)
        .args(["query", "%n|%v|%o|%c"])
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
                    origin: parts[2].to_string(),
                    comment: parts[3].to_string(),
                    is_installed: true,
                });
            }
        }
    }
    packages
}

/// Search the repo catalog for packages matching `query` (case-insensitive
/// substring on name or comment). Returns all matches, merged with installed state.
pub fn search_packages(query: &str) -> Vec<PackageEntry> {
    let q = query.to_lowercase();
    list_all_packages()
        .into_iter()
        .filter(|p| p.name.to_lowercase().contains(&q) || p.comment.to_lowercase().contains(&q))
        .collect()
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
