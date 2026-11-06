//! Reads hardware facts for `vamorasys device`. Every path is resolved under `root`
//! ("/" on a real device, `VAMORASYS_PROBE_ROOT` in tests) so it can run against a fake tree.

use crate::device::Signals;
use std::fs;
use std::path::{Path, PathBuf};

fn read(root: &Path, relative: &str) -> Option<String> {
    fs::read_to_string(root.join(relative.trim_start_matches('/')))
        .ok()
        .map(|text| text.trim().to_string())
}

fn list(root: &Path, relative: &str) -> Vec<PathBuf> {
    match fs::read_dir(root.join(relative.trim_start_matches('/'))) {
        Ok(entries) => {
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .collect();
            paths.sort();
            paths
        }
        Err(_) => Vec::new(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn read_power(root: &Path, signals: &mut Signals) {
    for directory in list(root, "sys/class/power_supply") {
        let kind = fs::read_to_string(directory.join("type")).unwrap_or_default();
        let scope = fs::read_to_string(directory.join("scope")).unwrap_or_default();
        // scope "Device" = a peripheral's battery (mouse, headset), not the machine's own
        if kind.trim() == "Battery" && scope.trim() != "Device" {
            signals.has_battery = true;
        }
    }
}

fn parse_hex(value: &str) -> u64 {
    u64::from_str_radix(value.split_whitespace().last().unwrap_or("0"), 16).unwrap_or(0)
}

/// Parses /proc/bus/input/devices
fn read_inputs(root: &Path, signals: &mut Signals) {
    let Some(text) = read(root, "proc/bus/input/devices") else {
        return;
    };
    for block in text.split("\n\n") {
        let (mut name, mut handlers) = (String::new(), String::new());
        let (mut properties, mut events, mut switches) = (0u64, 0u64, 0u64);
        let mut bus = 0u64;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("I: ") {
                // "I: Bus=0011 Vendor=0001 ..." (0003 USB, 0005 Bluetooth, 0011 PS/2, 0018 I2C-HID, 0019 platform)
                if let Some(hex) = value
                    .split_whitespace()
                    .find_map(|part| part.strip_prefix("Bus="))
                {
                    bus = u64::from_str_radix(hex, 16).unwrap_or(0);
                }
            } else if let Some(value) = line.strip_prefix("N: Name=") {
                name = value.trim().trim_matches('"').to_ascii_lowercase();
            } else if let Some(value) = line.strip_prefix("H: Handlers=") {
                handlers = value.to_ascii_lowercase();
            } else if let Some(value) = line.strip_prefix("B: PROP=") {
                properties = parse_hex(value);
            } else if let Some(value) = line.strip_prefix("B: EV=") {
                events = parse_hex(value);
            } else if let Some(value) = line.strip_prefix("B: SW=") {
                switches = parse_hex(value);
            }
        }
        if name.is_empty() {
            continue;
        }
        let is_remote = [
            "remote",
            "infrared",
            "ir receiver",
            "cec",
            "consumer control",
        ]
        .iter()
        .any(|word| name.contains(word));
        let is_pen = name.contains("pen") || name.contains("stylus");
        let has_abs = events & (1 << 3) != 0; // EV_ABS
        let has_repeat = events & (1 << 20) != 0; // EV_REP: real keyboards repeat keys
        let direct = properties & (1 << 1) != 0; // INPUT_PROP_DIRECT: touchscreen-like

        if direct && has_abs && !is_pen {
            signals.has_touch = true;
        } else if handlers.contains("kbd") && has_repeat && !is_remote {
            signals.has_keyboard = true;
            // Part of the machine, as opposed to a Bluetooth or USB keyboard that can come and go.
            if matches!(bus, 0x11 | 0x18 | 0x19) {
                signals.has_builtin_keyboard = true;
            }
        } else if handlers.contains("mouse") && !is_remote {
            signals.has_pointer = true;
        }
        if switches & 1 != 0 {
            signals.has_lid_switch = true; // SW_LID
        }
    }
}

fn read_connectivity(root: &Path, signals: &mut Signals) {
    signals.has_modem = list(root, "sys/class/net").iter().any(|path| {
        let name = file_name(path);
        name.starts_with("wwan") || name.starts_with("rmnet")
    }) || !list(root, "sys/class/wwan").is_empty();
    signals.has_cec = !list(root, "sys/class/cec").is_empty() || root.join("dev/cec0").exists();
}

/// IIO ABI: `in_angl*` is reported in radians after scale.
fn read_hinge(root: &Path, signals: &mut Signals) {
    for directory in list(root, "sys/bus/iio/devices") {
        let name = fs::read_to_string(directory.join("name"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !name.contains("hinge") {
            continue;
        }
        signals.has_hinge_sensor = true;
        let raw = fs::read_to_string(directory.join("in_angl_raw"))
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok());
        let scale = fs::read_to_string(directory.join("in_angl_scale"))
            .ok()
            .and_then(|value| value.trim().parse::<f32>().ok())
            .unwrap_or(1.0);
        if let Some(raw) = raw {
            signals.hinge_angle = Some((raw * scale).to_degrees());
        }
    }
}

/// Physical diagonal from the EDID size bytes (21 = width cm, 22 = height cm).
pub fn edid_diagonal_in(edid: &[u8]) -> Option<f32> {
    if edid.len() < 23 {
        return None;
    }
    let (width, height) = (edid[21] as f32, edid[22] as f32);
    if width == 0.0 || height == 0.0 {
        return None; // projector / aspect-ratio-only encoding
    }
    Some((width * width + height * height).sqrt() / 2.54)
}

fn read_displays(root: &Path, signals: &mut Signals) {
    for directory in list(root, "sys/class/drm") {
        let name = file_name(&directory);
        if !name.starts_with("card") || !name.contains('-') {
            continue;
        }
        if fs::read_to_string(directory.join("status"))
            .unwrap_or_default()
            .trim()
            != "connected"
        {
            continue;
        }
        let Some(diagonal) = fs::read(directory.join("edid"))
            .ok()
            .and_then(|edid| edid_diagonal_in(&edid))
        else {
            continue;
        };
        let internal = ["edp", "lvds", "dsi"]
            .iter()
            .any(|kind| name.to_ascii_lowercase().contains(kind));
        let slot = if internal {
            &mut signals.internal_diag_in
        } else {
            &mut signals.external_diag_in
        };
        *slot = Some(slot.map_or(diagonal, |old| old.max(diagonal)));
    }
}

/// "0-7" or "0,2-3" -> number of CPUs
fn count_cpu_list(text: &str) -> Option<u32> {
    let mut count = 0u32;
    for part in text.trim().split(',') {
        if part.is_empty() {
            continue;
        }
        match part.split_once('-') {
            Some((first, last)) => {
                count += last
                    .parse::<u32>()
                    .ok()?
                    .checked_sub(first.parse::<u32>().ok()?)?
                    + 1
            }
            None => {
                part.parse::<u32>().ok()?;
                count += 1;
            }
        }
    }
    (count > 0).then_some(count)
}

fn read_performance(root: &Path, signals: &mut Signals) {
    if let Some(meminfo) = read(root, "proc/meminfo") {
        for line in meminfo.lines() {
            if let Some(value) = line.strip_prefix("MemTotal:") {
                let kilobytes: u64 = value
                    .trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                if kilobytes > 0 {
                    signals.ram_mb = Some((kilobytes / 1024) as u32);
                }
            }
        }
    }
    signals.cpu_cores = read(root, "sys/devices/system/cpu/online")
        .and_then(|text| count_cpu_list(&text))
        .or_else(|| {
            let count = list(root, "sys/devices/system/cpu")
                .iter()
                .filter(|path| {
                    let name = file_name(path);
                    name.len() > 3
                        && name.starts_with("cpu")
                        && name[3..].chars().all(|c| c.is_ascii_digit())
                })
                .count() as u32;
            (count > 0).then_some(count)
        });
    let mut fastest: Option<u32> = None;
    for directory in list(root, "sys/devices/system/cpu") {
        if let Ok(value) = fs::read_to_string(directory.join("cpufreq/cpuinfo_max_freq")) {
            if let Ok(khz) = value.trim().parse::<u32>() {
                fastest = Some(fastest.map_or(khz / 1000, |best| best.max(khz / 1000)));
            }
        }
    }
    signals.max_cpu_mhz = fastest;
}

/// Board names that mean a single-board computer.
const SBC_MODELS: [&str; 12] = [
    "raspberry pi",
    "orange pi",
    "rock pi",
    "rock 5",
    "radxa",
    "banana pi",
    "odroid",
    "beaglebone",
    "jetson",
    "pine64",
    "libre computer",
    "khadas",
];

fn read_board(root: &Path, signals: &mut Signals) {
    let model = [
        "proc/device-tree/model",
        "sys/firmware/devicetree/base/model",
    ]
    .iter()
    .find_map(|path| fs::read(root.join(path)).ok())
    .map(|bytes| {
        String::from_utf8_lossy(&bytes)
            .trim_matches(char::from(0))
            .trim()
            .to_ascii_lowercase()
    });
    if let Some(model) = model {
        signals.is_sbc = SBC_MODELS.iter().any(|name| model.contains(name));
    }
}

pub fn gather(root: &Path) -> Signals {
    let mut signals = Signals {
        chassis_type: read(root, "sys/class/dmi/id/chassis_type")
            .and_then(|value| value.parse().ok()),
        ..Default::default()
    };
    read_power(root, &mut signals);
    read_inputs(root, &mut signals);
    read_connectivity(root, &mut signals);
    read_hinge(root, &mut signals);
    read_displays(root, &mut signals);
    read_performance(root, &mut signals);
    read_board(root, &mut signals);
    signals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{classify, AnimationLevel, FormFactor, Posture, Tier};
    use tempfile::{tempdir, TempDir};

    fn put(root: &Path, relative: &str, data: impl AsRef<[u8]>) {
        let file = root.join(relative);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, data).unwrap();
    }

    fn edid(width_cm: u8, height_cm: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 128];
        bytes[21] = width_cm;
        bytes[22] = height_cm;
        bytes
    }

    fn detect(dir: &TempDir) -> crate::device::DeviceInfo {
        classify(&gather(dir.path()))
    }

    const KEYBOARD: &str = "N: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event3 leds\nB: PROP=0\nB: EV=120013\n\n";
    const MOUSE: &str = "N: Name=\"Logitech USB Mouse\"\nH: Handlers=mouse1 event6\nB: EV=17\n\n";
    const TOUCHPAD: &str =
        "N: Name=\"SynPS/2 Touchpad\"\nH: Handlers=mouse0 event5\nB: PROP=5\nB: EV=b\n\n";
    const TOUCHSCREEN: &str =
        "N: Name=\"Goodix Capacitive TouchScreen\"\nH: Handlers=event4\nB: PROP=2\nB: EV=b\n\n";
    const LID: &str =
        "N: Name=\"Lid Switch\"\nH: Handlers=event0\nB: PROP=0\nB: EV=21\nB: SW=1\n\n";
    const IR_REMOTE: &str =
        "N: Name=\"sunxi-ir remote\"\nH: Handlers=kbd event2\nB: PROP=0\nB: EV=100013\n\n";

    #[test]
    fn small_desktop_monitor_is_a_desktop() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/dmi/id/chassis_type", "3\n");
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{KEYBOARD}{MOUSE}"),
        );
        put(
            r.path(),
            "sys/class/drm/card0-HDMI-A-1/status",
            "connected\n",
        );
        put(r.path(), "sys/class/drm/card0-HDMI-A-1/edid", edid(15, 9)); // ~7"
        let info = detect(&r);
        assert_eq!(info.form_factor, FormFactor::Desktop);
        assert!(info.diagonal_in.unwrap() < 8.0);
    }

    #[test]
    fn peripheral_battery_is_not_the_machines_battery() {
        let r = tempdir().unwrap();
        put(
            r.path(),
            "sys/class/power_supply/hidpp_battery_0/type",
            "Battery\n",
        );
        put(
            r.path(),
            "sys/class/power_supply/hidpp_battery_0/scope",
            "Device\n",
        );
        assert!(!gather(r.path()).has_battery);
    }

    #[test]
    fn big_touch_tablet_is_a_tablet() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/power_supply/BAT0/type", "Battery\n");
        put(r.path(), "proc/bus/input/devices", TOUCHSCREEN);
        put(r.path(), "sys/class/drm/card0-DSI-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-DSI-1/edid", edid(31, 20)); // ~14.5"
        let info = detect(&r);
        assert_eq!(info.form_factor, FormFactor::Tablet);
        assert!(info.diagonal_in.unwrap() > 14.0);
    }

    #[test]
    fn phone_has_a_modem_and_a_small_panel() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/power_supply/battery/type", "Battery\n");
        put(r.path(), "proc/bus/input/devices", TOUCHSCREEN);
        put(r.path(), "sys/class/net/wwan0/address", "00\n");
        put(r.path(), "sys/class/drm/card0-DSI-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-DSI-1/edid", edid(7, 15));
        assert_eq!(detect(&r).form_factor, FormFactor::Phone);
    }

    #[test]
    fn laptop_via_chassis() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/dmi/id/chassis_type", "10\n");
        put(r.path(), "sys/class/power_supply/BAT0/type", "Battery\n");
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{KEYBOARD}{TOUCHPAD}{LID}"),
        );
        assert_eq!(detect(&r).form_factor, FormFactor::Laptop);
    }

    #[test]
    fn foldable_laptop_with_hinge_and_posture() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/power_supply/BAT0/type", "Battery\n");
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{KEYBOARD}{TOUCHSCREEN}"),
        );
        put(r.path(), "sys/class/drm/card0-eDP-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-eDP-1/edid", edid(38, 28)); // ~18.6"
        put(r.path(), "sys/bus/iio/devices/iio:device3/name", "hinge\n");
        put(
            r.path(),
            "sys/bus/iio/devices/iio:device3/in_angl_raw",
            "3141\n",
        );
        put(
            r.path(),
            "sys/bus/iio/devices/iio:device3/in_angl_scale",
            "0.001\n",
        ); // 3.141 rad = 180 deg
        let info = detect(&r);
        assert_eq!(info.form_factor, FormFactor::FoldableLaptop);
        assert_eq!(info.posture, Some(Posture::Unfolded));
    }

    const PS2_KEYBOARD: &str = "I: Bus=0011 Vendor=0001 Product=0001 Version=ab41\nN: Name=\"AT Translated Set 2 keyboard\"\nH: Handlers=sysrq kbd event3 leds\nB: PROP=0\nB: EV=120013\n\n";
    const BLUETOOTH_KEYBOARD: &str = "I: Bus=0005 Vendor=046d Product=b35b Version=0001\nN: Name=\"MX Keys Mini\"\nH: Handlers=sysrq kbd event7 leds\nB: PROP=0\nB: EV=12001f\n\n";
    const USB_KEYBOARD: &str = "I: Bus=0003 Vendor=04d9 Product=0169 Version=0110\nN: Name=\"Mini USB Keyboard\"\nH: Handlers=sysrq kbd event3 leds\nB: PROP=0\nB: EV=120013\n\n";

    #[test]
    fn foldable_laptop_with_a_wireless_keyboard_is_still_a_foldable_laptop() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/power_supply/BAT0/type", "Battery\n");
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{BLUETOOTH_KEYBOARD}{TOUCHSCREEN}"),
        );
        put(r.path(), "sys/class/drm/card0-eDP-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-eDP-1/edid", edid(38, 28));
        put(r.path(), "sys/bus/iio/devices/iio:device3/name", "hinge\n");
        let signals = gather(r.path());
        assert!(signals.has_keyboard && !signals.has_builtin_keyboard);
        assert_eq!(classify(&signals).form_factor, FormFactor::FoldableLaptop);
    }

    #[test]
    fn laptop_with_a_hinge_sensor_and_a_built_in_keyboard_is_a_laptop() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/dmi/id/chassis_type", "31\n");
        put(r.path(), "sys/class/power_supply/BAT0/type", "Battery\n");
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{PS2_KEYBOARD}{TOUCHSCREEN}{LID}"),
        );
        put(r.path(), "sys/class/drm/card0-eDP-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-eDP-1/edid", edid(30, 20));
        put(r.path(), "sys/bus/iio/devices/iio:device2/name", "hinge\n");
        let signals = gather(r.path());
        assert!(signals.has_builtin_keyboard);
        assert_eq!(classify(&signals).form_factor, FormFactor::Laptop);
    }

    #[test]
    fn raspberry_pi_with_a_small_screen_and_keyboard_is_a_cyberdeck() {
        let r = tempdir().unwrap();
        put(
            r.path(),
            "proc/device-tree/model",
            "Raspberry Pi 5 Model B Rev 1.0\0",
        );
        put(r.path(), "proc/bus/input/devices", USB_KEYBOARD);
        put(
            r.path(),
            "sys/class/drm/card1-HDMI-A-1/status",
            "connected\n",
        );
        put(r.path(), "sys/class/drm/card1-HDMI-A-1/edid", edid(15, 9)); // ~7" HDMI display
        let signals = gather(r.path());
        assert!(signals.is_sbc);
        assert_eq!(classify(&signals).form_factor, FormFactor::Cyberdeck);
    }

    #[test]
    fn raspberry_pi_on_a_big_monitor_is_a_desktop() {
        let r = tempdir().unwrap();
        put(
            r.path(),
            "proc/device-tree/model",
            "Raspberry Pi 4 Model B Rev 1.4\0",
        );
        put(
            r.path(),
            "proc/bus/input/devices",
            format!("{USB_KEYBOARD}{MOUSE}"),
        );
        put(
            r.path(),
            "sys/class/drm/card1-HDMI-A-1/status",
            "connected\n",
        );
        put(r.path(), "sys/class/drm/card1-HDMI-A-1/edid", edid(60, 34)); // ~27"
        assert_eq!(detect(&r).form_factor, FormFactor::Desktop);
    }

    #[test]
    fn tv_does_not_count_an_ir_remote_as_a_keyboard() {
        let r = tempdir().unwrap();
        put(r.path(), "proc/bus/input/devices", IR_REMOTE);
        put(r.path(), "dev/cec0", "");
        put(
            r.path(),
            "sys/class/drm/card0-HDMI-A-1/status",
            "connected\n",
        );
        put(r.path(), "sys/class/drm/card0-HDMI-A-1/edid", edid(121, 68)); // ~54.6"
        assert_eq!(detect(&r).form_factor, FormFactor::Tv);
    }

    #[test]
    fn watch_is_tiny_touch_with_a_battery() {
        let r = tempdir().unwrap();
        put(r.path(), "sys/class/power_supply/battery/type", "Battery\n");
        put(r.path(), "proc/bus/input/devices", TOUCHSCREEN);
        put(r.path(), "sys/class/drm/card0-DSI-1/status", "connected\n");
        put(r.path(), "sys/class/drm/card0-DSI-1/edid", edid(3, 3)); // ~1.7"
        assert_eq!(detect(&r).form_factor, FormFactor::Watch);
    }

    #[test]
    fn reads_ram_cores_and_clock_and_picks_a_tier() {
        let r = tempdir().unwrap();
        put(
            r.path(),
            "proc/meminfo",
            "MemTotal:        3686400 kB\nMemFree:  1000 kB\n",
        );
        put(r.path(), "sys/devices/system/cpu/online", "0-7\n");
        put(
            r.path(),
            "sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq",
            "1800000\n",
        );
        put(
            r.path(),
            "sys/devices/system/cpu/cpu7/cpufreq/cpuinfo_max_freq",
            "2000000\n",
        );
        let signals = gather(r.path());
        assert_eq!(signals.ram_mb, Some(3600));
        assert_eq!(signals.cpu_cores, Some(8));
        assert_eq!(signals.max_cpu_mhz, Some(2000));
        let info = classify(&signals);
        assert_eq!(info.tier, Tier::Entry);
        assert_eq!(info.adapt.animations, AnimationLevel::Reduced);
    }

    #[test]
    fn cpu_core_count_falls_back_to_cpu_directories() {
        let r = tempdir().unwrap();
        for n in 0..4 {
            put(
                r.path(),
                &format!("sys/devices/system/cpu/cpu{n}/online"),
                "1\n",
            );
        }
        put(r.path(), "sys/devices/system/cpu/cpufreq/policy0", "x");
        assert_eq!(gather(r.path()).cpu_cores, Some(4));
    }

    #[test]
    fn cpu_lists_are_counted() {
        assert_eq!(count_cpu_list("0-7"), Some(8));
        assert_eq!(count_cpu_list("0,2-3"), Some(3));
        assert_eq!(count_cpu_list("0"), Some(1));
        assert_eq!(count_cpu_list(""), None);
        assert_eq!(count_cpu_list("7-0"), None);
    }

    #[test]
    fn empty_probe_root_still_yields_a_usable_answer() {
        let r = tempdir().unwrap();
        let info = detect(&r);
        assert_eq!(info.form_factor, FormFactor::Desktop);
        assert_eq!(info.tier, Tier::Midrange);
    }
}
