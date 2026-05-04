slint::include_modules!();

mod auth;
mod sysctl;
mod service;
mod pkg;

use slint::{Model, ModelRc, VecModel, SharedString, Weak};
use std::rc::Rc;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    // ── Login ────────────────────────────────────────────────────────────────
    let ui_handle = ui.as_weak();
    ui.on_login(move |username, password| {
        let ui = ui_handle.unwrap();
        if auth::authenticate(&username, &password) {
            ui.set_logged_in(true);
            load_page_data_async(ui.as_weak(), "sysctl");
        } else {
            eprintln!("Login failed for user: {}", username);
        }
    });

    let ui_handle = ui.as_weak();
    ui.on_change_page(move |page| {
        let ui = ui_handle.unwrap();
        ui.set_active_page(page.clone());
        load_page_data_async(ui.as_weak(), &page);
    });

    // ── Service actions ───────────────
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
                    load_page_data_async(ui.as_weak(), "services");
                }
            });
        });
    });

    // ── Toggle mark (main-thread: access model via ui) ─────────────────────
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

    // ── Per-row install ─────────────────
    let ui_handle = ui.as_weak();
    ui.on_install_package(move |name| {
        if let Some(ui) = ui_handle.upgrade() { ui.set_loading(true); }
        let ui_weak = ui_handle.clone();
        let name_str = name.to_string();
        std::thread::spawn(move || {
            pkg::install(&name_str);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    load_page_data_async(ui.as_weak(), "packages");
                }
            });
        });
    });

    // ── remove Per-row ─────────────────────────────────────
    let ui_handle = ui.as_weak();
    ui.on_remove_package(move |name| {
        if let Some(ui) = ui_handle.upgrade() { ui.set_loading(true); }
        let ui_weak = ui_handle.clone();
        let name_str = name.to_string();
        std::thread::spawn(move || {
            pkg::remove(&name_str);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    load_page_data_async(ui.as_weak(), "packages");
                }
            });
        });
    });

    // ── Batch remove ──────────────────────────────────────────────────────────
    let ui_handle = ui.as_weak();
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
        std::thread::spawn(move || {
            for name in &names { pkg::remove(name); }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    load_page_data_async(ui.as_weak(), "packages");
                }
            });
        });
    });

    // ── install Batch ──────────────────────────────
    let ui_handle = ui.as_weak();
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
        std::thread::spawn(move || {
            for name in &names { pkg::install(name); }
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    load_page_data_async(ui.as_weak(), "packages");
                }
            });
        });
    });

    // ── Search / filter ───────────────────────────────────────────────────────
    let ui_handle = ui.as_weak();
    ui.on_search_packages(move |query| {
        let query_str = query.to_string().to_lowercase();
        let ui_weak = ui_handle.clone();
        if let Some(ui) = ui_weak.upgrade() { ui.set_loading(true); }
        std::thread::spawn(move || {
            let packages = if query_str.is_empty() {
                pkg::list_all_packages()
            } else {
                pkg::search_packages(&query_str)
            };
            let entries = make_slint_entries(packages);
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


fn make_slint_entries(packages: Vec<pkg::PackageEntry>) -> Vec<PackageEntry> {
    // packages must already be sorted by origin (category/name)
    let mut result = Vec::new();
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

fn load_page_data_async(ui_weak: Weak<AppWindow>, page: &str) {
    let page_str = page.to_string();

    if let Some(ui) = ui_weak.upgrade() {
        ui.set_loading(true);
    }

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
        "packages" => {
            let entries = make_slint_entries(pkg::list_all_packages());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_packages(ModelRc::from(Rc::new(VecModel::from(entries))));
                    ui.set_marked_count(0);
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
