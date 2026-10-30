use crate::config::{Config, atomic_write};
use anyhow::{Context, Result, bail};
use nix::unistd::{User, geteuid};
use std::{
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
};

const GROUP: &str = "keyresolve";
const RULE_PATH: &str = "/etc/udev/rules.d/99-keyresolve.rules";
const MODULES_PATH: &str = "/etc/modules-load.d/keyresolve-uinput.conf";

const UDEV_RULE: &str = "\
# Installed by KeyResolve. Allows the keyresolve group to use input devices and
# uinput so KeyResolve can run as an unprivileged systemd user service.
SUBSYSTEM==\"input\", KERNEL==\"event*\", GROUP=\"keyresolve\", MODE=\"0660\", TAG+=\"uaccess\"
SUBSYSTEM==\"misc\", KERNEL==\"uinput\", GROUP=\"keyresolve\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"
";

pub struct AccessProblem {
    pub device: Option<PathBuf>,
    pub uinput: bool,
}

/// Check the selected keyboard (when known) and uinput using the current user's
/// permissions. This does not grab the keyboard or create a virtual device.
pub fn check(config: Option<&Config>) -> Result<Option<AccessProblem>> {
    let device = match config {
        Some(config) => {
            let candidates = crate::keyboard::enumerate_candidates();
            let candidate = candidates
                .iter()
                .find(|candidate| config.keyboard.matches_candidate(candidate));
            match candidate {
                Some(candidate) => Some(candidate.path.clone()),
                None => {
                    // The saved keyboard may be unplugged. That is separate from
                    // a permission problem; uinput still needs to be accessible.
                    None
                }
            }
        }
        None => None,
    };

    let device_problem = match &device {
        Some(path) => !can_open(path, true),
        None => false,
    };
    let uinput_problem = !can_open(Path::new("/dev/uinput"), true);

    if device_problem || uinput_problem {
        Ok(Some(AccessProblem {
            device: if device_problem { device } else { None },
            uinput: uinput_problem,
        }))
    } else {
        Ok(None)
    }
}

fn can_open(path: &Path, write: bool) -> bool {
    OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .is_ok()
}

pub fn require_unprivileged_user() -> Result<()> {
    if geteuid().is_root() {
        bail!(
            "Run KeyResolve setup/service commands as your normal login user, without sudo. \
             Setup will ask for sudo only when it needs to install device permissions."
        );
    }
    Ok(())
}

/// Offer the one-time privileged permission setup from an interactive command.
pub fn ensure_interactive(config: Option<&Config>) -> Result<()> {
    let Some(problem) = check(config)? else {
        return Ok(());
    };
    if !io::stdin().is_terminal() {
        bail!(
            "Device permissions are missing. Run 'KeyResolve permissions install' \
             interactively as this user, then log out/in if prompted."
        );
    }

    eprintln!("KeyResolve cannot access the required input devices as this user.");
    if let Some(path) = &problem.device {
        eprintln!("  Missing read/write access: {}", path.display());
    }
    if problem.uinput {
        eprintln!("  Missing read/write access: /dev/uinput");
    }
    print!(
        "Install persistent device permissions now? This uses sudo once, creates the \
         '{GROUP}' group, and adds your user to it. [Y/n]: "
    );
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    ) {
        install_with_sudo(config)?;
        if let Some(problem) = check(config)? {
            if problem.device.is_some() || problem.uinput {
                bail!(
                    "Permissions were installed, but this session still cannot access the devices. \
                     Log out and back in (or reboot), then run 'KeyResolve setup --service' again."
                );
            }
        }
        Ok(())
    } else {
        bail!(
            "Device permissions are required. Run 'KeyResolve permissions install' later, \
             or follow the manual permissions section in README.md."
        );
    }
}

