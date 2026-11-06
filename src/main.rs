mod config;
mod database;
mod device;
mod device_probe;
mod device_setup;
mod release;
mod theme;
mod tui;
mod vmf;

use clap::{Args, Parser, Subcommand};
use config::{
    normalize_database_name, probe_root_from_environment, release_root_from_environment,
    split_setting_key, SettingsPaths,
};
use database::{
    database_for_table, delete_user_setting, discover_databases, effective_settings,
    initialize_database, is_core_database, layer_settings, list_tables, required_database_tables,
    reset_system_setting, setting_from_default, sync_user_defaults, table_exists,
    write_system_setting, write_user_setting, ACCESSIBILITY_DATABASE, CORE_DATABASES,
    VAMIFY_DATABASE,
};
use release::{release_summary, ReleaseMetadata};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ffi::CStr;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::{Command as ProcessCommand, ExitCode, Output};
use thiserror::Error;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CYAN: &str = "\x1b[38;2;14;165;233m";
const GREEN: &str = "\x1b[38;2;22;163;74m";
const RED: &str = "\x1b[38;2;220;38;38m";
const YELLOW: &str = "\x1b[38;2;245;158;11m";
const DIM: &str = "\x1b[38;2;100;116;139m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Error)]
enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Database(#[from] database::DatabaseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Release(#[from] release::ReleaseError),
    #[error(transparent)]
    Device(#[from] device::DeviceError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Parser, Debug)]
#[command(name = "vamorasys", version = APP_VERSION, about = "VamoraSys Linux system middleware and configuration CLI")]
struct Cli {
    /// Print machine-readable JSON instead of terminal-oriented output.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Status,
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    Keyboard {
        #[command(subcommand)]
        command: KeyboardCommand,
    },
    /// What kind of device this is, how powerful it is, and what apps should do about it.
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    #[command(hide = true)]
    Initialize(InitializeArgs),
}

#[derive(Subcommand, Debug)]
enum SettingsCommand {
    Get {
        key: String,
        /// Required for app databases, for example apps/vamify-calendar.db.
        #[arg(short, long)]
        database: Option<String>,
    },
    Set {
        key: String,
        value: String,
        /// Required for app databases, for example apps/vamify-calendar.db.
        #[arg(short, long)]
        database: Option<String>,
    },
    Reset {
        key: String,
        /// Required for app databases, for example apps/vamify-calendar.db.
        #[arg(short, long)]
        database: Option<String>,
    },
    List {
        database: String,
    },
    Dump {
        database: String,
    },
    Databases,
    Tables,
}

#[derive(Subcommand, Debug)]
enum KeyboardCommand {
    /// Show the active layout, configured layouts, and detected Wayland backend.
    Status,
    /// List configured keyboard layouts.
    List,
    /// Add a layout to the cycle list.
    Add { layout: String },
    /// Remove a layout from the cycle list.
    Remove { layout: String },
    /// Make a configured layout active and apply it to the compositor.
    Set { layout: String },
    /// Select the next configured layout and apply it to the compositor.
    Next,
    /// Re-apply the active layout to the compositor.
    Apply,
    /// Pick a layout from a searchable list instead of typing its code (interactive).
    Pick,
}

#[derive(Subcommand, Debug)]
enum DeviceCommand {
    /// Show the form factor, performance tier and adaptation hints, and where each came from.
    Status,
    /// Print one value, for example `vamorasys device get form_factor`.
    Get { key: String },
    /// Interactive wizard that writes /etc/VamoraSys/device.vmf (offered automatically when it is missing).
    Setup,
}

#[derive(Args, Debug)]
struct InitializeArgs {
    #[arg(long)]
    system: bool,
    #[arg(long)]
    user: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json_output = cli.json;
    match run(cli) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            if json_output {
                print_json(json!({ "ok": false, "error": error.to_string() }));
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<bool, AppError> {
    // `device` only reads hardware facts and device.vmf. It needs no home directory and must not
    // create, sync or migrate any settings database, so it is handled before all of that.
    let command = match cli.command {
        Command::Device { command } => return run_device(command, cli.json),
        other => other,
    };

    let paths = SettingsPaths::from_environment().map_err(AppError::Message)?;
    if !running_as_root() {
        sync_user_defaults(&paths)?;
    }
    migrate_core_databases(&paths)?;

    match command {
        Command::Device { .. } => unreachable!("handled before the settings databases are opened"),
        Command::Status => run_status(&paths, cli.json),
        Command::Settings { command } => {
            run_settings(&paths, command, cli.json)?;
            Ok(true)
        }
        Command::Keyboard { command } => {
            run_keyboard(&paths, command, cli.json)?;
            Ok(true)
        }
        Command::Initialize(args) => {
            run_initialize(&paths, args, cli.json)?;
            Ok(true)
        }
    }
}

fn migrate_core_databases(paths: &SettingsPaths) -> Result<(), AppError> {
    if running_as_root() {
        for database in CORE_DATABASES {
            let path = paths.system_database(database);
            if path.is_file() {
                initialize_database(&path, database, true)?;
            }
        }
    } else {
        for database in CORE_DATABASES {
            let system_path = paths.system_database(database);
            let user_path = paths.user_database(database);
            if system_path.is_file() || user_path.is_file() {
                initialize_database(&user_path, database, true)?;
            }
        }
    }
    Ok(())
}

fn run_initialize(
    paths: &SettingsPaths,
    args: InitializeArgs,
    json_output: bool,
) -> Result<(), AppError> {
    if !args.system && !args.user {
        return Err(AppError::Message(
            "initialize requires --system, --user, or both".to_string(),
        ));
    }

    let mut messages = Vec::new();
    if args.system {
        fs::create_dir_all(&paths.system_settings)?;
        initialize_database(
            &paths.system_database(VAMIFY_DATABASE),
            VAMIFY_DATABASE,
            true,
        )?;
        initialize_database(
            &paths.system_database(ACCESSIBILITY_DATABASE),
            ACCESSIBILITY_DATABASE,
            true,
        )?;
        messages.push(format!(
            "initialized factory databases in {}",
            paths.system_settings.display()
        ));
    }
    if args.user {
        fs::create_dir_all(&paths.user_settings)?;
        initialize_database(&paths.user_database(VAMIFY_DATABASE), VAMIFY_DATABASE, true)?;
        initialize_database(
            &paths.user_database(ACCESSIBILITY_DATABASE),
            ACCESSIBILITY_DATABASE,
            true,
        )?;
        messages.push(format!(
            "initialized user databases in {}",
            paths.user_settings.display()
        ));
    }

    if json_output {
        print_json(json!({ "ok": true, "messages": messages }));
    } else {
        for message in messages {
            println!("{message}");
        }
    }
    Ok(())
}

fn run_settings(
    paths: &SettingsPaths,
    command: SettingsCommand,
    json_output: bool,
) -> Result<(), AppError> {
    match command {
        SettingsCommand::Get { key, database } => {
            settings_get(paths, &key, database.as_deref(), json_output)
        }
        SettingsCommand::Set {
            key,
            value,
            database,
        } => settings_set(paths, &key, &value, database.as_deref(), json_output),
        SettingsCommand::Reset { key, database } => {
            settings_reset(paths, &key, database.as_deref(), json_output)
        }
        SettingsCommand::List { database } => settings_list(paths, &database, json_output),
        SettingsCommand::Dump { database } => settings_dump(paths, &database, json_output),
        SettingsCommand::Databases => settings_databases(paths, json_output),
        SettingsCommand::Tables => settings_tables(paths, json_output),
    }
}

fn resolve_database(table: &str, requested: Option<&str>) -> Result<String, AppError> {
    if let Some(database) = requested {
        return normalize_database_name(database).map_err(AppError::Message);
    }
    database_for_table(table)
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::Message(format!(
                "unknown settings table '{table}'; use --database for app settings tables"
            ))
        })
}

fn settings_get(
    paths: &SettingsPaths,
    qualified_key: &str,
    requested_database: Option<&str>,
    json_output: bool,
) -> Result<(), AppError> {
    let (table, key) = split_setting_key(qualified_key).map_err(AppError::Message)?;
    let database = resolve_database(table, requested_database)?;
    let user_path = paths.user_database(&database);
    let system_path = paths.system_database(&database);

    let (setting, source) = if user_path.is_file() {
        if let Some(setting) = find_setting(&user_path, table, key)? {
            (Some(setting), Some("user"))
        } else if system_path.is_file() {
            (find_setting(&system_path, table, key)?, Some("system"))
        } else {
            (None, None)
        }
    } else if system_path.is_file() {
        (find_setting(&system_path, table, key)?, Some("system"))
    } else {
        (None, None)
    };

    let Some(setting) = setting else {
        return Err(AppError::Message(format!(
            "setting '{qualified_key}' was not found in user or factory configuration"
        )));
    };

    if json_output {
        print_json(setting_json(
            &database,
            &setting,
            source.unwrap_or("unknown"),
        ));
    } else {
        println!("{}", setting.value);
    }
    Ok(())
}

fn settings_set(
    paths: &SettingsPaths,
    qualified_key: &str,
    value: &str,
    requested_database: Option<&str>,
    json_output: bool,
) -> Result<(), AppError> {
    let (table, key) = split_setting_key(qualified_key).map_err(AppError::Message)?;
    let database = resolve_database(table, requested_database)?;
    let system_scope = running_as_root();
    if system_scope {
        write_system_setting(paths, &database, table, key, value)?;
    } else {
        write_user_setting(paths, &database, table, key, value)?;
    }
    let scope = if system_scope { "system" } else { "user" };

    if json_output {
        print_json(json!({
            "ok": true,
            "action": "set",
            "database": database,
            "table": table,
            "key": key,
            "value": value,
            "scope": scope,
        }));
    } else if system_scope {
        println!("set {qualified_key} = {value} (system default)");
    } else {
        println!("set {qualified_key} = {value} (user override)");
    }
    Ok(())
}

fn settings_reset(
    paths: &SettingsPaths,
    qualified_key: &str,
    requested_database: Option<&str>,
    json_output: bool,
) -> Result<(), AppError> {
    let (table, key) = split_setting_key(qualified_key).map_err(AppError::Message)?;
    let database = resolve_database(table, requested_database)?;
    let system_scope = running_as_root();
    let removed = if system_scope {
        reset_system_setting(paths, &database, table, key)?
    } else {
        delete_user_setting(paths, &database, table, key)?
    };
    let effective = effective_value(paths, &database, table, key)?;
    let scope = if system_scope { "system" } else { "user" };

    if json_output {
        print_json(json!({
            "ok": true,
            "action": "reset",
            "database": database,
            "table": table,
            "key": key,
            "scope": scope,
            "removed": removed,
            "effective_value": effective,
        }));
    } else if removed {
        match effective {
            Some(value) => println!("reset {qualified_key}; effective value is {value}"),
            None => println!("reset {qualified_key}; no value remains"),
        }
    } else {
        println!("no {scope} setting exists for {qualified_key}");
    }
    Ok(())
}

fn settings_list(paths: &SettingsPaths, input: &str, json_output: bool) -> Result<(), AppError> {
    let database = normalize_database_name(input).map_err(AppError::Message)?;
    let settings = effective_settings(paths, &database)?;
    if settings.is_empty() {
        return Err(AppError::Message(format!(
            "database '{database}' is not available or contains no settings"
        )));
    }

    if json_output {
        print_json(json!({
            "database": database,
            "settings": settings.iter().map(|setting| setting_json_value(setting)).collect::<Vec<_>>(),
        }));
    } else {
        println!("{}", paint(&database, &theme::accent(), true));
        print_settings(&settings);
    }
    Ok(())
}

fn settings_dump(paths: &SettingsPaths, input: &str, json_output: bool) -> Result<(), AppError> {
    let database = normalize_database_name(input).map_err(AppError::Message)?;
    let (system, user) = layer_settings(paths, &database)?;
    let effective = effective_settings(paths, &database)?;
    if system.is_empty() && user.is_empty() {
        return Err(AppError::Message(format!(
            "database '{database}' is not available or contains no settings"
        )));
    }

    if json_output {
        print_json(json!({
            "database": database,
            "effective": effective.iter().map(|setting| setting_json_value(setting)).collect::<Vec<_>>(),
            "user_overrides": user.iter().map(|setting| setting_json_value(setting)).collect::<Vec<_>>(),
            "factory": system.iter().map(|setting| setting_json_value(setting)).collect::<Vec<_>>(),
        }));
    } else {
        println!(
            "{} {}",
            paint("database:", &theme::accent(), true),
            database
        );
        println!("{}", paint("[effective]", CYAN, true));
        print_settings(&effective);
        println!("{}", paint("[user overrides]", CYAN, true));
        print_settings(&user);
        println!("{}", paint("[factory]", CYAN, true));
        print_settings(&system);
    }
    Ok(())
}

fn settings_databases(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let names = discover_databases(paths)?;
    let mut all_names = BTreeSet::new();
    all_names.extend(CORE_DATABASES.iter().map(|name| (*name).to_string()));
    all_names.extend(names);

    let rows = all_names
        .into_iter()
        .map(|name| {
            let registration = if is_core_database(&name) {
                "core"
            } else {
                "app"
            };
            vec![
                name.clone(),
                registration.to_string(),
                availability(paths.system_database(&name).as_path()).to_string(),
                availability(paths.user_database(&name).as_path()).to_string(),
            ]
        })
        .collect::<Vec<_>>();

    if json_output {
        let databases = rows
            .iter()
            .map(|row| {
                json!({
                    "database": row[0],
                    "registration": row[1],
                    "system": row[2],
                    "user": row[3],
                })
            })
            .collect::<Vec<_>>();
        print_json(json!({ "databases": databases }));
    } else {
        print_table(&["DATABASE", "KIND", "SYSTEM", "USER"], &rows);
    }
    Ok(())
}

fn settings_tables(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let names = discover_databases(paths)?;
    let mut all_names = BTreeSet::new();
    all_names.extend(CORE_DATABASES.iter().map(|name| (*name).to_string()));
    all_names.extend(names);

    let mut rows = Vec::new();
    let user_name = current_user_name();
    for name in all_names {
        for (layer, path) in [
            ("system", paths.system_database(&name)),
            (user_name.as_str(), paths.user_database(&name)),
        ] {
            if !path.is_file() {
                continue;
            }
            let tables = list_tables(&path)?;
            if tables.is_empty() {
                rows.push(vec![
                    name.clone(),
                    layer.to_string(),
                    "(no tables)".to_string(),
                ]);
            } else {
                for table in tables {
                    rows.push(vec![name.clone(), layer.to_string(), table]);
                }
            }
        }
    }
    if rows.is_empty() {
        return Err(AppError::Message(
            "no settings databases are available".to_string(),
        ));
    }

    if json_output {
        let tables = rows
            .iter()
            .map(|row| json!({ "database": row[0], "owner": row[1], "table": row[2] }))
            .collect::<Vec<_>>();
        print_json(json!({ "tables": tables }));
    } else {
        print_table(&["DATABASE", "OWNER", "TABLE"], &rows);
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct KeyboardConfig {
    layout: String,
    layouts: Vec<String>,
    variant: String,
    options: String,
    model: String,
}

#[derive(Debug)]
struct KeyboardApplyResult {
    backend: String,
    applied: bool,
    message: String,
}

fn run_keyboard(
    paths: &SettingsPaths,
    command: KeyboardCommand,
    json_output: bool,
) -> Result<(), AppError> {
    match command {
        KeyboardCommand::Status => keyboard_status(paths, json_output),
        KeyboardCommand::List => keyboard_list(paths, json_output),
        KeyboardCommand::Add { layout } => keyboard_add(paths, &layout, json_output),
        KeyboardCommand::Remove { layout } => keyboard_remove(paths, &layout, json_output),
        KeyboardCommand::Set { layout } => keyboard_set(paths, &layout, json_output),
        KeyboardCommand::Next => keyboard_next(paths, json_output),
        KeyboardCommand::Apply => keyboard_apply(paths, json_output),
        KeyboardCommand::Pick => keyboard_pick(paths, json_output),
    }
}

fn keyboard_status(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let config = keyboard_config(paths)?;
    let backend = keyboard_backend().to_string();
    let wayland = is_wayland_session();

    if json_output {
        print_json(json!({
            "database": VAMIFY_DATABASE,
            "table": "keyboard",
            "layout": config.layout,
            "layouts": config.layouts,
            "variant": config.variant,
            "options": config.options,
            "model": config.model,
            "wayland": wayland,
            "backend": backend,
        }));
    } else {
        println!("{}", paint("Keyboard layout", &theme::accent(), true));
        println!("active: {}", config.layout);
        println!("layouts: {}", config.layouts.join(", "));
        println!("backend: {backend}");
        println!(
            "session: {}",
            if wayland { "Wayland" } else { "not Wayland" }
        );
    }
    Ok(())
}

fn keyboard_list(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let config = keyboard_config(paths)?;
    if json_output {
        print_json(json!({
            "layouts": config.layouts.iter().map(|layout| json!({
                "layout": layout,
                "active": layout == &config.layout,
            })).collect::<Vec<_>>(),
        }));
    } else {
        let rows = config
            .layouts
            .iter()
            .map(|layout| {
                vec![
                    layout.clone(),
                    if layout == &config.layout {
                        "active".to_string()
                    } else {
                        String::new()
                    },
                ]
            })
            .collect::<Vec<_>>();
        print_table(&["LAYOUT", "STATE"], &rows);
    }
    Ok(())
}

fn keyboard_add(
    paths: &SettingsPaths,
    requested_layout: &str,
    json_output: bool,
) -> Result<(), AppError> {
    let layout = validate_layout(requested_layout)?;
    let mut config = keyboard_config(paths)?;
    if config.layouts.contains(&layout) {
        return Err(AppError::Message(format!(
            "keyboard layout '{layout}' is already configured"
        )));
    }
    config.layouts.push(layout.clone());
    write_keyboard_setting(paths, "layouts", &config.layouts.join(","))?;
    print_keyboard_change(
        "add",
        &config,
        None,
        Some(format!("added {layout} to the layout cycle")),
        json_output,
    );
    Ok(())
}

fn keyboard_remove(
    paths: &SettingsPaths,
    requested_layout: &str,
    json_output: bool,
) -> Result<(), AppError> {
    let layout = validate_layout(requested_layout)?;
    let mut config = keyboard_config(paths)?;
    let Some(index) = config.layouts.iter().position(|item| item == &layout) else {
        return Err(AppError::Message(format!(
            "keyboard layout '{layout}' is not configured"
        )));
    };
    if config.layouts.len() == 1 {
        return Err(AppError::Message(
            "cannot remove the last configured keyboard layout".to_string(),
        ));
    }

    config.layouts.remove(index);
    let active_changed = config.layout == layout;
    if active_changed {
        config.layout = config.layouts[0].clone();
        write_keyboard_setting(paths, "layout", &config.layout)?;
    }
    write_keyboard_setting(paths, "layouts", &config.layouts.join(","))?;

    let apply_result = if active_changed {
        Some(apply_keyboard_layout(&config, Some(&layout)))
    } else {
        None
    };
    print_keyboard_change(
        "remove",
        &config,
        apply_result.as_ref(),
        Some(format!("removed {layout} from the layout cycle")),
        json_output,
    );
    Ok(())
}

fn keyboard_set(
    paths: &SettingsPaths,
    requested_layout: &str,
    json_output: bool,
) -> Result<(), AppError> {
    let layout = validate_layout(requested_layout)?;
    let mut config = keyboard_config(paths)?;
    if !config.layouts.contains(&layout) {
        return Err(AppError::Message(format!(
            "keyboard layout '{layout}' is not configured; add it first with `vamorasys keyboard add {layout}`"
        )));
    }

    let previous_layout = config.layout.clone();
    config.layout = layout;
    write_keyboard_setting(paths, "layout", &config.layout)?;
    let apply_result = apply_keyboard_layout(&config, Some(&previous_layout));
    print_keyboard_change("set", &config, Some(&apply_result), None, json_output);
    Ok(())
}

fn keyboard_next(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let mut config = keyboard_config(paths)?;
    let current_index = config
        .layouts
        .iter()
        .position(|layout| layout == &config.layout)
        .unwrap_or(0);
    let next_index = (current_index + 1) % config.layouts.len();
    let previous_layout = config.layout.clone();
    config.layout = config.layouts[next_index].clone();
    write_keyboard_setting(paths, "layout", &config.layout)?;
    let apply_result = apply_keyboard_layout(&config, Some(&previous_layout));
    print_keyboard_change("next", &config, Some(&apply_result), None, json_output);
    Ok(())
}

fn keyboard_apply(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    let config = keyboard_config(paths)?;
    let apply_result = apply_keyboard_layout(&config, None);
    print_keyboard_change("apply", &config, Some(&apply_result), None, json_output);
    Ok(())
}

/// (layout code, description) from the `! layout` section of the XKB rules list.
fn parse_xkb_layouts(text: &str) -> Vec<(String, String)> {
    let mut layouts = Vec::new();
    let mut in_layouts = false;
    for line in text.lines() {
        if line.starts_with('!') {
            in_layouts = line.trim() == "! layout";
            continue;
        }
        let line = line.trim();
        if !in_layouts || line.is_empty() {
            continue;
        }
        if let Some((code, description)) = line.split_once(char::is_whitespace) {
            layouts.push((code.to_string(), description.trim().to_string()));
        }
    }
    layouts
}

const COMMON_LAYOUTS: [(&str, &str); 30] = [
    ("us", "English (US)"),
    ("gb", "English (UK)"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("br", "Portuguese (Brazil)"),
    ("nl", "Dutch"),
    ("be", "Belgian"),
    ("ch", "Swiss"),
    ("at", "German (Austria)"),
    ("se", "Swedish"),
    ("no", "Norwegian"),
    ("dk", "Danish"),
    ("fi", "Finnish"),
    ("pl", "Polish"),
    ("cz", "Czech"),
    ("ru", "Russian"),
    ("ua", "Ukrainian"),
    ("tr", "Turkish"),
    ("gr", "Greek"),
    ("il", "Hebrew"),
    ("ara", "Arabic"),
    ("jp", "Japanese"),
    ("kr", "Korean"),
    ("cn", "Chinese"),
    ("in", "Indian"),
    ("ca", "Canadian"),
    ("latam", "Spanish (Latin American)"),
];

/// Every layout the system knows, or a short list of common ones when the XKB data is not installed.
fn xkb_layouts(root: &Path) -> Vec<(String, String)> {
    for relative in [
        "usr/share/X11/xkb/rules/base.lst",
        "usr/share/xkeyboard-config-2/rules/base.lst",
        "usr/share/X11/xkb/rules/evdev.lst",
    ] {
        if let Ok(text) = fs::read_to_string(root.join(relative)) {
            let layouts = parse_xkb_layouts(&text);
            if !layouts.is_empty() {
                return layouts;
            }
        }
    }
    COMMON_LAYOUTS
        .iter()
        .map(|(code, description)| (code.to_string(), description.to_string()))
        .collect()
}

/// Same effect as `keyboard add` (when needed) followed by `keyboard set`, chosen from a list.
fn keyboard_pick(paths: &SettingsPaths, json_output: bool) -> Result<(), AppError> {
    if json_output {
        return Err(AppError::Message(
            "keyboard pick is interactive and cannot produce JSON; use `keyboard set` or `keyboard add`".to_string(),
        ));
    }
    if !tui::is_interactive() {
        return Err(AppError::Message(
            "keyboard pick needs an interactive terminal; use `vamorasys keyboard set <layout>`"
                .to_string(),
        ));
    }
    let config = keyboard_config(paths)?;
    let mut available = xkb_layouts(&probe_root_from_environment());
    for layout in &config.layouts {
        if !available.iter().any(|(code, _)| code == layout) {
            available.push((layout.clone(), String::new()));
        }
    }
    let items = available
        .iter()
        .map(|(code, description)| {
            let marker = if code == &config.layout {
                "active"
            } else if config.layouts.contains(code) {
                "in your cycle"
            } else {
                ""
            };
            let detail = match (description.is_empty(), marker.is_empty()) {
                (_, true) => description.clone(),
                (true, false) => marker.to_string(),
                (false, false) => format!("{description} · {marker}"),
            };
            tui::Item::new(code.clone(), detail)
        })
        .collect::<Vec<_>>();
    let initial = available
        .iter()
        .position(|(code, _)| code == &config.layout)
        .unwrap_or(0);
    let info = vec![format!(
        "Active: {}     In your cycle: {}",
        config.layout,
        config.layouts.join(", ")
    )];

    let chosen = {
        let mut terminal = tui::Terminal::enter()
            .map_err(|error| AppError::Message(format!("cannot use the terminal: {error}")))?;
        let screen = tui::Screen {
            title: "Keyboard Layout",
            hint: "↑↓  Navigate     Type  Search     Enter  Select     Esc  Cancel",
            info: &info,
            filterable: true,
        };
        tui::select(&mut terminal, &screen, &items, initial)
    }; // terminal restored before the normal output below
    let Some(index) = chosen else {
        println!("cancelled; nothing changed");
        return Ok(());
    };
    let layout = available[index].0.clone();
    if config.layout == layout {
        println!("keyboard layout '{layout}' is already active");
        return Ok(());
    }
    if !config.layouts.contains(&layout) {
        keyboard_add(paths, &layout, false)?;
    }
    keyboard_set(paths, &layout, false)
}

fn keyboard_config(paths: &SettingsPaths) -> Result<KeyboardConfig, AppError> {
    let layout = keyboard_setting(paths, "layout")?;
    let layouts = parse_layouts(&keyboard_setting(paths, "layouts")?)?;
    let mut layouts = layouts;
    if !layouts.contains(&layout) {
        layouts.insert(0, layout.clone());
    }
    Ok(KeyboardConfig {
        layout,
        layouts,
        variant: keyboard_setting(paths, "variant")?,
        options: keyboard_setting(paths, "options")?,
        model: keyboard_setting(paths, "model")?,
    })
}

fn keyboard_setting(paths: &SettingsPaths, key: &str) -> Result<String, AppError> {
    if let Some(value) = effective_value(paths, VAMIFY_DATABASE, "keyboard", key)? {
        return Ok(value);
    }
    setting_from_default("keyboard", key)
        .map(|(value, _)| value.to_string())
        .ok_or_else(|| AppError::Message(format!("keyboard setting 'keyboard.{key}' is missing")))
}

fn write_keyboard_setting(paths: &SettingsPaths, key: &str, value: &str) -> Result<(), AppError> {
    if running_as_root() {
        write_system_setting(paths, VAMIFY_DATABASE, "keyboard", key, value)?;
    } else {
        write_user_setting(paths, VAMIFY_DATABASE, "keyboard", key, value)?;
    }
    Ok(())
}

fn parse_layouts(value: &str) -> Result<Vec<String>, AppError> {
    let mut layouts = Vec::new();
    for item in value.split(',') {
        let layout = validate_layout(item)?;
        if !layouts.contains(&layout) {
            layouts.push(layout);
        }
    }
    if layouts.is_empty() {
        return Err(AppError::Message(
            "keyboard.layouts must contain at least one layout".to_string(),
        ));
    }
    Ok(layouts)
}

fn validate_layout(value: &str) -> Result<String, AppError> {
    let layout = value.trim();
    if layout.is_empty()
        || !layout
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_+-".contains(character))
    {
        return Err(AppError::Message(format!(
            "invalid keyboard layout '{value}'; use an XKB layout name such as us or fr"
        )));
    }
    Ok(layout.to_string())
}

fn print_keyboard_change(
    action: &str,
    config: &KeyboardConfig,
    apply_result: Option<&KeyboardApplyResult>,
    message: Option<String>,
    json_output: bool,
) {
    let action_message = message.unwrap_or_default();
    if json_output {
        print_json(json!({
            "ok": true,
            "action": action,
            "layout": config.layout,
            "layouts": config.layouts,
            "backend": apply_result.map(|result| result.backend.as_str()),
            "applied": apply_result.map(|result| result.applied),
            "message": if action_message.is_empty() {
                apply_result.map(|result| result.message.as_str()).unwrap_or("")
            } else {
                action_message.as_str()
            },
        }));
    } else {
        if !action_message.is_empty() {
            println!("{action_message}");
        }
        println!("active keyboard layout: {}", config.layout);
        if let Some(result) = apply_result {
            let marker = if result.applied {
                paint("[ OK ]", GREEN, true)
            } else {
                paint("[WARN]", YELLOW, true)
            };
            println!("{marker} {}", result.message);
        }
    }
}

fn apply_keyboard_layout(
    config: &KeyboardConfig,
    previous_layout: Option<&str>,
) -> KeyboardApplyResult {
    let backend = keyboard_backend().to_string();
    if !is_wayland_session() {
        return KeyboardApplyResult {
            backend,
            applied: false,
            message:
                "saved to settings; no Wayland session is active, so the layout was not applied"
                    .to_string(),
        };
    }

    let result = match backend.as_str() {
        "sway" => apply_sway_layout(config),
        "hyprland" => apply_hyprland_layout(config),
        "gnome" => apply_gnome_layout(config),
        "labwc" => apply_labwc_layout(config, previous_layout),
        _ => Err(
            "no supported Wayland compositor adapter was detected; the layout was saved to settings"
                .to_string(),
        ),
    };

    match result {
        Ok(()) => KeyboardApplyResult {
            backend,
            applied: true,
            message: format!("applied {} through the Wayland compositor", config.layout),
        },
        Err(error) => KeyboardApplyResult {
            backend: backend.clone(),
            applied: false,
            message: if backend == "labwc" {
                error
            } else {
                format!("saved to settings, but the compositor rejected the layout: {error}")
            },
        },
    }
}

fn apply_sway_layout(config: &KeyboardConfig) -> Result<(), String> {
    let mut commands = vec![vec![
        "input".to_string(),
        "type:keyboard".to_string(),
        "xkb_layout".to_string(),
        config.layouts.join(","),
    ]];
    if !config.variant.is_empty() {
        commands.push(vec![
            "input".to_string(),
            "type:keyboard".to_string(),
            "xkb_variant".to_string(),
            config.variant.clone(),
        ]);
    }
    if !config.options.is_empty() {
        commands.push(vec![
            "input".to_string(),
            "type:keyboard".to_string(),
            "xkb_options".to_string(),
            config.options.clone(),
        ]);
    }
    commands.push(vec![
        "input".to_string(),
        "type:keyboard".to_string(),
        "xkb_model".to_string(),
        config.model.clone(),
    ]);
    let active_index = config
        .layouts
        .iter()
        .position(|layout| layout == &config.layout)
        .unwrap_or(0);
    commands.push(vec![
        "input".to_string(),
        "type:keyboard".to_string(),
        "xkb_switch_layout".to_string(),
        active_index.to_string(),
    ]);
    for arguments in commands {
        run_keyboard_command("swaymsg", &arguments)?;
    }
    Ok(())
}

fn apply_hyprland_layout(config: &KeyboardConfig) -> Result<(), String> {
    let commands = [
        ("input:kb_layout", config.layouts.join(",")),
        ("input:kb_variant", config.variant.clone()),
        ("input:kb_options", config.options.clone()),
        ("input:kb_model", config.model.clone()),
    ];
    for (key, value) in commands {
        run_keyboard_command("hyprctl", &["keyword".to_string(), key.to_string(), value])?;
    }
    let active_index = config
        .layouts
        .iter()
        .position(|layout| layout == &config.layout)
        .unwrap_or(0);
    run_keyboard_command(
        "hyprctl",
        &[
            "switchxkblayout".to_string(),
            "all".to_string(),
            active_index.to_string(),
        ],
    )?;
    Ok(())
}

fn apply_gnome_layout(config: &KeyboardConfig) -> Result<(), String> {
    let sources = config
        .layouts
        .iter()
        .map(|layout| {
            if config.variant.is_empty() {
                format!("('xkb', '{layout}')")
            } else {
                format!("('xkb', '{layout}+{}')", config.variant)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    run_keyboard_command(
        "gsettings",
        &[
            "set".to_string(),
            "org.gnome.desktop.input-sources".to_string(),
            "sources".to_string(),
            format!("[{sources}]"),
        ],
    )?;
    let current_index = config
        .layouts
        .iter()
        .position(|layout| layout == &config.layout)
        .unwrap_or(0);
    run_keyboard_command(
        "gsettings",
        &[
            "set".to_string(),
            "org.gnome.desktop.input-sources".to_string(),
            "current".to_string(),
            current_index.to_string(),
        ],
    )?;
    Ok(())
}

fn apply_labwc_layout(
    config: &KeyboardConfig,
    previous_layout: Option<&str>,
) -> Result<(), String> {
    let config_directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|home| home.join(".config"))
        })
        .ok_or_else(|| "cannot determine the user config directory".to_string())?;
    let labwc_directory = config_directory.join("labwc");
    let environment_path = labwc_directory.join("environment");

    fs::create_dir_all(&labwc_directory)
        .map_err(|error| format!("could not create {}: {error}", labwc_directory.display()))?;
    let existing = match fs::read_to_string(&environment_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(format!(
                "could not read {}: {error}",
                environment_path.display()
            ))
        }
    };
    let existing_layouts = labwc_environment_value(&existing, "XKB_DEFAULT_LAYOUT")
        .map(|value| value.split(',').map(str::trim).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut ordered_layouts = vec![config.layout.clone()];
    ordered_layouts.extend(
        config
            .layouts
            .iter()
            .filter(|layout| *layout != &config.layout)
            .cloned(),
    );
    let options = if config.options.is_empty() && ordered_layouts.len() > 1 {
        "grp:alt_shift_toggle".to_string()
    } else {
        config.options.clone()
    };
    let mut managed_lines = vec![
        format!("XKB_DEFAULT_LAYOUT={}", ordered_layouts.join(",")),
        format!("XKB_DEFAULT_VARIANT={}", config.variant),
        format!("XKB_DEFAULT_OPTIONS={options}"),
        format!("XKB_DEFAULT_MODEL={}", config.model),
    ];

    let mut preserved = existing
        .lines()
        .filter(|line| {
            ![
                "XKB_DEFAULT_LAYOUT",
                "XKB_DEFAULT_VARIANT",
                "XKB_DEFAULT_OPTIONS",
                "XKB_DEFAULT_MODEL",
            ]
            .iter()
            .any(|key| labwc_environment_key(line) == Some(*key))
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !preserved.is_empty() && preserved.last().is_some_and(|line| !line.is_empty()) {
        preserved.push(String::new());
    }
    preserved.append(&mut managed_lines);
    let mut contents = preserved.join("\n");
    contents.push('\n');
    fs::write(&environment_path, contents)
        .map_err(|error| format!("could not write {}: {error}", environment_path.display()))?;

    let reconfigure = ProcessCommand::new("labwc")
        .arg("--reconfigure")
        .output()
        .map_err(|error| {
            format!("settings saved, but could not run labwc --reconfigure: {error}")
        })?;
    if !reconfigure.status.success() {
        let detail = String::from_utf8_lossy(&reconfigure.stderr)
            .trim()
            .to_string();
        return Err(if detail.is_empty() {
            format!(
                "settings saved to {}; labwc --reconfigure failed; restart labwc to load them",
                environment_path.display()
            )
        } else {
            format!(
                "settings saved to {}; labwc --reconfigure failed: {detail}; restart labwc to load them",
                environment_path.display()
            )
        });
    }

    let can_switch_loaded_layout = previous_layout.is_some_and(|previous| {
        previous != config.layout
            && existing_layouts
                .iter()
                .any(|layout| *layout == config.layout)
            && existing_layouts.iter().any(|layout| *layout == previous)
    });
    if can_switch_loaded_layout {
        if let Err(error) = run_keyboard_command(
            "wtype",
            &[
                "-M".to_string(),
                "alt".to_string(),
                "-P".to_string(),
                "shift".to_string(),
                "-p".to_string(),
                "shift".to_string(),
                "-m".to_string(),
                "alt".to_string(),
            ],
        ) {
            return Err(format!(
                "saved {}; could not switch the already-loaded layout with wtype: {error}; restart labwc to load the selected layout",
                environment_path.display()
            ));
        }
        return Ok(());
    }

    Err(format!(
        "saved {} and reconfigured labwc, but labwc must be restarted once to load the new XKB map; after that use Alt+Shift to cycle layouts",
        environment_path.display()
    ))
}

fn labwc_environment_key(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (key, _) = line.split_once('=')?;
    match key {
        "XKB_DEFAULT_LAYOUT"
        | "XKB_DEFAULT_VARIANT"
        | "XKB_DEFAULT_OPTIONS"
        | "XKB_DEFAULT_MODEL" => Some(key),
        _ => None,
    }
}

fn labwc_environment_value<'a>(contents: &'a str, wanted_key: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        if labwc_environment_key(line) == Some(wanted_key) {
            line.trim_start()
                .strip_prefix("export ")
                .unwrap_or(line.trim_start())
                .split_once('=')
                .map(|(_, value)| value.trim())
        } else {
            None
        }
    })
}

fn run_keyboard_command(program: &str, arguments: &[String]) -> Result<(), String> {
    let output: Output = ProcessCommand::new(program)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if detail.is_empty() {
        Err(format!("{program} exited with {}", output.status))
    } else {
        Err(detail)
    }
}

fn is_wayland_session() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|value| value.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

fn keyboard_backend() -> &'static str {
    let desktop = [
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        std::env::var("XDG_SESSION_DESKTOP").unwrap_or_default(),
        std::env::var("DESKTOP_SESSION").unwrap_or_default(),
    ]
    .join(":")
    .to_ascii_lowercase();

    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() || desktop.contains("hyprland") {
        "hyprland"
    } else if std::env::var_os("SWAYSOCK").is_some() || desktop.contains("sway") {
        "sway"
    } else if desktop.contains("gnome") {
        "gnome"
    } else if desktop.contains("labwc") || process_running("labwc") {
        "labwc"
    } else {
        "unsupported"
    }
}

fn process_running(name: &str) -> bool {
    ProcessCommand::new("pgrep")
        .args(["-x", name])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

struct DeviceRow {
    key: &'static str,
    text: String,
    json: Value,
    source: String,
}

fn unknown_if_none<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "unknown".to_string(), |value| value.to_string())
}

/// One list, used by `device status`, `device get` and `--json`, so they can never disagree.
fn device_rows(info: &device::DeviceInfo) -> Vec<DeviceRow> {
    let row = |key: &'static str, text: String, json: Value, source: &str| DeviceRow {
        key,
        text,
        json,
        source: source.to_string(),
    };
    let hint = |overridden: bool| if overridden { "device.vmf" } else { "tier" };
    let tier_source = match info.tier_source {
        device::Source::Detected => "estimated",
        other => other.as_str(),
    };
    let overrides = &info.overrides;
    let adapt = &info.adapt;
    let posture_source = if info.form_factor.is_foldable() {
        "hinge sensor"
    } else {
        "-"
    };

    vec![
        row(
            "form_factor",
            info.form_factor.as_str().to_string(),
            json!(info.form_factor.as_str()),
            info.form_factor_source.as_str(),
        ),
        row(
            "posture",
            info.posture.map_or_else(
                || {
                    if info.form_factor.is_foldable() {
                        "unknown"
                    } else {
                        "none"
                    }
                    .to_string()
                },
                |posture| posture.as_str().to_string(),
            ),
            json!(info.posture.map(|posture| posture.as_str())),
            posture_source,
        ),
        row(
            "diagonal_in",
            unknown_if_none(info.diagonal_in.map(|d| format!("{d:.1}"))),
            json!(info.diagonal_in.map(|d| (d * 10.0).round() / 10.0)),
            "display",
        ),
        row(
            "tier",
            info.tier.as_str().to_string(),
            json!(info.tier.as_str()),
            tier_source,
        ),
        row(
            "animations",
            adapt.animations.as_str().to_string(),
            json!(adapt.animations.as_str()),
            hint(overrides.animations.is_some()),
        ),
        row(
            "blur",
            adapt.blur.to_string(),
            json!(adapt.blur),
            hint(overrides.blur.is_some()),
        ),
        row(
            "target_fps",
            adapt.target_fps.to_string(),
            json!(adapt.target_fps),
            hint(overrides.target_fps.is_some()),
        ),
        row(
            "background_processes",
            adapt.background_processes.to_string(),
            json!(adapt.background_processes),
            hint(overrides.background_processes.is_some()),
        ),
        row(
            "preload",
            adapt.preload.to_string(),
            json!(adapt.preload),
            hint(overrides.preload.is_some()),
        ),
        row(
            "image_cache_mb",
            adapt.image_cache_mb.to_string(),
            json!(adapt.image_cache_mb),
            hint(overrides.image_cache_mb.is_some()),
        ),
        row(
            "ram_mb",
            unknown_if_none(info.ram_mb),
            json!(info.ram_mb),
            "detected",
        ),
        row(
            "cpu_cores",
            unknown_if_none(info.cpu_cores),
            json!(info.cpu_cores),
            "detected",
        ),
        row(
            "max_cpu_mhz",
            unknown_if_none(info.max_cpu_mhz),
            json!(info.max_cpu_mhz),
            "detected",
        ),
        row(
            "touch",
            info.touch.to_string(),
            json!(info.touch),
            "detected",
        ),
        row(
            "keyboard",
            info.keyboard.to_string(),
            json!(info.keyboard),
            "detected",
        ),
        row(
            "pointer",
            info.pointer.to_string(),
            json!(info.pointer),
            "detected",
        ),
        row(
            "battery",
            info.battery.to_string(),
            json!(info.battery),
            "detected",
        ),
    ]
}

/// Asks, in a terminal only, whether to run the setup wizard. `Ok(true)` when a device.vmf was written.
fn offer_device_setup(
    release_root: &Path,
    probe_root: &Path,
    headline: Vec<String>,
) -> Result<bool, AppError> {
    let path = config::device_file_path(release_root);
    let outcome = {
        let mut terminal = device_setup::enter_terminal()?;
        let items = vec![
            tui::Item::new(
                "Run the setup wizard",
                format!("a few questions, saved to {}", path.display()),
            ),
            tui::Item::new("Not now", "keep auto-detecting this device"),
        ];
        let screen = tui::Screen {
            title: "Device profile",
            hint: "↑↓  Navigate     Enter  Select     Esc  Not now",
            info: &headline,
            filterable: false,
        };
        match tui::select(&mut terminal, &screen, &items, 0) {
            Some(0) => {
                device_setup::run_with(&mut terminal, release_root, probe_root, headline.clone())?
            }
            _ => device_setup::Outcome::Cancelled,
        }
    }; // the terminal is restored here, before anything is printed
    match outcome {
        device_setup::Outcome::Saved(saved) => {
            println!(
                "{}",
                paint(&format!("saved {}", saved.display()), GREEN, true)
            );
            Ok(true)
        }
        device_setup::Outcome::Cancelled => Ok(false),
    }
}

fn run_device(command: DeviceCommand, json_output: bool) -> Result<bool, AppError> {
    let release_root = release_root_from_environment();
    let probe_root = probe_root_from_environment();

    if matches!(command, DeviceCommand::Setup) {
        if json_output {
            return Err(AppError::Message(
                "device setup is interactive and cannot produce JSON".to_string(),
            ));
        }
        let mut terminal = device_setup::enter_terminal()?;
        let outcome =
            device_setup::run_with(&mut terminal, &release_root, &probe_root, Vec::new())?;
        drop(terminal);
        match outcome {
            device_setup::Outcome::Saved(path) => {
                println!(
                    "{}",
                    paint(&format!("saved {}", path.display()), GREEN, true)
                );
                return run_device(DeviceCommand::Status, false);
            }
            device_setup::Outcome::Cancelled => {
                println!("cancelled; nothing was written");
                return Ok(true);
            }
        }
    }

    // Apps and scripts never see a prompt: only a person at a terminal asking for `status` does.
    let may_offer_setup =
        tui::is_interactive() && !json_output && matches!(command, DeviceCommand::Status);
    let mut detected = device::detect(&release_root, &probe_root);
    if may_offer_setup {
        let path = config::device_file_path(&release_root);
        let headline = match &detected {
            Ok(info) if info.profile.is_none() => Some(vec![
                format!(
                    "There is no {} yet, so this device is being auto-detected.",
                    path.display()
                ),
                "The device image normally states what the device is; the wizard can write it."
                    .to_string(),
            ]),
            Err(error) => Some(vec![
                format!("{error}"),
                "The wizard can write a fresh, valid file.".to_string(),
            ]),
            _ => None,
        };
        if let Some(headline) = headline {
            if offer_device_setup(&release_root, &probe_root, headline)? {
                detected = device::detect(&release_root, &probe_root);
            }
        }
    }
    let info = detected?;
    let rows = device_rows(&info);
    match command {
        DeviceCommand::Setup => unreachable!("handled above"),
        DeviceCommand::Status => {
            if json_output {
                let properties = rows
                    .iter()
                    .map(|row| {
                        (
                            row.key.to_string(),
                            json!({ "value": row.json, "source": row.source }),
                        )
                    })
                    .collect::<serde_json::Map<String, Value>>();
                print_json(json!({
                    "ok": true,
                    "profile": info.profile,
                    "properties": properties,
                }));
            } else {
                let use_color =
                    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                println!("{}", paint("VamoraSys device", &theme::accent(), use_color));
                let table = rows
                    .iter()
                    .map(|row| vec![row.key.to_string(), row.text.clone(), row.source.clone()])
                    .collect::<Vec<_>>();
                print_table(&["PROPERTY", "VALUE", "SOURCE"], &table);
                match &info.profile {
                    Some(path) => println!(
                        "{}",
                        paint(&format!("profile: {}", path.display()), DIM, use_color)
                    ),
                    None => println!(
                        "{}",
                        paint("profile: none (fully auto-detected)", DIM, use_color)
                    ),
                }
            }
        }
        DeviceCommand::Get { key } => {
            let Some(row) = rows.iter().find(|row| row.key == key) else {
                let known = rows
                    .iter()
                    .map(|row| row.key)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(AppError::Message(format!(
                    "unknown device value '{key}'; use one of: {known}"
                )));
            };
            if json_output {
                print_json(
                    json!({ "ok": true, "key": row.key, "value": row.json, "source": row.source }),
                );
            } else {
                println!("{}", row.text);
            }
        }
    }
    Ok(true)
}

fn run_status(paths: &SettingsPaths, json_output: bool) -> Result<bool, AppError> {
    let mut checks: Vec<(bool, String)> = Vec::new();
    checks.push(check_directory(
        &paths.system_root,
        "VamoraSys system directory",
    ));
    checks.push(check_directory(
        &paths.system_settings,
        "factory settings directory",
    ));
    checks.push(check_database(paths, VAMIFY_DATABASE));
    checks.push(check_database(paths, ACCESSIBILITY_DATABASE));

    let release = match ReleaseMetadata::read(paths) {
        Ok(metadata) => {
            checks.push((
                true,
                format!("release metadata: {}", release_summary(&metadata)),
            ));
            Some(metadata)
        }
        Err(error) => {
            checks.push((false, error.to_string()));
            None
        }
    };

    match device::DeviceProfile::read(&paths.release_root) {
        Ok(Some(profile)) => {
            checks.push((true, format!("device profile: {}", profile.path.display())))
        }
        Ok(None) => checks.push((
            true,
            "device profile: none (auto-detected; run `vamorasys device setup`)".to_string(),
        )),
        Err(error) => checks.push((false, error.to_string())),
    }

    let healthy = checks.iter().all(|(ok, _)| *ok);
    if json_output {
        let check_values = checks
            .iter()
            .map(|(ok, message)| json!({ "ok": ok, "message": message }))
            .collect::<Vec<_>>();
        let release_value = release.as_ref().map(|metadata| {
            json!({
                "path": metadata.path,
                "os_name": metadata.fields.get("AL_NAME"),
                "os_version": metadata.os_version(),
                "vamorasys_version": metadata.vamorasys_version(),
            })
        });
        print_json(json!({
            "ok": healthy,
            "status": if healthy { "healthy" } else { "unhealthy" },
            "checks": check_values,
            "release": release_value,
        }));
    } else {
        println!("{}", paint("VamoraSys status", &theme::accent(), true));
        for (ok, message) in checks {
            let marker = if ok {
                paint("[ OK ]", GREEN, true)
            } else {
                paint("[FAIL]", RED, true)
            };
            println!("{marker} {message}");
        }
        let overall = if healthy {
            paint("overall: healthy", GREEN, true)
        } else {
            paint("overall: unhealthy", RED, true)
        };
        println!("{overall}");
    }
    Ok(healthy)
}

fn check_directory(path: &Path, label: &str) -> (bool, String) {
    if path.is_dir() {
        (true, format!("{label}: {}", path.display()))
    } else {
        (false, format!("{label} is missing: {}", path.display()))
    }
}

fn check_database(paths: &SettingsPaths, database: &str) -> (bool, String) {
    let path = paths.system_database(database);
    if !path.is_file() {
        return (
            false,
            format!("factory database is missing: {}", path.display()),
        );
    }

    let connection = match Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(connection) => connection,
        Err(error) => {
            return (
                false,
                format!("cannot open {} read-only: {error}", path.display()),
            )
        }
    };
    let mut missing = Vec::new();
    for table in required_database_tables(database) {
        let exists: Result<bool, rusqlite::Error> = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get(0),
        );
        match exists {
            Ok(true) => {}
            Ok(false) => missing.push(*table),
            Err(error) => return (false, format!("cannot inspect {}: {error}", path.display())),
        }
    }
    if !missing.is_empty() {
        return (
            false,
            format!(
                "{} is missing required tables: {}",
                path.display(),
                missing.join(", ")
            ),
        );
    }

    for table in required_database_tables(database) {
        let sql = format!(
            "SELECT setting_key, value, value_type FROM \"{}\" LIMIT 1",
            table.replace('"', "\"\"")
        );
        if let Err(error) = connection.prepare(&sql) {
            return (
                false,
                format!("table {table} in {} is unusable: {error}", path.display()),
            );
        }
    }
    (
        true,
        format!("factory database {database}: readable, required tables present"),
    )
}

