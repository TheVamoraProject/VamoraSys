use crate::config::SettingsPaths;
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

pub const VAMIFY_DATABASE: &str = "vamify.db";
pub const ACCESSIBILITY_DATABASE: &str = "accessibility.db";
pub const CORE_DATABASES: [&str; 2] = [VAMIFY_DATABASE, ACCESSIBILITY_DATABASE];
pub const VAMIFY_TABLES: [&str; 9] = [
    "appearance",
    "icons",
    "homescreen",
    "lockscreen",
    "wm",
    "bootloader",
    "statusbar",
    "dock",
    "keyboard",
];
pub const ACCESSIBILITY_TABLES: [&str; 1] = ["accessibility"];

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid setting: {0}")]
    InvalidSetting(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    pub table: String,
    pub key: String,
    pub value: String,
    pub value_type: String,
}

#[derive(Clone, Copy)]
struct DefaultSetting {
    table: &'static str,
    key: &'static str,
    value: &'static str,
    value_type: &'static str,
}

const VAMIFY_DEFAULTS: [DefaultSetting; 39] = [
    DefaultSetting {
        table: "appearance",
        key: "theme",
        value: "light",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "accent_color",
        value: "#2563eb",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "primary_color",
        value: "#2563eb",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "secondary_color",
        value: "#0ea5e9",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "surface_color",
        value: "#eff6ff",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "text_color",
        value: "#0f172a",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "success_color",
        value: "#16a34a",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "warning_color",
        value: "#f59e0b",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "error_color",
        value: "#dc2626",
        value_type: "string",
    },
    DefaultSetting {
        table: "appearance",
        key: "font_size",
        value: "16",
        value_type: "integer",
    },
    DefaultSetting {
        table: "appearance",
        key: "icon_theme",
        value: "default",
        value_type: "string",
    },
    DefaultSetting {
        table: "icons",
        key: "corner_radius",
        value: "8",
        value_type: "integer",
    },
    DefaultSetting {
        table: "icons",
        key: "style",
        value: "normal",
        value_type: "string",
    },
    DefaultSetting {
        table: "icons",
        key: "bg_color",
        value: "#2563eb",
        value_type: "string",
    },
    DefaultSetting {
        table: "icons",
        key: "stroke",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "icons",
        key: "stroke_color",
        value: "#0f172a",
        value_type: "string",
    },
    DefaultSetting {
        table: "homescreen",
        key: "layout",
        value: "default",
        value_type: "string",
    },
    DefaultSetting {
        table: "homescreen",
        key: "grid",
        value: "4x6",
        value_type: "string",
    },
    DefaultSetting {
        table: "homescreen",
        key: "widgets",
        value: "clock",
        value_type: "string",
    },
    DefaultSetting {
        table: "homescreen",
        key: "wallpaper",
        value: "default",
        value_type: "string",
    },
    DefaultSetting {
        table: "homescreen",
        key: "icon_size",
        value: "48",
        value_type: "integer",
    },
    DefaultSetting {
        table: "lockscreen",
        key: "clock_style",
        value: "digital",
        value_type: "string",
    },
    DefaultSetting {
        table: "lockscreen",
        key: "show_date",
        value: "true",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "lockscreen",
        key: "show_notifications",
        value: "true",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "lockscreen",
        key: "wallpaper",
        value: "default",
        value_type: "string",
    },
    DefaultSetting {
        table: "wm",
        key: "animations",
        value: "true",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "wm",
        key: "transparency",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "wm",
        key: "window_effects",
        value: "true",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "wm",
        key: "focus_mode",
        value: "click",
        value_type: "string",
    },
    DefaultSetting {
        table: "bootloader",
        key: "timeout",
        value: "5",
        value_type: "integer",
    },
    DefaultSetting {
        table: "bootloader",
        key: "default_entry",
        value: "vamora",
        value_type: "string",
    },
    DefaultSetting {
        table: "statusbar",
        key: "size",
        value: "30",
        value_type: "integer",
    },
    DefaultSetting {
        table: "statusbar",
        key: "transparency",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "dock",
        key: "position",
        value: "bottom",
        value_type: "string",
    },
    DefaultSetting {
        table: "keyboard",
        key: "layout",
        value: "us",
        value_type: "string",
    },
    DefaultSetting {
        table: "keyboard",
        key: "layouts",
        value: "us",
        value_type: "string",
    },
    DefaultSetting {
        table: "keyboard",
        key: "variant",
        value: "",
        value_type: "string",
    },
    DefaultSetting {
        table: "keyboard",
        key: "options",
        value: "",
        value_type: "string",
    },
    DefaultSetting {
        table: "keyboard",
        key: "model",
        value: "pc105",
        value_type: "string",
    },
];

