# VamoraSys

VamoraSys is native Rust Linux system middleware exposed through the
`vamorasys` CLI. It resolves system defaults from `/etc/VamoraSys/settings`
with writable per-user overrides from `~/.VamoraSys/settings`.

## Build

```bash
cargo build --release
cargo test
```

The binary is `target/release/vamorasys`.

## Commands

```bash
vamorasys status
vamorasys settings get appearance.theme
vamorasys settings set appearance.theme dark
vamorasys settings reset appearance.theme
vamorasys settings list vamify.db
vamorasys settings dump vamify.db
vamorasys settings databases
vamorasys settings tables
vamorasys keyboard status
vamorasys keyboard list
vamorasys keyboard add fr
vamorasys keyboard set fr
vamorasys keyboard next
vamorasys keyboard apply
vamorasys device status
vamorasys device get form_factor
vamorasys device get tier
vamorasys --json status
vamorasys --json device status
vamorasys --json settings list vamify.db
```

`status` reads release metadata from `/etc/VamoraSys/vamora-release.vmf`
first, then `/etc/VamoraSys/vamora-release`. It validates `VMF_VERSION` and
`AL_NAME`. `VERSION_ID`/`VERSION` identify VamoraOS, while
`VAMORASYS_VERSION` identifies the VamoraSys binary and is never used as the OS
version. VamoraSys never overwrites an existing release file; a bundled
`vamora-release.vmf` is installed only when the system has no release file.

## Device type and performance

`vamorasys device` tells apps what kind of device they run on, so they stop guessing from screen size
(a desktop can have a tiny screen; a tablet can have a huge one).

```bash
vamorasys device status                 # table: value + where each value came from
vamorasys device get form_factor        # phone, flip-phone, foldable-phone, tablet, foldable-tablet,
                                        # laptop, foldable-laptop, cyberdeck, desktop, tv, watch
vamorasys device setup                  # interactive wizard that writes /etc/VamoraSys/device.vmf
vamorasys device get posture            # foldables: folded | half-open | unfolded (otherwise "none")
vamorasys device get tier               # budget | entry | midrange | flagship
vamorasys device get animations         # also: blur target_fps background_processes preload image_cache_mb
vamorasys --json device status
```

Other values: `diagonal_in`, `ram_mb`, `cpu_cores`, `max_cpu_mhz`, `touch`, `keyboard`, `pointer`, `battery`
(`unknown` / `null` when they cannot be read).

The **tier** gives ready-made adaptation hints. They are ceilings, not targets: an app may always do less.

| tier     | animations | blur | target_fps | background_processes | preload | image_cache_mb |
|----------|-----------|------|-----------|----------------------|---------|----------------|
| budget   | minimal   | off  | 30        | 2                    | off     | 16             |
| entry    | reduced   | off  | 60        | 4                    | off     | 48             |
| midrange | full      | on   | 60        | 8                    | on      | 128            |
| flagship | full      | on   | 120       | 16                   | on      | 256            |

What the form factors mean:

- **laptop**: a built-in keyboard and a screen. A convertible that reports a lid angle is still a laptop.
- **foldable-laptop**: one big folding screen (12 in or more) with no keyboard of its own. The keyboard is on screen or wireless.
- **foldable-tablet / foldable-phone / flip-phone**: smaller folding screens (a flip phone is the clamshell kind).
- **cyberdeck**: a DIY mini computer with a small screen (10.5 in or less) and a keyboard, usually a Raspberry Pi or
  another single-board computer. The same board on a big monitor is a `desktop`, and so is a tower with a tiny screen.
- **desktop**, **tablet**, **phone**, **tv**, **watch** as you would expect. Size is only a tie-breaker, never the decider.

A keyboard counts as built in when it is a PS/2, I2C/SPI-HID or platform device; Bluetooth and USB keyboards count as
external. Hardware cannot always tell, so a `device.vmf` always wins.

A `watch` is always capped further (reduced animations, no blur, at most 2 background processes).
Use the form factor for *what the device is* (input model, hinge, TV-style navigation) and the window
width for *layout*; never one for the other.