fn availability(path: &Path) -> &'static str {
    if path.is_file() {
        "available"
    } else {
        "missing"
    }
}

fn current_user_name() -> String {
    std::env::var("SUDO_USER")
        .ok()
        .filter(|name| !name.is_empty() && name != "root")
        .or_else(|| std::env::var("USER").ok().filter(|name| !name.is_empty()))
        .or_else(|| {
            std::env::var("LOGNAME")
                .ok()
                .filter(|name| !name.is_empty())
        })
        .or_else(passwd_user_name)
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(target_os = "linux")]
fn passwd_user_name() -> Option<String> {
    let passwd = unsafe { libc::getpwuid(libc::geteuid()) };
    if passwd.is_null() {
        return None;
    }
    unsafe {
        CStr::from_ptr((*passwd).pw_name)
            .to_str()
            .ok()
            .map(str::to_string)
    }
}

#[cfg(not(target_os = "linux"))]
fn passwd_user_name() -> Option<String> {
    None
}

fn find_setting(
    path: &Path,
    table: &str,
    key: &str,
) -> Result<Option<database::Setting>, AppError> {
    let connection = database::open_read_only(path)?;
    if !table_exists(&connection, table)? {
        return Ok(None);
    }
    let sql = format!(
        "SELECT setting_key, value, value_type FROM \"{}\" WHERE setting_key = ?1",
        table.replace('"', "\"\"")
    );
    let result = connection
        .query_row(&sql, [key], |row| {
            Ok(database::Setting {
                table: table.to_string(),
                key: row.get(0)?,
                value: row.get(1)?,
                value_type: row.get(2)?,
            })
        })
        .optional()?;
    Ok(result)
}

