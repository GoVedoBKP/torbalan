use std::{
    collections::HashSet,
    io::Write,
    process::{Command, Stdio},
};

const PW: &str = "/usr/sbin/pw";

#[derive(Clone, Debug)]
pub struct UserInfo {
    pub name: String,
    pub uid: u32,
    pub primary_group: String,
    pub extra_groups: String, // comma-separated secondary group names
    pub home: String,
    pub shell: String,
    pub comment: String,
    pub locked: bool,
}

#[derive(Clone, Debug)]
pub struct GroupInfo {
    pub name: String,
    pub gid: u32,
    pub members: String, // comma-separated member usernames
}

pub struct AddUserParams {
    pub name: String,
    pub comment: String,
    pub uid: String,         // empty = auto
    pub primary_group: String, // empty = create matching group
    pub extra_groups: String,
    pub home: String,        // empty = /home/<name>
    pub shell: String,
    pub password: String,    // empty = lock the account
    pub create_home: bool,
    pub locked: bool,
}

pub struct EditUserParams {
    pub name: String,
    pub comment: String,
    pub primary_group: String,
    pub extra_groups: String,
    pub home: String,
    pub shell: String,
    pub password: String, // empty = keep current password
    pub locked: bool,
}

pub struct AddGroupParams {
    pub name: String,
    pub gid: String,     // empty = auto
    pub members: String, // comma-separated
}

pub struct EditGroupParams {
    pub name: String,
    pub members: String,
}

// ── Read helpers ──────────────────────────────────────────────────────────────

fn locked_users() -> HashSet<String> {
    let mut set = HashSet::new();
    let Ok(content) = std::fs::read_to_string("/etc/master.passwd") else {
        return set;
    };
    for line in content.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ':');
        let name = parts.next().unwrap_or("").to_string();
        let pw_field = parts.next().unwrap_or("");
        if pw_field.starts_with("*LOCKED*") {
            set.insert(name);
        }
    }
    set
}

fn group_name_for_gid(gid: u32) -> String {
    let out = Command::new(PW)
        .args(["groupshow", "-g", &gid.to_string()])
        .output()
        .ok();
    out.and_then(|o| {
        let s = String::from_utf8_lossy(&o.stdout).into_owned();
        s.lines()
            .next()
            .and_then(|l| l.split(':').next())
            .map(|n| n.to_string())
    })
    .unwrap_or_else(|| gid.to_string())
}

/// Returns secondary groups a user belongs to (read from /etc/group).
fn extra_groups_for(username: &str, primary_group: &str) -> String {
    let content = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let mut groups = Vec::new();
    for line in content.lines() {
        if line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() < 4 {
            continue;
        }
        let gname = parts[0];
        if gname == primary_group {
            continue; // skip primary group
        }
        let members = parts[3];
        if members.split(',').any(|m| m.trim() == username) {
            groups.push(gname.to_string());
        }
    }
    groups.join(",")
}

fn parse_user_line(line: &str, locked: &HashSet<String>) -> Option<UserInfo> {
    // pw usershow -a format:
    // name:*:uid:gid:class:pw_change:pw_expire:comment:home:shell
    let parts: Vec<&str> = line.split(':').collect();
    if parts.len() < 10 {
        return None;
    }
    let name = parts[0].to_string();
    let uid: u32 = parts[2].parse().ok()?;
    let gid: u32 = parts[3].parse().ok()?;
    let comment = parts[7].to_string();
    let home = parts[8].to_string();
    let shell = parts[9].trim().to_string();
    let primary_group = group_name_for_gid(gid);
    let extra_groups = extra_groups_for(&name, &primary_group);
    let is_locked = locked.contains(&name);
    Some(UserInfo {
        name,
        uid,
        primary_group,
        extra_groups,
        home,
        shell,
        comment,
        locked: is_locked,
    })
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn list_users() -> Vec<UserInfo> {
    let out = Command::new(PW).args(["usershow", "-a"]).output().ok();
    let Some(out) = out else { return vec![] };
    let locked = locked_users();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .filter_map(|l| parse_user_line(l, &locked))
        .collect()
}

pub fn get_user(name: &str) -> Option<UserInfo> {
    let out = Command::new(PW)
        .args(["usershow", "-n", name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let locked = locked_users();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    parse_user_line(stdout.lines().next()?, &locked)
}

pub fn list_groups() -> Vec<GroupInfo> {
    let out = Command::new(PW).args(["groupshow", "-a"]).output().ok();
    let Some(out) = out else { return vec![] };
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, ':').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(GroupInfo {
                name: parts[0].to_string(),
                gid: parts[2].parse().unwrap_or(0),
                members: parts[3].trim().to_string(),
            })
        })
        .collect()
}