const ACCESSIBILITY_DEFAULTS: [DefaultSetting; 5] = [
    DefaultSetting {
        table: "accessibility",
        key: "high_contrast",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "accessibility",
        key: "large_text",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "accessibility",
        key: "cursor_size",
        value: "1",
        value_type: "integer",
    },
    DefaultSetting {
        table: "accessibility",
        key: "reduce_motion",
        value: "false",
        value_type: "boolean",
    },
    DefaultSetting {
        table: "accessibility",
        key: "screen_reader",
        value: "false",
        value_type: "boolean",
    },
];

pub fn is_core_database(name: &str) -> bool {
    CORE_DATABASES.contains(&name)
}

pub fn tables_for_database(name: &str) -> &'static [&'static str] {
    match name {
        VAMIFY_DATABASE => &VAMIFY_TABLES,
        ACCESSIBILITY_DATABASE => &ACCESSIBILITY_TABLES,
        _ => &[],
    }
}

pub fn database_for_table(table: &str) -> Option<&'static str> {
    if VAMIFY_TABLES.contains(&table) {
        Some(VAMIFY_DATABASE)
    } else if ACCESSIBILITY_TABLES.contains(&table) {
        Some(ACCESSIBILITY_DATABASE)
    } else {
        None
    }
}

pub fn initialize_database(
    path: &Path,
    database: &str,
    seed_defaults: bool,
) -> Result<(), DatabaseError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    ensure_schema(&connection, database)?;

    if seed_defaults {
        let defaults = defaults_for_database(database);
        for setting in defaults {
            let sql = format!(
                "INSERT OR IGNORE INTO {} (setting_key, value, value_type) VALUES (?1, ?2, ?3)",
                quote_identifier(setting.table)
            );
            connection.execute(&sql, (&setting.key, &setting.value, &setting.value_type))?;
        }
    }

    Ok(())
}

pub fn ensure_schema(connection: &Connection, database: &str) -> Result<(), DatabaseError> {
    for table in tables_for_database(database) {
        ensure_setting_table(connection, table)?;
    }
    Ok(())
}

pub fn ensure_setting_table(connection: &Connection, table: &str) -> Result<(), DatabaseError> {
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (
            setting_key TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL,
            value_type TEXT NOT NULL CHECK(value_type IN ('string', 'integer', 'boolean', 'real'))
        )",
        quote_identifier(table)
    );
    connection.execute_batch(&sql)?;
    Ok(())
}

pub fn open_read_only(path: &Path) -> Result<Connection, DatabaseError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?)
}

pub fn list_tables(path: &Path) -> Result<Vec<String>, DatabaseError> {
    let connection = open_read_only(path)?;
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut tables = Vec::new();
    for row in rows {
        tables.push(row?);
    }
    Ok(tables)
}

pub fn read_settings(path: &Path, tables: &[String]) -> Result<Vec<Setting>, DatabaseError> {
    let connection = open_read_only(path)?;
    let mut settings = Vec::new();
    for table in tables {
        if !table_exists(&connection, table)? {
            continue;
        }
        let sql = format!(
            "SELECT setting_key, value, value_type FROM {} ORDER BY setting_key",
            quote_identifier(table)
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok(Setting {
                table: table.clone(),
                key: row.get(0)?,
                value: row.get(1)?,
                value_type: row.get(2)?,
            })
        })?;
        for row in rows {
            settings.push(row?);
        }
    }
    Ok(settings)
}

