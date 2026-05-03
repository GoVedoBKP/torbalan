use std::{collections::HashSet, process::Command};

const SERVICE: &str = "/usr/sbin/service";
const SYSRC: &str = "/usr/sbin/sysrc";

#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub running: bool,
    pub enabled: bool,
    pub description: String,
}

pub fn list_services() -> Vec<ServiceInfo> {
    let mut services = Vec::new();

    let output = Command::new(SERVICE).arg("-l").output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for name in stdout.lines() {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            // Skip ALL-CAPS category headers (DAEMON, FILESYSTEMS, …)
            if name.chars().all(|c| c.is_uppercase() || c == '_') {
                continue;
            }
            services.push(ServiceInfo {
                name: name.to_string(),
                running: false,
                enabled: false,
                description: String::new(),
            });
        }
    }

    let output = Command::new(SERVICE).arg("-e").output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let enabled: HashSet<&str> = stdout
            .lines()
            .filter_map(|line| line.trim().split('/').last())
            .collect();

        for s in &mut services {
            if enabled.contains(s.name.as_str()) {
                s.enabled = true;
                let status = Command::new(SERVICE)
                    .arg(&s.name)
                    .arg("status")
                    .output()
                    .ok();
                if let Some(st) = status {
                    s.running = st.status.success();
                }
            }
        }
    }

    services
}

pub fn manage_service(name: &str, action: &str) -> bool {
    let status = match action {
        "enable" => Command::new(SYSRC).arg(format!("{}_enable=YES", name)).status(),
        "disable" => Command::new(SYSRC).arg(format!("{}_enable=NO", name)).status(),
        _ => Command::new(SERVICE).arg(name).arg(action).status(),
    };
    status.map(|s| s.success()).unwrap_or(false)
}
