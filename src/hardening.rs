use std::fs;
use std::process::Command;

/// The ten hardening options matching bsdinstall's hardening script.
#[derive(Debug, Clone, Default)]
pub struct HardeningState {
    pub hide_uids: bool,
    pub hide_gids: bool,
    pub hide_jail: bool,
    pub read_msgbuf: bool,
    pub proc_debug: bool,
    pub random_pid: bool,
    pub clear_tmp: bool,
    pub disable_syslogd: bool,
    pub secure_console: bool,
    pub disable_ddtrace: bool,
}

/// Read the current state of each hardening option from system config files.
pub fn read_state() -> HardeningState {
    let sysctl_conf = fs::read_to_string("/etc/sysctl.conf").unwrap_or_default();
    let rc_conf     = fs::read_to_string("/etc/rc.conf").unwrap_or_default();
    let loader_conf = fs::read_to_string("/boot/loader.conf").unwrap_or_default();
    let ttys        = fs::read_to_string("/etc/ttys").unwrap_or_default();

    // Parse uncommented key=value pairs from a conf file.
    fn active_value_is(text: &str, key: &str, expected: &str) -> bool {
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .any(|l| {
                if let Some((k, v)) = l.split_once('=') {
                    k.trim() == key && v.trim().trim_matches('"') == expected.trim_matches('"')
                } else {
                    false
                }
            })
    }

    let sctl = &sysctl_conf;
    let rc   = &rc_conf;
    let ldr  = &loader_conf;

    HardeningState {
        hide_uids:       active_value_is(sctl, "security.bsd.see_other_uids", "0"),
        hide_gids:       active_value_is(sctl, "security.bsd.see_other_gids", "0"),
        hide_jail:       active_value_is(sctl, "security.bsd.see_jail_proc",  "0"),
        read_msgbuf:     active_value_is(sctl, "security.bsd.unprivileged_read_msgbuf", "0"),
        proc_debug:      active_value_is(sctl, "security.bsd.unprivileged_proc_debug", "0"),
        random_pid:      active_value_is(sctl, "kern.randompid", "1"),
        clear_tmp:       active_value_is(rc,   "clear_tmp_enable", "YES"),
        disable_syslogd: active_value_is(rc,   "syslogd_flags", "-ss"),
        secure_console:  ttys.contains("insecure"),
        disable_ddtrace: active_value_is(ldr,  "security.bsd.allow_destructive_dtrace", "0"),
    }
}

/// Apply the desired hardening state, returning Ok or an error string.
pub fn apply_state(desired: &HardeningState) -> Result<(), String> {
    apply_sysctl_entry("security.bsd.see_other_uids",            "0", desired.hide_uids)?;
    apply_sysctl_entry("security.bsd.see_other_gids",            "0", desired.hide_gids)?;
    apply_sysctl_entry("security.bsd.see_jail_proc",             "0", desired.hide_jail)?;
    apply_sysctl_entry("security.bsd.unprivileged_read_msgbuf",  "0", desired.read_msgbuf)?;
    apply_sysctl_entry("security.bsd.unprivileged_proc_debug",   "0", desired.proc_debug)?;
    apply_sysctl_entry("kern.randompid",                         "1", desired.random_pid)?;
    apply_rc_conf_entry("clear_tmp_enable",  "YES",  desired.clear_tmp)?;
    apply_rc_conf_entry("syslogd_flags",     "-ss",  desired.disable_syslogd)?;
    apply_secure_console(desired.secure_console)?;
    apply_loader_conf_entry("security.bsd.allow_destructive_dtrace", "0", desired.disable_ddtrace)?;
    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Set or remove a key in /etc/sysctl.conf.
/// If `enabled`, ensures `key=value` is present (uncommented).
/// If disabled, removes or comments out the line.
fn apply_sysctl_entry(key: &str, value: &str, enabled: bool) -> Result<(), String> {
    let path = "/etc/sysctl.conf";
    let content = fs::read_to_string(path).unwrap_or_default();
    let entry = format!("{}={}", key, value);
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Find any existing line that references this key (commented or not).
    let existing_idx = lines.iter().position(|l| {
        let stripped = l.trim_start_matches('#').trim();
        stripped.starts_with(key) && stripped.contains('=')
    });

    if enabled {
        match existing_idx {
            Some(i) => lines[i] = entry.clone(),
            None    => lines.push(entry.clone()),
        }
    } else if let Some(i) = existing_idx {
        // Comment it out.
        lines[i] = format!("#{}", lines[i].trim_start_matches('#'));
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') { out.push('\n'); }
    fs::write(path, out).map_err(|e| format!("sysctl.conf write: {e}"))?;

    // Apply the setting live (best-effort, may fail for some keys).
    if enabled {
        let _ = Command::new("sysctl").arg(format!("{key}={value}")).output();
    }
    Ok(())
}

/// Set or remove a key in /etc/rc.conf using sysrc.
fn apply_rc_conf_entry(key: &str, value: &str, enabled: bool) -> Result<(), String> {
    if enabled {
        let out = Command::new("sysrc")
            .arg(format!("{key}={value}"))
            .output()
            .map_err(|e| format!("sysrc: {e}"))?;
        if !out.status.success() {
            return Err(format!("sysrc {key}: {}", String::from_utf8_lossy(&out.stderr)));
        }
    } else {
        // Remove the key; ignore errors if key wasn't set.
        let _ = Command::new("sysrc").arg("-x").arg(key).output();
    }
    Ok(())
}

/// Set or remove a key in /boot/loader.conf.
fn apply_loader_conf_entry(key: &str, value: &str, enabled: bool) -> Result<(), String> {
    let path = "/boot/loader.conf";
    let content = fs::read_to_string(path).unwrap_or_default();
    let entry = format!("{key}=\"{value}\"");
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let existing_idx = lines.iter().position(|l| {
        let stripped = l.trim_start_matches('#').trim();
        stripped.starts_with(key) && stripped.contains('=')
    });

    if enabled {
        match existing_idx {
            Some(i) => lines[i] = entry.clone(),
            None    => lines.push(entry.clone()),
        }
    } else if let Some(i) = existing_idx {
        lines[i] = format!("#{}", lines[i].trim_start_matches('#'));
    }

    let mut out = lines.join("\n");
    if !out.ends_with('\n') { out.push('\n'); }
    fs::write(path, out).map_err(|e| format!("loader.conf write: {e}"))?;
    Ok(())
}

/// Enable or disable "secure console" by toggling /etc/ttys terminal mode.
/// bsdinstall sets all `off secure` → `off insecure` to require a password
/// at the console before dropping to single-user mode.
fn apply_secure_console(enabled: bool) -> Result<(), String> {
    let path = "/etc/ttys";
    let content = fs::read_to_string(path).map_err(|e| format!("ttys read: {e}"))?;

    let new_content = if enabled {
        // "off secure" → "off insecure"
        content.replace("\toff secure", "\toff insecure")
               .replace(" off secure",  " off insecure")
    } else {
        // "off insecure" → "off secure"
        content.replace("\toff insecure", "\toff secure")
               .replace(" off insecure",  " off secure")
    };

    fs::write(path, new_content).map_err(|e| format!("ttys write: {e}"))?;
    Ok(())
}
