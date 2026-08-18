mod config;
mod database;
mod release;

use clap::{Args, Parser, Subcommand};
use config::{normalize_database_name, split_setting_key, SettingsPaths};
use database::{
    database_for_table, delete_user_setting, discover_databases, effective_settings,
    initialize_database, is_core_database, layer_settings, list_tables, required_database_tables,
    reset_system_setting, sync_user_defaults, table_exists, write_system_setting,
    write_user_setting, ACCESSIBILITY_DATABASE, CORE_DATABASES, VAMIFY_DATABASE,
};
use release::{release_summary, ReleaseMetadata};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::ffi::CStr;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::ExitCode;
use thiserror::Error;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BLUE: &str = "\x1b[38;2;37;99;235m";
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
    let paths = SettingsPaths::from_environment().map_err(AppError::Message)?;
    if !running_as_root() {
        sync_user_defaults(&paths)?;
    }

    match cli.command {
        Command::Status => run_status(&paths, cli.json),
        Command::Settings { command } => {
            run_settings(&paths, command, cli.json)?;
            Ok(true)
        }
        Command::Initialize(args) => {
            run_initialize(&paths, args, cli.json)?;
            Ok(true)
        }
    }
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
        println!("{}", paint(&database, BLUE, true));
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
        println!("{} {}", paint("database:", BLUE, true), database);
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
        println!("{}", paint("VamoraSys status", BLUE, true));
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
    println!("{}", paint(&header, &format!("{BOLD}{BLUE}"), use_color));
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
    if enabled {
        format!("{color}{text}{RESET}")
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
}
