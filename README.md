# VamoraSys Configuration Architecture

## Goal

Create a stable and future-proof configuration system for VamoraSys that:

* keeps system settings organized
* allows apps to integrate with the Settings app
* provides consistent theme/colors across all apps
* avoids breaking changes later
* supports both simple and complex apps
* remains lightweight and easy to debug

---

# Core Principles

## 1. Registry != Config

Registry files only describe:

* app metadata
* settings pages
* config backend
* paths
* permissions

Registry files DO NOT contain actual settings.

---

## 2. Apps own their configs

Each app stores its own settings.

Examples:

```text
/etc/VamoraSys/apps/browser/settings.vmf
/etc/VamoraSys/apps/music/settings.db
```

The Settings app edits those files.

Apps read those files.

---

## 3. System settings stay separated

Core OS settings are stored only in:

```text
/etc/VamoraSys/settings/settings.db
```

Contains:

* themes
* wallpaper
* language
* audio
* networking
* accessibility
* appearance

NOT app-specific data.

---

## 4. Apps never access system internals directly

Apps must NEVER manually parse `settings.db` or theme preset files.

Instead use the stable API provided by `libvamora-settings`.

This is the most important rule for long-term compatibility.

---

# Filesystem Layout

```text
/etc/VamoraSys/
│
├── settings/                     # Core OS settings
│   │
│   ├── settings.db               # Main SQLite database for system settings
│   │                              # Stores ONLY OS-level settings
│   │
│   ├── defaults.db               # Factory/default settings backup
│   │
│   ├── migrations/               # Database migration scripts
│   │   ├── v1.sql
│   │   └── v2.sql
│   │
│   └── themes/                   # Installed theme definitions (NOT read by apps directly)
│       ├── dark.theme.json
│       ├── light.theme.json
│       └── amoled.theme.json
│
│
├── registry/                     # Settings registry — describes how Settings app loads pages
│   │
│   ├── browser.reg.json
│   ├── gallery.reg.json
│   ├── music.reg.json
│   │
│   ├── categories.json           # Settings categories metadata
│   │
│   └── schemas/                  # Validation schemas
│       ├── registry.schema.json
│       ├── vmf.schema.json
│       └── theme.schema.json
│
│
├── apps/                         # App-owned configuration storage
│   │
│   ├── browser/
│   │   ├── settings.vmf          # Simple app settings (VMF format)
│   │   ├── cache/                # Temporary cache data
│   │   │   └── session.tmp
│   │   ├── data/                 # Persistent user/app data
│   │   │   └── bookmarks.db
│   │   └── assets/               # App-owned assets
│   │       └── icon.png
│   │
│   ├── music/
│   │   ├── settings.db           # Complex settings database
│   │   ├── cache/
│   │   │   └── artwork.cache
│   │   └── data/
│   │       └── library.db
│   │
│   └── gallery/
│       ├── settings.vmf
│       ├── cache/
│       └── data/
│
│
├── themes/                       # Runtime active theme — what apps actually read
│   │
│   ├── active/                   # Currently applied theme values (compiled from theme preset)
│   │   ├── colors.json
│   │   ├── radius.json
│   │   └── fonts.json
│   │
│   ├── defaults/                 # Fallback values used if active theme is missing a key
│   │   ├── colors.json
│   │   ├── radius.json
│   │   └── fonts.json
│   │
│   └── wallpapers/               # System wallpapers
│       ├── default.jpg
│       └── dark.jpg
│
│
├── device-info/                  # Device and build information
│   │
│   ├── hardware.json
│   ├── firmware.json
│   ├── build.json
│   │
│   └── updater/
│       ├── channels.json
│       └── updater.conf
│
│
├── cache/                        # Global system cache
│   ├── thumbnails/
│   ├── package-cache/
│   └── temp/
│
│
├── logs/                         # System/service logs
│   ├── settings.log
│   ├── theme.log
│   └── updater.log
│
│
├── bin/                          # Vamora system CLI tools
│   ├── vamctl                    # Settings and theme CLI utility
│   ├── vmfctl                    # VMF file utility
│   └── vamsettingsd              # Settings service daemon
│
│
└── lib/                          # Shared Vamora libraries (also symlinked into /usr/lib/)
    ├── libvamora-settings.so
    └── libvmf.so
```

---

# Settings Database Schema

File:

```text
/etc/VamoraSys/settings/settings.db
```

Table:

```sql
CREATE TABLE settings (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    type        TEXT NOT NULL,
    updated_at  INTEGER
);
```

Example values:

| key                | value  | type   |
|--------------------|--------|--------|
| system.theme       | dark   | string |
| system.language    | en_US  | string |
| display.brightness | 85     | int    |
| audio.volume       | 70     | int    |

Test data:

```sql
INSERT INTO settings VALUES
('system.theme',       'dark',   'string', 1740000000),
('system.language',    'en_US',  'string', 1740000000),
('display.brightness', '85',     'int',    1740000000),
('audio.volume',       '70',     'int',    1740000000);
```

---

# Registry File Format

File:

```text
/etc/VamoraSys/registry/browser.reg.json
```

Contents:

```json
{
  "version": 1,

  "id": "vamora.browser",
  "label": "Browser",
  "icon": "browser.png",

  "category": "internet",

  "config": {
    "backend": "vmf",
    "path": "/etc/VamoraSys/apps/browser/settings.vmf"
  },

  "settings": {
    "visible": true,
    "searchable": true
  }
}
```

