use std::{
    collections::HashMap,
    process::Command,
    sync::{Arc, Mutex},
};

const PKG: &str = "/usr/local/sbin/pkg";
const MAX_DISPLAY: usize = 1000;

#[derive(Clone, Debug)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub comment: String,
    pub origin: String,
    pub is_installed: bool,
}

impl PackageEntry {
    pub fn category(&self) -> &str {
        self.origin.split('/').next().unwrap_or("misc")
    }
}

/// Shared catalog cache — populated once in a background thread.
pub type Catalog = Arc<Mutex<Option<Vec<PackageEntry>>>>;

pub fn new_catalog() -> Catalog {
    Arc::new(Mutex::new(None))
}

/// Load the full repo catalog (slow, ~37k packages) and store in the cache.
/// Intended to be called from a background thread at startup.
pub fn preload_catalog(cache: &Catalog) {
    let installed: HashMap<String, PackageEntry> = query_installed()
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect();

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

    packages.sort_by(|a, b| {
        a.origin.cmp(&b.origin)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    *cache.lock().unwrap() = Some(packages);
}

/// Installed packages, sorted by category then name. Fast.
pub fn list_installed() -> Vec<PackageEntry> {
    let mut packages = query_installed();
    packages.sort_by(|a, b| {
        a.origin.cmp(&b.origin)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    packages
}

/// Filter the catalog (or installed list if catalog not ready) by query.
/// Returns at most MAX_DISPLAY results, already sorted by origin/name.
pub fn search(query: &str, catalog: &Catalog) -> (Vec<PackageEntry>, usize) {
    let q = query.to_lowercase();
    let guard = catalog.lock().unwrap();

    let source: &[PackageEntry] = match &*guard {
        Some(all) => all.as_slice(),
        None => return (list_installed_filtered(&q), 0),
    };

    let total = source.len();
    let matches: Vec<PackageEntry> = source
        .iter()
        .filter(|p| {
            p.name.to_lowercase().contains(&q)
                || p.comment.to_lowercase().contains(&q)
        })
        .take(MAX_DISPLAY)
        .cloned()
        .collect();

    (matches, total)
}

fn list_installed_filtered(q: &str) -> Vec<PackageEntry> {
    let mut pkgs = query_installed();
    pkgs.retain(|p| {
        p.name.to_lowercase().contains(q) || p.comment.to_lowercase().contains(q)
    });
    pkgs.sort_by(|a, b| {
        a.origin.cmp(&b.origin)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    pkgs
}

pub fn total_in_catalog(catalog: &Catalog) -> Option<usize> {
    catalog.lock().unwrap().as_ref().map(|v| v.len())
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

pub fn install(name: &str) -> bool {
    Command::new(PKG)
        .args(["install", "-y", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn remove(name: &str) -> bool {
    Command::new(PKG)
        .args(["delete", "-y", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
