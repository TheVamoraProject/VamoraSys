//! The `.vmf` file syntax shared by `vamora-release.vmf` and `device.vmf`.
//!
//! One `KEY=value` per line. Keys are `A-Z`, `0-9` and `_` only. Values may be bare or
//! wrapped in matching `"` or `'` quotes. Blank lines and lines starting with `#` are
//! ignored. Anything else is a syntax error.

use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VmfError {
    #[error("line {line}: {message}")]
    Syntax { line: usize, message: String },
}

pub fn parse_vmf(contents: &str) -> Result<BTreeMap<String, String>, VmfError> {
    let mut fields = BTreeMap::new();
    for (index, raw_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(VmfError::Syntax {
                line: line_number,
                message: "expected KEY=value".to_string(),
            });
        };
        let key = raw_key.trim();
        if key.is_empty()
            || key.chars().any(|character| {
                !character.is_ascii_uppercase() && !character.is_ascii_digit() && character != '_'
            })
        {
            return Err(VmfError::Syntax {
                line: line_number,
                message: format!("invalid field name '{key}'"),
            });
        }
        let value = unquote(raw_value.trim()).ok_or_else(|| VmfError::Syntax {
            line: line_number,
            message: "unterminated quoted value".to_string(),
        })?;
        fields.insert(key.to_string(), value);
    }
    Ok(fields)
}

fn unquote(value: &str) -> Option<String> {
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        Some(
            value[1..value.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\"),
        )
    } else if value.contains('"') || value.contains('\'') {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_bare_and_commented_lines() {
        let fields =
            parse_vmf("# c\n\nVMF_VERSION=\"1\"\nA=b\nC='d e'\n  F_1 = \"g\"  \n").unwrap();
        assert_eq!(fields["VMF_VERSION"], "1");
        assert_eq!(fields["A"], "b");
        assert_eq!(fields["C"], "d e");
        assert_eq!(fields["F_1"], "g");
    }

    #[test]
    fn rejects_bad_lines_with_the_line_number() {
        assert_eq!(
            parse_vmf("A=1\nnot a pair\n"),
            Err(VmfError::Syntax {
                line: 2,
                message: "expected KEY=value".into()
            })
        );
        assert!(matches!(
            parse_vmf("lower=1"),
            Err(VmfError::Syntax { line: 1, .. })
        ));
        assert!(matches!(
            parse_vmf("A=\"open"),
            Err(VmfError::Syntax { line: 1, .. })
        ));
        assert!(matches!(
            parse_vmf("A=b\"c"),
            Err(VmfError::Syntax { line: 1, .. })
        ));
    }

    #[test]
    fn last_duplicate_wins_like_release_files() {
        assert_eq!(parse_vmf("A=1\nA=2\n").unwrap()["A"], "2");
    }
}
