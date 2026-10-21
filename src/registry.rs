//! `~/.config/blackbox/ports.toml`: the one place ports are written down.
//!
//! ```toml
//! [services.holodrive]
//! port = 8768
//! note = "web file manager (127.0.0.1, tailscale serve)"
//! pinned = true   # existing port recorded by `import`, not allocated
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    #[serde(default)]
    pub services: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub port: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Recorded from what was already running, rather than allocated.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Registry, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => {
                let r: Registry =
                    toml::from_str(&t).map_err(|e| format!("{}: {e}", path.display()))?;
                r.validate()?;
                Ok(r)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut seen: BTreeMap<u16, &str> = BTreeMap::new();
        for (name, e) in &self.services {
            if !valid_name(name) {
                return Err(format!("service name {name:?}: use [a-z0-9-]"));
            }
            if let Some(other) = seen.insert(e.port, name) {
                return Err(format!(
                    "port {} is registered twice ({other} and {name})",
                    e.port
                ));
            }
        }
        Ok(())
    }

    /// Write atomically, keeping a header comment.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let body = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        let text = format!("# Port registry, managed by portmirage (https://github.com/cybercoretech/portmirage).\n# Units read the rendered env file; edit with `portmirage`, not by hand.\n\n{body}");
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn ports(&self) -> std::collections::BTreeSet<u16> {
        self.services.values().map(|e| e.port).collect()
    }

    /// `PORT_HOLODRIVE=8768` lines for an EnvironmentFile.
    pub fn env(&self) -> String {
        self.services
            .iter()
            .map(|(n, e)| {
                format!(
                    "PORT_{}={}\n",
                    n.to_ascii_uppercase().replace('-', "_"),
                    e.port
                )
            })
            .collect()
    }

    /// A markdown table for docs.
    pub fn table(&self) -> String {
        let mut s = String::from("| Service | Port | Kind | Note |\n|---|---|---|---|\n");
        let mut rows: Vec<_> = self.services.iter().collect();
        rows.sort_by_key(|(_, e)| e.port);
        for (n, e) in rows {
            s.push_str(&format!(
                "| `{n}` | {} | {} | {} |\n",
                e.port,
                if e.pinned { "pinned" } else { "allocated" },
                e.note.replace('|', "\\|")
            ));
        }
        s
    }
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 48
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !n.starts_with('-')
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        });
    base.join("blackbox/ports.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(entries: &[(&str, u16, bool)]) -> Registry {
        Registry {
            services: entries
                .iter()
                .map(|(n, p, pin)| {
                    (
                        n.to_string(),
                        Entry {
                            port: *p,
                            note: String::new(),
                            pinned: *pin,
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn roundtrips_and_renders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blackbox/ports.toml");
        let r = reg(&[("holodrive", 8768, true), ("night-watch", 8769, false)]);
        r.save(&path).unwrap();
        let back = Registry::load(&path).unwrap();
        assert_eq!(back.services, r.services);
        assert_eq!(back.env(), "PORT_HOLODRIVE=8768\nPORT_NIGHT_WATCH=8769\n");
        assert!(back.table().contains("| `holodrive` | 8768 | pinned |"));
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("# Port registry"));
    }

    #[test]
    fn rejects_duplicates_and_bad_names() {
        assert!(reg(&[("a", 1, false), ("b", 1, false)])
            .validate()
            .unwrap_err()
            .contains("twice"));
        assert!(reg(&[("Bad_Name", 2, false)]).validate().is_err());
        assert!(valid_name("cyber-deck2") && !valid_name("-x") && !valid_name(""));
    }

    #[test]
    fn missing_file_is_an_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Registry::load(&dir.path().join("nope.toml"))
            .unwrap()
            .services
            .is_empty());
    }
}
