# KeyResolve

**Snap Tap / SOCD-style keyboard input handling for Linux (Wayland & X11)**

Fork Of Antosser/KeyResolve, Just More Daily drivable in my opinion 

# BUILD FROM SOURCE ONLY

You need the Original Project's Dependencies

It adds systemd usermode service integration and also the ability for the tool to remember what device you used 

## Configuring Snap Tap

Run `KeyResolve setup` to select a keyboard, then run `KeyResolve tui` to open
the terminal configuration menu. The default pairs are A/D and W/S; other
keys pass through normally. Add or remove independent pairs, restore WASD,
then save, or quit to discard changes. Each key can belong to only one pair.
The last key pressed in a pair wins; releasing it restores the other key if it
is still held. Names include letters, digits, arrows and common modifiers. For
other keys use `#` followed by the decimal Linux evdev key code from
`linux/input-event-codes.h` (for example `#59` for F1).

Changes take effect the next time KeyResolve starts. Stop and start a foreground
instance, or restart the service with `systemctl --user restart keyresolve`.
The menu also accepts `KeyResolve tui --config PATH`. Existing configurations
without pairs automatically use WASD. Re-running setup preserves your pairs.
Input device and `/dev/uinput` permissions are still required for running;
the configuration menu itself does not grab the keyboard. Some games prohibit
SOCD/Snap Tap features; check their rules before using it.

If you installed the service with an older build, run `KeyResolve service install`
using the new executable once to update its installed copy.
