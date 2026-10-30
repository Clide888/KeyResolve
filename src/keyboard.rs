use crate::config::{Config, SavedKeyboard};
use anyhow::{Context, Result, bail};
use evdev::{AttributeSet, BusType, Device, EventType, InputEvent, KeyCode, uinput::VirtualDevice};
use nix::poll::{PollFd, PollFlags, poll};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    os::fd::{AsRawFd, BorrowedFd},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub type Keyboard = (PathBuf, Device);
const VIRTUAL_SUFFIX: &str = " (KeyResolve)";

/// A keyboard discovered from sysfs. Unlike opening /dev/input/eventX, this does
/// not require device permissions, so setup can list keyboards before the
/// one-time udev/group permission helper has run.
pub struct KeyboardCandidate {
    pub path: PathBuf,
    pub identity: SavedKeyboard,
}

pub fn enumerate_candidates() -> Vec<KeyboardCandidate> {
    let mut candidates = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/input") else {
        return candidates;
    };
    for entry in entries.flatten() {
        let filename = entry.file_name();
        let Some(name) = filename.to_str() else {
            continue;
        };
        if !name.starts_with("event") {
            continue;
        }
        let device_dir = entry.path().join("device");
        let Some(identity) = read_identity(&device_dir, Path::new("/dev/input").join(name)) else {
            continue;
        };
        if !is_likely_keyboard_sysfs(&device_dir) || is_keyresolve_identity(&identity) {
            continue;
        }
        candidates.push(KeyboardCandidate {
            path: Path::new("/dev/input").join(name),
            identity,
        });
    }
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates
}

fn read_string(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(&read_string(path)?, 16).ok()
}

fn read_identity(device_dir: &Path, path: PathBuf) -> Option<SavedKeyboard> {
    let id = device_dir.join("id");
    Some(SavedKeyboard {
        name: read_string(&device_dir.join("name")).unwrap_or_else(|| "Keyboard".into()),
        last_path: path,
        bus_type: read_hex(&id.join("bus"))?,
        vendor: read_hex(&id.join("vendor"))?,
        product: read_hex(&id.join("product"))?,
        version: read_hex(&id.join("version"))?,
        physical_path: read_string(&device_dir.join("phys")),
        unique_name: read_string(&device_dir.join("uniq")),
    })
}

fn is_likely_keyboard_sysfs(device_dir: &Path) -> bool {
    let Some(bits) = read_string(&device_dir.join("capabilities/key")) else {
        return false;
    };
    has_key_bit(&bits, KeyCode::KEY_A.code())
        && has_key_bit(&bits, KeyCode::KEY_Z.code())
        && has_key_bit(&bits, KeyCode::KEY_SPACE.code())
}

fn has_key_bit(bits: &str, code: u16) -> bool {
    // sysfs prints the key bitmap as space-separated hexadecimal words, most
    // significant word first. Concatenating the words gives a big-endian hex
    // number whose least-significant bit is KEY_RESERVED.
    let hex: String = bits.split_whitespace().collect();
    let word = code as usize / 4;
    let bit = code as usize % 4;
    if word >= hex.len() {
        return false;
    }
    let index = hex.len() - 1 - word;
    let Some(nibble) = (hex.as_bytes()[index] as char).to_digit(16) else {
        return false;
    };
    nibble & (1u32 << bit) != 0
}

fn is_keyresolve_identity(identity: &SavedKeyboard) -> bool {
    identity.bus_type == BusType::BUS_VIRTUAL.0
        && (identity.name.ends_with(VIRTUAL_SUFFIX) || identity.name == "snap-tap-virtual")
}

pub fn enumerate_keyboards() -> Vec<Keyboard> {
    let mut keyboards: Vec<_> = evdev::enumerate()
        .filter(|(_, device)| is_likely_keyboard(device) && !is_keyresolve_device(device))
        .collect();
    keyboards.sort_by(|(a, _), (b, _)| a.cmp(b));
    keyboards
}

fn is_likely_keyboard(device: &Device) -> bool {
    device
        .supported_keys()
        .map(|keys| {
            keys.contains(KeyCode::KEY_A)
                && keys.contains(KeyCode::KEY_Z)
                && keys.contains(KeyCode::KEY_SPACE)
        })
        .unwrap_or(false)
}

fn is_keyresolve_device(device: &Device) -> bool {
    // Never grab our own output device, including after a service restart while
    // an older foreground instance is still running.
    device.input_id().bus_type() == BusType::BUS_VIRTUAL
        && device
            .name()
            .is_some_and(|name| name.ends_with(VIRTUAL_SUFFIX) || name == "snap-tap-virtual")
}

