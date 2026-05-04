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

fn running_process_names() -> HashSet<String> {
    let out = Command::new("/bin/ps")
        .args(["-ax", "-o", "comm="])
        .output()
        .ok();
    out.map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn is_running(name: &str, procs: &HashSet<String>) -> bool {
    // Exact match
    if procs.contains(name) {
        return true;
    }
    // Hyphenated daemon variant: "dbus" → "dbus-daemon", "dbus-launch"
    if procs.iter().any(|p| p.starts_with(&format!("{}-", name))) {
        return true;
    }
    // "-d" suffix variant: "cron" → "crond" (less common on FreeBSD but handled)
    if procs.contains(&format!("{}d", name)) {
        return true;
    }
    false
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

    // Get all running process names in one call (avoids pidfile permission issues)
    let procs = running_process_names();

    let output = Command::new(SERVICE).arg("-e").output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let enabled: HashSet<String> = stdout
            .lines()
            .filter_map(|line| {
                let base = line.trim().split('/').last()?;
                if base.is_empty() { None } else { Some(base.to_string()) }
            })
            .collect();

        for s in &mut services {
            if enabled.contains(&s.name) {
                s.enabled = true;
            }
            s.running = is_running(&s.name, &procs);
        }
    } else {
        // Even without enabled list, still fill running state
        for s in &mut services {
            s.running = is_running(&s.name, &procs);
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
