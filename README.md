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
* ✅ Clean Ctrl-C shutdown (no stuck keys)

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

* `/dev/input/event*` (read)
* `/dev/uinput` (write)

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

Then **log out or reboot**.

## Usage

Run the program:

```bash
./KeyResolve
```

You will be prompted to select which keyboard to grab.

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
* Adding configuration support
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