#[cfg(test)]
pub fn read_all_settings(path: &Path) -> Result<Vec<Setting>, DatabaseError> {
    let tables = list_tables(path)?;
    read_settings(path, &tables)
}

pub fn setting_from_default(table: &str, key: &str) -> Option<(&'static str, &'static str)> {
    VAMIFY_DEFAULTS
        .iter()
        .chain(ACCESSIBILITY_DEFAULTS.iter())
        .find(|setting| setting.table == table && setting.key == key)
        .map(|setting| (setting.value, setting.value_type))
}

pub fn validate_setting_value(
    database: &str,
    table: &str,
    key: &str,
    value: &str,
) -> Result<&'static str, DatabaseError> {
    let builtin_setting =
        (database_for_table(table) == Some(database)).then(|| setting_from_default(table, key));
    let Some((_, value_type)) = builtin_setting.flatten() else {
        if is_core_database(database) {
            return Err(DatabaseError::InvalidSetting(format!(
                "{table}.{key} is not a registered setting for {database}"
            )));
        }
        return Ok(infer_value_type(table, key, value));
    };

    let type_error = || {
        Err(DatabaseError::InvalidSetting(format!(
            "{table}.{key} expects a {value_type} value, got '{value}'"
        )))
    };
    match value_type {
        "boolean" if value != "true" && value != "false" => return type_error(),
        "integer" if value.parse::<i64>().is_err() => return type_error(),
        "real" if value.parse::<f64>().is_err() => return type_error(),
        _ => {}
    }

    let invalid = |message: &str| {
        Err(DatabaseError::InvalidSetting(format!(
            "{table}.{key} = '{value}' {message}"
        )))
    };
    match (table, key) {
        ("appearance", "theme") if !matches!(value, "light" | "dark" | "system") => {
            invalid("must be one of: light, dark, system")
        }
        ("appearance", key)
            if matches!(
                key,
                "accent_color"
                    | "primary_color"
                    | "secondary_color"
                    | "surface_color"
                    | "text_color"
                    | "success_color"
                    | "warning_color"
                    | "error_color"
            ) && !is_hex_color(value) =>
        {
            invalid("must be a #RRGGBB or #RRGGBBAA color")
        }
        ("appearance", "font_size") => bounded_integer(value, 8, 72, table, key),
        ("icons", "corner_radius") => bounded_integer(value, 0, 32, table, key),
        ("icons", "style") if !matches!(value, "normal" | "light" | "dark" | "single_color") => {
            invalid("must be one of: normal, light, dark, single_color")
        }
        ("icons", "bg_color" | "stroke_color") if !is_hex_color(value) => {
            invalid("must be a #RRGGBB or #RRGGBBAA color")
        }
        ("homescreen", "grid") if !is_grid_size(value) => {
            invalid("must use the WIDTHxHEIGHT format, for example 4x6")
        }
        ("homescreen", "icon_size") => bounded_integer(value, 16, 256, table, key),
        ("lockscreen", "clock_style") if !matches!(value, "digital" | "analog") => {
            invalid("must be one of: digital, analog")
        }
        ("wm", "focus_mode") if !matches!(value, "click" | "sloppy" | "focus") => {
            invalid("must be one of: click, sloppy, focus")
        }
        ("bootloader", "timeout") => bounded_integer(value, 0, 60, table, key),
        ("statusbar", "size") => bounded_integer(value, 29, 128, table, key),
        ("dock", "position") if !matches!(value, "top" | "bottom" | "left" | "right") => {
            invalid("must be one of: top, bottom, left, right")
        }
        ("keyboard", "layout") if !is_xkb_token(value) => {
            invalid("must be a valid XKB layout name, for example us or fr")
        }
        ("keyboard", "layouts") if !is_xkb_layout_list(value) => {
            invalid("must be a comma-separated list of XKB layout names, for example us,fr")
        }
        ("keyboard", "variant" | "options" | "model") if !is_xkb_value(value) => {
            invalid("contains characters that are not valid in an XKB value")
        }
        ("accessibility", "cursor_size") => bounded_integer(value, 1, 4, table, key),
        _ => Ok(value_type),
    }
}

