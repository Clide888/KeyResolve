# KeyResolve

**Snap Tap / SOCD-style keyboard input handling for Linux (Wayland & X11)**

`KeyResolve` is a low-latency userspace input interceptor for Linux that enforces *mutually exclusive movement keys* (e.g. **A/D**, **W/S**) using a **last-pressed-wins** policy.

It works by intercepting raw keyboard events via `evdev`, applying deterministic key logic, and re-emitting corrected events through a virtual keyboard (`uinput`).
This approach works reliably on **Wayland**, **X11**, and in **games that use raw input**.

## Features

* ✅ Last-pressed-wins logic for movement keys
* ✅ Guarantees no simultaneous `A+D` and `W+S`
* ✅ Works on **Wayland and X11**
* ✅ Game-compatible (raw input / evdev level)
* ✅ Written in **Rust**
* ✅ No kernel modules
* ✅ No compositor plugins
* ✅ Clean Ctrl-C / SIGTERM shutdown (releases held keys)
* ✅ Remembers the selected keyboard across launches and reconnects
* ✅ Names the virtual keyboard after the selected device
* ✅ Optional systemd **user** service, started at login

## Installation

### Install from the AUR

```
yay -S keyresolve-git
# OR
paru -S keyresolve-git
```

See https://aur.archlinux.org/packages/keyresolve-git

### Download from GitHub Releases

Navigate to the [Releases](https://github.com/Antosser/KeyResolve/releases) and download the binary for your architecture.

### Build from source

```bash
git clone https://github.com/Antosser/KeyResolve.git
cd KeyResolve
cargo build --release
```

The binary will be located at:

```
target/release/KeyResolve
```

## Permissions & udev setup (required unless root)

`KeyResolve` needs access to:

* `/dev/input/event*` (read **and write**, for the exclusive grab)
* `/dev/uinput` (write)

These permissions are required for both foreground runs and the user service.
Installing the service does not grant device access or change udev rules.
Membership in the `input` group grants access to keyboard input; only grant it
to accounts you trust.

### Create a udev rule

Create `/etc/udev/rules.d/99-input.rules` (automatically created when intalled via the AUR):

```ini
KERNEL=="event*", SUBSYSTEM=="input", GROUP="input", MODE="660"
KERNEL=="uinput", GROUP="input", MODE="660"
```

Reload rules:

```bash
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### Add your user to the `input` group

```bash
sudo usermod -aG input $USER
```

Then **log out or reboot** so the new group membership applies to your login
session (including the systemd user manager).

If `/dev/uinput` does not exist, load the module with `sudo modprobe uinput`.
To load it at boot, add `uinput` to `/etc/modules-load.d/uinput.conf`.

## Setup and saved device

Run the interactive setup as your normal user, **without sudo**:

```bash
./KeyResolve setup
```

1. Pick your keyboard from the numbered list.
2. The selection is saved immediately to
   `$XDG_CONFIG_HOME/keyresolve/config.toml` (normally
   `~/.config/keyresolve/config.toml`).
3. Optionally answer **y** to install, enable, and start the systemd user service.

The next plain `./KeyResolve` launch uses the saved keyboard without asking again.
The first plain launch also offers the device picker if no configuration exists.
Run setup again to change keyboards; Enter keeps the previous selection when it
is connected.

The saved hardware identity (name, bus/vendor/product/version, and physical path
or unique ID) is used to find the keyboard again after `/dev/input/eventX` changes.
If it is unplugged, KeyResolve waits for that keyboard rather than grabbing a
different one. If multiple connected devices have the same saved identity,
KeyResolve reports the ambiguity instead of grabbing an arbitrary one; disconnect
the extra identical devices and try again.

The output device is named **`<your keyboard name> (KeyResolve)`**, shortened if
necessary to fit the kernel's name limit. Linux still assigns its `/dev/input/eventX`
node automatically; the device's display name is what appears in input-device
lists. KeyResolve excludes its own virtual devices from the picker.

To select/save a device without the service prompt:

```bash
./KeyResolve setup --no-service
```

## systemd user service

During setup, choose the service option, or select/save a keyboard and install it
in one command:

```bash
./KeyResolve setup --service
```

If a keyboard is already saved, you can install the service separately:

```bash
./KeyResolve service install
```

Setup copies the executable to
`$XDG_DATA_HOME/keyresolve/bin/KeyResolve` (normally
`~/.local/share/keyresolve/bin/KeyResolve`) and writes
`$XDG_CONFIG_HOME/systemd/user/keyresolve.service`. The unit points at the saved
configuration explicitly. It starts at your next login and restarts on failure;
no system-wide service or root-owned KeyResolve process is installed.

Manage it with:

```bash
systemctl --user status keyresolve.service
journalctl --user -u keyresolve.service -f
systemctl --user restart keyresolve.service
systemctl --user stop keyresolve.service
```

After rebuilding or changing the saved device, run `./KeyResolve service install`
again to update the installed executable/unit and restart the service. If you ran
setup while an old foreground instance was running, stop that instance first.

To stop, disable, and remove the service and its copied executable:

```bash
./KeyResolve service uninstall
```

This keeps your saved keyboard configuration. The service starts on **login**,
not before login; enabling lingering is not required or done automatically.

## Usage

After setup, run:

```bash
./KeyResolve
```

For noninteractive use with an existing saved configuration:

```bash
./KeyResolve run
# Optional alternate configuration:
./KeyResolve run --config /absolute/path/to/config.toml
```

Once running:

* Pressing **A** releases **D**
* Pressing **D** releases **A**
* Pressing **W** releases **S**
* Pressing **S** releases **W**
* When the last pressed key is released, the previous still-held key (if any) is restored

Exit cleanly with **Ctrl+C**.

## Safety notes

* The selected keyboard is **exclusively grabbed**
* If the program crashes, input from that keyboard may temporarily stop
* Always test from:

  * a TTY, or
  * an SSH session, or
  * with a second keyboard available

The program handles Ctrl-C correctly and releases all keys on exit.

## Supported platforms

* ✅ Linux
* ✅ Wayland
* ✅ X11
* ❌ Windows
* ❌ macOS

## Why not X11 key remapping?

* X11 remapping does not work reliably on Wayland
* Many games bypass X11 entirely
* `KeyResolve` operates at the **evdev level**, where games actually read input

## Contributing

Contributions are welcome!

Good areas to help with:

* Generalizing key-pair handling
* Extending configuration options
* Improving documentation
* Testing on different keyboards
* Packaging (Arch / Nix / etc.)

### Development setup

```bash
cargo check
cargo clippy
cargo fmt
```

Please keep changes:

* Idiomatic Rust
* Well-commented
* Minimal unsafe code (preferably none)

## License

`GPL-3.0` License
See `LICENSE` for details.
