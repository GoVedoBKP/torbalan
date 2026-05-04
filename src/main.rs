slint::include_modules!();

mod auth;
mod sysctl;
mod service;
mod pkg;

use pkg::Catalog;
use slint::{Model, ModelRc, VecModel, SharedString, Weak};
use std::rc::Rc;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // Catalog cache shared between background preload and all pkg callbacks.
    let catalog: Catalog = pkg::new_catalog();

    // Preload full package catalog in the background right away.
    {
        let cache = catalog.clone();
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            pkg::preload_catalog(&cache);
            let total = pkg::total_in_catalog(&cache).unwrap_or(0);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_catalog_total(total as i32);
                    ui.set_catalog_ready(true);
                }
            });
        });
    }

    // Login
    let ui_handle = ui.as_weak();
    ui.on_login(move |username, password| {
        let ui = ui_handle.unwrap();
        if auth::authenticate(&username, &password) {
            ui.set_logged_in(true);
            load_page_data_async(ui.as_weak());
        } else {
            eprintln!("Login failed for user: {}", username);
        }
    });

    // Navigation — sysctl / services reload data; packages resets to installed view
    let ui_handle = ui.as_weak();
    ui.on_change_page(move |page| {
        let ui = ui_handle.unwrap();
        ui.set_active_page(page.clone());
        match page.as_str() {
            "packages" => load_page_data_async(ui.as_weak()),
            other => load_sysctl_or_service(ui.as_weak(), other),
        }
    });

    // Service start/stop/enable/disable
    let ui_handle = ui.as_weak();
    ui.on_service_action(move |name, action| {
        let ui = ui_handle.unwrap();
        ui.set_loading(true);
        let ui_weak = ui.as_weak();
        let name_str = name.to_string();
        let action_str = action.to_string();
        std::thread::spawn(move || {
            service::manage_service(&name_str, &action_str);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    load_sysctl_or_service(ui.as_weak(), "services");
                }
            });
        });
    });

    // Toggle mark (runs on main thread via Slint event)
    let ui_handle = ui.as_weak();
    ui.on_toggle_package_mark(move |name| {
        let ui = match ui_handle.upgrade() { Some(u) => u, None => return };
        let model = ui.get_packages();
        let updated: Vec<PackageEntry> = (0..model.row_count())
            .filter_map(|i| model.row_data(i))
            .map(|mut e| {
                if !e.is_header && e.name == name { e.marked = !e.marked; }
                e
            })
            .collect();
        let count = updated.iter().filter(|e| !e.is_header && e.marked).count();
        ui.set_packages(ModelRc::from(Rc::new(VecModel::from(updated))));
        ui.set_marked_count(count as i32);
    });

    // Per-row install
    let ui_handle = ui.as_weak();
    let catalog_ref = catalog.clone();
    ui.on_install_package(move |name| {
        if let Some(ui) = ui_handle.upgrade() { ui.set_loading(true); }
        let ui_weak = ui_handle.clone();
        let name_str = name.to_string();
        let cache = catalog_ref.clone();
        std::thread::spawn(move || {
            pkg::install(&name_str);
            reload_packages(ui_weak, cache);
        });
    });

    // Per-row remove
    let ui_handle = ui.as_weak();
    let catalog_ref = catalog.clone();
    ui.on_remove_package(move |name| {
        if let Some(ui) = ui_handle.upgrade() { ui.set_loading(true); }
        let ui_weak = ui_handle.clone();
        let name_str = name.to_string();
        let cache = catalog_ref.clone();
        std::thread::spawn(move || {
            pkg::remove(&name_str);
            reload_packages(ui_weak, cache);
        });
    });

    // Batch remove marked installed packages
    let ui_handle = ui.as_weak();
    let catalog_ref = catalog.clone();
    ui.on_remove_marked(move || {
        let ui = match ui_handle.upgrade() { Some(u) => u, None => return };
        let model = ui.get_packages();
        let names: Vec<String> = (0..model.row_count())
            .filter_map(|i| model.row_data(i))
            .filter(|e| !e.is_header && e.marked && e.is_installed)
            .map(|e| e.name.to_string())
            .collect();
        if names.is_empty() { return; }
        ui.set_loading(true);
        let ui_weak = ui_handle.clone();
        let cache = catalog_ref.clone();
        std::thread::spawn(move || {
            for name in &names { pkg::remove(name); }
            reload_packages(ui_weak, cache);
        });
    });

    // Batch install marked available packages
    let ui_handle = ui.as_weak();
    let catalog_ref = catalog.clone();
    ui.on_install_marked(move || {
        let ui = match ui_handle.upgrade() { Some(u) => u, None => return };
        let model = ui.get_packages();
        let names: Vec<String> = (0..model.row_count())
            .filter_map(|i| model.row_data(i))
            .filter(|e| !e.is_header && e.marked && !e.is_installed)
            .map(|e| e.name.to_string())
            .collect();
        if names.is_empty() { return; }
        ui.set_loading(true);
        let ui_weak = ui_handle.clone();
        let cache = catalog_ref.clone();
        std::thread::spawn(move || {
            for name in &names { pkg::install(name); }
            reload_packages(ui_weak, cache);
        });
    });

    // Search / filter — uses cached catalog; falls back to installed if not ready
    let ui_handle = ui.as_weak();
    let catalog_ref = catalog.clone();
    ui.on_search_packages(move |query| {
        let query_str = query.to_string();
        let ui_weak = ui_handle.clone();
        if let Some(ui) = ui_weak.upgrade() { ui.set_loading(true); }
        let cache = catalog_ref.clone();
        std::thread::spawn(move || {
            let pkgs = if query_str.trim().is_empty() {
                pkg::list_installed()
            } else {
                let (results, _total) = pkg::search(&query_str, &cache);
                results
            };
            let entries = group_into_slint_entries(pkgs);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_packages(ModelRc::from(Rc::new(VecModel::from(entries))));
                    ui.set_marked_count(0);
                    ui.set_loading(false);
                }
            });
        });
    });

    ui.run()
}