---

# VMF Format

VMF is designed for:

* simple settings
* readability
* easy parsing

Format:

```ini
# Browser Settings

homepage=https://vamora.local
darkmode=true
tracking_protection=true

[downloads]
path=/home/downloads
ask_before_download=true
```

Rules:

* UTF-8 only
* comments start with `#`
* no nested objects
* no arrays
* keys use lowercase
* sections are optional but allowed

---

# Theme System

## Overview

Every app should automatically match:

* colors
* accent
* dark/light mode
* rounded corners
* font scaling

without hardcoding values.

---

## Theme Preset Files

Installed theme presets live at:

```text
/etc/VamoraSys/settings/themes/
```

Apps NEVER read these directly. These are source definitions, not runtime values.

Example — `dark.theme.json`:

```json
{
  "id": "dark",

  "colors": {
    "background":  "#121212",
    "surface":     "#1e1e1e",
    "surface_alt": "#242424",

    "primary":   "#7c4dff",
    "secondary": "#03dac6",

    "text":      "#ffffff",
    "text_muted":"#aaaaaa",

    "border":  "#2c2c2c",

    "success": "#4caf50",
    "warning": "#ff9800",
    "danger":  "#f44336"
  },

  "radius": {
    "small":  8,
    "medium": 16,
    "large":  24,
    "pill":   999
  },

  "font": {
    "family": "Inter",
    "scale":  1.0
  }
}
```

---

## Active Theme (Runtime)

When a theme is applied, its values are compiled into the runtime active files:

```text
/etc/VamoraSys/themes/active/colors.json
/etc/VamoraSys/themes/active/radius.json
/etc/VamoraSys/themes/active/fonts.json
```

Apps ONLY read from these files — never from the presets in `settings/themes/`.

This indirection is what allows theme internals to change without breaking apps.

Example — `colors.json`:

```json
{
  "background":  "#121212",
  "surface":     "#1e1e1e",
  "surface_alt": "#242424",
  "primary":     "#7c4dff",
  "secondary":   "#03dac6",
  "text":        "#ffffff",
  "text_muted":  "#aaaaaa",
  "border":      "#2c2c2c",
  "success":     "#4caf50",
  "warning":     "#ff9800",
  "danger":      "#f44336"
}
```

If a key is missing from the active theme, apps fall back to:

```text
/etc/VamoraSys/themes/defaults/
```

---

## How Apps Access Themes

Apps must use `libvamora-settings` — never parse files manually.

```c
vamora_get_color("primary")
vamora_get_radius("medium")
vamora_is_dark_mode()
vamora_get_font_scale()
```

This provides a stable ABI. If theme file formats change in the future, old apps still work because the library handles compatibility.

---

# Stable Naming Standards

These names must NEVER change once released.

## Theme Color Keys

```text
background
surface
surface_alt
primary
secondary
text
text_muted
border
success
warning
danger
```

## Radius Keys

```text
small
medium
large
pill
```

## Theme Modes

```text
light
dark
amoled
auto
```

## Registry Top-Level Keys

```text
version
id
label
icon
category
config
settings
```

## Registry `config` Sub-Keys

```text
backend
path
```

## Registry `settings` Sub-Keys

```text
visible
searchable
```

---

# Settings App Responsibilities

The Settings app:

* loads registry files from `/etc/VamoraSys/registry/`
* displays settings pages per registry descriptors
* edits VMF/database configs in `/etc/VamoraSys/apps/*/`
* handles search and filtering
* validates registry schema
* validates setting types

The Settings app does NOT:

* store app data
* own app configs
* directly modify app runtime state
* let apps edit registry files
* give apps direct access to `settings.db`

---

# Device Info Files

File:

```text
/etc/VamoraSys/device-info/hardware.json
```

Example:

```json
{
  "model": "Vamora Dev Device",
  "cpu": "Snapdragon 8 Gen 2",
  "ram_mb": 8192,
  "storage_gb": 256,
  "display": {
    "width": 2400,
    "height": 1080,
    "refresh_rate": 120
  }
}
```

---

# CLI Reference

## Settings

```bash
vamctl get system.theme
vamctl set audio.volume 70
vamctl list
```

## Themes

```bash
vamctl theme primary        # Read a theme color value
vamctl set-theme dark       # Apply a theme preset
vamctl reload-theme         # Recompile active theme from current preset
```

## VMF

```bash
vmfctl validate settings.vmf
vmfctl get homepage
vmfctl set darkmode true
```

---

# Shared Libraries

## libvamora-settings

Handles:

* themes (color, radius, font access)
* settings access
* validation
* config reading
* backward compatibility across theme format versions

## libvmf

Handles:

* VMF parsing and writing
* schema validation

Both are installed to `/etc/VamoraSys/lib/` and symlinked into `/usr/lib/` for system-wide access.

---

# Long-Term Stability Rules

## NEVER:

* rename stable theme color, radius, or font keys
* change VMF syntax
* remove public registry fields
* hardcode paths in apps
* let apps edit registry files
* let apps directly access `settings.db`
* let apps read theme preset files directly

## ALWAYS:

* version schemas (use the `version` field)
* validate registry and theme files on load
* keep all APIs backward compatible
* separate cache / config / data clearly
* use stable naming conventions from day one
* add new keys rather than renaming existing ones
