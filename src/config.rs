use std::env;
use std::path::{Path, PathBuf};

pub const SYSTEM_ROOT: &str = "/etc/VamoraSys";
pub const USER_ROOT_NAME: &str = ".VamoraSys";
pub const SETTINGS_DIR_NAME: &str = "settings";
pub const RELEASE_FILENAMES: [&str; 2] = ["vamora-release.vmf", "vamora-release"];

#[derive(Clone, Debug)]
pub struct SettingsPaths {
    pub system_root: PathBuf,
    pub system_settings: PathBuf,
    pub user_settings: PathBuf,
    pub release_root: PathBuf,
}

impl SettingsPaths {
    pub fn from_environment() -> Result<Self, String> {
        let system_settings = env::var_os("VAMORASYS_SYSTEM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(SYSTEM_ROOT).join(SETTINGS_DIR_NAME));

        let user_root = env::var_os("VAMORASYS_USER_ROOT")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(USER_ROOT_NAME)))
            .ok_or_else(|| {
                "cannot determine the user home directory; set HOME or VAMORASYS_USER_ROOT"
                    .to_string()
            })?;

        let user_settings = env::var_os("VAMORASYS_USER_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_root.join(SETTINGS_DIR_NAME));

        let release_root = env::var_os("VAMORASYS_RELEASE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                system_settings
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from(SYSTEM_ROOT))
            });

        Ok(Self {
            system_root: release_root.clone(),
            system_settings,
            user_settings,
            release_root,
        })
    }

    #[cfg(test)]
    pub fn for_directories(
        system_settings: impl Into<PathBuf>,
        user_settings: impl Into<PathBuf>,
        release_root: impl Into<PathBuf>,
    ) -> Self {
        let system_settings = system_settings.into();
        let user_settings = user_settings.into();
        let release_root = release_root.into();
        Self {
            system_root: release_root.clone(),
            system_settings,
            user_settings,
            release_root,
        }
    }

    pub fn system_database(&self, database: &str) -> PathBuf {
        self.system_settings.join(database)
    }

    pub fn user_database(&self, database: &str) -> PathBuf {
        self.user_settings.join(database)
    }
}

pub fn normalize_database_name(input: &str) -> Result<String, String> {
    let normalized_input = input.trim().replace('\\', "/");
    let name = if normalized_input.ends_with(".db") {
        normalized_input
    } else {
        format!("{normalized_input}.db")
    };

    let valid_name = |value: &str| {
        !value.is_empty()
            && value != ".db"
            && value.ends_with(".db")
            && !value.contains("..")
            && value.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || character == '_'
                    || character == '-'
                    || character == '.'
            })
    };

    let valid = if let Some(app_name) = name.strip_prefix("apps/") {
        valid_name(app_name) && !app_name.contains('/')
    } else {
        valid_name(&name) && !name.contains('/')
    };

    if !valid {
        return Err(format!("invalid database name '{input}'"));
    }

    Ok(name)
}

pub fn split_setting_key(input: &str) -> Result<(&str, &str), String> {
    let Some((table, key)) = input.split_once('.') else {
        return Err(format!(
            "invalid setting key '{input}'; use the table.key form, for example appearance.theme"
        ));
    };

    if table.is_empty()
        || key.is_empty()
        || key.contains('.')
        || table.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        || key.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
    {
        return Err(format!("invalid setting key '{input}'"));
    }

    Ok((table, key))
}

pub fn release_file_path(root: &Path) -> Option<PathBuf> {
    RELEASE_FILENAMES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_database_names() {
        assert_eq!(normalize_database_name("vamify").unwrap(), "vamify.db");
        assert_eq!(normalize_database_name("vamify.db").unwrap(), "vamify.db");
        assert!(normalize_database_name("../vamify").is_err());
    }

    #[test]
    fn splits_qualified_setting_keys() {
        assert_eq!(
            split_setting_key("appearance.theme").unwrap(),
            ("appearance", "theme")
        );
        assert!(split_setting_key("theme").is_err());
        assert!(split_setting_key("appearance.theme.extra").is_err());
    }
}
