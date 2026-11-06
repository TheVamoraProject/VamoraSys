//! `vamorasys device`: what kind of device this is, how powerful it is, and what apps should do about it.
//!
//! Apps must not guess from window size: a desktop can have a tiny screen and a tablet a huge one.
//! The form factor comes from what the hardware *is* (declared profile, chassis, battery, inputs,
//! modem, hinge, panel size). Window width stays a separate, per-window concern.
//!
//! Priority for every value: 1. `/etc/VamoraSys/device.vmf` (set by the device image)
//! 2. hardware facts 3. heuristics. `Source` says which one decided.

use crate::config::device_file_path;
use crate::device_probe;
use crate::vmf::{parse_vmf, VmfError};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid {path} on line {line}: {message}")]
    Syntax {
        path: PathBuf,
        line: usize,
        message: String,
    },
    #[error("cannot write {path}: {source}{hint}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
        hint: &'static str,
    },
    #[error("{0}")]
    Terminal(String),
    #[error("{path} is missing {field}")]
    MissingField { path: PathBuf, field: &'static str },
    #[error("{path}: unsupported VMF_VERSION '{version}' (this VamoraSys reads version 1)")]
    UnsupportedVersion { path: PathBuf, version: String },
    #[error("{path}: unknown field '{field}'")]
    UnknownField { path: PathBuf, field: String },
    #[error("{path}: invalid {field} '{value}'; expected {expected}")]
    InvalidValue {
        path: PathBuf,
        field: &'static str,
        value: String,
        expected: &'static str,
    },
}

// ───────────────────────────── types ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFactor {
    Phone,
    FlipPhone,
    FoldablePhone,
    Tablet,
    FoldableTablet,
    Laptop,
    FoldableLaptop,
    Cyberdeck,
    Desktop,
    Tv,
    Watch,
}

pub const FORM_FACTORS: &str = "phone | flip-phone | foldable-phone | tablet | foldable-tablet | laptop | foldable-laptop | cyberdeck | desktop | tv | watch";

impl FormFactor {
    pub const ALL: [FormFactor; 11] = [
        FormFactor::Phone,
        FormFactor::FlipPhone,
        FormFactor::FoldablePhone,
        FormFactor::Tablet,
        FormFactor::FoldableTablet,
        FormFactor::Laptop,
        FormFactor::FoldableLaptop,
        FormFactor::Cyberdeck,
        FormFactor::Desktop,
        FormFactor::Tv,
        FormFactor::Watch,
    ];

    pub fn description(self) -> &'static str {
        match self {
            FormFactor::Phone => "Pocket device with a modem and a touchscreen",
            FormFactor::FlipPhone => "Clamshell phone with a folding screen",
            FormFactor::FoldablePhone => "Book-style phone that unfolds into a bigger screen",
            FormFactor::Tablet => "Touchscreen slate, whatever its size",
            FormFactor::FoldableTablet => "Tablet-size folding screen",
            FormFactor::Laptop => "Built-in keyboard and screen",
            FormFactor::FoldableLaptop => {
                "One big folding screen; the keyboard is on screen or wireless"
            }
            FormFactor::Cyberdeck => {
                "DIY mini computer: small screen and keyboard, often a Raspberry Pi"
            }
            FormFactor::Desktop => "Tower, mini PC or all-in-one with its own monitor",
            FormFactor::Tv => "Big screen driven by a remote control",
            FormFactor::Watch => "Tiny wearable screen",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FormFactor::Phone => "phone",
            FormFactor::FlipPhone => "flip-phone",
            FormFactor::FoldablePhone => "foldable-phone",
            FormFactor::Tablet => "tablet",
            FormFactor::FoldableTablet => "foldable-tablet",
            FormFactor::Laptop => "laptop",
            FormFactor::FoldableLaptop => "foldable-laptop",
            FormFactor::Cyberdeck => "cyberdeck",
            FormFactor::Desktop => "desktop",
            FormFactor::Tv => "tv",
            FormFactor::Watch => "watch",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(
            match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
                "phone" => FormFactor::Phone,
                "flip-phone" => FormFactor::FlipPhone,
                "foldable-phone" => FormFactor::FoldablePhone,
                "tablet" => FormFactor::Tablet,
                "foldable-tablet" => FormFactor::FoldableTablet,
                "laptop" => FormFactor::Laptop,
                "foldable-laptop" => FormFactor::FoldableLaptop,
                "cyberdeck" => FormFactor::Cyberdeck,
                "desktop" => FormFactor::Desktop,
                "tv" => FormFactor::Tv,
                "watch" => FormFactor::Watch,
                _ => return None,
            },
        )
    }

    pub fn is_foldable(self) -> bool {
        matches!(
            self,
            FormFactor::FlipPhone
                | FormFactor::FoldablePhone
                | FormFactor::FoldableTablet
                | FormFactor::FoldableLaptop
        )
    }
}

/// How far a foldable is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    Folded,
    HalfOpen,
    Unfolded,
}