/// Normal-user entry point: re-exec only the permission helper through sudo.
pub fn install_with_sudo(config: Option<&Config>) -> Result<()> {
    require_unprivileged_user()?;
    let uid = nix::unistd::getuid();
    let user = User::from_uid(uid)?
        .with_context(|| format!("Cannot look up user for uid {}", uid.as_raw()))?;
    let executable = std::env::current_exe().context("Cannot locate KeyResolve executable")?;
    let device = config.and_then(|config| {
        crate::keyboard::enumerate_candidates()
            .into_iter()
            .find(|candidate| config.keyboard.matches_candidate(candidate))
            .map(|candidate| candidate.path)
    });

    let mut command = Command::new("sudo");
    command
        .arg("--")
        .arg(executable)
        .args(["permissions", "apply", "--uid"])
        .arg(uid.as_raw().to_string())
        .arg("--user")
        .arg(&user.name);
    if let Some(device) = device {
        command.arg("--device").arg(device);
    }

    let status = command.status().context(
        "Could not run sudo. Install sudo or follow the manual permissions section in README.md.",
    )?;
    if !status.success() {
        bail!("Permission setup failed.");
    }
    Ok(())
}

/// Root-only helper. It changes system device permissions, never user service
/// files or the saved keyboard configuration.
pub fn apply(uid: u32, username: &str, device: Option<&Path>) -> Result<()> {
    if !geteuid().is_root() {
        bail!("'permissions apply' must be run through sudo by 'KeyResolve permissions install'.");
    }
    if uid == 0 {
        bail!("Refusing to grant device access to root; pass the normal login user's uid.");
    }
    let user = User::from_name(username)
        .with_context(|| format!("Cannot look up user '{username}'"))?
        .with_context(|| format!("User '{username}' does not exist"))?;
    if user.uid.as_raw() != uid {
        bail!("--uid and --user do not refer to the same account.");
    }

    if nix::unistd::Group::from_name(GROUP)?.is_none() {
        run("groupadd", &["--system", GROUP])?;
    }
    run("usermod", &["-aG", GROUP, username])?;

    if let Some(parent) = Path::new(RULE_PATH).parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(Path::new(RULE_PATH), UDEV_RULE.as_bytes(), 0o644)
        .with_context(|| format!("Failed to write {RULE_PATH}"))?;
    if let Some(parent) = Path::new(MODULES_PATH).parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(Path::new(MODULES_PATH), b"uinput\n", 0o644)
        .with_context(|| format!("Failed to write {MODULES_PATH}"))?;

    // Built-in uinput may make modprobe unnecessary; a missing module is not
    // fatal if /dev/uinput already exists.
    let _ = run("modprobe", &["uinput"]);
    run("udevadm", &["control", "--reload-rules"])?;
    run(
        "udevadm",
        &["trigger", "--subsystem-match=input", "--action=change"],
    )?;
    if Path::new("/dev/uinput").exists() {
        run("udevadm", &["trigger", "/dev/uinput"])?;
    }
    let _ = run("udevadm", &["settle", "--timeout=10"]);

    // Group membership is not retroactively added to existing processes. ACLs
    // let the current setup/session continue immediately when setfacl exists;
    // the udev rule/group provide persistent access after the next login.
    let mut acl_paths = vec![PathBuf::from("/dev/uinput")];
    if let Some(device) = device {
        acl_paths.push(device.to_path_buf());
    }
    let acl_arg = format!("u:{uid}:rw");
    let mut acl_applied = true;
    for path in acl_paths {
        if path.exists() {
            let path_string = path.to_string_lossy();
            if run("setfacl", &["-m", &acl_arg, path_string.as_ref()]).is_err() {
                acl_applied = false;
            }
        }
    }

    println!("Installed KeyResolve device permissions for '{username}'.");
    if !acl_applied {
        println!(
            "If this session still cannot see the keyboard, log out and back in so the \
             '{GROUP}' group membership applies, then run setup again."
        );
    }
    Ok(())
}

fn run(program: &str, arguments: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(arguments)
        .status()
        .with_context(|| format!("Could not run {program}. Is it installed?"))?;
    if !status.success() {
        bail!("{program} {} failed ({status})", arguments.join(" "));
    }
    Ok(())
}