pub fn available_shells() -> Vec<String> {
    std::fs::read_to_string("/etc/shells")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

pub fn next_uid() -> String {
    let out = Command::new(PW).arg("usernext").output().ok();
    out.and_then(|o| {
        let s = String::from_utf8_lossy(&o.stdout).into_owned();
        s.trim().split(':').next().map(|n| n.to_string())
    })
    .unwrap_or_default()
}

pub fn next_gid() -> String {
    let out = Command::new(PW).arg("groupnext").output().ok();
    out.and_then(|o| {
        let s = String::from_utf8_lossy(&o.stdout).into_owned();
        s.trim().split(':').next().map(|n| n.to_string())
    })
    .unwrap_or_default()
}

// ── Write operations ──────────────────────────────────────────────────────────

pub fn add_user(p: AddUserParams) -> Result<(), String> {
    let mut args = vec!["useradd".to_string(), "-n".to_string(), p.name.clone()];

    if !p.comment.is_empty() {
        args.extend(["-c".to_string(), p.comment]);
    }
    if !p.uid.is_empty() {
        args.extend(["-u".to_string(), p.uid]);
    }
    if !p.primary_group.is_empty() {
        args.extend(["-g".to_string(), p.primary_group]);
    }
    if !p.extra_groups.is_empty() {
        args.extend(["-G".to_string(), p.extra_groups]);
    }

    // Resolve home directory
    let home = if p.home.is_empty() {
        format!("/home/{}", p.name)
    } else {
        p.home.clone()
    };
    args.extend(["-d".to_string(), home]);

    if !p.shell.is_empty() {
        args.extend(["-s".to_string(), p.shell]);
    }
    if p.create_home {
        args.push("-m".to_string());
    }

    // Password handling: -h 0 reads from stdin, -h -1 locks account
    let has_password = !p.password.is_empty() && !p.locked;
    if has_password {
        args.extend(["-h".to_string(), "0".to_string()]);
    } else {
        args.extend(["-h".to_string(), "-1".to_string()]);
    }

    let mut child = Command::new(PW)
        .args(&args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if has_password {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(p.password.as_bytes());
        }
    }

    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn edit_user(p: EditUserParams) -> Result<(), String> {
    let mut args = vec!["usermod".to_string(), "-n".to_string(), p.name.clone()];

    if !p.comment.is_empty() {
        args.extend(["-c".to_string(), p.comment]);
    }
    if !p.primary_group.is_empty() {
        args.extend(["-g".to_string(), p.primary_group]);
    }
    // Always set extra groups so we can clear them by passing empty string
    args.extend(["-G".to_string(), p.extra_groups.clone()]);

    if !p.home.is_empty() {
        args.extend(["-d".to_string(), p.home]);
    }
    if !p.shell.is_empty() {
        args.extend(["-s".to_string(), p.shell]);
    }

    let has_password = !p.password.is_empty();
    if has_password {
        args.extend(["-h".to_string(), "0".to_string()]);
    }

    let mut child = Command::new(PW)
        .args(&args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    if has_password {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(p.password.as_bytes());
        }
    }

    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }

    // Apply lock / unlock separately
    let lock_cmd = if p.locked { "lock" } else { "unlock" };
    let lock_out = Command::new(PW)
        .args([lock_cmd, "-n", &p.name])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if lock_out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&lock_out.stderr).trim().to_string())
    }
}

pub fn delete_user(name: &str, remove_home: bool) -> Result<(), String> {
    let mut args = vec!["userdel".to_string(), "-n".to_string(), name.to_string()];
    if remove_home {
        args.push("-r".to_string());
    }
    let out = Command::new(PW)
        .args(&args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn add_group(p: AddGroupParams) -> Result<(), String> {
    let mut args = vec!["groupadd".to_string(), "-n".to_string(), p.name];
    if !p.gid.is_empty() {
        args.extend(["-g".to_string(), p.gid]);
    }
    if !p.members.is_empty() {
        args.extend(["-M".to_string(), p.members]);
    }
    let out = Command::new(PW)
        .args(&args)
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn edit_group(p: EditGroupParams) -> Result<(), String> {
    let out = Command::new(PW)
        .args(["groupmod", "-n", &p.name, "-M", &p.members])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn delete_group(name: &str) -> Result<(), String> {
    let out = Command::new(PW)
        .args(["groupdel", "-n", name])
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