impl Posture {
    pub fn as_str(self) -> &'static str {
        match self {
            Posture::Folded => "folded",
            Posture::HalfOpen => "half-open",
            Posture::Unfolded => "unfolded",
        }
    }

    /// Hinge angle in degrees (0 = closed, 180 = flat).
    pub fn from_angle(degrees: f32) -> Self {
        if degrees < 25.0 {
            Posture::Folded
        } else if degrees < 155.0 {
            Posture::HalfOpen
        } else {
            Posture::Unfolded
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldStyle {
    /// Opens like a book (the inner screen is bigger).
    Book,
    /// Clamshell flip.
    Flip,
}

/// How much performance headroom the device has. Apps scale effort by this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Budget,
    Entry,
    Midrange,
    Flagship,
}

impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Budget, Tier::Entry, Tier::Midrange, Tier::Flagship];

    pub fn description(self) -> &'static str {
        match self {
            Tier::Budget => "Low-end hardware: fewest effects and the least background work",
            Tier::Entry => "Modest hardware: light effects",
            Tier::Midrange => "Capable hardware: full effects",
            Tier::Flagship => "Top hardware: everything on, highest frame rate",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Budget => "budget",
            Tier::Entry => "entry",
            Tier::Midrange => "midrange",
            Tier::Flagship => "flagship",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(
            match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
                "budget" => Tier::Budget,
                "entry" | "entry-level" => Tier::Entry,
                "midrange" | "mid-range" | "mid" => Tier::Midrange,
                "flagship" => Tier::Flagship,
                _ => return None,
            },
        )
    }

    fn from_index(index: u8) -> Tier {
        match index {
            0 => Tier::Budget,
            1 => Tier::Entry,
            2 => Tier::Midrange,
            _ => Tier::Flagship,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AnimationLevel {
    /// Essential state changes only: no transitions, no ambient motion.
    Minimal,
    /// Short, simple transitions; no ambient or parallax effects.
    Reduced,
    /// Everything the design calls for.
    Full,
}

impl AnimationLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            AnimationLevel::Minimal => "minimal",
            AnimationLevel::Reduced => "reduced",
            AnimationLevel::Full => "full",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "minimal" => AnimationLevel::Minimal,
            "reduced" => AnimationLevel::Reduced,
            "full" => AnimationLevel::Full,
            _ => return None,
        })
    }
}

/// Ready-made budgets so every app adapts the same way instead of inventing its own rules.
/// These are upper bounds: an app may always do less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Adaptation {
    pub animations: AnimationLevel,
    /// Blur / frosted-glass effects.
    pub blur: bool,
    /// Frame-rate ceiling for UI animation (also cap it by the display's refresh rate).
    pub target_fps: u16,
    /// How many background processes/services an app may keep alive.
    pub background_processes: u8,
    /// May the app preload or prefetch content ahead of time.
    pub preload: bool,
    /// Image / texture cache ceiling in MB.
    pub image_cache_mb: u32,
}

/// Individual hints replaced by `device.vmf`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AdaptOverrides {
    pub animations: Option<AnimationLevel>,
    pub blur: Option<bool>,
    pub target_fps: Option<u16>,
    pub background_processes: Option<u8>,
    pub preload: Option<bool>,
    pub image_cache_mb: Option<u32>,
}

pub fn adaptation_for(
    tier: Tier,
    form_factor: FormFactor,
    overrides: &AdaptOverrides,
) -> Adaptation {
    let mut a = match tier {
        Tier::Budget => Adaptation {
            animations: AnimationLevel::Minimal,
            blur: false,
            target_fps: 30,
            background_processes: 2,
            preload: false,
            image_cache_mb: 16,
        },
        Tier::Entry => Adaptation {
            animations: AnimationLevel::Reduced,
            blur: false,
            target_fps: 60,
            background_processes: 4,
            preload: false,
            image_cache_mb: 48,
        },
        Tier::Midrange => Adaptation {
            animations: AnimationLevel::Full,
            blur: true,
            target_fps: 60,
            background_processes: 8,
            preload: true,
            image_cache_mb: 128,
        },
        Tier::Flagship => Adaptation {
            animations: AnimationLevel::Full,
            blur: true,
            target_fps: 120,
            background_processes: 16,
            preload: true,
            image_cache_mb: 256,
        },
    };
    // A watch has a tiny battery: be stricter whatever the chip can do.
    if form_factor == FormFactor::Watch {
        a.animations = a.animations.min(AnimationLevel::Reduced);
        a.blur = false;
        a.target_fps = a.target_fps.min(60);
        a.background_processes = a.background_processes.min(2);
        a.preload = false;
        a.image_cache_mb = a.image_cache_mb.min(16);
    }
    if let Some(v) = overrides.animations {
        a.animations = v
    }
    if let Some(v) = overrides.blur {
        a.blur = v
    }
    if let Some(v) = overrides.target_fps {
        a.target_fps = v
    }
    if let Some(v) = overrides.background_processes {
        a.background_processes = v
    }
    if let Some(v) = overrides.preload {
        a.preload = v
    }
    if let Some(v) = overrides.image_cache_mb {
        a.image_cache_mb = v
    }
    a
}

// ───────────────────────────── signals + classifier ─────────────────────────────