pub fn find_saved(saved: &SavedKeyboard, keyboards: &[Keyboard]) -> Result<Option<usize>> {
    let mut matches = keyboards
        .iter()
        .enumerate()
        .filter(|(_, (path, device))| saved.matches(path, device));
    let Some(first) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        bail!(
            "More than one connected keyboard matches '{}'. Run setup with only the intended keyboard connected.",
            saved.name
        );
    }
    Ok(Some(first.0))
}

/// Interactive selection always saves before grabbing. An existing selection is
/// offered as the default when setup is run again. Candidates come from sysfs,
/// so the picker works even before the user has permission to open event nodes.
pub fn select_and_save(keyboards: &[KeyboardCandidate], config_path: &Path) -> Result<Config> {
    if keyboards.is_empty() {
        bail!("No keyboards found in /sys/class/input. Connect a keyboard and try again.");
    }
    if !io::stdin().is_terminal() {
        bail!("Device selection needs a terminal. Run 'KeyResolve setup' interactively.");
    }
    let previous = match Config::load(config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Warning: existing configuration cannot be used as the default: {error:#}");
            None
        }
    };
    let default = previous.as_ref().and_then(|config| {
        keyboards
            .iter()
            .position(|candidate| config.keyboard.matches_candidate(candidate))
    });
    for (index, candidate) in keyboards.iter().enumerate() {
        println!(
            "{index}: {} ({})",
            candidate.identity.name,
            candidate.path.display()
        );
    }

    let selected = loop {
        match default {
            Some(index) => print!("Select keyboard [Enter keeps {index}]: "),
            None => print!("Select keyboard: "),
        }
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            bail!("Device selection cancelled.");
        }
        if input.trim().is_empty() {
            if let Some(index) = default {
                break index;
            }
        } else if let Ok(index) = input.trim().parse::<usize>() {
            if index < keyboards.len() {
                break index;
            }
        }
        println!("Enter a number from 0 to {}.", keyboards.len() - 1);
    };

    let candidate = &keyboards[selected];
    let config = Config::new(candidate.identity.clone());
    config.save(config_path)?;
    println!("Saved keyboard: {}", config.keyboard.name);
    println!("Configuration: {}", config_path.display());
    Ok(config)
}

pub fn virtual_device_name(name: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() { "Keyboard" } else { name };
    // uinput has an 80-byte name buffer. evdev 0.13 requires room for a NUL
    // and its strict bound leaves a maximum of 78 bytes for the name itself.
    let mut end = name.len().min(78 - VIRTUAL_SUFFIX.len());
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{VIRTUAL_SUFFIX}", &name[..end])
}

pub fn create_virtual_keyboard(device: &Device) -> Result<VirtualDevice> {
    let keys = device
        .supported_keys()
        .context("Failed to query supported keys")?;
    let id = device.input_id();
    let name = virtual_device_name(device.name().unwrap_or("Keyboard"));
    let mut virtual_device = VirtualDevice::builder()
        .context("Failed to open /dev/uinput; check the permissions in the README")?
        .name(&name)
        .input_id(evdev::InputId::new(
            BusType::BUS_VIRTUAL,
            id.vendor(),
            id.product(),
            id.version(),
        ))
        .with_keys(keys)?
        .build()
        .context("Failed to create virtual keyboard")?;

    // The kernel assigns the eventX number; the human-readable device name is
    // what appears in input-device lists, libinput, and game device selectors.
    if let Ok(nodes) = virtual_device.enumerate_dev_nodes_blocking() {
        for node in nodes.flatten() {
            println!("Virtual keyboard: {name} ({})", node.display());
        }
    }
    Ok(virtual_device)
}

fn emit_key(vdev: &mut VirtualDevice, key: KeyCode, value: i32) -> Result<()> {
    vdev.emit(&[InputEvent::new(EventType::KEY.0, key.code(), value)])
        .context("Failed to write to the virtual keyboard")
}

fn handle_axis_pair(
    vdev: &mut VirtualDevice,
    pressed_keys: &mut AttributeSet<KeyCode>,
    pressed: bool,
    this_down: &mut bool,
    other_down: bool,
    this_key: KeyCode,
    other_key: KeyCode,
) -> Result<()> {
    *this_down = pressed;
    emit_key(vdev, this_key, pressed as i32)?;
    set_key_state(pressed_keys, this_key, pressed);
    if other_down {
        emit_key(vdev, other_key, (!pressed) as i32)?;
        set_key_state(pressed_keys, other_key, !pressed);
    }
    Ok(())
}

