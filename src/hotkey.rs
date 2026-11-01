#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(target_os = "linux")]
use std::process::Command;

pub const KEYBINDING_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/lightshot/";
pub const BINDING: &str = "<Ctrl>Print";
const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const LIST_KEY: &str = "custom-keybindings";

#[cfg(target_os = "linux")]
fn custom_schema() -> String {
    format!("{MEDIA_KEYS}.custom-keybinding:{KEYBINDING_PATH}")
}

#[cfg(target_os = "linux")]
fn gsettings(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|e| format!("gsettings: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "gsettings {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn parse_list(s: &str) -> Vec<String> {
    let s = s.trim();
    let s = s.strip_prefix("@as").unwrap_or(s).trim();
    let inner = s
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(s);
    inner
        .split(',')
        .map(|e| e.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

pub fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        return "@as []".into();
    }
    let quoted: Vec<String> = items.iter().map(|i| gvariant_str(i)).collect();
    format!("[{}]", quoted.join(", "))
}

fn gvariant_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(target_os = "linux")]
fn shell_quote(s: &str) -> String {
    let plain = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain && !s.is_empty() {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(target_os = "linux")]
fn read_list() -> Result<Vec<String>, String> {
    Ok(parse_list(&gsettings(&["get", MEDIA_KEYS, LIST_KEY])?))
}

#[cfg(target_os = "linux")]
fn write_list(items: &[String]) -> Result<(), String> {
    gsettings(&["set", MEDIA_KEYS, LIST_KEY, &format_list(items)]).map(drop)
}

#[cfg(target_os = "linux")]
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let command = format!("{} capture", shell_quote(&exe.to_string_lossy()));
    let mut list = read_list()?;
    if !list.iter().any(|p| p == KEYBINDING_PATH) {
        list.push(KEYBINDING_PATH.to_string());
    }
    write_list(&list)?;
    let schema = custom_schema();
    gsettings(&["set", &schema, "name", &gvariant_str("Lightshot")])?;
    gsettings(&["set", &schema, "command", &gvariant_str(&command)])?;
    gsettings(&["set", &schema, "binding", &gvariant_str(BINDING)])?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<(), String> {
    let list = read_list()?;
    if list.iter().any(|p| p == KEYBINDING_PATH) {
        let kept: Vec<String> = list.into_iter().filter(|p| p != KEYBINDING_PATH).collect();
        write_list(&kept)?;
    }
    let schema = custom_schema();
    for key in ["name", "command", "binding"] {
        gsettings(&["reset", &schema, key])?;
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install() -> Result<(), String> {
    Err(unsupported("install-hotkey"))
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall() -> Result<(), String> {
    Err(unsupported("uninstall-hotkey"))
}

#[cfg(not(target_os = "linux"))]
fn unsupported(command: &str) -> String {
    let exe = match std::env::current_exe() {
        Ok(p) => p.display().to_string(),
        Err(e) => format!("<current_exe: {e}>"),
    };
    format!(
        "{command} only manages a GNOME binding. Bind Ctrl+Print to `lightshot capture` yourself in your OS settings; the command for this binary is: \"{exe}\" capture"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_round_trip_keeps_other_entries() {
        let got = parse_list(
            "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/busca-local/']\n",
        );
        assert_eq!(got.len(), 1);
        assert!(got[0].ends_with("/busca-local/"));
        assert!(parse_list("@as []").is_empty());
        let mut l = got.clone();
        l.push(KEYBINDING_PATH.into());
        assert_eq!(parse_list(&format_list(&l)), l);
        assert_eq!(format_list(&[]), "@as []");
    }
}