/// Raw facts about the machine. Everything is optional / false when unknown.
#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub override_form_factor: Option<FormFactor>,
    pub fold_hint: Option<FoldStyle>,
    pub override_tier: Option<Tier>,
    pub adapt_overrides: AdaptOverrides,
    /// SMBIOS chassis type (/sys/class/dmi/id/chassis_type).
    pub chassis_type: Option<u8>,
    /// A built-in battery (not a mouse/keyboard battery).
    pub has_battery: bool,
    pub has_touch: bool,
    /// A real keyboard (IR remotes don't count). May be wireless or USB.
    pub has_keyboard: bool,
    /// A keyboard that is part of the machine (PS/2, I2C/SPI-HID, platform), not Bluetooth or USB.
    pub has_builtin_keyboard: bool,
    /// A single-board computer (Raspberry Pi and friends), recognised from the device-tree model.
    pub is_sbc: bool,
    pub has_pointer: bool,
    pub has_lid_switch: bool,
    pub has_modem: bool,
    pub has_cec: bool,
    pub has_hinge_sensor: bool,
    pub hinge_angle: Option<f32>,
    /// Diagonal of the biggest built-in panel, fully unfolded, in inches.
    pub internal_diag_in: Option<f32>,
    /// Diagonal of the biggest external display, in inches.
    pub external_diag_in: Option<f32>,
    /// Total RAM (MB), CPU cores, fastest core clock (MHz).
    pub ram_mb: Option<u32>,
    pub cpu_cores: Option<u32>,
    pub max_cpu_mhz: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Set in device.vmf.
    Profile,
    /// Decided by the SMBIOS chassis type.
    Chassis,
    /// Worked out from hardware facts.
    Detected,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Profile => "device.vmf",
            Source::Chassis => "chassis",
            Source::Detected => "detected",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub form_factor: FormFactor,
    pub form_factor_source: Source,
    pub posture: Option<Posture>,
    pub diagonal_in: Option<f32>,
    pub touch: bool,
    pub keyboard: bool,
    pub pointer: bool,
    pub battery: bool,
    pub tier: Tier,
    pub tier_source: Source,
    pub ram_mb: Option<u32>,
    pub cpu_cores: Option<u32>,
    pub max_cpu_mhz: Option<u32>,
    pub adapt: Adaptation,
    pub overrides: AdaptOverrides,
    /// The device.vmf that was read, if there is one.
    pub profile: Option<PathBuf>,
}

fn is_laptop_chassis(chassis: u8) -> bool {
    matches!(chassis, 8 | 9 | 10 | 14 | 31) // portable, laptop, notebook, sub-notebook, convertible
}

fn is_desktop_chassis(chassis: u8) -> bool {
    matches!(
        chassis,
        3 | 4 | 5 | 6 | 7 | 13 | 15 | 16 | 17 | 23 | 24 | 35 | 36
    )
}

fn foldable_class(s: &Signals, diagonal: Option<f32>) -> FormFactor {
    let d = diagonal.unwrap_or(7.5);
    let style = s.fold_hint.unwrap_or(if d < 7.2 {
        FoldStyle::Flip
    } else {
        FoldStyle::Book
    });
    if style == FoldStyle::Flip {
        return FormFactor::FlipPhone;
    }
    if s.has_modem && d < 11.0 {
        FormFactor::FoldablePhone
    } else if d >= 12.0 {
        // A big folding screen with no keyboard of its own: the keyboard is on screen or wireless.
        FormFactor::FoldableLaptop
    } else {
        FormFactor::FoldableTablet
    }
}

/// Cyberdecks are small-screen, keyboard-first DIY computers, usually a Raspberry Pi or another SBC.
const CYBERDECK_MAX_DIAGONAL_IN: f32 = 10.5;

fn mobile_class(s: &Signals, diagonal: Option<f32>) -> FormFactor {
    match diagonal {
        Some(d) if d < 7.0 => FormFactor::Phone,
        Some(d) if d < 8.5 => {
            if s.has_modem {
                FormFactor::Phone
            } else {
                FormFactor::Tablet
            }
        }
        Some(_) => FormFactor::Tablet,
        None => {
            if s.has_modem {
                FormFactor::Phone
            } else {
                FormFactor::Tablet
            }
        }
    }
}

/// Weakest link wins: plenty of CPU doesn't help with 2 GB of RAM. MemTotal is lower than the marketed
/// size (kernel/GPU reserve), so the cut-offs sit below round numbers ("6 GB" reads ~5.5 GB = midrange).
pub fn estimate_tier(s: &Signals) -> Option<Tier> {
    let ram = s.ram_mb.map(|m| {
        if m < 3000 {
            0u8
        } else if m < 5000 {
            1
        } else if m < 10000 {
            2
        } else {
            3
        }
    });
    let cpu = match (s.max_cpu_mhz, s.cpu_cores) {
        (Some(f), _) => Some(if f < 1600 {
            0u8
        } else if f < 2000 {
            1
        } else if f < 2600 {
            2
        } else {
            3
        }),
        (None, Some(c)) => Some(if c <= 2 {
            0u8
        } else if c <= 4 {
            1
        } else if c <= 8 {
            2
        } else {
            3
        }),
        _ => None,
    };
    match (ram, cpu) {
        (Some(r), Some(c)) => Some(Tier::from_index(r.min(c))),
        (Some(r), None) => Some(Tier::from_index(r)),
        (None, Some(c)) => Some(Tier::from_index(c)),
        (None, None) => None,
    }
}