### Setup wizard

When `device.vmf` is missing (or broken), `vamorasys device status` in a terminal offers a keyboard-driven wizard in the
style of Vaminfo: arrow keys or `j`/`k` to move, Enter to choose, Esc to go back, `q` to cancel. It asks for the form
factor, the tier and (optionally) each adaptation hint, shows a review screen and writes the file. Run it any time
with `vamorasys device setup`; it starts from the values already in the file. Saving to `/etc/VamoraSys` needs root
(`sudo vamorasys device setup`). Apps and scripts never get a prompt: nothing is asked unless both stdin and stdout are
a terminal, and `device get` and `--json` never ask.

### device.vmf

`/etc/VamoraSys/device.vmf` is written by the device image, next to `vamora-release.vmf`, and uses exactly the
same syntax (`VMF_VERSION="1"`, `KEY="value"`, UPPERCASE keys, `#` comments). See `examples/device.vmf`.

| field | values |
|-------|--------|
| `FORM_FACTOR` | any form factor above; authoritative |
| `FOLD_STYLE` | `book` or `flip` (foldables) |
| `TIER` | `budget`, `entry`, `midrange`, `flagship` |
| `ANIMATIONS`, `BLUR`, `TARGET_FPS`, `BACKGROUND_PROCESSES`, `PRELOAD`, `IMAGE_CACHE_MB` | replace one hint |

Every field is optional and an empty value means "not set". `FORM_FACTOR` also accepts `cyberdeck`. Without a `device.vmf` everything is detected:
declared profile first, then hardware facts (SMBIOS chassis type, built-in battery, touch/keyboard/pointer/lid
inputs, modem, HDMI-CEC, hinge sensor, real panel size from EDID), then heuristics. The tier is estimated from RAM
and CPU, and the weaker of the two wins (unknown hardware is `midrange`). Foldable type cannot always be detected,
so ship a `device.vmf` on foldable images.

A malformed `device.vmf` (syntax error, unknown field, bad value) is an error naming the problem, and
`vamorasys status` reports it as unhealthy; it is never silently ignored. `device` is read-only: it needs no home
directory and never creates or migrates settings databases. `install.sh` and `update.sh` never touch `device.vmf`,
and no `device.vmf` is bundled, because a default would claim the wrong device. For isolated checks
`VAMORASYS_PROBE_ROOT` points the hardware probe at a fake tree instead of `/`, and `VAMORASYS_RELEASE_DIR` moves
`device.vmf` like the release file.

## Keyboard layout picker

`vamorasys keyboard pick` chooses a layout from a searchable list (all XKB layouts with their names) instead of typing a
code like `us` or `fr`. Type to search, arrows / PageUp / PageDown / Home / End to move, Enter to choose, Esc to cancel.
A layout that is not in the cycle yet is added and then made active, exactly like `keyboard add` followed by
`keyboard set`. All existing keyboard commands work exactly as before.

## Colors

The accent color in the terminal output (titles, the wizard and the pickers) is `appearance.accent_color` from the settings
database: your user value, else the factory value `#2563eb`. Terminals cannot all show every color, so it is mapped
to the closest one the terminal supports: the exact color on true-color terminals (`COLORTERM=truecolor`), the nearest
of the 256-color palette on 256-color terminals, the nearest of the 16 ANSI colors otherwise. The green/red/yellow
status colors are mapped the same way. `NO_COLOR` or `TERM=dumb` turns color off. Output that is not going to a
terminal keeps the 24-bit sequences it always had, so with the factory accent it is byte-for-byte what it was.

## Settings layers

Normal commands read and write the current user's settings database. When a
user database is first initialized, it receives a copy of the current system
defaults; each user's values can then diverge independently:

```bash
vamorasys settings set appearance.theme dark
vamorasys settings set --database apps/calendar.db calendar.week_start monday
```

Run a write command with `sudo` to change the system default in
`/etc/VamoraSys/settings/<database>.db`. App databases use the matching
`apps/<database>.db` path under both the system and user settings roots:

