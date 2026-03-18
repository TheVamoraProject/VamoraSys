use std::collections::LinkedList;
use std::fs;
use std::path::Path;
use crate::cli::VamoraError;


#[derive(Debug, Clone)]
enum VmfLine {
    Comment(String),
    Blank,
    Entry { key: String, value: String },
}

#[derive(Debug)]
pub enum VmfAction {
    Put,
    Get,
    Delete,
    List,
}

/// turn a .vmf file into a list of lines
fn parse_file(path: &Path) -> Result<Vec<VmfLine>, VamoraError> {
    let content = fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            VamoraError::new(3, format!("permission denied reading '{}'", path.display()))
        } else {
            VamoraError::new(1, format!("cannot read '{}': {}", path.display(), e))
        }
    })?;

    let mut lines = Vec::new();
    for (i, raw) in content.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            lines.push(VmfLine::Blank);
        } else if trimmed.starts_with('#') {
            lines.push(VmfLine::Comment(raw.to_string()));
        } else {
            let eq = trimmed.find('=').ok_or_else(|| {
                VamoraError::new(4, format!("invalid VMF syntax at line {}: '{}'", i + 1, raw))
            })?;
            let key = trimmed[..eq].trim().to_string();
            let raw_val = trimmed[eq + 1..].trim();
            let value = if (raw_val.starts_with('"') && raw_val.ends_with('"'))
                || (raw_val.starts_with('\'') && raw_val.ends_with('\''))
            {
                raw_val[1..raw_val.len() - 1].to_string()
            } else {
                raw_val.to_string()
            };

            if key.is_empty() {
                return Err(VamoraError::new(4, format!("empty key at line {}", i + 1)));
            }
            lines.push(VmfLine::Entry { key, value });
        }
    }
    Ok(lines)
}

fn serialize(lines: &[VmfLine]) -> String {
    lines
        .iter()
        .map(|l| match l {
            VmfLine::Comment(s) => s.clone(),
            VmfLine::Blank => String::new(),
            VmfLine::Entry { key, value } => format!("{}=\"{}\"", key, value),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn write_file(path: &Path, lines: &[VmfLine]) -> Result<(), VamoraError> {
    fs::write(path, serialize(lines)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            VamoraError::new(3, format!("permission denied writing '{}'", path.display()))
        } else {
            VamoraError::new(1, format!("cannot write '{}': {}", path.display(), e))
        }
    })
}


// Public API
pub fn get_key(path: &Path, key: &str) -> Result<Option<String>, VamoraError> {
    if !path.exists() {
        return Ok(None);
    }
    let lines = parse_file(path)?;
    for line in &lines {
        if let VmfLine::Entry { key: k, value: v } = line {
            if k == key {
                return Ok(Some(v.clone()));
            }
        }
    }
    Ok(None)
}


pub fn put_key(path: &Path, key: &str, value: &str) -> Result<(), VamoraError> {
    let mut lines = if path.exists() { parse_file(path)? } else { Vec::new() };

    let mut found = false;
    for line in &mut lines {
        if let VmfLine::Entry { key: k, value: v } = line {
            if k == key {
                *v = value.to_string();
                found = true;
                break;
            }
        }
    }
    if !found {
        lines.push(VmfLine::Entry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    write_file(path, &lines)
}


pub fn delete_key(path: &Path, key: &str) -> Result<bool, VamoraError> {
    if !path.exists() {
        return Ok(false);
    }
    let lines = parse_file(path)?;
    let original_len = lines.len();
    let new_lines: Vec<VmfLine> = lines
        .into_iter()
        .filter(|l| match l {
            VmfLine::Entry { key: k, .. } => k != key,
            _ => true,
        })
        .collect();

    let removed = new_lines.len() < original_len;
    if removed {
        write_file(path, &new_lines)?;
    }
    Ok(removed)
}

pub fn list_keys(path: &Path) -> Result<Vec<(String, String)>, VamoraError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let lines = parse_file(path)?;
    Ok(lines
        .into_iter()
        .filter_map(|l| {
            if let VmfLine::Entry { key, value } = l {
                Some((key, value))
            } else {
                None
            }
        })
        .collect())
}


pub fn print_raw(path: &Path) -> Result<(), VamoraError> {
    if !path.exists() {
        return Ok(());
    }
    let lines = parse_file(path)?;
    print!("{}", serialize(&lines));
    Ok(())
}

// Direct VMF editing

pub fn handle_direct(
    file: &str,
    action: VmfAction,
    key: &str,
    value: Option<&str>,
) -> Result<(), VamoraError> {
    let path = Path::new(file);

    match action {
        VmfAction::Get => {
            match get_key(path, key)? {
                Some(v) => println!("{}", v),
                None => return Err(VamoraError::new(2, format!("key '{}' not found in '{}'", key, file))),
            }
        }
        VmfAction::Put => {
            let v = value.ok_or_else(|| VamoraError::new(1, "put requires a value"))?;
            
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent).map_err(|e| {
                        VamoraError::new(3, format!("cannot create directory '{}': {}", parent.display(), e))
                    })?;
                }
            }
            put_key(path, key, v)?;
        }
        VmfAction::Delete => {
            let removed = delete_key(path, key)?;
            if !removed {
                return Err(VamoraError::new(2, format!("key '{}' not found in '{}'", key, file)));
            }
        }
        VmfAction::List => {
            print_raw(path)?;
        }
    }

    Ok(())
}