pub fn classify(s: &Signals) -> DeviceInfo {
    let diagonal = s.internal_diag_in.or(s.external_diag_in);
    let foldable_hw = s.has_hinge_sensor || s.fold_hint.is_some();

    let (form_factor, form_factor_source) = if let Some(ff) = s.override_form_factor {
        (ff, Source::Profile)
    } else if s.has_battery
        && s.has_touch
        && !s.has_keyboard
        && !s.has_pointer
        && s.internal_diag_in.map(|d| d <= 2.6).unwrap_or(false)
    {
        (FormFactor::Watch, Source::Detected)
    } else if foldable_hw && !s.has_builtin_keyboard {
        // A hinge sensor alone is not a folding screen: convertible laptops report a lid angle too.
        // A built-in keyboard means it is a laptop (handled below), not a foldable.
        (foldable_class(s, diagonal), Source::Detected)
    } else if s.is_sbc
        && s.has_keyboard
        && diagonal
            .map(|d| d <= CYBERDECK_MAX_DIAGONAL_IN)
            .unwrap_or(false)
    {
        (FormFactor::Cyberdeck, Source::Detected)
    } else if s.chassis_type.map(is_laptop_chassis).unwrap_or(false) {
        (FormFactor::Laptop, Source::Chassis)
    } else if s.chassis_type.map(is_desktop_chassis).unwrap_or(false) {
        if !s.has_keyboard
            && !s.has_pointer
            && s.external_diag_in.map(|d| d >= 30.0).unwrap_or(false)
        {
            (FormFactor::Tv, Source::Detected)
        } else {
            (FormFactor::Desktop, Source::Chassis)
        }
    } else if matches!(s.chassis_type, Some(30) | Some(32)) {
        (mobile_class(s, diagonal), Source::Chassis)
    } else if !s.has_battery
        && (s.has_cec || (!s.has_keyboard && !s.has_pointer))
        && diagonal.map(|d| d >= 30.0).unwrap_or(false)
    {
        (FormFactor::Tv, Source::Detected)
    } else if s.has_battery && s.has_touch && !(s.has_keyboard && s.has_lid_switch) {
        (mobile_class(s, diagonal), Source::Detected)
    } else if s.has_keyboard || s.has_pointer {
        if s.has_battery {
            (FormFactor::Laptop, Source::Detected)
        } else {
            (FormFactor::Desktop, Source::Detected)
        }
    } else {
        (FormFactor::Desktop, Source::Detected)
    };

    let posture = if form_factor.is_foldable() {
        s.hinge_angle.map(Posture::from_angle)
    } else {
        None
    };

    // Unknown hardware gets the neutral middle: a wrong "flagship" burns battery, a wrong "budget" looks plain.
    let (tier, tier_source) = if let Some(t) = s.override_tier {
        (t, Source::Profile)
    } else if let Some(t) = estimate_tier(s) {
        (t, Source::Detected)
    } else {
        (Tier::Midrange, Source::Detected)
    };

    DeviceInfo {
        form_factor,
        form_factor_source,
        posture,
        diagonal_in: diagonal,
        touch: s.has_touch,
        keyboard: s.has_keyboard,
        pointer: s.has_pointer,
        battery: s.has_battery,
        tier,
        tier_source,
        ram_mb: s.ram_mb,
        cpu_cores: s.cpu_cores,
        max_cpu_mhz: s.max_cpu_mhz,
        adapt: adaptation_for(tier, form_factor, &s.adapt_overrides),
        overrides: s.adapt_overrides,
        profile: None,
    }
}

// ───────────────────────────── device.vmf ─────────────────────────────

const KNOWN_FIELDS: [&str; 10] = [
    "VMF_VERSION",
    "FORM_FACTOR",
    "FOLD_STYLE",
    "TIER",
    "ANIMATIONS",
    "BLUR",
    "TARGET_FPS",
    "BACKGROUND_PROCESSES",
    "PRELOAD",
    "IMAGE_CACHE_MB",
];

/// `/etc/VamoraSys/device.vmf`, same syntax as `vamora-release.vmf`.
#[derive(Debug, Clone, Default)]
pub struct DeviceProfile {
    pub path: PathBuf,
    pub form_factor: Option<FormFactor>,
    pub fold_style: Option<FoldStyle>,
    pub tier: Option<Tier>,
    pub overrides: AdaptOverrides,
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

impl DeviceProfile {
    /// `Ok(None)` when there is no device.vmf: the device is then fully auto-detected.
    pub fn read(release_root: &Path) -> Result<Option<Self>, DeviceError> {
        let path = device_file_path(release_root);
        if !path.is_file() {
            return Ok(None);
        }
        Self::read_from_path(path).map(Some)
    }

    pub fn read_from_path(path: impl Into<PathBuf>) -> Result<Self, DeviceError> {
        let path = path.into();
        let contents = fs::read_to_string(&path).map_err(|source| DeviceError::Io {
            path: path.clone(),
            source,
        })?;
        let fields = parse_vmf(&contents).map_err(|error| match error {
            VmfError::Syntax { line, message } => DeviceError::Syntax {
                path: path.clone(),
                line,
                message,
            },
        })?;
        Self::from_fields(path, &fields)
    }