// Reload installed packages into the model after an install/remove action.
fn reload_packages(ui_weak: Weak<AppWindow>, cache: Catalog) {
    let installed = pkg::list_installed();
    // Refresh install state in cached catalog too (update is_installed flags).
    if let Ok(mut guard) = cache.lock() {
        if let Some(ref mut all) = *guard {
            let inst_names: std::collections::HashSet<String> =
                installed.iter().map(|p| p.name.clone()).collect();
            let inst_versions: std::collections::HashMap<String, String> =
                installed.iter().map(|p| (p.name.clone(), p.version.clone())).collect();
            for p in all.iter_mut() {
                p.is_installed = inst_names.contains(&p.name);
                if p.is_installed {
                    if let Some(v) = inst_versions.get(&p.name) {
                        p.version = v.clone();
                    }
                }
            }
        }
    }
    let entries = group_into_slint_entries(installed);
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_packages(ModelRc::from(Rc::new(VecModel::from(entries))));
            ui.set_marked_count(0);
            ui.set_loading(false);
        }
    });
}

fn group_into_slint_entries(packages: Vec<pkg::PackageEntry>) -> Vec<PackageEntry> {
    let mut result = Vec::with_capacity(packages.len() + 32);
    let mut current_cat = String::new();
    for p in packages {
        let cat = p.category().to_string();
        if cat != current_cat {
            result.push(PackageEntry {
                name: SharedString::from(cat.to_uppercase()),
                version: SharedString::new(),
                comment: SharedString::new(),
                is_installed: false,
                marked: false,
                is_header: true,
            });
            current_cat = cat;
        }
        result.push(PackageEntry {
            name: SharedString::from(&p.name),
            version: SharedString::from(&p.version),
            comment: SharedString::from(&p.comment),
            is_installed: p.is_installed,
            marked: false,
            is_header: false,
        });
    }
    result
}

// Initial packages page load: show installed grouped.
fn load_page_data_async(ui_weak: Weak<AppWindow>) {
    if let Some(ui) = ui_weak.upgrade() { ui.set_loading(true); }
    std::thread::spawn(move || {
        let entries = group_into_slint_entries(pkg::list_installed());
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_packages(ModelRc::from(Rc::new(VecModel::from(entries))));
                ui.set_marked_count(0);
                ui.set_loading(false);
            }
        });
    });
}

fn load_sysctl_or_service(ui_weak: Weak<AppWindow>, page: &str) {
    let page_str = page.to_string();
    if let Some(ui) = ui_weak.upgrade() { ui.set_loading(true); }
    std::thread::spawn(move || match page_str.as_str() {
        "sysctl" => {
            let entries: Vec<SysctlEntry> = sysctl::list_sysctl_entries("vfs")
                .into_iter()
                .take(200)
                .map(|(name, value)| SysctlEntry {
                    name: SharedString::from(name),
                    value: SharedString::from(value),
                })
                .collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_sysctls(ModelRc::from(Rc::new(VecModel::from(entries))));
                    ui.set_loading(false);
                }
            });
        }
        "services" => {
            let entries: Vec<ServiceEntry> = service::list_services()
                .into_iter()
                .map(|s| ServiceEntry {
                    name: SharedString::from(s.name),
                    running: s.running,
                    enabled: s.enabled,
                })
                .collect();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_services(ModelRc::from(Rc::new(VecModel::from(entries))));
                    ui.set_loading(false);
                }
            });
        }
        _ => {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() { ui.set_loading(false); }
            });
        }
    });
}