pub fn infer_value_type(table: &str, key: &str, value: &str) -> &'static str {
    if let Some((_, value_type)) = setting_from_default(table, key) {
        return value_type;
    }
    if value == "true" || value == "false" {
        "boolean"
    } else if value.parse::<i64>().is_ok() {
        "integer"
    } else if value.parse::<f64>().is_ok() {
        "real"
    } else {
        "string"
    }
}

fn bounded_integer(
    value: &str,
    minimum: i64,
    maximum: i64,
    table: &str,
    key: &str,
) -> Result<&'static str, DatabaseError> {
    let Ok(number) = value.parse::<i64>() else {
        return Err(DatabaseError::InvalidSetting(format!(
            "{table}.{key} expects an integer value, got '{value}'"
        )));
    };
    if !(minimum..=maximum).contains(&number) {
        return Err(DatabaseError::InvalidSetting(format!(
            "{table}.{key} must be between {minimum} and {maximum}, got {number}"
        )));
    }
    Ok("integer")
}

fn is_hex_color(value: &str) -> bool {
    matches!(value.len(), 4 | 5 | 7 | 9)
        && value.starts_with('#')
        && value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn is_grid_size(value: &str) -> bool {
    let Some((width, height)) = value.split_once('x') else {
        return false;
    };
    let Ok(width) = width.parse::<u16>() else {
        return false;
    };
    let Ok(height) = height.parse::<u16>() else {
        return false;
    };
    (1..=12).contains(&width) && (1..=12).contains(&height)
}

fn is_xkb_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_+-".contains(character))
}

fn is_xkb_layout_list(value: &str) -> bool {
    !value.is_empty() && value.split(',').all(is_xkb_token)
}

fn is_xkb_value(value: &str) -> bool {
    value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_+:-,()".contains(character))
}

pub fn write_user_setting(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), DatabaseError> {
    let value_type = validate_setting_value(database, table, key, value)?;
    fs::create_dir_all(&paths.user_settings)?;
    let path = paths.user_database(database);
    initialize_database(&path, database, false)?;
    let connection = Connection::open(path)?;
    ensure_setting_table(&connection, table)?;
    let sql = format!(
        "INSERT INTO {} (setting_key, value, value_type) VALUES (?1, ?2, ?3)
         ON CONFLICT(setting_key) DO UPDATE SET value = excluded.value, value_type = excluded.value_type",
        quote_identifier(table)
    );
    connection.execute(&sql, (key, value, value_type))?;
    Ok(())
}

pub fn write_system_setting(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), DatabaseError> {
    let value_type = validate_setting_value(database, table, key, value)?;
    fs::create_dir_all(&paths.system_settings)?;
    let path = paths.system_database(database);
    initialize_database(&path, database, false)?;
    let connection = Connection::open(path)?;
    ensure_setting_table(&connection, table)?;
    let sql = format!(
        "INSERT INTO {} (setting_key, value, value_type) VALUES (?1, ?2, ?3)
         ON CONFLICT(setting_key) DO UPDATE SET value = excluded.value, value_type = excluded.value_type",
        quote_identifier(table)
    );
    connection.execute(&sql, (key, value, value_type))?;
    Ok(())
}

pub fn delete_user_setting(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
) -> Result<bool, DatabaseError> {
    let path = paths.user_database(database);
    if !path.is_file() {
        return Ok(false);
    }
    let connection = Connection::open(path)?;
    if !table_exists(&connection, table)? {
        return Ok(false);
    }
    let sql = format!(
        "DELETE FROM {} WHERE setting_key = ?1",
        quote_identifier(table)
    );
    Ok(connection.execute(&sql, [key])? > 0)
}

