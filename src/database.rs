use crate::config::SettingsPaths;
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use thiserror::Error;

pub const VAMIFY_DATABASE: &str = "vamify.db";
pub const ACCESSIBILITY_DATABASE: &str = "accessibility.db";
pub const CORE_DATABASES: [&str; 2] = [VAMIFY_DATABASE, ACCESSIBILITY_DATABASE];
pub const VAMIFY_TABLES: [&str; 5] = ["appearance", "homescreen", "lockscreen", "wm", "bootloader"];
pub const ACCESSIBILITY_TABLES: [&str; 1] = ["accessibility"];

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
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

const VAMIFY_DEFAULTS: [DefaultSetting; 26] = [
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

pub fn write_user_setting(
    paths: &SettingsPaths,
    database: &str,
    table: &str,
    key: &str,
    value: &str,
) -> Result<(), DatabaseError> {
    fs::create_dir_all(&paths.user_settings)?;
    let path = paths.user_database(database);
    initialize_database(&path, database, false)?;
    let connection = Connection::open(path)?;
    ensure_setting_table(&connection, table)?;
    let value_type = infer_value_type(table, key, value);
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
    fs::create_dir_all(&paths.system_settings)?;
    let path = paths.system_database(database);
    initialize_database(&path, database, false)?;
    let connection = Connection::open(path)?;
    ensure_setting_table(&connection, table)?;
    let value_type = infer_value_type(table, key, value);
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
        assert_eq!(read_all_settings(&path).unwrap().len(), 26);
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
}
