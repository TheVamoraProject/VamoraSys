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
    └── settings.vmf

/etc/VamoraSys/bin/         System executables
└── <binary>

/etc/VamoraSys/utilities/   System utilities
└── <utility>

~/.vamorasys/               User settings (per-user)
└── <category>/
    └── settings.vmf
```

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
vamorasys settings <action> <category> <key> [value]

# System layer (requires root)
vamorasys settings <action> <category> <key> [value] --system
```

#### `put` — Add or update a key

```bash
vamorasys settings put theme COLOR Red
```

Creates the key if it doesn't exist, updates it if it does. Idempotent.

#### `get` — Read a value

```bash
vamorasys settings get theme COLOR
# Red
```

Outputs the bare value to stdout. Checks user layer first, falls back to system default. Useful in scripts:

```bash
BG=$(vamorasys settings get theme COLOR)
```

#### `delete` — Remove a key

```bash
vamorasys settings delete theme COLOR
```

Removes the key from the user layer only. After deletion, `get` falls back to the system default if one exists.

#### `list` — Show all settings for a category

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

### `vmf`

Operates directly on any `.vmf` file, bypassing the layer system entirely. Useful for app installers and scripts.

```bash
vamorasys vmf <file> <action> <key> [value]
```

**Example:**

```bash
vamorasys vmf /etc/VamoraSys/default/theme/settings.vmf list
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

Ensures a category exists in the user config. If it doesn't, copies it from the system default.

```bash
vamorasys check theme
```

Useful in app post-install scripts that need to guarantee a category is present before writing to it.

---

## ⚡ Auto-Initialization

VamoraSys handles missing user directories automatically. No manual setup needed.

On any command that touches the user layer:

1. Does `~/.vamorasys/` exist? → No: create it
2. Does `~/.vamorasys/<category>/` exist? → No: copy from `/etc/VamoraSys/default/<category>/`
3. Proceed with the command

**Example — internal flow for `vamorasys settings get theme COLOR`:**

```
1. Check ~/.vamorasys/ exists       → create if not
2. Check ~/.vamorasys/theme/ exists → copy from /etc/VamoraSys/default/theme/ if not
3. Check user layer for COLOR
4. Not found → fall back to system default
5. Output value
```

---

## 📊 Behavior Reference

| Situation | Behavior |
|---|---|
| New user, no commands run yet | Nothing created |
| First `vamorasys` command runs | `~/.vamorasys/` created automatically |
| Category missing from user layer | Copied from system default |
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

## 🎯 Design Principles# VamoraSys — Configuration Engine

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

---

## VMF format

```ini
COLOR="Blue"
DARK_MODE="true"
FONT_FAMILY="JetBrains Mono"
```

**Rules:**


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
