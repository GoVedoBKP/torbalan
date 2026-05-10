slint::include_modules!();

mod auth;
mod sysctl;
mod service;
mod pkg;
mod users;
mod hardening;

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
            "packages"  => load_page_data_async(ui.as_weak()),
            "users"     => load_users_and_groups(ui.as_weak()),
            "hardening" => load_hardening(ui.as_weak()),
            other       => load_sysctl_or_service(ui.as_weak(), other),
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

    // ── User / Group callbacks ────────────────────────────────────────────────

    // Populate form for a new user
    let ui_handle = ui.as_weak();
    ui.on_new_user_form(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let uid = users::next_uid();
            ui.set_uf_name("".into());
            ui.set_uf_comment("".into());
            ui.set_uf_uid(uid.into());
            ui.set_uf_primary_group("".into());
            ui.set_uf_extra_groups("".into());
            ui.set_uf_home("".into());
            ui.set_uf_shell("/bin/sh".into());
            ui.set_uf_password("".into());
            ui.set_uf_create_home(true);
            ui.set_uf_locked(false);
            ui.set_user_form_is_edit(false);
            ui.set_show_user_form(true);
            ui.set_users_status("".into());
        }
    });

    // Populate form for editing an existing user
    let ui_handle = ui.as_weak();
    ui.on_edit_user_form(move |name| {
        let name_str = name.to_string();
        if let Some(info) = users::get_user(&name_str) {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_uf_name(info.name.into());
                ui.set_uf_comment(info.comment.into());
                ui.set_uf_uid(info.uid.to_string().into());
                ui.set_uf_primary_group(info.primary_group.into());
                ui.set_uf_extra_groups(info.extra_groups.into());
                ui.set_uf_home(info.home.into());
                ui.set_uf_shell(info.shell.into());
                ui.set_uf_password("".into());
                ui.set_uf_create_home(false);
                ui.set_uf_locked(info.locked);
                ui.set_user_form_is_edit(true);
                ui.set_show_user_form(true);
                ui.set_users_status("".into());
            }
        }
    });

    // Save user (add or edit)
    let ui_handle = ui.as_weak();
    ui.on_save_user(move || {
        let Some(ui) = ui_handle.upgrade() else { return };
        let is_edit = ui.get_user_form_is_edit();
        let name = ui.get_uf_name().to_string();
        let comment = ui.get_uf_comment().to_string();
        let uid_str = ui.get_uf_uid().to_string();
        let primary_group = ui.get_uf_primary_group().to_string();
        let extra_groups = ui.get_uf_extra_groups().to_string();
        let home = ui.get_uf_home().to_string();
        let shell = ui.get_uf_shell().to_string();
        let password = ui.get_uf_password().to_string();
        let create_home = ui.get_uf_create_home();
        let locked = ui.get_uf_locked();
        let ui_weak = ui_handle.clone();

        std::thread::spawn(move || {
            let result = if is_edit {
                users::edit_user(users::EditUserParams {
                    name,
                    comment,
                    primary_group,
                    extra_groups,
                    home,
                    shell,
                    password,
                    locked,
                })
            } else {
                users::add_user(users::AddUserParams {
                    name,
                    comment,
                    uid: uid_str,
                    primary_group,
                    extra_groups,
                    home,
                    shell,
                    password,
                    create_home,
                    locked,
                })
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match result {
                        Ok(()) => {
                            ui.set_show_user_form(false);
                            ui.set_users_status("OK".into());
                            load_users_and_groups(ui.as_weak());
                        }
                        Err(e) => {
                            ui.set_users_status(e.into());
                        }
                    }
                }
            });
        });
    });

    // Delete user (confirmed)
    let ui_handle = ui.as_weak();
    ui.on_do_delete_user(move |name| {
        let name_str = name.to_string();
        let ui_weak = ui_handle.clone();
        std::thread::spawn(move || {
            let result = users::delete_user(&name_str, false);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match result {
                        Ok(()) => {
                            ui.set_users_status("OK".into());
                            load_users_and_groups(ui.as_weak());
                        }
                        Err(e) => ui.set_users_status(e.into()),
                    }
                }
            });
        });
    });

    // Populate form for a new group
    let ui_handle = ui.as_weak();
    ui.on_new_group_form(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let gid = users::next_gid();
            ui.set_gf_name("".into());
            ui.set_gf_gid(gid.into());
            ui.set_gf_members("".into());
            ui.set_group_form_is_edit(false);
            ui.set_show_group_form(true);
            ui.set_users_status("".into());
        }
    });

    // Populate form for editing an existing group
    let ui_handle = ui.as_weak();
    ui.on_edit_group_form(move |name| {
        let name_str = name.to_string();
        let found = users::list_groups()
            .into_iter()
            .find(|g| g.name == name_str);
        if let Some(g) = found {
            if let Some(ui) = ui_handle.upgrade() {
                ui.set_gf_name(g.name.into());
                ui.set_gf_gid(g.gid.to_string().into());
                ui.set_gf_members(g.members.into());
                ui.set_group_form_is_edit(true);
                ui.set_show_group_form(true);
                ui.set_users_status("".into());
            }
        }
    });

    // Save group (add or edit)
    let ui_handle = ui.as_weak();
    ui.on_save_group(move || {
        let Some(ui) = ui_handle.upgrade() else { return };
        let is_edit = ui.get_group_form_is_edit();
        let name = ui.get_gf_name().to_string();
        let gid = ui.get_gf_gid().to_string();
        let members = ui.get_gf_members().to_string();
        let ui_weak = ui_handle.clone();

        std::thread::spawn(move || {
            let result = if is_edit {
                users::edit_group(users::EditGroupParams { name, members })
            } else {
                users::add_group(users::AddGroupParams { name, gid, members })
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match result {
                        Ok(()) => {
                            ui.set_show_group_form(false);
                            ui.set_users_status("OK".into());
                            load_users_and_groups(ui.as_weak());
                        }
                        Err(e) => ui.set_users_status(e.into()),
                    }
                }
            });
        });
    });

    // Delete group (confirmed)
    let ui_handle = ui.as_weak();
    ui.on_do_delete_group(move |name| {
        let name_str = name.to_string();
        let ui_weak = ui_handle.clone();
        std::thread::spawn(move || {
            let result = users::delete_group(&name_str);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    match result {
                        Ok(()) => {
                            ui.set_users_status("OK".into());
                            load_users_and_groups(ui.as_weak());
                        }
                        Err(e) => ui.set_users_status(e.into()),
                    }
                }
            });
        });
    });

    // Hardening: apply checked options to system config files.
    let ui_handle = ui.as_weak();
    ui.on_apply_hardening(move || {
        if let Some(ui) = ui_handle.upgrade() {
            let state = hardening::HardeningState {
                hide_uids:       ui.get_hd_hide_uids(),
                hide_gids:       ui.get_hd_hide_gids(),
                hide_jail:       ui.get_hd_hide_jail(),
                read_msgbuf:     ui.get_hd_read_msgbuf(),
                proc_debug:      ui.get_hd_proc_debug(),
                random_pid:      ui.get_hd_random_pid(),
                clear_tmp:       ui.get_hd_clear_tmp(),
                disable_syslogd: ui.get_hd_disable_syslogd(),
                secure_console:  ui.get_hd_secure_console(),
                disable_ddtrace: ui.get_hd_disable_ddtrace(),
            };
            match hardening::apply_state(&state) {
                Ok(()) => ui.set_hardening_status("OK".into()),
                Err(e) => ui.set_hardening_status(e.into()),
            }
        }
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

fn load_users_and_groups(ui_weak: Weak<AppWindow>) {
    if let Some(ui) = ui_weak.upgrade() { ui.set_loading(true); }
    std::thread::spawn(move || {
        let user_entries: Vec<UserEntry> = users::list_users()
            .into_iter()
            .map(|u| UserEntry {
                name: SharedString::from(u.name),
                uid: u.uid as i32,
                primary_group: SharedString::from(u.primary_group),
                home: SharedString::from(u.home),
                shell: SharedString::from(u.shell),
                comment: SharedString::from(u.comment),
                locked: u.locked,
            })
            .collect();

        let group_entries: Vec<GroupEntry> = users::list_groups()
            .into_iter()
            .map(|g| GroupEntry {
                name: SharedString::from(g.name),
                gid: g.gid as i32,
                members: SharedString::from(g.members),
            })
            .collect();

        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_users(ModelRc::from(Rc::new(VecModel::from(user_entries))));
                ui.set_groups(ModelRc::from(Rc::new(VecModel::from(group_entries))));
                ui.set_loading(false);
            }
        });
    });
}

fn load_hardening(ui_weak: Weak<AppWindow>) {
    let state = hardening::read_state();
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_hd_hide_uids(state.hide_uids);
        ui.set_hd_hide_gids(state.hide_gids);
        ui.set_hd_hide_jail(state.hide_jail);
        ui.set_hd_read_msgbuf(state.read_msgbuf);
        ui.set_hd_proc_debug(state.proc_debug);
        ui.set_hd_random_pid(state.random_pid);
        ui.set_hd_clear_tmp(state.clear_tmp);
        ui.set_hd_disable_syslogd(state.disable_syslogd);
        ui.set_hd_secure_console(state.secure_console);
        ui.set_hd_disable_ddtrace(state.disable_ddtrace);
        ui.set_hardening_status("".into());
    }
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
