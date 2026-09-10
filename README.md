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
vamorasys --json status
vamorasys --json settings list vamify.db
```

`status` reads release metadata from `/etc/VamoraSys/vamora-release.vmf`
first, then `/etc/VamoraSys/vamora-release`. It validates `VMF_VERSION` and
`AL_NAME`. `VERSION_ID`/`VERSION` identify VamoraOS, while
`VAMORASYS_VERSION` identifies the VamoraSys binary and is never used as the OS
version. VamoraSys never overwrites an existing release file; a bundled
`vamora-release.vmf` is installed only when the system has no release file.

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
`VAMORASYS_SYSTEM_DIR`, `VAMORASYS_USER_DIR`, and `VAMORASYS_RELEASE_DIR`
for isolated checks.