slint::include_modules!();

mod auth;
mod sysctl;
mod service;
mod pkg;

use slint::{ModelRc, VecModel, SharedString, Weak};
use std::rc::Rc;
use tokio::runtime::Runtime;

fn main() -> Result<(), slint::PlatformError> {
    std::env::set_var("SLINT_BACKEND", "software");
    let ui = AppWindow::new()?;
    let rt = Runtime::new().unwrap();

    let ui_handle = ui.as_weak();
    ui.on_login(move |username, password| {
        let ui = ui_handle.unwrap();
        if auth::authenticate(&username, &password) {
            println!("Login successful for user: {}", username);
            ui.set_logged_in(true);
            
            // Trigger initial data load
            let ui_weak = ui.as_weak();
            load_page_data_async(ui_weak, "sysctl");
        } else {
            println!("Login failed for user: {}", username);
        }
    });

    let ui_handle = ui.as_weak();
    ui.on_change_page(move |page| {
        let ui = ui_handle.unwrap();
        ui.set_active_page(page.clone());
        load_page_data_async(ui.as_weak(), &page);
    });

    let ui_handle = ui.as_weak();
    ui.on_service_action(move |name, action| {
        let ui = ui_handle.unwrap();
        let ui_weak = ui.as_weak();
        let name_str = name.to_string();
        let action_str = action.to_string();
        
        ui.set_loading(true);
        std::thread::spawn(move || {
            if service::manage_service(&name_str, &action_str) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        load_page_data_async(ui.as_weak(), "services");
                    }
                });
            } else {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_loading(false);
                    }
                });
            }
        });
    });

    ui.run()
}

fn load_page_data_async(ui_weak: Weak<AppWindow>, page: &str) {
    let page_str = page.to_string();
    
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_loading(true);
    }

    std::thread::spawn(move || {
        match page_str.as_str() {
            "sysctl" => {
                let names = sysctl::list_sysctls("vfs.");
                let entries: Vec<SysctlEntry> = names.into_iter().take(100).map(|name| {
                    let value = sysctl::get_sysctl_value(&name).unwrap_or_else(|| "N/A".to_string());
                    SysctlEntry {
                        name: SharedString::from(name),
                        value: SharedString::from(value),
                    }
                }).collect();
                
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let model = Rc::new(VecModel::from(entries));
                        ui.set_sysctls(ModelRc::from(model));
                        ui.set_loading(false);
                    }
                });
            }
            "services" => {
                let services = service::list_services();
                let entries: Vec<ServiceEntry> = services.into_iter().take(100).map(|s| {
                    ServiceEntry {
                        name: SharedString::from(s.name),
                        running: s.running,
                        enabled: s.enabled,
                    }
                }).collect();
                
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let model = Rc::new(VecModel::from(entries));
                        ui.set_services(ModelRc::from(model));
                        ui.set_loading(false);
                    }
                });
            }
            "packages" => {
                let packages = pkg::list_installed();
                let entries: Vec<PackageEntry> = packages.into_iter().take(100).map(|p| {
                    PackageEntry {
                        name: SharedString::from(p.name),
                        version: SharedString::from(p.version),
                        comment: SharedString::from(p.comment),
                    }
                }).collect();
                
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        let model = Rc::new(VecModel::from(entries));
                        ui.set_packages(ModelRc::from(model));
                        ui.set_loading(false);
                    }
                });
            }
            _ => {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_loading(false);
                    }
                });
            }
        }
    });
}
