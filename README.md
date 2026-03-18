<div align="center">

# VamoraSys

**Configuration engine for VamoraOS — manages `.vmf` files through a clean, composable CLI.**

[![GitHub Release](https://img.shields.io/github/v/release/TheVamoraProject/VamoraSys?style=flat-square&color=blue)](https://github.com/TheVamoraProject/VamoraSys/releases)
[![GitHub Stars](https://img.shields.io/github/stars/TheVamoraProject/VamoraSys?style=flat-square&color=yellow)](https://github.com/TheVamoraProject/VamoraSys/stargazers)
[![License](https://img.shields.io/github/license/TheVamoraProject/VamoraSys?style=flat-square&color=green)](https://github.com/TheVamoraProject/VamoraSys/blob/main/LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/TheVamoraProject/VamoraSys/build.yml?style=flat-square)](https://github.com/TheVamoraProject/VamoraSys/actions)

*A component of [VamoraOS](https://vamora.vercel.app/projects/vamoraos)*

</div>

---

## 📋 Table of Contents

- [Overview](#-overview)
- [Directory Layout](#-directory-layout)
- [VMF Format](#-vmf-format)
- [Commands](#-commands)
  - [settings](#settings)
  - [vmf](#vmf)
  - [exec](#exec)
  - [utility](#utility)
  - [check](#check)
  - [info](#info)
- [Auto-Initialization](#-auto-initialization)
- [Behavior Reference](#-behavior-reference)
- [Exit Codes](#-exit-codes)
- [Design Principles](#-design-principles)

---

## 🧩 Overview

VamoraSys uses a **layered configuration system**:

```
User config  >  System default
```

Each user has their own config folder under `~/.vamorasys/`. The system provides fallback defaults under `/etc/VamoraSys/default/`. When a key is missing from the user layer, VamoraSys falls back to the system default automatically.

---

## 📁 Directory Layout

```
/etc/VamoraSys/default/     System defaults / templates (root-owned)
└── <category>/
    ├── settings.vmf        ← default sub-category
    ├── colors.vmf
    └── fonts.vmf

/etc/VamoraSys/bin/         System executables
└── <binary>

/etc/VamoraSys/utilities/   System utilities
└── <utility>

~/.vamorasys/               User settings (per-user)
└── <category>/
    ├── settings.vmf
    ├── colors.vmf
    └── fonts.vmf
```

Each `.vmf` file inside a category folder is a **sub-category**. The file `settings.vmf` is the default sub-category used when none is specified.

---

## 📄 VMF Format

VMF is a flat key-value format:

```ini
COLOR="Blue"
DARK_MODE="true"
FONT_FAMILY="JetBrains Mono"
```

**Rules:**
- Keys are `SCREAMING_SNAKE_CASE` — always
- Multi-word keys use underscores: `FONT_FAMILY`, `DARK_MODE`
- One key per line
- Lines starting with `#` are comments — preserved across reads and writes

> **📦 Categories are app-defined.** VamoraSys ships with zero categories. They are created by apps and components installed on top of VamoraOS. VamoraSys treats categories as plain directories and does not validate their names.

---

## 🛠️ Commands

### `settings`

Reads and writes settings through the layer system.

```bash
# User layer (default)
vamorasys settings <action> <category> [subcategory] <key> [value]

# System layer (requires root)
vamorasys settings <action> <category> [subcategory] <key> [value] --system
```

The `subcategory` is optional. When omitted, the command targets `settings.vmf`. When provided, it targets `<subcategory>.vmf` inside the category folder.

```
vamorasys settings get theme COLOR          →  theme/settings.vmf
vamorasys settings get theme colors COLOR   →  theme/colors.vmf
```

---

#### `put` — Add or update a key

```bash
# Default sub-category (settings.vmf)
vamorasys settings put theme COLOR Red

# Explicit sub-category (colors.vmf)
vamorasys settings put theme colors COLOR Red
```

Creates the key if it doesn't exist, updates it if it does. Idempotent.

---

#### `get` — Read a value

```bash
vamorasys settings get theme COLOR
# Red

vamorasys settings get theme colors COLOR
# Red
```

Outputs the bare value to stdout. Checks user layer first, falls back to system default. Useful in scripts:

```bash
BG=$(vamorasys settings get theme colors COLOR)
```

---

#### `delete` — Remove a key

```bash
vamorasys settings delete theme COLOR
vamorasys settings delete theme colors COLOR
```

Removes the key from the user layer only. After deletion, `get` falls back to the system default if one exists.

---

#### `list` — Show all settings for a category or sub-category

```bash
# List the default sub-category (settings.vmf)
vamorasys settings list theme

# List a specific sub-category (colors.vmf)
vamorasys settings list theme colors
```

```
COLOR="Red"
DARK_MODE="true"
FONT_FAMILY="JetBrains Mono"
```

Shows the fully merged result — user values where set, system defaults where not. Raw VMF output, pipeable.

---

### `vmf`

Operates directly on any `.vmf` file, bypassing the layer system entirely. Useful for app installers and scripts.

```bash
vamorasys vmf <file> <action> [key] [value]
```

**Example:**

```bash
vamorasys vmf /etc/VamoraSys/default/theme/colors.vmf list
vamorasys vmf /etc/VamoraSys/default/theme/colors.vmf put COLOR Blue
```

---

### `exec`

Executes a binary from `/etc/VamoraSys/bin/`.

```bash
vamorasys exec <thing>
```

A simple, consistent way to run VamoraSys system binaries without knowing their full path.

---

### `utility`

Executes a utility from `/etc/VamoraSys/utilities/`.

```bash
vamorasys utility <thing>
```

Same idea as `exec`, but for lighter tools and helpers that ship with VamoraOS components.

---

### `check`

Ensures a category (or specific sub-category) exists in the user config. If it doesn't, copies it from the system default.

```bash
# Ensure the whole category is initialized
vamorasys check theme

# Ensure a specific sub-category file exists
vamorasys check theme colors
```

Useful in app post-install scripts that need to guarantee a category is present before writing to it.

---

### `info`

Prints VamoraSys version, paths, and design summary.

```bash
vamorasys info
```

---

## ⚡ Auto-Initialization

VamoraSys handles missing user directories automatically. No manual setup needed.

On any command that touches the user layer:

1. Does `~/.vamorasys/` exist? → No: create it
2. Does `~/.vamorasys/<category>/` exist? → No: copy from `/etc/VamoraSys/default/<category>/`
3. If a sub-category is specified and its `.vmf` file is missing → copy that file from the system default
4. Proceed with the command

**Example — internal flow for `vamorasys settings get theme colors COLOR`:**

```
1. Check ~/.vamorasys/ exists              → create if not
2. Check ~/.vamorasys/theme/ exists        → copy from /etc/VamoraSys/default/theme/ if not
3. Check ~/.vamorasys/theme/colors.vmf     → copy from system default if not
4. Check user layer for COLOR
5. Not found → fall back to system default
6. Output value
```

---

## 📊 Behavior Reference

| Situation | Behavior |
|---|---|
| New user, no commands run yet | Nothing created |
| First `vamorasys` command runs | `~/.vamorasys/` created automatically |
| Category missing from user layer | Copied from system default |
| Sub-category file missing from user layer | That `.vmf` file copied from system default |
| Key missing from user layer | Falls back to system default |
| Key missing from both layers | ❌ Fails with error |
| `--system` flag | Reads/writes `/etc/VamoraSys/default/` instead |
| `exec <thing>` not found | ❌ Fails with error |
| `utility <thing>` not found | ❌ Fails with error |

---

## 🚦 Exit Codes

| Code | Meaning |
|---|---|
| `0` | ✅ Success |
| `1` | ❌ File, category, or executable not found |
| `2` | ❌ Key not found in either layer |
| `3` | 🔒 Permission denied |
| `4` | ⚠️ Invalid VMF syntax |

---

## 💡 Example Workflow

```bash
# App installer registers system defaults
vamorasys settings put theme COLOR "Blue" --system
vamorasys settings put theme colors ACCENT "#8B5CF6" --system
vamorasys settings put theme fonts FONT_FAMILY "JetBrains Mono" --system

# User customizes their own config
vamorasys settings put theme COLOR "Red"
vamorasys settings put theme colors ACCENT "#EF4444"

# A script reads the resolved values
COLOR=$(vamorasys settings get theme COLOR)
ACCENT=$(vamorasys settings get theme colors ACCENT)

# Direct VMF editing (bypasses layer system)
vamorasys vmf /etc/VamoraSys/default/theme/colors.vmf list

# Ensure a sub-category exists before writing to it
vamorasys check theme fonts

# Run a system binary
vamorasys exec vamora-hello

# Run a utility
vamorasys utility fetch-updates
```

---

## 🎯 Design Principles

- 🚫 **No daemon** — `vamorasys` is a stateless binary that reads and writes files directly
- 🔗 **Composable** — stdout output, predictable exit codes, plays well with pipes and scripts
- 🧱 **Flat format** — VMF has no nesting by design
- 🔎 **Layer transparency** — `list` always shows the fully resolved merge
- 📦 **App-driven** — VamoraSys is infrastructure; apps own their config structure
- 🚪 **Single entry point** — settings, file editing, and execution all go through one binary
- 🔒 **Stable API** — the main `vamorasys` binary interface never changes

---

<div align="center">
  <sub>Part of the <a href="https://vamora.vercel.app">Vamora</a> ecosystem · <a href="https://github.com/TheVamoraProject/VamoraSys">TheVamoraProject/VamoraSys</a></sub>
</div>
