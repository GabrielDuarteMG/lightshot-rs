use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Config {
    pub copy_format: String,
    pub jpeg_quality: u8,
    pub auto_copy: bool,
    pub auto_close: bool,
    pub last_saved_dir: String,
    pub save_index: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            copy_format: "png".into(),
            jpeg_quality: 80,
            auto_copy: false,
            auto_close: true,
            last_saved_dir: String::new(),
            save_index: 1,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct RawConfig {
    copy_format: String,
    jpeg_quality: i64,
    auto_copy: bool,
    auto_close: bool,
    last_saved_dir: String,
    save_index: i64,
}

impl Default for RawConfig {
    fn default() -> Self {
        let d = Config::default();
        RawConfig {
            copy_format: d.copy_format,
            jpeg_quality: d.jpeg_quality.into(),
            auto_copy: d.auto_copy,
            auto_close: d.auto_close,
            last_saved_dir: d.last_saved_dir,
            save_index: d.save_index.into(),
        }
    }
}

impl From<RawConfig> for Config {
    fn from(r: RawConfig) -> Self {
        Config {
            copy_format: match r.copy_format.as_str() {
                "jpeg" => "jpeg".into(),
                _ => "png".into(),
            },
            jpeg_quality: r.jpeg_quality.clamp(1, 100) as u8,
            auto_copy: r.auto_copy,
            auto_close: r.auto_close,
            last_saved_dir: r.last_saved_dir,
            save_index: r.save_index.clamp(1, u32::MAX.into()) as u32,
        }
    }
}

impl Config {
    pub fn is_jpeg(&self) -> bool {
        self.copy_format == "jpeg"
    }
}

fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn config_dir() -> PathBuf {
    match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x).join("lightshot"),
        None => home()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("lightshot"),
    }
}

#[cfg(target_os = "macos")]
pub fn config_dir() -> PathBuf {
    home()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
        .join("lightshot")
}

#[cfg(windows)]
pub fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(home)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lightshot")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load() -> Config {
    load_from(&config_path())
}

pub fn store(cfg: &Config) -> io::Result<()> {
    store_to(&config_path(), cfg)
}

pub fn load_from(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<RawConfig>(&text) {
            Ok(raw) => raw.into(),
            Err(e) => {
                eprintln!("lightshot: config {}: {}", path.display(), e);
                Config::default()
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let cfg = Config::default();
            if let Err(e) = store_to(path, &cfg) {
                eprintln!("lightshot: config {}: {}", path.display(), e);
            }
            cfg
        }
        Err(e) => {
            eprintln!("lightshot: config {}: {}", path.display(), e);
            Config::default()
        }
    }
}

pub fn store_to(path: &Path, cfg: &Config) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(cfg).map_err(io::Error::other)?;
    std::fs::write(path, text)
}

pub fn pictures_dir() -> PathBuf {
    if let Some(home) = home() {
        if cfg!(not(any(windows, target_os = "macos"))) {
            let dirs = home.join(".config").join("user-dirs.dirs");
            if let Ok(text) = std::fs::read_to_string(dirs) {
                if let Some(p) = parse_pictures_dir(&text, &home) {
                    if p.is_dir() {
                        return p;
                    }
                }
            }
        }
        let p = home.join("Pictures");
        if p.is_dir() {
            return p;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn parse_pictures_dir(text: &str, home: &Path) -> Option<PathBuf> {
    text.lines().find_map(|line| {
        let value = line.trim().strip_prefix("XDG_PICTURES_DIR=")?;
        let value = value.trim().trim_matches('"');
        let expanded = match value.strip_prefix("$HOME") {
            Some(rest) => format!("{}{}", home.display(), rest),
            None => value.to_string(),
        };
        (!expanded.is_empty()).then(|| PathBuf::from(expanded))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_copy_format_loads_png_without_rewrite() {
        let dir = std::env::temp_dir().join(format!("lightshot-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let text = "copy_format = \"gif\"\njpeg_quality = 250\nsave_index = 0\n";
        std::fs::write(&path, text).unwrap();
        let cfg = load_from(&path);
        assert_eq!(cfg.copy_format, "png");
        assert_eq!(cfg.jpeg_quality, 100);
        assert_eq!(cfg.save_index, 1);
        assert!(cfg.auto_close);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pictures_dir_expands_home() {
        let p = parse_pictures_dir(
            "# comment\nXDG_PICTURES_DIR=\"$HOME/Imagens\"\n",
            Path::new("/home/u"),
        );
        assert_eq!(p, Some(PathBuf::from("/home/u/Imagens")));
    }
}