    fn from_fields(path: PathBuf, fields: &BTreeMap<String, String>) -> Result<Self, DeviceError> {
        for key in fields.keys() {
            if !KNOWN_FIELDS.contains(&key.as_str()) {
                return Err(DeviceError::UnknownField {
                    path,
                    field: key.clone(),
                });
            }
        }
        match fields.get("VMF_VERSION").map(String::as_str) {
            None | Some("") => {
                return Err(DeviceError::MissingField {
                    path,
                    field: "VMF_VERSION",
                })
            }
            Some("1") => {}
            Some(other) => {
                return Err(DeviceError::UnsupportedVersion {
                    path,
                    version: other.to_string(),
                })
            }
        }

        // An empty value (`TIER=""`) means "not set", so a template can list every field.
        let get = |field: &str| {
            fields
                .get(field)
                .map(String::as_str)
                .filter(|v| !v.is_empty())
        };
        let bad =
            |field: &'static str, value: &str, expected: &'static str| DeviceError::InvalidValue {
                path: path.clone(),
                field,
                value: value.to_string(),
                expected,
            };

        let mut profile = DeviceProfile {
            path: path.clone(),
            ..Default::default()
        };
        if let Some(v) = get("FORM_FACTOR") {
            profile.form_factor =
                Some(FormFactor::parse(v).ok_or_else(|| bad("FORM_FACTOR", v, FORM_FACTORS))?);
        }
        if let Some(v) = get("FOLD_STYLE") {
            profile.fold_style = Some(match v.to_ascii_lowercase().as_str() {
                "book" => FoldStyle::Book,
                "flip" => FoldStyle::Flip,
                _ => return Err(bad("FOLD_STYLE", v, "book | flip")),
            });
        }
        if let Some(v) = get("TIER") {
            profile.tier = Some(
                Tier::parse(v)
                    .ok_or_else(|| bad("TIER", v, "budget | entry | midrange | flagship"))?,
            );
        }
        if let Some(v) = get("ANIMATIONS") {
            profile.overrides.animations = Some(
                AnimationLevel::parse(v)
                    .ok_or_else(|| bad("ANIMATIONS", v, "minimal | reduced | full"))?,
            );
        }
        if let Some(v) = get("BLUR") {
            profile.overrides.blur = Some(parse_bool(v).ok_or_else(|| bad("BLUR", v, "on | off"))?);
        }
        if let Some(v) = get("PRELOAD") {
            profile.overrides.preload =
                Some(parse_bool(v).ok_or_else(|| bad("PRELOAD", v, "on | off"))?);
        }
        if let Some(v) = get("TARGET_FPS") {
            profile.overrides.target_fps = Some(
                v.parse()
                    .map_err(|_| bad("TARGET_FPS", v, "a whole number, for example 60"))?,
            );
        }
        if let Some(v) = get("BACKGROUND_PROCESSES") {
            profile.overrides.background_processes = Some(
                v.parse()
                    .map_err(|_| bad("BACKGROUND_PROCESSES", v, "a whole number from 0 to 255"))?,
            );
        }
        if let Some(v) = get("IMAGE_CACHE_MB") {
            profile.overrides.image_cache_mb = Some(
                v.parse()
                    .map_err(|_| bad("IMAGE_CACHE_MB", v, "a whole number of megabytes"))?,
            );
        }
        Ok(profile)
    }

