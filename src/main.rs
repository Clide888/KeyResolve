mod config;
mod keyboard;
mod permissions;
mod service;

use anyhow::{Context, Result, bail};
use config::Config;
use keyboard::SessionExit;
use std::{
    io::{self, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const HELP: &str = "\
KeyResolve — last-pressed-wins keyboard input for Linux

Usage:
  KeyResolve                     Use the saved keyboard (first run offers setup)
  KeyResolve setup               Select/save a keyboard; optionally install a user service
  KeyResolve setup --service     Select/save a keyboard and install the user service
  KeyResolve setup --no-service  Select/save a keyboard without the service prompt
  KeyResolve run [--config PATH] Use the saved keyboard without interactive prompts
  KeyResolve service install     Install, enable, and start the systemd user service
  KeyResolve service uninstall   Stop/disable/remove the service, keeping configuration
  KeyResolve permissions install Grant this user device access (uses sudo once)
  KeyResolve permissions check   Check device access as the current user
  KeyResolve --help              Show this help
  KeyResolve --version           Show the version

The virtual keyboard is named '<selected keyboard> (KeyResolve)'.
Configuration: $XDG_CONFIG_HOME/keyresolve/config.toml
              (default: ~/.config/keyresolve/config.toml)
Input and uinput permissions are required; see README.md.
";

fn main() -> Result<()> {
    if let Err(error) = run_command() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
    Ok(())
}

fn run_command() -> Result<()> {
    let arguments: Vec<_> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let arguments: Vec<_> = arguments.iter().map(String::as_str).collect();
    let config_path = config::default_path()?;

    match arguments.as_slice() {
        [] => match Config::load(&config_path)? {
            Some(config) => {
                permissions::ensure_interactive(Some(&config))?;
                run(config, &config_path)
            }
            None => {
                // First run is setup: offer the user service before grabbing the
                // keyboard, so the interactive prompt is not hidden behind the
                // long-running foreground event loop.
                if setup(&config_path, None)? {
                    Ok(())
                } else {
                    let config = require_config(&config_path)?;
                    println!("Starting in the foreground. Press Ctrl+C to stop.");
                    run(config, &config_path)
                }
            }
        },
        ["run"] | ["run", "--config", _] => {
            let path = match arguments.as_slice() {
                ["run", "--config", path] => Path::new(path),
                _ => config_path.as_path(),
            };
            let config = require_config(path)?;
            run(config, path)
        }
        ["setup", "--service"] => setup(&config_path, Some(true)).map(|_| ()),
        ["setup", "--no-service"] => setup(&config_path, Some(false)).map(|_| ()),
        ["setup"] => setup(&config_path, None).map(|_| ()),
        ["service", "install"] => {
            permissions::require_unprivileged_user()?;
            let config = require_config(&config_path)?;
            permissions::ensure_interactive(Some(&config))?;
            service::install(&service::ServicePaths::from_xdg()?, &config_path)
        }
        ["service", "uninstall"] => service::uninstall(&service::ServicePaths::from_xdg()?),
        ["permissions", "install"] => {
            permissions::require_unprivileged_user()?;
            let config = Config::load(&config_path)?;
            permissions::install_with_sudo(config.as_ref())
        }
        ["permissions", "check"] => {
            let config = Config::load(&config_path)?;
            match permissions::check(config.as_ref())? {
                None => {
                    println!("Device permissions look OK for this user.");
                    Ok(())
                }
                Some(problem) => {
                    if let Some(path) = problem.device {
                        eprintln!("Missing read/write access: {}", path.display());
                    }
                    if problem.uinput {
                        eprintln!("Missing read/write access: /dev/uinput");
                    }
                    bail!("Run 'KeyResolve permissions install' to fix device access.");
                }
            }
        }
        ["permissions", "apply", "--uid", uid, "--user", username] => {
            permissions::apply(uid.parse().context("Invalid --uid")?, username, None)
        }
        [
            "permissions",
            "apply",
            "--uid",
            uid,
            "--user",
            username,
            "--device",
            device,
        ] => permissions::apply(
            uid.parse().context("Invalid --uid")?,
            username,
            Some(Path::new(device)),
        ),
        ["--help" | "-h" | "help"] => {
            print!("{HELP}");
            Ok(())
        }
        ["--version" | "-V"] => {
            println!("KeyResolve {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => bail!("Unknown arguments. Run 'KeyResolve --help' for usage."),
    }
}

fn require_config(path: &Path) -> Result<Config> {
    Config::load(path)?.context("No saved keyboard. Run 'KeyResolve setup' first.")
}

fn setup(config_path: &Path, install_service: Option<bool>) -> Result<bool> {
    permissions::require_unprivileged_user()?;
    let config = keyboard::select_and_save(&keyboard::enumerate_candidates(), config_path)?;
    println!(
        "Virtual keyboard name: {}",
        keyboard::virtual_device_name(&config.keyboard.name)
    );
    // Fix device access as the normal user before offering either foreground or
    // service use. The privileged helper only changes udev/group/ACL settings.
    permissions::ensure_interactive(Some(&config))?;
    let install = match install_service {
        Some(value) => value,
        None => ask_yes_no("Install and start KeyResolve as a service for your user? [y/N]: ")?,
    };
    if install {
        service::install(&service::ServicePaths::from_xdg()?, config_path)?;
    } else {
        println!("Run 'KeyResolve' to use the saved keyboard, or 'KeyResolve setup' to change it.");
    }
    Ok(install)
}

fn ask_yes_no(question: &str) -> Result<bool> {
    print!("{question}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn run(mut config: Config, config_path: &Path) -> Result<()> {
    if let Some(problem) = permissions::check(Some(&config))? {
        if let Some(path) = problem.device {
            eprintln!("Missing read/write access: {}", path.display());
        }
        if problem.uinput {
            eprintln!("Missing read/write access: /dev/uinput");
        }
        bail!(
            "Device permissions are missing. Run 'KeyResolve permissions install' as this user, \
             then log out/in if prompted."
        );
    }
    let running = install_signal_handler()?;
    let mut waiting = false;
    while running.load(Ordering::SeqCst) {
        let keyboards = keyboard::enumerate_keyboards();
        let Some(index) = keyboard::find_saved(&config.keyboard, &keyboards)? else {
            if !waiting {
                println!(
                    "Waiting for saved keyboard '{}' (or its reconnect). Ctrl+C to stop.",
                    config.keyboard.name
                );
                waiting = true;
            }
            sleep_interruptibly(&running, Duration::from_secs(2));
            continue;
        };
        waiting = false;
        let (path, mut device) = keyboards.into_iter().nth(index).expect("matched index");
        if config.keyboard.last_path != path {
            config.keyboard.last_path = path.clone();
            // Event numbers can change. Keep the hint current, but do not stop
            // working if the existing configuration is temporarily read-only.
            if let Err(error) = config.save(config_path) {
                eprintln!("Warning: could not update the saved event path: {error:#}");
            }
        }
        match keyboard::run_session(&path, &mut device, &running) {
            Ok(SessionExit::Stopped) => break,
            Ok(SessionExit::Disconnected) => {
                println!("Keyboard disconnected; waiting for the same keyboard to return.");
            }
            Err(error) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                if is_disconnection(&error) {
                    println!("Keyboard disconnected; waiting for the same keyboard to return.");
                } else {
                    return Err(error);
                }
            }
        }
        sleep_interruptibly(&running, Duration::from_secs(1));
    }
    Ok(())
}

fn is_disconnection(error: &anyhow::Error) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|error| {
        error.kind() == io::ErrorKind::NotConnected
            || error.raw_os_error() == Some(nix::errno::Errno::ENODEV as i32)
    })
}

fn sleep_interruptibly(running: &AtomicBool, duration: Duration) {
    let mut remaining = duration;
    let step = Duration::from_millis(50);
    while !remaining.is_zero() && running.load(Ordering::SeqCst) {
        let delay = remaining.min(step);
        thread::sleep(delay);
        remaining = remaining.saturating_sub(delay);
    }
}

fn install_signal_handler() -> Result<Arc<AtomicBool>> {
    let running = Arc::new(AtomicBool::new(true));
    let flag = running.clone();
    // ctrlc handles SIGTERM as well as Ctrl+C on Unix, allowing systemctl stop
    // to take the same cleanup path as an interactive shutdown.
    ctrlc::set_handler(move || flag.store(false, Ordering::SeqCst))
        .context("Failed to install shutdown signal handler")?;
    Ok(running)
}

#[cfg(test)]
mod test_support {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    pub struct TestDirectory(PathBuf);

    impl TestDirectory {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "keyresolve-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
