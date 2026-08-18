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