    fn apply(&self, signals: &mut Signals) {
        signals.override_form_factor = self.form_factor;
        signals.fold_hint = self.fold_style;
        signals.override_tier = self.tier;
        signals.adapt_overrides = self.overrides;
    }
}

/// What the hardware says on its own, ignoring any device.vmf (the wizard shows this as "detected").
pub fn detect_hardware(probe_root: &Path) -> DeviceInfo {
    classify(&device_probe::gather(probe_root))
}

/// Reads the hardware under `probe_root` and `device.vmf` under `release_root`.
pub fn detect(release_root: &Path, probe_root: &Path) -> Result<DeviceInfo, DeviceError> {
    let mut signals = device_probe::gather(probe_root);
    let profile = DeviceProfile::read(release_root)?;
    if let Some(profile) = &profile {
        profile.apply(&mut signals);
    }
    let mut info = classify(&signals);
    info.profile = profile.map(|p| p.path);
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    fn check(name: &str, s: Signals, want: FormFactor) {
        let got = classify(&s).form_factor;
        assert_eq!(
            got,
            want,
            "{name}: got {} want {}",
            got.as_str(),
            want.as_str()
        );
    }

    fn perf(ram: u32, cores: u32, mhz: Option<u32>) -> Signals {
        Signals {
            ram_mb: Some(ram),
            cpu_cores: Some(cores),
            max_cpu_mhz: mhz,
            ..Default::default()
        }
    }

    #[test]
    fn tiny_screen_desktop_is_still_a_desktop() {
        check(
            "small desktop",
            Signals {
                chassis_type: Some(3),
                has_keyboard: true,
                has_pointer: true,
                external_diag_in: Some(7.0),
                ..Default::default()
            },
            FormFactor::Desktop,
        );
        check(
            "small desktop, no chassis info",
            Signals {
                has_keyboard: true,
                has_pointer: true,
                external_diag_in: Some(7.0),
                ..Default::default()
            },
            FormFactor::Desktop,
        );
    }

    #[test]
    fn huge_tablet_is_still_a_tablet() {
        check(
            "14in tablet",
            Signals {
                has_battery: true,
                has_touch: true,
                internal_diag_in: Some(14.6),
                ..Default::default()
            },
            FormFactor::Tablet,
        );
        check(
            "tablet chassis",
            Signals {
                chassis_type: Some(30),
                has_battery: true,
                has_touch: true,
                internal_diag_in: Some(14.6),
                ..Default::default()
            },
            FormFactor::Tablet,
        );
    }

    #[test]
    fn phones() {
        check(
            "phone",
            Signals {
                has_battery: true,
                has_touch: true,
                has_modem: true,
                internal_diag_in: Some(6.1),
                ..Default::default()
            },
            FormFactor::Phone,
        );
        check(
            "big phone 7.2 with modem",
            Signals {
                has_battery: true,
                has_touch: true,
                has_modem: true,
                internal_diag_in: Some(7.2),
                ..Default::default()
            },
            FormFactor::Phone,
        );
        check(
            "7.2 without modem is a tablet",
            Signals {
                has_battery: true,
                has_touch: true,
                internal_diag_in: Some(7.2),
                ..Default::default()
            },
            FormFactor::Tablet,
        );
    }

    #[test]
    fn foldables() {
        check(
            "flip phone",
            Signals {
                has_battery: true,
                has_touch: true,
                has_modem: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(6.7),
                ..Default::default()
            },
            FormFactor::FlipPhone,
        );
        check(
            "book foldable phone",
            Signals {
                has_battery: true,
                has_touch: true,
                has_modem: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(7.6),
                ..Default::default()
            },
            FormFactor::FoldablePhone,
        );
        check(
            "foldable tablet",
            Signals {
                has_battery: true,
                has_touch: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(10.2),
                ..Default::default()
            },
            FormFactor::FoldableTablet,
        );
        check(
            "foldable laptop 18in",
            Signals {
                has_battery: true,
                has_touch: true,
                has_keyboard: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(18.0),
                ..Default::default()
            },
            FormFactor::FoldableLaptop,
        );
        check(
            "fold style without a sensor",
            Signals {
                has_battery: true,
                has_touch: true,
                has_modem: true,
                fold_hint: Some(FoldStyle::Flip),
                internal_diag_in: Some(6.9),
                ..Default::default()
            },
            FormFactor::FlipPhone,
        );
    }

    #[test]
    fn foldable_laptop_is_a_big_folding_screen_whose_keyboard_is_virtual_or_wireless() {
        // no keyboard at all: virtual keyboard on the screen
        check(
            "18in fold, no keyboard",
            Signals {
                has_battery: true,
                has_touch: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(18.0),
                ..Default::default()
            },
            FormFactor::FoldableLaptop,
        );
        // a wireless keyboard paired: still a foldable laptop (the keyboard is not built in)
        check(
            "18in fold, wireless keyboard",
            Signals {
                has_battery: true,
                has_touch: true,
                has_keyboard: true,
                has_builtin_keyboard: false,
                has_hinge_sensor: true,
                internal_diag_in: Some(18.0),
                ..Default::default()
            },
            FormFactor::FoldableLaptop,
        );
        check(
            "12.5in fold",
            Signals {
                has_battery: true,
                has_touch: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(12.5),
                ..Default::default()
            },
            FormFactor::FoldableLaptop,
        );
        check(
            "11.5in fold stays a tablet",
            Signals {
                has_battery: true,
                has_touch: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(11.5),
                ..Default::default()
            },
            FormFactor::FoldableTablet,
        );
    }

    #[test]
    fn a_laptop_has_its_own_keyboard_even_if_it_reports_a_hinge_angle() {
        // 2-in-1 with a lid-angle sensor and a built-in keyboard is a laptop, not a foldable
        check(
            "convertible with hinge sensor",
            Signals {
                chassis_type: Some(31),
                has_battery: true,
                has_touch: true,
                has_keyboard: true,
                has_builtin_keyboard: true,
                has_lid_switch: true,
                has_hinge_sensor: true,
                internal_diag_in: Some(14.0),
                ..Default::default()
            },
            FormFactor::Laptop,
        );
        check(
            "plain laptop",
            Signals {
                has_battery: true,
                has_keyboard: true,
                has_builtin_keyboard: true,
                has_pointer: true,
                has_lid_switch: true,
                internal_diag_in: Some(14.0),
                ..Default::default()
            },
            FormFactor::Laptop,
        );
    }

    #[test]
    fn cyberdeck_is_a_small_screen_plus_keyboard_on_a_single_board_computer() {
        check(
            "pi + 7in screen + keyboard",
            Signals {
                is_sbc: true,
                has_keyboard: true,
                external_diag_in: Some(7.0),
                ..Default::default()
            },
            FormFactor::Cyberdeck,
        );
        check(
            "pi + 5in DSI panel + keyboard + battery",
            Signals {
                is_sbc: true,
                has_keyboard: true,
                has_battery: true,
                internal_diag_in: Some(5.0),
                ..Default::default()
            },
            FormFactor::Cyberdeck,
        );
        // the same board with a big monitor is just a desktop
        check(
            "pi + 27in monitor",
            Signals {
                is_sbc: true,
                has_keyboard: true,
                has_pointer: true,
                external_diag_in: Some(27.0),
                ..Default::default()
            },
            FormFactor::Desktop,
        );
        // a tower with a tiny monitor is still a desktop (not an SBC)
        check(
            "tower + 7in monitor",
            Signals {
                chassis_type: Some(3),
                has_keyboard: true,
                has_pointer: true,
                external_diag_in: Some(7.0),
                ..Default::default()
            },
            FormFactor::Desktop,
        );
        // an SBC without a keyboard is not a cyberdeck
        check(
            "pi + 7in touch screen only",
            Signals {
                is_sbc: true,
                has_touch: true,
                external_diag_in: Some(7.0),
                ..Default::default()
            },
            FormFactor::Desktop,
        );
    }

    #[test]
    fn laptops_and_two_in_ones() {
        check(
            "laptop",
            Signals {
                chassis_type: Some(10),
                has_battery: true,
                has_keyboard: true,
                has_pointer: true,
                has_lid_switch: true,
                internal_diag_in: Some(14.0),
                ..Default::default()
            },
            FormFactor::Laptop,
        );
        check(
            "convertible stays a laptop",
            Signals {
                chassis_type: Some(31),
                has_battery: true,
                has_touch: true,
                has_keyboard: true,
                has_lid_switch: true,
                internal_diag_in: Some(13.0),
                ..Default::default()
            },
            FormFactor::Laptop,
        );
        check(
            "touch laptop without chassis",
            Signals {
                has_battery: true,
                has_touch: true,
                has_keyboard: true,
                has_pointer: true,
                has_lid_switch: true,
                internal_diag_in: Some(15.0),
                ..Default::default()
            },
            FormFactor::Laptop,
        );
    }

    #[test]
    fn tv_and_watch() {
        check(
            "tv",
            Signals {
                external_diag_in: Some(55.0),
                has_cec: true,
                ..Default::default()
            },
            FormFactor::Tv,
        );
        check(
            "tv box with chassis",
            Signals {
                chassis_type: Some(35),
                external_diag_in: Some(55.0),
                ..Default::default()
            },
            FormFactor::Tv,
        );
        check(
            "watch",
            Signals {
                has_battery: true,
                has_touch: true,
                internal_diag_in: Some(1.4),
                ..Default::default()
            },
            FormFactor::Watch,
        );
    }

    #[test]
    fn profile_form_factor_wins_and_is_reported() {
        let s = Signals {
            override_form_factor: Some(FormFactor::FoldableLaptop),
            has_battery: true,
            has_touch: true,
            internal_diag_in: Some(18.0),
            ..Default::default()
        };
        let info = classify(&s);
        assert_eq!(info.form_factor, FormFactor::FoldableLaptop);
        assert_eq!(info.form_factor_source, Source::Profile);
    }

    #[test]
    fn posture_comes_from_the_hinge_angle_and_only_for_foldables() {
        let mut s = Signals {
            has_battery: true,
            has_touch: true,
            has_modem: true,
            has_hinge_sensor: true,
            internal_diag_in: Some(7.6),
            ..Default::default()
        };
        s.hinge_angle = Some(5.0);
        assert_eq!(classify(&s).posture, Some(Posture::Folded));
        s.hinge_angle = Some(100.0);
        assert_eq!(classify(&s).posture, Some(Posture::HalfOpen));
        s.hinge_angle = Some(180.0);
        assert_eq!(classify(&s).posture, Some(Posture::Unfolded));
        let phone = Signals {
            has_battery: true,
            has_touch: true,
            has_modem: true,
            internal_diag_in: Some(6.1),
            hinge_angle: Some(90.0),
            ..Default::default()
        };
        assert_eq!(classify(&phone).posture, None);
    }

    #[test]
    fn tier_from_ram_and_cpu() {
        assert_eq!(classify(&perf(1900, 4, Some(1400))).tier, Tier::Budget);
        assert_eq!(classify(&perf(2700, 8, Some(1800))).tier, Tier::Budget); // "3 GB" phone
        assert_eq!(classify(&perf(3600, 8, Some(1800))).tier, Tier::Entry); // "4 GB"
        assert_eq!(classify(&perf(5600, 8, Some(2400))).tier, Tier::Midrange); // "6 GB"
        assert_eq!(classify(&perf(7500, 8, Some(2400))).tier, Tier::Midrange); // "8 GB"
        assert_eq!(classify(&perf(15500, 8, Some(3200))).tier, Tier::Flagship);
        assert_eq!(classify(&perf(31000, 16, Some(3600))).tier, Tier::Flagship);
        assert_eq!(classify(&perf(16000, 4, Some(1200))).tier, Tier::Budget); // weakest link
        assert_eq!(classify(&perf(7500, 8, None)).tier, Tier::Midrange); // no clock: core count
        assert_eq!(classify(&Signals::default()).tier, Tier::Midrange); // nothing known
    }

    #[test]
    fn profile_tier_wins() {
        let mut s = perf(1900, 2, Some(1000));
        s.override_tier = Some(Tier::Flagship);
        let info = classify(&s);
        assert_eq!(info.tier, Tier::Flagship);
        assert_eq!(info.tier_source, Source::Profile);
    }

    #[test]
    fn adaptation_scales_with_tier() {
        let none = AdaptOverrides::default();
        let b = adaptation_for(Tier::Budget, FormFactor::Phone, &none);
        let f = adaptation_for(Tier::Flagship, FormFactor::Phone, &none);
        assert_eq!(b.animations, AnimationLevel::Minimal);
        assert!(!b.blur && !b.preload);
        assert!(f.animations == AnimationLevel::Full && f.blur && f.preload);
        assert!(b.background_processes < f.background_processes);
        assert!(b.target_fps < f.target_fps);
        assert!(b.image_cache_mb < f.image_cache_mb);
    }

    #[test]
    fn watch_is_strict_even_on_a_flagship_chip() {
        let a = adaptation_for(
            Tier::Flagship,
            FormFactor::Watch,
            &AdaptOverrides::default(),
        );
        assert_eq!(a.animations, AnimationLevel::Reduced);
        assert!(!a.blur && !a.preload);
        assert!(a.background_processes <= 2 && a.target_fps <= 60);
    }

    #[test]
    fn profile_hints_beat_the_tier_table() {
        let o = AdaptOverrides {
            blur: Some(false),
            target_fps: Some(90),
            ..Default::default()
        };
        let a = adaptation_for(Tier::Flagship, FormFactor::Phone, &o);
        assert!(!a.blur);
        assert_eq!(a.target_fps, 90);
        assert_eq!(a.animations, AnimationLevel::Full);
    }

    // ── device.vmf ──

    fn profile_from(text: &str) -> Result<DeviceProfile, DeviceError> {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{text}").unwrap();
        DeviceProfile::read_from_path(file.path())
    }

    #[test]
    fn reads_a_full_device_vmf_in_release_file_syntax() {
        let p = profile_from(
            "# comment\nVMF_VERSION=\"1\"\nFORM_FACTOR=\"foldable-laptop\"\nFOLD_STYLE=book\nTIER='flagship'\n\
             ANIMATIONS=reduced\nBLUR=off\nTARGET_FPS=90\nBACKGROUND_PROCESSES=12\nPRELOAD=\"on\"\nIMAGE_CACHE_MB=192\n",
        )
        .unwrap();
        assert_eq!(p.form_factor, Some(FormFactor::FoldableLaptop));
        assert_eq!(p.fold_style, Some(FoldStyle::Book));
        assert_eq!(p.tier, Some(Tier::Flagship));
        assert_eq!(p.overrides.animations, Some(AnimationLevel::Reduced));
        assert_eq!(p.overrides.blur, Some(false));
        assert_eq!(p.overrides.target_fps, Some(90));
        assert_eq!(p.overrides.background_processes, Some(12));
        assert_eq!(p.overrides.preload, Some(true));
        assert_eq!(p.overrides.image_cache_mb, Some(192));
    }

    #[test]
    fn only_vmf_version_is_required_and_empty_values_mean_unset() {
        let p = profile_from("VMF_VERSION=1\nTIER=\"\"\nFORM_FACTOR=\n").unwrap();
        assert!(p.form_factor.is_none() && p.tier.is_none());
    }

    #[test]
    fn rejects_bad_files_with_clear_errors() {
        assert!(matches!(
            profile_from("TIER=flagship\n"),
            Err(DeviceError::MissingField {
                field: "VMF_VERSION",
                ..
            })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=2\n"),
            Err(DeviceError::UnsupportedVersion { .. })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nTEIR=flagship\n"),
            Err(DeviceError::UnknownField { .. })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nTIER=ultra\n"),
            Err(DeviceError::InvalidValue { field: "TIER", .. })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nFORM_FACTOR=toaster\n"),
            Err(DeviceError::InvalidValue {
                field: "FORM_FACTOR",
                ..
            })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nBLUR=maybe\n"),
            Err(DeviceError::InvalidValue { field: "BLUR", .. })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nTARGET_FPS=fast\n"),
            Err(DeviceError::InvalidValue {
                field: "TARGET_FPS",
                ..
            })
        ));
        // release-file syntax rules apply: lowercase keys and lines without '=' are errors, with line numbers
        assert!(matches!(
            profile_from("VMF_VERSION=1\ntier=flagship\n"),
            Err(DeviceError::Syntax { line: 2, .. })
        ));
        assert!(matches!(
            profile_from("VMF_VERSION=1\nnonsense\n"),
            Err(DeviceError::Syntax { line: 2, .. })
        ));
    }

    #[test]
    fn missing_device_vmf_means_full_auto_detection() {
        let dir = tempdir().unwrap();
        assert!(DeviceProfile::read(dir.path()).unwrap().is_none());
    }

    #[test]
    fn detect_applies_the_profile_over_the_hardware() {
        let release = tempdir().unwrap();
        let probe = tempdir().unwrap();
        fs::write(
            release.path().join("device.vmf"),
            "VMF_VERSION=\"1\"\nFORM_FACTOR=\"tv\"\nTIER=\"budget\"\nBLUR=on\n",
        )
        .unwrap();
        let info = detect(release.path(), probe.path()).unwrap();
        assert_eq!(info.form_factor, FormFactor::Tv);
        assert_eq!(info.form_factor_source, Source::Profile);
        assert_eq!(info.tier, Tier::Budget);
        assert!(info.adapt.blur); // hint override beats the budget table
        assert_eq!(info.profile, Some(release.path().join("device.vmf")));
    }
}
