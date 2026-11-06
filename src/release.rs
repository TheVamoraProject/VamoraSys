use crate::config::{release_file_path, SettingsPaths};
use crate::vmf::{parse_vmf, VmfError};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReleaseError {
    #[error("release metadata was not found; checked {0} and {1}")]
    NotFound(PathBuf, PathBuf),
    #[error("cannot read release metadata: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid release metadata on line {line}: {message}")]
    Invalid { line: usize, message: String },
    #[error("release metadata is missing {0}")]
    MissingField(&'static str),
}

#[derive(Clone, Debug)]
pub struct ReleaseMetadata {
    pub path: PathBuf,
    pub fields: BTreeMap<String, String>,
}

impl ReleaseMetadata {
    pub fn read(paths: &SettingsPaths) -> Result<Self, ReleaseError> {
        let first = paths.release_root.join("vamora-release.vmf");
        let second = paths.release_root.join("vamora-release");
        let Some(path) = release_file_path(&paths.release_root) else {
            return Err(ReleaseError::NotFound(first, second));
        };
        Self::read_from_path(path)
    }

    pub fn read_from_path(path: impl Into<PathBuf>) -> Result<Self, ReleaseError> {
        let path = path.into();
        let contents = fs::read_to_string(&path)?;
        let fields = parse_vmf(&contents).map_err(|error| match error {
            VmfError::Syntax { line, message } => ReleaseError::Invalid { line, message },
        })?;

        let metadata = Self { path, fields };
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), ReleaseError> {
        require_nonempty(&self.fields, "VMF_VERSION")?;
        require_nonempty(&self.fields, "AL_NAME")?;
        if self.os_version().is_none() {
            return Err(ReleaseError::MissingField("VERSION_ID or VERSION"));
        }
        Ok(())
    }

    pub fn os_version(&self) -> Option<&str> {
        ["VERSION_ID", "VERSION"]
            .iter()
            .find_map(|key| self.fields.get(*key).filter(|value| !value.is_empty()))
            .map(String::as_str)
    }

    pub fn vamorasys_version(&self) -> Option<&str> {
        self.fields
            .get("VAMORASYS_VERSION")
            .filter(|value| !value.is_empty())
            .map(String::as_str)
    }
}

fn require_nonempty(
    fields: &BTreeMap<String, String>,
    field: &'static str,
) -> Result<(), ReleaseError> {
    if fields.get(field).is_some_and(|value| !value.is_empty()) {
        Ok(())
    } else {
        Err(ReleaseError::MissingField(field))
    }
}

pub fn release_summary(metadata: &ReleaseMetadata) -> String {
    let name = metadata
        .fields
        .get("AL_NAME")
        .map(String::as_str)
        .unwrap_or("unknown");
    let os_version = metadata.os_version().unwrap_or("unknown");
    let binary_version = metadata.vamorasys_version().unwrap_or("unknown");
    format!(
        "{} {} · VamoraSys binary {} ({})",
        name,
        os_version,
        binary_version,
        metadata.path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn accepts_sample_style_release_metadata() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "VMF_VERSION=\"1\"").unwrap();
        writeln!(file, "AL_NAME=\"VamoraOS\"").unwrap();
        writeln!(file, "VERSION_ID=\"0.2\"").unwrap();
        let metadata = ReleaseMetadata::read_from_path(file.path()).unwrap();
        assert_eq!(metadata.os_version(), Some("0.2"));
        assert_eq!(metadata.vamorasys_version(), None);
    }

    #[test]
    fn accepts_vamorasys_version_and_both_value_styles() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "VMF_VERSION=1").unwrap();
        writeln!(file, "AL_NAME=VamoraOS").unwrap();
        writeln!(file, "VERSION_ID=0.2").unwrap();
        writeln!(file, "VAMORASYS_VERSION=\"1.4.0\"").unwrap();
        let metadata = ReleaseMetadata::read_from_path(file.path()).unwrap();
        assert_eq!(metadata.os_version(), Some("0.2"));
        assert_eq!(metadata.vamorasys_version(), Some("1.4.0"));
    }

    #[test]
    fn binary_version_cannot_stand_in_for_os_version() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "VMF_VERSION=1").unwrap();
        writeln!(file, "AL_NAME=VamoraOS").unwrap();
        writeln!(file, "VAMORASYS_VERSION=1.4.0").unwrap();
        assert!(matches!(
            ReleaseMetadata::read_from_path(file.path()),
            Err(ReleaseError::MissingField("VERSION_ID or VERSION"))
        ));
    }
}
