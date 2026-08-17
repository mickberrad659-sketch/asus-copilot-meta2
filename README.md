# ASUS Copilot Meta2

Turns the Copilot/assistant key on ASUS laptops into a genuinely independent
modifier for Linux. On the ASUS Zenbook S 16 UX5606 the firmware reports that
key as `Left Meta + Left Shift + F23`; the daemon removes this synthetic chord
and emits `F24`, while the supplied XKB keymap maps `F24` to
`ISO_Level5_Shift` (`Mod3`). Think of it as a second Super key — **Meta2**.

On the configured laptop the same input proxy also provides reliable keyboard
pointer buttons before events reach the compositor:

- `Super+J` is a holdable left button (tap, double-click and drag);
- `Super+K` is a holdable right button;
- while `Super+N` is held, vertical touchpad motion becomes wheel scrolling.

These combinations suppress Super at the physical input boundary and emit
buttons through a separate virtual pointer. They never synthesize a Super
release on an unrelated device, so quick presses and either key-release order
remain balanced.

The implementation is a small optimized Rust daemon. It uses `EVIOCGRAB` on
the built-in keyboard and mirrors every normal key through `/dev/uinput`.
Stopping or crashing the process closes the file descriptor, so the kernel
immediately releases the physical keyboard.

## Supported hardware

- Tested target: ASUS Zenbook S 16 UX5606SA
- Expected assistant-key sequence: `KEY_LEFTMETA`, `KEY_LEFTSHIFT`, `KEY_F23`
- Default keyboard path: `/dev/input/by-path/platform-i8042-serio-0-event-kbd`

Run `asus-copilot-meta2 doctor` after installation. A different input path can
be passed as the last argument to both `doctor` and `run`.

## Install on CachyOS / Arch Linux

```bash
git clone https://github.com/mickberrad659-sketch/asus-copilot-meta2.git
cd asus-copilot-meta2
./install.sh
```

The installer builds a release binary, installs a user systemd service, adds
udev access for the keyboard and `/dev/uinput`, and generates
`~/.config/niri/asus-meta2-alt-caps.xkb`. It does not overwrite the Niri config.

Use the generated keymap in Niri:

```kdl
input {
    keyboard {
        xkb {
            file "~/.config/niri/asus-meta2-alt-caps.xkb"
        }
    }
}
```

Then bind the new modifier independently from Super:

```kdl
binds {
    ISO_Level5_Shift+S { spawn "your-command"; }
}
```

Niri reloads its configuration without logging out. Your existing
`grp:caps_toggle` switches the default `us,ru` layout. The Caps Lock LED is
off for English and on for Russian (`grp_led:caps`).
The generated keymap keeps plain Caps Lock as the layout toggle and moves the
real capitalization lock to Alt+Caps Lock, without changing the layout LED.
For another layout, install with environment overrides:

```bash
ASUS_META2_LAYOUT="us,de" ASUS_META2_OPTIONS="grp:caps_toggle,grp_led:caps" ./install.sh
```

## Verify and troubleshoot

```bash
asus-copilot-meta2 doctor
systemctl --user status asus-copilot-meta2.service
journalctl --user -u asus-copilot-meta2.service -b
```

Emergency stop (the physical keyboard is released immediately):

```bash
systemctl --user stop asus-copilot-meta2.service
```

To disable it permanently:

```bash
systemctl --user disable --now asus-copilot-meta2.service
```

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

The filter has unit tests for the ASUS chord, ordinary Super/Shift shortcuts,
and keys typed while Meta2 is held. No keystrokes are stored or sent anywhere.

## License

MIT
