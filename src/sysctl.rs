use std::process::Command;

pub fn list_sysctl_entries(prefix: &str) -> Vec<(String, String)> {
    let output = match Command::new("/sbin/sysctl").arg(prefix).output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let colon_pos = line.find(':')?;
            let name = line[..colon_pos].trim().to_string();
            let value = line[colon_pos + 1..].trim().to_string();
            if value.is_empty() {
                return None; // skip node-only entries
            }
            Some((name, value))
        })
        .collect()
}
