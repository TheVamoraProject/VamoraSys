# VamoraSys — Configuration Engine

**Component of VamoraOS · Concept Specification**

---

## Overview

**VamoraSys** is the configuration engine of VamoraOS, designed to manage `.vmf` files through a clean command-line interface.

It supports:
- Fast config editing
- Per-user customization
- System defaults with smart fallback
- System binary and utility execution

---

## Core concept

VamoraSys uses a **layered configuration system**:

```
User config  >  System default
```

- Each user has their own config folder
- System provides fallback defaults
- Only overridden values are stored per user

---

## VMF format

```ini
COLOR="Blue"
DARK_MODE="true"
FONT_FAMILY="JetBrains Mono"
```

**Rules:**
- Keys are `SCREAMING_SNAKE_CASE` — always
- Multi-word keys use a single underscore: `FONT_FAMILY`, `DARK_MODE`
- Values are always double-quoted, no exceptions
- One key per line
- Lines starting with `#` are comments, preserved across reads/writes

---

## Categories are app-defined

VamoraSys ships with **zero categories**. They are created entirely by apps and components installed on top of VamoraOS.

- A fresh system may have no categories at all
- Categories appear when apps are installed
- VamoraSys does not validate category names — it treats them as plain directories
- Apps are responsible for creating and documenting their own category structure

**Example:** if a Vamora Desktop Environment is installed, it might create:

```
/etc/VamoraSys/default/theme/settings.vmf
/etc/VamoraSys/default/ui/settings.vmf
/etc/VamoraSys/default/fonts/settings.vmf
```

**Dev rules:**
- Apps must create their own category folders under `/etc/VamoraSys/default/`
- Apps define their own `.vmf` structure
- VamoraSys does not validate category names
- VamoraSys treats categories as just folders

---

## Directory layout

```
/etc/VamoraSys/default/          System defaults (root-owned)
└── <category>/
    └── settings.vmf

/etc/VamoraSys/bin/              System executables
└── <binary>

/etc/VamoraSys/utilities/        System utilities
└── <utility>

~/.vamorasys/                    User overrides (per-user)
└── <category>/
    └── settings.vmf

/etc/skel/.vamorasys/            Template for new users (optional)
└── <category>/
    └── settings.vmf
```

---

## Command structure

VamoraSys has five modes:

### Mode 1 — User settings (default)

```bash
vamorasys settings <action> <category> <key> [value]
```

Affects the current user (`~/.vamorasys/`).

### Mode 2 — System settings (global)

```bash
vamorasys settings <action> <category> <key> [value] --system
```

Affects all users (`/etc/VamoraSys/default/`). Requires root.

### Mode 3 — Direct VMF editing

```bash
vamorasys vmf <file> <action> <key> [value]
```

Operates directly on any `.vmf` file. Bypasses the layer system entirely — useful for app installers and scripts.

### Mode 4 — Execute a binary

```bash
vamorasys exec <thing>
```

Executes `/etc/VamoraSys/bin/<thing>`. A simple, consistent way to run VamoraOS system binaries without needing to know their full path.

### Mode 5 — Execute a utility

```bash
vamorasys utility <thing>
```

Executes `/etc/VamoraSys/utilities/<thing>`. Same idea as `exec`, but for utilities — lighter tools and helpers that ship with VamoraOS components.

---

## Commands

**`put`** — add or update a key

```bash
vamorasys settings put theme COLOR Red
```

Creates the key if it doesn't exist, updates it if it does. Idempotent.

---

**`get`** — read a value

```bash
vamorasys settings get theme COLOR
# Red
```

Outputs the bare value to stdout. Checks user layer first, falls back to system default. Useful in scripts:

```bash
BG=$(vamorasys settings get theme COLOR)
```

---

**`delete`** — remove a key

```bash
vamorasys settings delete theme COLOR
```

Removes the key from the user layer only. After deletion, `get` falls back to the system default if one exists.

---

**`list`** — show all settings for a category

```bash
vamorasys settings list theme
```

```
COLOR="Red"
DARK_MODE="true"
FONT_FAMILY="JetBrains Mono"
```

Shows the fully merged result — user values where set, system defaults where not. Raw VMF output, pipeable.

---

## Check command

```bash
vamorasys check theme
```

Ensures the category exists in the user config. If it doesn't, copies it from `/etc/VamoraSys/default/`. Useful for app postinstall scripts that need to guarantee a category is present before writing to it.

---

## Auto-initialization

VamoraSys handles missing user directories automatically. No manual setup needed.

On any command that touches the user layer:

```
1. Does ~/.vamorasys/ exist?             No → create it
2. Does ~/.vamorasys/<category>/ exist?  No → copy from /etc/VamoraSys/default/<category>/
3. Proceed with the command
```

**Full internal flow example:**

```bash
vamorasys settings get theme COLOR
```

Internally:
```
1. Check ~/.vamorasys/ exists       → create if not
2. Check ~/.vamorasys/theme/ exists → copy from /etc/VamoraSys/default/theme/ if not
3. Check user layer for COLOR
4. Not found → fall back to system default
5. Output value
```

---

## /etc/skel integration (optional)

Pre-populate configs for new users by placing defaults in:

```
/etc/skel/.vamorasys/
```

When a new user is created with `useradd -m`, the system automatically copies `/etc/skel/*` into their home directory — giving them a pre-configured baseline without any extra tooling.

---

## Behavior reference

| Situation | Behavior |
|-----------|----------|
| New user, no commands run yet | Nothing created |
| First command runs | `~/.vamorasys/` created automatically |
| Category missing from user layer | Copied from system default |
| Key missing from user layer | Falls back to system default |
| Key missing from both layers | Fails with error |
| `--system` flag | Reads/writes `/etc/VamoraSys/default/` instead |
| `exec <thing>` not found | Fails with error |
| `utility <thing>` not found | Fails with error |

---

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | File, category, or executable not found |
| `2` | Key not found in either layer |
| `3` | Permission denied |
| `4` | Invalid VMF syntax |

---

## Example workflow

```bash
# App installer registers system defaults
vamorasys settings put theme COLOR "Blue" --system
vamorasys settings put theme DARK_MODE "false" --system

# User customizes their own config
vamorasys settings put theme COLOR "Red"
vamorasys settings put theme DARK_MODE "true"

# A script reads the resolved value
COLOR=$(vamorasys settings get theme COLOR)

# Direct VMF editing (bypasses layer system)
vamorasys vmf /etc/VamoraSys/default/theme/settings.vmf list

# Run a system binary
vamorasys exec vamora-hello

# Run a utility
vamorasys utility fetch-updates
```

---

## Design principles

- **No daemon** — `vamorasys` is a stateless binary that reads and writes files directly
- **Composable** — stdout output, predictable exit codes, plays well with pipes and scripts
- **Flat format** — VMF has no nesting by design
- **Layer transparency** — `list` always shows the fully resolved merge
- **App-driven** — VamoraSys is infrastructure; apps own their config structure
- **Single entry point** — settings, file editing, and execution all go through one binary
