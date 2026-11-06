//! `vamorasys device setup`: a keyboard-driven wizard that writes `/etc/VamoraSys/device.vmf`.
//! Offered automatically (in a terminal only) when `device.vmf` is missing or broken.

use crate::config::device_file_path;
use crate::device::{
    self, adaptation_for, AdaptOverrides, AnimationLevel, DeviceError, DeviceInfo, DeviceProfile,
    FormFactor, Tier,
};
use crate::tui::{Item, Screen, Terminal};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choices {
    /// `None` = auto-detect.
    pub form_factor: Option<FormFactor>,
    pub tier: Option<Tier>,
    pub overrides: AdaptOverrides,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Saved(PathBuf),
    Cancelled,
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

/// The file the wizard writes: same syntax as vamora-release.vmf, warning on top.
pub fn render_vmf(choices: &Choices) -> String {
    let mut out = String::new();
    out.push_str("# /etc/VamoraSys/device.vmf\n");
    out.push_str(
        "# WARNING: A WRONG VALUE IN THIS FILE CAN MAKE APPS ADAPT BADLY TO THIS DEVICE (SCREEN LAYOUT, ANIMATIONS, PERFORMANCE)\n",
    );
    out.push_str(
        "# Only change it if you know what this device is. Delete this file to go back to automatic detection.\n",
    );
    out.push_str("# Written by `vamorasys device setup`. Same syntax as vamora-release.vmf.\n");
    out.push_str(
        "# `vamorasys device status` shows what each value means and where it came from.\n\n",
    );
    out.push_str("VMF_VERSION=\"1\"\n");
    match choices.form_factor {
        Some(form_factor) => out.push_str(&format!("FORM_FACTOR=\"{}\"\n", form_factor.as_str())),
        None => out.push_str("# FORM_FACTOR is not set: detected from the hardware\n"),
    }
    match choices.tier {
        Some(tier) => out.push_str(&format!("TIER=\"{}\"\n", tier.as_str())),
        None => out.push_str("# TIER is not set: estimated from RAM and CPU\n"),
    }
    let o = &choices.overrides;
    if let Some(v) = o.animations {
        out.push_str(&format!("ANIMATIONS=\"{}\"\n", v.as_str()));
    }
    if let Some(v) = o.blur {
        out.push_str(&format!("BLUR=\"{}\"\n", on_off(v)));
    }
    if let Some(v) = o.target_fps {
        out.push_str(&format!("TARGET_FPS=\"{v}\"\n"));
    }
    if let Some(v) = o.background_processes {
        out.push_str(&format!("BACKGROUND_PROCESSES=\"{v}\"\n"));
    }
    if let Some(v) = o.preload {
        out.push_str(&format!("PRELOAD=\"{}\"\n", on_off(v)));
    }
    if let Some(v) = o.image_cache_mb {
        out.push_str(&format!("IMAGE_CACHE_MB=\"{v}\"\n"));
    }
    out
}

fn choices_from(profile: &DeviceProfile) -> Choices {
    Choices {
        form_factor: profile.form_factor,
        tier: profile.tier,
        overrides: profile.overrides,
    }
}

fn write_profile(release_root: &Path, choices: &Choices) -> Result<PathBuf, DeviceError> {
    let path = device_file_path(release_root);
    let fail = |source: std::io::Error| DeviceError::Write {
        hint: if source.kind() == ErrorKind::PermissionDenied {
            "; run `sudo vamorasys device setup`"
        } else {
            ""
        },
        path: path.clone(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    fs::write(&path, render_vmf(choices)).map_err(fail)?;
    Ok(path)
}

// ───────────────────────────── screens ─────────────────────────────

const HINT: &str = "↑↓  Navigate     Enter  Select     Esc  Back";
const HINT_FIRST: &str = "↑↓  Navigate     Enter  Select     Esc  Cancel";

fn describe_detected(info: &DeviceInfo) -> String {
    let ram = info
        .ram_mb
        .map(|mb| format!("{:.1} GB RAM", mb as f32 / 1024.0))
        .unwrap_or_else(|| "RAM unknown".to_string());
    let cores = info
        .cpu_cores
        .map(|c| format!("{c} core{}", if c == 1 { "" } else { "s" }))
        .unwrap_or_else(|| "cores unknown".to_string());
    format!(
        "Detected: {} · tier {} ({ram}, {cores})",
        info.form_factor.as_str(),
        info.tier.as_str()
    )
}

fn pick_form_factor(
    terminal: &mut Terminal,
    detected: &DeviceInfo,
    notice: &[String],
    current: Option<FormFactor>,
) -> Option<Option<FormFactor>> {
    let mut items = vec![Item::new(
        "Auto-detect",
        format!(
            "recommended · this device looks like a {}",
            detected.form_factor.as_str()
        ),
    )];
    items.extend(
        FormFactor::ALL
            .iter()
            .map(|f| Item::new(f.as_str(), f.description())),
    );
    let initial = current
        .and_then(|c| FormFactor::ALL.iter().position(|f| *f == c))
        .map_or(0, |i| i + 1);
    let mut info = notice.to_vec();
    info.push(describe_detected(detected));
    let screen = Screen {
        title: "Device Setup · Form factor",
        hint: HINT_FIRST,
        info: &info,
        filterable: false,
    };
    let index = crate::tui::select(terminal, &screen, &items, initial)?;
    Some(if index == 0 {
        None
    } else {
        Some(FormFactor::ALL[index - 1])
    })
}

fn pick_tier(
    terminal: &mut Terminal,
    detected: &DeviceInfo,
    current: Option<Tier>,
) -> Option<Option<Tier>> {
    let mut items = vec![Item::new(
        "Auto",
        format!(
            "recommended · estimated from RAM and CPU: {}",
            detected.tier.as_str()
        ),
    )];
    items.extend(
        Tier::ALL
            .iter()
            .map(|t| Item::new(t.as_str(), t.description())),
    );
    let initial = current
        .and_then(|c| Tier::ALL.iter().position(|t| *t == c))
        .map_or(0, |i| i + 1);
    let info = ["How much performance headroom apps can count on.".to_string()];
    let screen = Screen {
        title: "Device Setup · Performance tier",
        hint: HINT,
        info: &info,
        filterable: false,
    };
    let index = crate::tui::select(terminal, &screen, &items, initial)?;
    Some(if index == 0 {
        None
    } else {
        Some(Tier::ALL[index - 1])
    })
}

/// One option list for a single hint. `None` = use the tier default.
fn pick_hint<T: Copy>(
    terminal: &mut Terminal,
    title: &str,
    default_text: String,
    options: &[(&str, T)],
    current: Option<T>,
    same: impl Fn(T, T) -> bool,
) -> Option<Option<T>> {
    let mut items = vec![Item::new("Tier default", default_text)];
    items.extend(options.iter().map(|(label, _)| Item::new(*label, "")));
    let initial = current
        .and_then(|c| options.iter().position(|(_, v)| same(*v, c)))
        .map_or(0, |i| i + 1);
    let screen = Screen {
        title,
        hint: HINT,
        info: &[],
        filterable: false,
    };
    let index = crate::tui::select(terminal, &screen, &items, initial)?;
    Some(if index == 0 {
        None
    } else {
        Some(options[index - 1].1)
    })
}

fn customize_hints(terminal: &mut Terminal, choices: &mut Choices, detected: &DeviceInfo) {
    let form_factor = choices.form_factor.unwrap_or(detected.form_factor);
    let tier = choices.tier.unwrap_or(detected.tier);
    let base = adaptation_for(tier, form_factor, &AdaptOverrides::default());

    if let Some(v) = pick_hint(
        terminal,
        "Hint · Animations",
        base.animations.as_str().to_string(),
        &[
            ("minimal", AnimationLevel::Minimal),
            ("reduced", AnimationLevel::Reduced),
            ("full", AnimationLevel::Full),
        ],
        choices.overrides.animations,
        |a, b| a == b,
    ) {
        choices.overrides.animations = v;
    }
    if let Some(v) = pick_hint(
        terminal,
        "Hint · Blur",
        on_off(base.blur).to_string(),
        &[("on", true), ("off", false)],
        choices.overrides.blur,
        |a, b| a == b,
    ) {
        choices.overrides.blur = v;
    }
    if let Some(v) = pick_hint(
        terminal,
        "Hint · Frame rate ceiling",
        format!("{} fps", base.target_fps),
        &[
            ("30 fps", 30u16),
            ("60 fps", 60),
            ("90 fps", 90),
            ("120 fps", 120),
            ("144 fps", 144),
        ],
        choices.overrides.target_fps,
        |a, b| a == b,
    ) {
        choices.overrides.target_fps = v;
    }
    if let Some(v) = pick_hint(
        terminal,
        "Hint · Background processes per app",
        base.background_processes.to_string(),
        &[
            ("1", 1u8),
            ("2", 2),
            ("4", 4),
            ("8", 8),
            ("16", 16),
            ("32", 32),
        ],
        choices.overrides.background_processes,
        |a, b| a == b,
    ) {
        choices.overrides.background_processes = v;
    }
    if let Some(v) = pick_hint(
        terminal,
        "Hint · Preloading",
        on_off(base.preload).to_string(),
        &[("on", true), ("off", false)],
        choices.overrides.preload,
        |a, b| a == b,
    ) {
        choices.overrides.preload = v;
    }
    if let Some(v) = pick_hint(
        terminal,
        "Hint · Image cache",
        format!("{} MB", base.image_cache_mb),
        &[
            ("16 MB", 16u32),
            ("48 MB", 48),
            ("128 MB", 128),
            ("256 MB", 256),
            ("512 MB", 512),
        ],
        choices.overrides.image_cache_mb,
        |a, b| a == b,
    ) {
        choices.overrides.image_cache_mb = v;
    }
}

fn pick_hints_mode(terminal: &mut Terminal, choices: &Choices) -> Option<bool> {
    let customized = choices.overrides != AdaptOverrides::default();
    let items = vec![
        Item::new("Use the tier defaults", "recommended"),
        Item::new(
            "Customize individual hints…",
            "animations, blur, frame rate, background processes, preload, image cache",
        ),
    ];
    let info = [
        "Hints tell apps how much effort to spend. Each tier has sensible defaults.".to_string(),
    ];
    let screen = Screen {
        title: "Device Setup · Adaptation hints",
        hint: HINT,
        info: &info,
        filterable: false,
    };
    let index = crate::tui::select(terminal, &screen, &items, usize::from(customized))?;
    Some(index == 1)
}

fn summary_lines(path: &Path, choices: &Choices, detected: &DeviceInfo) -> Vec<String> {
    let form_factor = match choices.form_factor {
        Some(f) => f.as_str().to_string(),
        None => format!("auto-detect ({})", detected.form_factor.as_str()),
    };
    let tier = match choices.tier {
        Some(t) => t.as_str().to_string(),
        None => format!("auto ({})", detected.tier.as_str()),
    };
    let o = &choices.overrides;
    let mut hints = Vec::new();
    if let Some(v) = o.animations {
        hints.push(format!("animations={}", v.as_str()));
    }
    if let Some(v) = o.blur {
        hints.push(format!("blur={}", on_off(v)));
    }
    if let Some(v) = o.target_fps {
        hints.push(format!("fps={v}"));
    }
    if let Some(v) = o.background_processes {
        hints.push(format!("background={v}"));
    }
    if let Some(v) = o.preload {
        hints.push(format!("preload={}", on_off(v)));
    }
    if let Some(v) = o.image_cache_mb {
        hints.push(format!("cache={v}MB"));
    }
    vec![
        format!("File         {}", path.display()),
        format!("Form factor  {form_factor}"),
        format!("Tier         {tier}"),
        format!(
            "Hints        {}",
            if hints.is_empty() {
                "tier defaults".to_string()
            } else {
                hints.join(", ")
            }
        ),
    ]
}

#[derive(Clone, Copy)]
enum Step {
    FormFactor,
    Tier,
    Hints,
    Review,
}

/// Runs the wizard inside an already-entered terminal. `notice` is shown on the first screen.
pub fn run_with(
    terminal: &mut Terminal,
    release_root: &Path,
    probe_root: &Path,
    notice: Vec<String>,
) -> Result<Outcome, DeviceError> {
    let detected = device::detect_hardware(probe_root);
    let path = device_file_path(release_root);
    // Start from what is already there, when it can be read.
    let mut choices = match DeviceProfile::read(release_root) {
        Ok(Some(profile)) => choices_from(&profile),
        _ => Choices::default(),
    };

    let mut step = Step::FormFactor;
    loop {
        match step {
            Step::FormFactor => {
                match pick_form_factor(terminal, &detected, &notice, choices.form_factor) {
                    Some(selection) => {
                        choices.form_factor = selection;
                        step = Step::Tier;
                    }
                    None => return Ok(Outcome::Cancelled),
                }
            }
            Step::Tier => match pick_tier(terminal, &detected, choices.tier) {
                Some(selection) => {
                    choices.tier = selection;
                    step = Step::Hints;
                }
                None => step = Step::FormFactor,
            },
            Step::Hints => match pick_hints_mode(terminal, &choices) {
                Some(true) => {
                    customize_hints(terminal, &mut choices, &detected);
                    step = Step::Review;
                }
                Some(false) => {
                    choices.overrides = AdaptOverrides::default();
                    step = Step::Review;
                }
                None => step = Step::Tier,
            },
            Step::Review => {
                let items = vec![
                    Item::new("Save", format!("write {}", path.display())),
                    Item::new("Back", "change an answer"),
                    Item::new("Cancel", "write nothing"),
                ];
                let info = summary_lines(&path, &choices, &detected);
                let screen = Screen {
                    title: "Device Setup · Review",
                    hint: HINT,
                    info: &info,
                    filterable: false,
                };
                match crate::tui::select(terminal, &screen, &items, 0) {
                    Some(0) => return write_profile(release_root, &choices).map(Outcome::Saved),
                    Some(2) => return Ok(Outcome::Cancelled),
                    _ => step = Step::FormFactor,
                }
            }
        }
    }
}

pub fn enter_terminal() -> Result<Terminal, DeviceError> {
    if !crate::tui::is_interactive() {
        return Err(DeviceError::Terminal(
            "the setup wizard needs an interactive terminal; see examples/device.vmf to write the file by hand"
                .to_string(),
        ));
    }
    Terminal::enter()
        .map_err(|error| DeviceError::Terminal(format!("cannot use the terminal: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{AnimationLevel, Tier};
    use tempfile::tempdir;

    fn profile_of(choices: &Choices) -> DeviceProfile {
        let dir = tempdir().unwrap();
        let path = write_profile(dir.path(), choices).unwrap();
        DeviceProfile::read_from_path(path).unwrap()
    }

    #[test]
    fn the_written_file_is_valid_device_vmf_and_round_trips() {
        let choices = Choices {
            form_factor: Some(FormFactor::Cyberdeck),
            tier: Some(Tier::Entry),
            overrides: AdaptOverrides {
                animations: Some(AnimationLevel::Reduced),
                blur: Some(false),
                target_fps: Some(90),
                background_processes: Some(6),
                preload: Some(true),
                image_cache_mb: Some(192),
            },
        };
        let profile = profile_of(&choices);
        assert_eq!(profile.form_factor, choices.form_factor);
        assert_eq!(profile.tier, choices.tier);
        assert_eq!(profile.overrides, choices.overrides);
    }

    #[test]
    fn all_auto_is_still_a_valid_file_that_changes_nothing() {
        let choices = Choices::default();
        let text = render_vmf(&choices);
        assert!(text.contains("VMF_VERSION=\"1\""));
        assert!(text.contains("# FORM_FACTOR is not set"));
        let profile = profile_of(&choices);
        assert!(profile.form_factor.is_none() && profile.tier.is_none());
        assert_eq!(profile.overrides, AdaptOverrides::default());
    }

    #[test]
    fn every_form_factor_and_tier_survives_the_file() {
        for form_factor in FormFactor::ALL {
            for tier in Tier::ALL {
                let choices = Choices {
                    form_factor: Some(form_factor),
                    tier: Some(tier),
                    ..Default::default()
                };
                let profile = profile_of(&choices);
                assert_eq!(
                    (profile.form_factor, profile.tier),
                    (Some(form_factor), Some(tier))
                );
            }
        }
    }

    #[test]
    fn the_warning_is_in_every_written_file_and_says_how_to_undo_it() {
        let text = render_vmf(&Choices::default());
        let second = text.lines().nth(1).unwrap();
        assert!(
            second.starts_with("# WARNING: A WRONG VALUE IN THIS FILE CAN MAKE APPS ADAPT BADLY")
        );
        assert!(!text.to_lowercase().contains("warranty"));
        assert!(text.lines().nth(2).unwrap().contains("Delete this file"));
    }

    #[test]
    fn writing_creates_the_directory_and_reports_permission_problems_with_the_fix() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("etc").join("VamoraSys");
        let path = write_profile(&nested, &Choices::default()).unwrap();
        assert!(path.is_file());

        // a file where the directory should be: the error names the path
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, "x").unwrap();
        let error = write_profile(&blocked.join("sub"), &Choices::default()).unwrap_err();
        assert!(matches!(error, DeviceError::Write { .. }));
        assert!(error.to_string().contains("cannot write"));
    }

    #[test]
    fn an_existing_profile_prefills_the_wizard() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("device.vmf"),
            "VMF_VERSION=1\nFORM_FACTOR=tv\nTIER=budget\nBLUR=on\n",
        )
        .unwrap();
        let profile = DeviceProfile::read(dir.path()).unwrap().unwrap();
        let choices = choices_from(&profile);
        assert_eq!(choices.form_factor, Some(FormFactor::Tv));
        assert_eq!(choices.tier, Some(Tier::Budget));
        assert_eq!(choices.overrides.blur, Some(true));
    }

    #[test]
    fn the_summary_names_auto_choices_with_what_was_detected() {
        let detected = device::detect_hardware(tempdir().unwrap().path());
        let lines = summary_lines(
            Path::new("/etc/VamoraSys/device.vmf"),
            &Choices::default(),
            &detected,
        );
        assert!(lines[1].contains("auto-detect (desktop)"));
        assert!(lines[2].contains("auto (midrange)"));
        assert!(lines[3].contains("tier defaults"));
    }
}