fn effective_value(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
) -> Result<Option<String>, AppError> {
    let user_path = paths.user_database(database);
    if user_path.is_file() {
        if let Some(setting) = find_setting(&user_path, table, key)? {
            return Ok(Some(setting.value));
        }
    }

    let system_path = paths.system_database(database);
    if system_path.is_file() {
        if let Some(setting) = find_setting(&system_path, table, key)? {
            return Ok(Some(setting.value));
        }
    }
    Ok(None)
}

fn setting_json(database: &str, setting: &database::Setting, source: &str) -> Value {
    let mut value = setting_json_value(setting);
    if let Some(object) = value.as_object_mut() {
        object.insert("database".to_string(), json!(database));
        object.insert("source".to_string(), json!(source));
    }
    value
}

fn setting_json_value(setting: &database::Setting) -> Value {
    json!({
        "table": setting.table,
        "key": setting.key,
        "value": setting.value,
        "value_type": setting.value_type,
    })
}

fn print_settings(settings: &[database::Setting]) {
    if settings.is_empty() {
        println!("{}", paint("(none)", DIM, true));
        return;
    }
    let rows = settings
        .iter()
        .map(|setting| {
            vec![
                format!("{}.{}", setting.table, setting.key),
                setting.value.clone(),
                setting.value_type.clone(),
            ]
        })
        .collect::<Vec<_>>();
    print_table(&["SETTING", "VALUE", "TYPE"], &rows);
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let use_color = io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let widths = (0..headers.len())
        .map(|index| {
            rows.iter()
                .map(|row| row.get(index).map_or(0, |cell| cell.chars().count()))
                .chain(std::iter::once(headers[index].chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let separator = widths
        .iter()
        .map(|width| "─".repeat(*width + 2))
        .collect::<Vec<_>>()
        .join("┼");
    let header = headers
        .iter()
        .enumerate()
        .map(|(index, value)| format!(" {:width$} ", value, width = widths[index]))
        .collect::<Vec<_>>()
        .join("│");
    println!(
        "{}",
        paint(&header, &format!("{BOLD}{}", theme::accent()), use_color)
    );
    println!("{}", paint(&separator, DIM, use_color));
    for (row_index, row) in rows.iter().enumerate() {
        let line = headers
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let value = row.get(index).map_or("", String::as_str);
                format!(" {:width$} ", value, width = widths[index])
            })
            .collect::<Vec<_>>()
            .join("│");
        let color = match row.get(1).map(String::as_str) {
            Some("available") | Some("registered") | Some("core") => GREEN,
            Some("missing") => YELLOW,
            _ if row_index % 2 == 1 => DIM,
            _ => RESET,
        };
        println!("{}", paint(&line, color, use_color && color != RESET));
    }
}

fn paint(text: &str, color: &str, enabled: bool) -> String {
    let support = theme::support();
    if enabled && support != theme::ColorSupport::None {
        format!("{}{text}{RESET}", theme::adapt(color, support))
    } else {
        text.to_string()
    }
}

fn print_json(value: Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{\"ok\":false}".to_string())
    );
}

#[cfg(target_os = "linux")]
fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(target_os = "linux"))]
fn running_as_root() -> bool {
    false
}

trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}

impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_paths(directory: &Path) -> SettingsPaths {
        SettingsPaths::for_directories(
            directory.join("etc/VamoraSys/settings"),
            directory.join("home/.VamoraSys/settings"),
            directory.join("etc/VamoraSys"),
        )
    }

    #[test]
    fn user_override_resolves_then_reset_falls_back() {
        let directory = tempdir().unwrap();
        let paths = test_paths(directory.path());
        fs::create_dir_all(&paths.release_root).unwrap();
        initialize_database(
            &paths.system_database(VAMIFY_DATABASE),
            VAMIFY_DATABASE,
            true,
        )
        .unwrap();
        initialize_database(
            &paths.user_database(VAMIFY_DATABASE),
            VAMIFY_DATABASE,
            false,
        )
        .unwrap();

        write_user_setting(&paths, VAMIFY_DATABASE, "appearance", "theme", "dark").unwrap();
        assert_eq!(
            find_setting(&paths.user_database(VAMIFY_DATABASE), "appearance", "theme")
                .unwrap()
                .unwrap()
                .value,
            "dark"
        );
        delete_user_setting(&paths, VAMIFY_DATABASE, "appearance", "theme").unwrap();
        assert_eq!(
            effective_value(&paths, VAMIFY_DATABASE, "appearance", "theme")
                .unwrap()
                .as_deref(),
            Some("light")
        );
    }

    #[test]
    fn system_write_changes_default_and_system_reset_restores_it() {
        let directory = tempdir().unwrap();
        let paths = test_paths(directory.path());
        initialize_database(
            &paths.system_database(VAMIFY_DATABASE),
            VAMIFY_DATABASE,
            true,
        )
        .unwrap();

        write_system_setting(&paths, VAMIFY_DATABASE, "appearance", "theme", "dark").unwrap();
        assert_eq!(
            effective_value(&paths, VAMIFY_DATABASE, "appearance", "theme")
                .unwrap()
                .as_deref(),
            Some("dark")
        );

        assert!(reset_system_setting(&paths, VAMIFY_DATABASE, "appearance", "theme").unwrap());
        assert_eq!(
            effective_value(&paths, VAMIFY_DATABASE, "appearance", "theme")
                .unwrap()
                .as_deref(),
            Some("light")
        );
    }

    #[test]
    fn app_database_is_discovered_and_has_user_layer() {
        let directory = tempdir().unwrap();
        let paths = test_paths(directory.path());
        write_user_setting(
            &paths,
            "apps/calendar.db",
            "calendar",
            "week_start",
            "monday",
        )
        .unwrap();
        assert!(discover_databases(&paths)
            .unwrap()
            .contains("apps/calendar.db"));
        assert_eq!(
            effective_value(&paths, "apps/calendar.db", "calendar", "week_start")
                .unwrap()
                .as_deref(),
            Some("monday")
        );
    }

    #[test]
    fn keyboard_layouts_are_validated_and_deduplicated() {
        assert_eq!(
            parse_layouts("us, fr, us").unwrap(),
            vec!["us".to_string(), "fr".to_string()]
        );
        assert!(parse_layouts("us,").is_err());
        assert!(validate_layout("us;rm").is_err());
    }

    #[test]
    fn device_get_and_status_share_one_list_of_values() {
        let release = tempdir().unwrap();
        let probe = tempdir().unwrap();
        fs::write(
            release.path().join("device.vmf"),
            "VMF_VERSION=\"1\"\nFORM_FACTOR=\"tablet\"\nTIER=\"midrange\"\nBLUR=off\n",
        )
        .unwrap();
        let info = device::detect(release.path(), probe.path()).unwrap();
        let rows = device_rows(&info);
        let find = |key: &str| rows.iter().find(|row| row.key == key).unwrap();
        assert_eq!(find("form_factor").text, "tablet");
        assert_eq!(find("form_factor").source, "device.vmf");
        assert_eq!(find("tier").text, "midrange");
        assert_eq!(find("blur").text, "false");
        assert_eq!(find("blur").source, "device.vmf"); // overridden hint
        assert_eq!(find("animations").source, "tier"); // table default
        assert_eq!(find("posture").text, "none"); // not a foldable
        assert_eq!(find("ram_mb").text, "unknown"); // empty fake system
    }

    #[test]
    fn device_works_without_a_home_directory_or_settings_databases() {
        // `run_device` must not need SettingsPaths at all.
        let release = tempdir().unwrap();
        let probe = tempdir().unwrap();
        assert!(device::detect(release.path(), probe.path()).is_ok());
        assert!(!release.path().join("settings").exists());
    }

    #[test]
    fn status_reports_a_broken_device_vmf_as_unhealthy() {
        let directory = tempdir().unwrap();
        let paths = test_paths(directory.path());
        fs::create_dir_all(&paths.release_root).unwrap();
        fs::write(
            paths.release_root.join("device.vmf"),
            "VMF_VERSION=1\nTIER=ultra\n",
        )
        .unwrap();
        assert!(device::DeviceProfile::read(&paths.release_root).is_err());
    }

    #[test]
    fn xkb_layout_list_is_parsed_from_the_layout_section_only() {
        let text = "! model\n  pc105  Generic 105-key PC\n\n! layout\n  us              English (US)\n  fr              French\n\n! variant\n  intl     us: English (US, intl.)\n";
        assert_eq!(
            parse_xkb_layouts(text),
            vec![
                ("us".to_string(), "English (US)".to_string()),
                ("fr".to_string(), "French".to_string())
            ]
        );
    }

    #[test]
    fn layout_picker_falls_back_to_common_layouts_without_xkb_data() {
        let empty = tempdir().unwrap();
        let layouts = xkb_layouts(empty.path());
        assert!(layouts.iter().any(|(code, _)| code == "us"));
        assert!(layouts.iter().any(|(code, _)| code == "fr"));
        assert_eq!(layouts.len(), COMMON_LAYOUTS.len());
        // and reads the real list when it is there
        let rules = empty.path().join("usr/share/X11/xkb/rules");
        fs::create_dir_all(&rules).unwrap();
        fs::write(rules.join("base.lst"), "! layout\n  zz   Test Layout\n").unwrap();
        assert_eq!(
            xkb_layouts(empty.path()),
            vec![("zz".to_string(), "Test Layout".to_string())]
        );
    }
}
