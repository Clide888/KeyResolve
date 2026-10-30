use crate::config::{atomic_write, xdg_base};
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, OpenOptions},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub const UNIT_NAME: &str = "keyresolve.service";

pub struct ServicePaths {
    pub unit: PathBuf,
    pub executable: PathBuf,
}

impl ServicePaths {
    pub fn from_xdg() -> Result<Self> {
        Ok(Self {
            unit: xdg_base("XDG_CONFIG_HOME", ".config")?
                .join("systemd/user")
                .join(UNIT_NAME),
            executable: xdg_base("XDG_DATA_HOME", ".local/share")?
                .join("keyresolve/bin/KeyResolve"),
        })
    }
}

pub fn install(paths: &ServicePaths, config_path: &Path) -> Result<()> {
    require_unprivileged_user()?;
    let source = std::env::current_exe().context("Cannot locate the KeyResolve executable")?;
    let source = source
        .canonicalize()
        .with_context(|| format!("Cannot resolve {}", source.display()))?;

    // Do not depend on a Downloads directory, a mounted disk, or cargo's target
    // directory remaining available after login. Renaming also allows replacing
    // the executable while an older service process is still running.
    if source != paths.executable {
        atomic_copy_executable(&source, &paths.executable)?;
    }
    let unit = render_unit(&paths.executable, config_path)?;
    let parent = paths.unit.parent().context("Invalid user service path")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    atomic_write(&paths.unit, unit.as_bytes(), 0o644)
        .with_context(|| format!("Failed to write {}", paths.unit.display()))?;

    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", UNIT_NAME])?;
    // enable --now would leave an already-running old version/config in place.
    systemctl(&["restart", UNIT_NAME]).with_context(|| {
        format!(
            "Service installed, but could not start. Check: journalctl --user -u {UNIT_NAME} -e"
        )
    })?;

    println!("Installed and enabled {UNIT_NAME} for this user.");
    println!("It will start at login and use the saved keyboard automatically.");
    Ok(())
}

pub fn uninstall(paths: &ServicePaths) -> Result<()> {
    require_unprivileged_user()?;
    if !paths.unit.exists() && !paths.executable.exists() {
        println!("The KeyResolve user service is not installed.");
        return Ok(());
    }

    systemctl(&["disable", "--now", UNIT_NAME])?;
    if paths.unit.exists() {
        fs::remove_file(&paths.unit)
            .with_context(|| format!("Failed to remove {}", paths.unit.display()))?;
    }
    systemctl(&["daemon-reload"])?;
    let _ = systemctl(&["reset-failed", UNIT_NAME]);
    if paths.executable.exists() {
        fs::remove_file(&paths.executable)
            .with_context(|| format!("Failed to remove {}", paths.executable.display()))?;
    }
    println!("Removed the user service. Your saved keyboard configuration was kept.");
    Ok(())
}

fn require_unprivileged_user() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        bail!("Install the user service as your normal login user, without sudo.");
    }
    Ok(())
}

fn systemctl(arguments: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(arguments)
        .status()
        .context("Could not run systemctl. A systemd user session is required.")?;
    if !status.success() {
        bail!(
            "systemctl --user {} failed ({status}). Is your systemd user session running?",
            arguments.join(" ")
        );
    }
    Ok(())
}

fn atomic_copy_executable(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Service executable path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let temporary = parent.join(format!(".KeyResolve.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut input = fs::File::open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("Failed to install executable at {}", destination.display()))
}

fn render_unit(executable: &Path, config_path: &Path) -> Result<String> {
    let executable = executable
        .to_str()
        .context("The service executable path must be valid UTF-8")?;
    let config_path = config_path
        .to_str()
        .context("The configuration path must be valid UTF-8")?;
    Ok(format!(
        "# Generated by KeyResolve setup. Run KeyResolve service uninstall to remove.\n\
         [Unit]\n\
         Description=KeyResolve keyboard input resolver\n\
         StartLimitIntervalSec=60\n\
         StartLimitBurst=5\n\n\
         [Service]\n\
         Type=simple\n\
         ExecStart={} run --config {}\n\
         Restart=on-failure\n\
         RestartSec=3\n\
         TimeoutStopSec=10\n\n\
         [Install]\n\
         WantedBy=default.target\n",
        quote_exec_start(executable),
        quote_exec_start(config_path)
    ))
}

/// systemd performs specifier and variable expansion even inside double quotes.
/// Escape both, as well as quotes/backslashes and control characters, so a
/// pathname cannot introduce additional unit directives.
fn quote_exec_start(path: &str) -> String {
    let mut quoted = String::from("\"");
    for character in path.chars() {
        match character {
            '%' => quoted.push_str("%%"),
            '$' => quoted.push_str("$$"),
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\x{:02x}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}
