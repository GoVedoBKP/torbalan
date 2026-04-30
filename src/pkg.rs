use std::process::Command;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub origin: String,
    pub comment: String,
}

pub fn list_installed() -> Vec<Package> {
    // pkg query "%n|%v|%o|%c"
    let output = Command::new("pkg")
        .arg("query")
        .arg("%n|%v|%o|%c")
        .output()
        .ok();

    let mut packages = Vec::new();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                packages.push(Package {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    origin: parts[2].to_string(),
                    comment: parts[3].to_string(),
                });
            }
        }
    }
    packages
}

pub fn search(query: &str) -> Vec<Package> {
    let output = Command::new("pkg")
        .arg("search")
        .arg("-Q")
        .arg("%n|%v|%o|%c")
        .arg(query)
        .output()
        .ok();

    let mut packages = Vec::new();
    if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                packages.push(Package {
                    name: parts[0].to_string(),
                    version: parts[1].to_string(),
                    origin: parts[2].to_string(),
                    comment: parts[3].to_string(),
                });
            }
        }
    }
    packages
}