pub fn reset_system_setting(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
) -> Result<bool, DatabaseError> {
    let path = paths.system_database(database);
    if !path.is_file() {
        return Ok(false);
    }
    let connection = Connection::open(path)?;
    if !table_exists(&connection, table)? {
        return Ok(false);
    }

    let sql = format!(
        "DELETE FROM {} WHERE setting_key = ?1",
        quote_identifier(table)
    );
    let removed = connection.execute(&sql, [key])? > 0;
    if let Some((default_value, value_type)) = setting_from_default(table, key) {
        let sql = format!(
            "INSERT OR IGNORE INTO {} (setting_key, value, value_type) VALUES (?1, ?2, ?3)",
            quote_identifier(table)
        );
        connection.execute(&sql, (key, default_value, value_type))?;
    }
    Ok(removed)
}

pub fn effective_settings(
    paths: &SettingsPaths,
    database: &str,
) -> Result<Vec<Setting>, DatabaseError> {
    let mut merged = BTreeMap::<String, Setting>::new();
    let tables = tables_for_display(paths, database)?;

    let system_path = paths.system_database(database);
    if system_path.is_file() {
        for setting in read_settings(&system_path, &tables)? {
            merged.insert(format!("{}.{}", setting.table, setting.key), setting);
        }
    }

    let user_path = paths.user_database(database);
    if user_path.is_file() {
        for setting in read_settings(&user_path, &tables)? {
            merged.insert(format!("{}.{}", setting.table, setting.key), setting);
        }
    }

    Ok(merged.into_values().collect())
}

pub fn layer_settings(
    paths: &SettingsPaths,
    database: &str,
) -> Result<(Vec<Setting>, Vec<Setting>), DatabaseError> {
    let tables = tables_for_display(paths, database)?;
    let system = if paths.system_database(database).is_file() {
        read_settings(&paths.system_database(database), &tables)?
    } else {
        Vec::new()
    };
    let user = if paths.user_database(database).is_file() {
        read_settings(&paths.user_database(database), &tables)?
    } else {
        Vec::new()
    };
    Ok((system, user))
}

pub fn discover_databases(paths: &SettingsPaths) -> Result<BTreeSet<String>, DatabaseError> {
    let mut names = BTreeSet::new();
    for root in [&paths.system_settings, &paths.user_settings] {
        collect_databases(root, root, &mut names)?;
    }
    Ok(names)
}

pub fn sync_user_defaults(paths: &SettingsPaths) -> Result<(), DatabaseError> {
    let mut databases = BTreeSet::new();
    databases.extend(CORE_DATABASES.iter().map(|name| (*name).to_string()));
    databases.extend(discover_databases(paths)?);

    for database in databases {
        let system_path = paths.system_database(&database);
        if !system_path.is_file() {
            continue;
        }

        let tables = list_tables(&system_path)?;
        let settings = read_settings(&system_path, &tables)?;
        if settings.is_empty() {
            continue;
        }

        let user_path = paths.user_database(&database);
        initialize_database(&user_path, &database, false)?;
        let connection = Connection::open(user_path)?;
        for setting in settings {
            ensure_setting_table(&connection, &setting.table)?;
            let sql = format!(
                "INSERT OR IGNORE INTO {} (setting_key, value, value_type) VALUES (?1, ?2, ?3)",
                quote_identifier(&setting.table)
            );
            connection.execute(&sql, (&setting.key, &setting.value, &setting.value_type))?;
        }
    }
    Ok(())
}

fn collect_databases(
    root: &Path,
    directory: &Path,
    names: &mut BTreeSet<String>,
) -> Result<(), DatabaseError> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_databases(root, &path, names)?;
        } else if path.is_file() && path.extension().is_some_and(|extension| extension == "db") {
            if let Ok(relative) = path.strip_prefix(root) {
                let name = relative.to_string_lossy().replace('\\', "/");
                if crate::config::normalize_database_name(&name).is_ok() {
                    names.insert(name);
                }
            }
        }
    }
    Ok(())
}

