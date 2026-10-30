use anyhow::{Context, Result, bail};
use evdev::Device;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub version: u32,
    pub keyboard: SavedKeyboard,
}

/// Keep hardware identity as well as the last event node. Event numbers are not
/// stable across reboots, and identical keyboards may share a vendor/product ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedKeyboard {
    pub name: String,
    pub last_path: PathBuf,
    pub bus_type: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
    pub physical_path: Option<String>,
    pub unique_name: Option<String>,
}

impl SavedKeyboard {
    pub fn from_device(path: &Path, device: &Device) -> Self {
        let id = device.input_id();
        Self {
            name: device.name().unwrap_or("Keyboard").to_owned(),
            last_path: path.to_owned(),
            bus_type: id.bus_type().0,
            vendor: id.vendor(),
            product: id.product(),
            version: id.version(),
            physical_path: nonempty(device.physical_path()),
            unique_name: nonempty(device.unique_name()),
        }
    }

    pub fn matches(&self, path: &Path, device: &Device) -> bool {
        let id = device.input_id();
        self.matches_identity(
            path,
            device.name().unwrap_or("Keyboard"),
            id.bus_type().0,
            id.vendor(),
            id.product(),
            id.version(),
            device.physical_path(),
            device.unique_name(),
        )
    }

    pub fn matches_candidate(&self, candidate: &crate::keyboard::KeyboardCandidate) -> bool {
        let identity = &candidate.identity;
        self.matches_identity(
            &candidate.path,
            &identity.name,
            identity.bus_type,
            identity.vendor,
            identity.product,
            identity.version,
            identity.physical_path.as_deref(),
            identity.unique_name.as_deref(),
        )
    }

    fn matches_identity(
        &self,
        path: &Path,
        name: &str,
        bus_type: u16,
        vendor: u16,
        product: u16,
        version: u16,
        physical_path: Option<&str>,
        unique_name: Option<&str>,
    ) -> bool {
        if self.name != name
            || self.bus_type != bus_type
            || self.vendor != vendor
            || self.product != product
            || self.version != version
        {
            return false;
        }

        let physical_path = nonempty(physical_path);
        let unique_name = nonempty(unique_name);

        // A serial/unique ID can follow a keyboard to another USB port. Otherwise
        // its physical connection identifies it. Never fall back to a different
        // keyboard of the same model just because the saved one is unplugged.
        if self.unique_name.is_some() && self.unique_name == unique_name {
            return true;
        }
        if self.physical_path.is_some() && self.physical_path == physical_path {
            return true;
        }

        self.unique_name.is_none()
            && self.physical_path.is_none()
            && unique_name.is_none()
            && physical_path.is_none()
            && self.last_path == path
    }
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

impl Config {
    pub fn new(keyboard: SavedKeyboard) -> Self {
        Self {
            version: CONFIG_VERSION,
            keyboard,
        }
    }

    pub fn load(path: &Path) -> Result<Option<Self>> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to read {}", path.display()));
            }
        };
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("Invalid configuration in {}", path.display()))?;
        if config.version != CONFIG_VERSION {
            bail!(
                "Unsupported configuration version {} in {}; run setup again",
                config.version,
                path.display()
            );
        }
        Ok(Some(config))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .context("Configuration path has no parent directory")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        let text = toml::to_string_pretty(self).context("Failed to serialize configuration")?;
        atomic_write(path, text.as_bytes(), 0o600)
            .with_context(|| format!("Failed to save {}", path.display()))
    }
}

/// Replace complete files, not partially written configuration or executables.
/// The temporary file is on the same filesystem so rename is atomic.
pub(crate) fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("Destination path has no parent directory")?;
    let filename = path
        .file_name()
        .context("Destination path has no filename")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{filename}.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn default_path() -> Result<PathBuf> {
    Ok(xdg_base("XDG_CONFIG_HOME", ".config")?.join("keyresolve/config.toml"))
}

pub(crate) fn xdg_base(variable: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(value) = std::env::var_os(variable) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return Ok(path);
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .context("Cannot locate your home directory; set HOME or the appropriate XDG directory")?;
    Ok(home.join(fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard() -> SavedKeyboard {
        SavedKeyboard {
            name: "Test keyboard".into(),
            last_path: "/dev/input/event3".into(),
            bus_type: 3,
            vendor: 0x1234,
            product: 0x5678,
            version: 1,
            physical_path: Some("usb-0000:00:14.0-2/input0".into()),
            unique_name: None,
        }
    }

    #[test]
    fn configuration_round_trip() {
        let config = Config::new(keyboard());
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
    }

    #[test]
    fn save_replaces_complete_file_and_missing_file_is_ok() {
        let dir = crate::test_support::TestDirectory::new();
        let path = dir.path().join("nested/config.toml");
        assert_eq!(Config::load(&path).unwrap(), None);
        let config = Config::new(keyboard());
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), Some(config.clone()));
        let mut changed = config;
        changed.keyboard.last_path = "/dev/input/event8".into();
        changed.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), Some(changed));
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn corrupt_or_future_configuration_is_not_silently_replaced() {
        let dir = crate::test_support::TestDirectory::new();
        let path = dir.path().join("config.toml");
        fs::write(&path, "not a valid configuration").unwrap();
        assert!(Config::load(&path).is_err());
        let mut config = Config::new(keyboard());
        config.version += 1;
        fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        assert!(Config::load(&path).is_err());
    }

    #[test]
    fn atomic_write_cleans_up_failed_rename() {
        let dir = crate::test_support::TestDirectory::new();
        assert!(atomic_write(&dir.path().join("missing/file"), b"data", 0o600).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
