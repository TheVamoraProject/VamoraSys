use std::fs;
use std::path::{Path, PathBuf};
use crate::cli::{SettingsAction, VamoraError};
use crate::vmf;

const SYSTEM_BASE: &str = "/etc/VamoraSys/default";

fn home_dir() -> Result<PathBuf, VamoraError> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| VamoraError::new(1, "HOME environment variable is not set"))
}

fn user_base() -> Result<PathBuf, VamoraError> {
    Ok(home_dir()?.join(".vamorasys"))
}

fn user_vmf(category: &str) -> Result<PathBuf, VamoraError> {
    Ok(user_base()?.join(category).join("settings.vmf"))
}

fn system_vmf(category: &str) -> PathBuf {
    PathBuf::from(SYSTEM_BASE).join(category).join("settings.vmf")
}

///check categories and make them
fn ensure_user_category(category: &str) -> Result<(), VamoraError> {
    let base = user_base()?;
    if !base.exists() {
        fs::create_dir_all(&base).map_err(|e| {
            VamoraError::new(3, format!("cannot create '{}': {}", base.display(), e))
        })?;
    }

    let cat_dir = base.join(category);
    if !cat_dir.exists() {
        let sys_dir = PathBuf::from(SYSTEM_BASE).join(category);
        if sys_dir.exists() {
            copy_dir(&sys_dir, &cat_dir)?;
        } else {
            // create the empty folder
            fs::create_dir_all(&cat_dir).map_err(|e| {
                VamoraError::new(3, format!("cannot create '{}': {}", cat_dir.display(), e))
            })?;
        }
    }

    Ok(())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), VamoraError> {
    fs::create_dir_all(dst).map_err(|e| {
        VamoraError::new(3, format!("cannot create '{}': {}", dst.display(), e))
    })?;

    for entry in fs::read_dir(src).map_err(|e| {
        VamoraError::new(1, format!("cannot read dir '{}': {}", src.display(), e))
    })? {
        let entry = entry.map_err(|e| VamoraError::new(1, e.to_string()))?;
        let dest_path = dst.join(entry.file_name());
        if entry.path().is_file() {
            fs::copy(entry.path(), &dest_path).map_err(|e| {
                VamoraError::new(3, format!("cannot copy file: {}", e))
            })?;
        }
    }
    Ok(())
}


pub fn handle(
    action: SettingsAction,
    category: &str,
    key: Option<&str>,
    value: Option<&str>,
    system: bool,
) -> Result<(), VamoraError> {
    if system {
        handle_system(action, category, key, value)
    } else {
        handle_user(action, category, key, value)
    }
}

fn handle_user(
    action: SettingsAction,
    category: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<(), VamoraError> {
    ensure_user_category(category)?;

    let user_path = user_vmf(category)?;
    let sys_path = system_vmf(category);

    match action {
        SettingsAction::Get => {
            let k = key.unwrap();
            // Check user side
            if let Some(v) = vmf::get_key(&user_path, k)? {
                println!("{}", v);
                return Ok(());
            }
            // Fallback to deafult 
            if let Some(v) = vmf::get_key(&sys_path, k)? {
                println!("{}", v);
                return Ok(());
            }
            Err(VamoraError::new(2, format!("key '{}' not found in category '{}'", k, category)))
        }

        SettingsAction::Put => {
            let k = key.unwrap();
            let v = value.unwrap();
            if let Some(parent) = user_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        VamoraError::new(3, format!("cannot create dir: {}", e))
                    })?;
                }
            }
            vmf::put_key(&user_path, k, v)
        }

        SettingsAction::Delete => {
            let k = key.unwrap();
            let removed = vmf::delete_key(&user_path, k)?;
            if !removed {
                return Err(VamoraError::new(2, format!("key '{}' not found in user config for '{}'", k, category)));
            }
            Ok(())
        }

        SettingsAction::List => {
            let mut merged: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for (k, v) in vmf::list_keys(&sys_path)? {
                if !merged.contains_key(&k) {
                    order.push(k.clone());
                }
                merged.insert(k, v);
            }
            
            for (k, v) in vmf::list_keys(&user_path)? {
                if !merged.contains_key(&k) {
                    order.push(k.clone());
                }
                merged.insert(k, v);
            }

            for k in &order {
                println!("{}=\"{}\"", k, merged[k]);
            }
            Ok(())
        }
    }
}

fn handle_system(
    action: SettingsAction,
    category: &str,
    key: Option<&str>,
    value: Option<&str>,
) -> Result<(), VamoraError> {
    let sys_path = system_vmf(category);

    match action {
        SettingsAction::Get => {
            let k = key.unwrap();
            match vmf::get_key(&sys_path, k)? {
                Some(v) => {
                    println!("{}", v);
                    Ok(())
                }
                None => Err(VamoraError::new(2, format!("key '{}' not found in system defaults for '{}'", k, category))),
            }
        }

        SettingsAction::Put => {
            if get_uid() != 0 {
                return Err(VamoraError::new(3, "writing system settings requires root"));
            }
            let k = key.unwrap();
            let v = value.unwrap();
            if let Some(parent) = sys_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        VamoraError::new(3, format!("cannot create system category dir: {}", e))
                    })?;
                }
            }
            vmf::put_key(&sys_path, k, v)
        }

        SettingsAction::Delete => {
            if get_uid() != 0 {
                return Err(VamoraError::new(3, "writing system settings requires root"));
            }
            let k = key.unwrap();
            let removed = vmf::delete_key(&sys_path, k)?;
            if !removed {
                return Err(VamoraError::new(2, format!("key '{}' not found in system config for '{}'", k, category)));
            }
            Ok(())
        }

        SettingsAction::List => {
            for (k, v) in vmf::list_keys(&sys_path)? {
                println!("{}=\"{}\"", k, v);
            }
            Ok(())
        }
    }
}

/// `Check commans i think
pub fn check(category: &str) -> Result<(), VamoraError> {
    ensure_user_category(category)?;
    Ok(())
}

// Whats dat

#[cfg(unix)]
fn get_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

#[cfg(not(unix))]
fn get_uid() -> u32 {
    0
}