```bash
sudo vamorasys settings set appearance.theme dark
sudo vamorasys settings set --database apps/calendar.db calendar.week_start monday
```

`sudo ... settings reset ...` restores a built-in VamoraSys default when one
exists. Without `sudo`, `reset` removes only the current user's value.

All databases under `settings/apps/` are discovered, displayed, and layered
the same way as core settings databases. App setting commands require
`--database` because app tables are owned by the app.

The `vamify.db` database includes `statusbar.size`, `statusbar.transparency`,
`dock.position`, and an `icons` table. `statusbar.size` defaults to 30 px and
accepts values from 29 to 128 px. For comparison, a standard Windows 10/11
taskbar is usually about 48 px at 100% display scaling. Icon settings include
`icons.corner_radius`, `icons.style`, `icons.bg_color`, `icons.stroke`, and
`icons.stroke_color`. The supported icon styles are `normal`, `light`, `dark`,
and `single_color`; colors accept any standard `#RGB`, `#RGBA`, `#RRGGBB`, or
`#RRGGBBAA` value.
The keyboard manager stores its configuration in the `keyboard` table:

```bash
vamorasys settings get statusbar.size
vamorasys settings set statusbar.size 36
vamorasys settings set statusbar.transparency true
vamorasys settings set dock.position left
vamorasys settings set keyboard.layouts us,fr
```

Built-in VamoraOS settings are validated before they are written. For example,
themes must be `light`, `dark`, or `system`; dock positions must be `top`,
`bottom`, `left`, or `right`; colors must be hexadecimal; and numeric settings
are checked against their supported ranges. Unknown settings in core databases
are rejected. App databases remain extensible because each app owns and
defines its own settings schema.

`homescreen.grid` is the exception to range checks: it takes any `WIDTHxHEIGHT` of whole numbers
(`4x6`, `13x6`, `20x20`, ...). VamoraOS runs on very different devices, so VamoraSys only checks the shape and
the Settings app decides which sizes make sense.

Wayland does not define a compositor-independent keyboard-layout control
protocol. `vamorasys keyboard` therefore keeps the configuration in the
database and applies it at runtime through compositor adapters for Sway,
Hyprland, GNOME, and labwc:

```bash
vamorasys keyboard add fr
vamorasys keyboard set fr
vamorasys keyboard next
vamorasys keyboard status
```

On another compositor, the layout is still persisted and `keyboard status`
reports `unsupported`; add a compositor adapter before treating runtime
application as available.

labwc reads its XKB map from `~/.config/labwc/environment`. VamoraSys
preserves the rest of that file, writes the selected layout first, and runs
`labwc --reconfigure`. labwc does not rebuild the active XKB map during a
reconfigure, so changing to a layout that was not already loaded requires
restarting labwc once. When multiple layouts are configured, the adapter adds
`grp:alt_shift_toggle`, so `Alt+Shift` cycles them after the restart.

Add `--json` before or after the command to receive valid pretty-printed JSON
for status, settings, initialization, and errors. JSON output never contains
terminal color escape codes. `settings tables` identifies the per-user owner
by account name instead of the generic word `user`.

## Installation and updates

`install.sh` is a complete destructive reinstall: it replaces the binary,
recreates factory and user databases, applies factory defaults, sets
permissions, and runs `status`. It preserves the release metadata file.
Run it as your normal user (`./install.sh`); it runs `cargo build` without
root and uses `sudo` only for system-owned paths. Running
`sudo ./install.sh` is also supported and drops the build back to the
invoking user.

`update.sh` only replaces the compiled binary. It does not initialize,
recreate, reset, or delete any databases or settings.
Run it as your normal user so compilation stays outside root; `sudo` is used
only when replacing `/usr/local/bin/vamorasys`.

For non-system test installations, override `VAMORASYS_SYSTEM_ROOT`,
`VAMORASYS_USER_ROOT`, and `VAMORASYS_BINARY`. The CLI also accepts
`VAMORASYS_SYSTEM_DIR`, `VAMORASYS_USER_DIR`, `VAMORASYS_RELEASE_DIR`, and
`VAMORASYS_PROBE_ROOT` for isolated checks.