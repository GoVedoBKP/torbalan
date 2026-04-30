use std::process::Command;

#[derive(Clone, Debug)]
pub struct ServiceInfo {
    pub name: String,
    pub running: bool,
    pub enabled: bool,
    pub description: String,
}

pub fn list_services() -> Vec<ServiceInfo> {
    let mut services = Vec::new();

    // List all services
    let output = Command::new("service").arg("-l").output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for name in stdout.lines() {
            services.push(ServiceInfo {
                name: name.to_string(),
                running: false, // Will fill later
                enabled: false, // Will fill later
                description: String::new(),
            });
        }
    }

    // Check enabled services
    let output = Command::new("service").arg("-e").output().ok();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let enabled_names: Vec<&str> = stdout.lines()
            .map(|line| line.split('/').last().unwrap_or(""))
            .collect();
        
        for s in &mut services {
            if enabled_names.contains(&s.name.as_str()) {
                s.enabled = true;
                // Check if running only if enabled (optimization)
                let status = Command::new("service").arg(&s.name).arg("status").output().ok();
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
        "enable" => Command::new("sysrc").arg(format!("{}_enable=YES", name)).status(),
        "disable" => Command::new("sysrc").arg(format!("{}_enable=NO", name)).status(),
        _ => Command::new("service").arg(name).arg(action).status(),
    };

    status.map(|s| s.success()).unwrap_or(false)
}