fn set_key_state(pressed_keys: &mut AttributeSet<KeyCode>, key: KeyCode, pressed: bool) {
    if pressed {
        pressed_keys.insert(key);
    } else {
        pressed_keys.remove(key);
    }
}

pub enum SessionExit {
    Stopped,
    Disconnected,
}

pub fn run_session(
    path: &Path,
    device: &mut Device,
    running: &Arc<AtomicBool>,
) -> Result<SessionExit> {
    // Create output before taking exclusive access to the physical keyboard.
    let mut vdev = create_virtual_keyboard(device)?;
    println!("Grabbing {}", path.display());
    device.grab().with_context(|| {
        format!(
            "Failed to grab {}. Stop any other KeyResolve instance and check input permissions.",
            path.display()
        )
    })?;
    println!("Grabbed {} — KeyResolve is running.", path.display());

    let result = run_event_loop(device, &mut vdev, running);
    // The evdev Device also ungrabs on drop, including on an error path.
    let ungrab = device.ungrab();
    drop(vdev);
    println!("Released keyboard");
    result?;
    ungrab.context("Failed to release the keyboard")?;
    Ok(if running.load(Ordering::SeqCst) {
        SessionExit::Disconnected
    } else {
        SessionExit::Stopped
    })
}

fn run_event_loop(
    device: &mut Device,
    vdev: &mut VirtualDevice,
    running: &Arc<AtomicBool>,
) -> Result<()> {
    let mut state = KeyState::default();
    let result = process_events(device, vdev, running, &mut state);
    // Release forwarded keys before removing the virtual device, including on
    // service stop or a read error, so applications do not see stuck movement.
    for key in state.pressed.iter() {
        let _ = emit_key(vdev, key, 0);
    }
    result
}

#[derive(Default)]
struct KeyState {
    a_down: bool,
    d_down: bool,
    w_down: bool,
    s_down: bool,
    pressed: AttributeSet<KeyCode>,
}

fn process_events(
    device: &mut Device,
    vdev: &mut VirtualDevice,
    running: &AtomicBool,
    state: &mut KeyState,
) -> Result<()> {
    // SAFETY: device owns this descriptor and outlives the poll descriptor.
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(device.as_raw_fd()) };
    let mut poll_fds = [PollFd::new(borrowed_fd, PollFlags::POLLIN)];

    while running.load(Ordering::SeqCst) {
        match poll(&mut poll_fds, 50u16) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(io::Error::from(error).into()),
        }
        let ready = poll_fds[0].revents().unwrap_or(PollFlags::empty());
        if ready.intersects(PollFlags::POLLERR | PollFlags::POLLHUP | PollFlags::POLLNVAL) {
            return Err(io::Error::from(nix::errno::Errno::ENODEV).into());
        }
        if !ready.contains(PollFlags::POLLIN) {
            continue;
        }

        for event in device.fetch_events()? {
            if event.event_type() != EventType::KEY {
                vdev.emit(&[event])?;
                continue;
            }
            if event.value() != 0 && event.value() != 1 {
                continue;
            }
            let pressed = event.value() == 1;
            let key = KeyCode::new(event.code());
            match key {
                KeyCode::KEY_A => handle_axis_pair(
                    vdev,
                    &mut state.pressed,
                    pressed,
                    &mut state.a_down,
                    state.d_down,
                    KeyCode::KEY_A,
                    KeyCode::KEY_D,
                )?,
                KeyCode::KEY_D => handle_axis_pair(
                    vdev,
                    &mut state.pressed,
                    pressed,
                    &mut state.d_down,
                    state.a_down,
                    KeyCode::KEY_D,
                    KeyCode::KEY_A,
                )?,
                KeyCode::KEY_W => handle_axis_pair(
                    vdev,
                    &mut state.pressed,
                    pressed,
                    &mut state.w_down,
                    state.s_down,
                    KeyCode::KEY_W,
                    KeyCode::KEY_S,
                )?,
                KeyCode::KEY_S => handle_axis_pair(
                    vdev,
                    &mut state.pressed,
                    pressed,
                    &mut state.s_down,
                    state.w_down,
                    KeyCode::KEY_S,
                    KeyCode::KEY_W,
                )?,
                _ => {
                    vdev.emit(&[event])?;
                    set_key_state(&mut state.pressed, key, pressed);
                }
            }
        }
    }
    Ok(())
}