pub fn tables_for_display(
    paths: &SettingsPaths,
    database: &str,
) -> Result<Vec<String>, DatabaseError> {
    let mut tables = BTreeSet::new();
    for table in tables_for_database(database) {
        tables.insert((*table).to_string());
    }

    for path in [
        paths.system_database(database),
        paths.user_database(database),
    ] {
        if path.is_file() {
            for table in list_tables(&path)? {
                tables.insert(table);
            }
        }
    }

    Ok(tables.into_iter().collect())
}

pub fn table_exists(connection: &Connection, table: &str) -> Result<bool, DatabaseError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )?)
}

pub fn required_database_tables(database: &str) -> &'static [&'static str] {
    tables_for_database(database)
}

fn defaults_for_database(database: &str) -> &'static [DefaultSetting] {
    match database {
        VAMIFY_DATABASE => &VAMIFY_DEFAULTS,
        ACCESSIBILITY_DATABASE => &ACCESSIBILITY_DEFAULTS,
        _ => &[],
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn initialization_creates_expected_schema_and_defaults() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(VAMIFY_DATABASE);
        initialize_database(&path, VAMIFY_DATABASE, true).unwrap();
        let mut tables = list_tables(&path).unwrap();
        let mut expected = VAMIFY_TABLES
            .iter()
            .map(|table| table.to_string())
            .collect::<Vec<_>>();
        tables.sort();
        expected.sort();
        assert_eq!(tables, expected);
        let settings = read_all_settings(&path).unwrap();
        assert_eq!(settings.len(), 39);
        assert!(settings.iter().any(|setting| {
            setting.table == "statusbar" && setting.key == "size" && setting.value == "30"
        }));
        assert!(settings.iter().any(|setting| {
            setting.table == "icons" && setting.key == "style" && setting.value == "normal"
        }));
    }

    #[test]
    fn read_only_database_can_be_checked_without_writing() {
        let directory = tempdir().unwrap();
        let path = directory.path().join(ACCESSIBILITY_DATABASE);
        initialize_database(&path, ACCESSIBILITY_DATABASE, true).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        assert_eq!(list_tables(&path).unwrap(), vec!["accessibility"]);
    }

    #[test]
    fn invalid_builtin_values_are_rejected() {
        let directory = tempdir().unwrap();
        let paths = SettingsPaths::for_directories(
            directory.path().join("etc/VamoraSys/settings"),
            directory.path().join("home/.VamoraSys/settings"),
            directory.path().join("etc/VamoraSys"),
        );

        let error = write_user_setting(
            &paths,
            VAMIFY_DATABASE,
            "appearance",
            "theme",
            "dfsuijsdrfijh",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("must be one of: light, dark, system"));

        assert!(
            write_user_setting(&paths, VAMIFY_DATABASE, "dock", "position", "diagonal",).is_err()
        );
        assert!(write_user_setting(&paths, VAMIFY_DATABASE, "icons", "style", "outline",).is_err());
        assert!(
            write_user_setting(&paths, VAMIFY_DATABASE, "icons", "bg_color", "not-a-color",)
                .is_err()
        );
        assert!(
            write_user_setting(&paths, VAMIFY_DATABASE, "icons", "bg_color", "#12ab34",).is_ok()
        );
        assert!(
            write_user_setting(&paths, VAMIFY_DATABASE, "icons", "stroke_color", "#abc",).is_ok()
        );
        assert!(write_user_setting(&paths, VAMIFY_DATABASE, "statusbar", "size", "28",).is_err());
        assert!(write_user_setting(&paths, VAMIFY_DATABASE, "statusbar", "size", "129",).is_err());
        assert!(write_user_setting(
            &paths,
            VAMIFY_DATABASE,
            "appearance",
            "accent_color",
            "#7c3aed",
        )
        .is_ok());
    }
}
