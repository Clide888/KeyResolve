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

* the selected `/dev/input/eventX` (read **and write**, for the exclusive grab)
* `/dev/uinput` (read/write, to create the virtual keyboard)

These permissions are required for both foreground runs and the user service.
The service itself runs as your normal login user, **not** as root. Setup checks
device permissions **before** it tries to list or grab a keyboard. If access is
missing, it offers a one-time sudo permission helper, then continues setup as
your normal user.

### Recommended: one-time permission helper

Run setup as your normal user:

```bash
./KeyResolve setup
```

If device access is missing, setup offers to install persistent permissions
before showing the keyboard picker. This uses sudo **once** for a small root-only
helper that:

* creates a dedicated `keyresolve` system group and adds your user to it
* writes `/etc/udev/rules.d/99-keyresolve.rules`
* writes `/etc/modules-load.d/keyresolve-uinput.conf` and loads `uinput`
* reloads/triggers udev and, when `setfacl` is available, grants your current
  session immediate access to the current `/dev/input/event*` nodes and
  `/dev/uinput`

You can also run the permission step by itself:

```bash
./KeyResolve permissions install
./KeyResolve permissions check
```

If the helper says this session still cannot access the devices, **log out and
back in** (or reboot), then run `./KeyResolve setup --service` again. The new
group membership must reach your login session and the systemd user manager.

The `keyresolve` group can read/write input event devices and uinput. Only add
trusted accounts to it.

### Manual permissions

If you prefer not to use the helper, create
`/etc/udev/rules.d/99-keyresolve.rules`:

```ini
SUBSYSTEM=="input", KERNEL=="event*", GROUP="keyresolve", MODE="0660", TAG+="uaccess"
SUBSYSTEM=="misc", KERNEL=="uinput", GROUP="keyresolve", MODE="0660", OPTIONS+="static_node=uinput"
```

Then:

```bash
sudo groupadd --system keyresolve
sudo usermod -aG keyresolve $USER
echo uinput | sudo tee /etc/modules-load.d/keyresolve-uinput.conf
sudo modprobe uinput
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=input --action=change
sudo udevadm trigger /dev/uinput
```

Log out and back in so the new group applies. If you already use the broader
`input` group and a working udev rule for `/dev/uinput`, that can also work; the
helper just avoids requiring the broader `input` group.

## Setup and saved device

Run the interactive setup as your normal user, **without sudo**:

```bash
./KeyResolve setup
```

1. If device access is missing, accept the one-time permission prompt first.
   Setup uses sudo only for the permission helper, then continues as your normal
   user.
2. Pick your keyboard from the numbered list. Setup checks sysfs metadata and,
   after permissions are granted, also opens devices through evdev.
3. The selection is saved immediately to
   `$XDG_CONFIG_HOME/keyresolve/config.toml` (normally
   `~/.config/keyresolve/config.toml`).
4. Optionally answer **y** to install, enable, and start the systemd user service.
   Service installation and startup use `systemctl --user`, not sudo.

The next plain `./KeyResolve` launch uses the saved keyboard without asking again.
The first plain launch also runs setup if no configuration exists: it checks
permissions, offers the device picker, and offers the optional user-service
prompt **before** grabbing the keyboard.
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

The `Grabbed … — KeyResolve is running.` message is the normal foreground state,
not a stuck setup step. The process stays there forwarding keys until you stop it.
If you want it managed in the background instead, stop the foreground process and
run `./KeyResolve setup` (or `./KeyResolve service install` if a keyboard is already
saved).

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
