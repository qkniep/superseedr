// SPDX-FileCopyrightText: 2026 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: u32 = 1;

fn temp_path_for(path: &Path) -> PathBuf {
    let tmp_extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!("{ext}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    path.with_extension(tmp_extension)
}

pub(crate) fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = temp_path_for(path);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub(crate) fn write_string_atomically(path: &Path, content: &str) -> io::Result<()> {
    write_bytes_atomically(path, content.as_bytes())
}

pub(crate) fn serialize_versioned_toml<T: Serialize>(value: &T) -> io::Result<String> {
    let mut toml_value = toml::Value::try_from(value).map_err(io::Error::other)?;
    let table = toml_value
        .as_table_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Expected TOML table"))?;
    table.insert(
        "schema_version".to_string(),
        toml::Value::Integer(i64::from(SCHEMA_VERSION)),
    );
    toml::to_string_pretty(&toml_value).map_err(io::Error::other)
}

pub(crate) fn deserialize_versioned_toml<T: DeserializeOwned>(content: &str) -> io::Result<T> {
    let parsed: toml::Value = toml::from_str(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(table) = parsed.as_table() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected TOML table",
        ));
    };

    if let Some(schema_version_value) = table.get("schema_version") {
        let Some(schema_version) = schema_version_value.as_integer() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "schema_version must be an integer",
            ));
        };
        if schema_version != i64::from(SCHEMA_VERSION) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported schema version {schema_version}"),
            ));
        }

        let mut stripped = table.clone();
        stripped.remove("schema_version");
        return toml::Value::Table(stripped)
            .try_into()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }

    toml::from_str(content).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) fn write_toml_atomically<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let content = serialize_versioned_toml(value)?;
    write_string_atomically(path, &content)
}

pub(crate) fn serialize_versioned_json<T: Serialize>(value: &T) -> io::Result<String> {
    let mut json_value = serde_json::to_value(value).map_err(io::Error::other)?;
    let object = json_value
        .as_object_mut()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Expected JSON object"))?;
    object.insert(
        "schema_version".to_string(),
        serde_json::Value::from(SCHEMA_VERSION),
    );
    serde_json::to_string_pretty(&json_value).map_err(io::Error::other)
}

pub(crate) fn deserialize_versioned_json<T: DeserializeOwned>(content: &str) -> io::Result<T> {
    let parsed: serde_json::Value = serde_json::from_str(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(object) = parsed.as_object() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Expected JSON object",
        ));
    };

    if let Some(schema_version_value) = object.get("schema_version") {
        let Some(schema_version) = schema_version_value.as_u64() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "schema_version must be an unsigned integer",
            ));
        };
        if schema_version != u64::from(SCHEMA_VERSION) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported schema version {schema_version}"),
            ));
        }

        let mut stripped = object.clone();
        stripped.remove("schema_version");
        return serde_json::from_value(serde_json::Value::Object(stripped))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
    }

    serde_json::from_str(content).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub(crate) async fn write_bytes_atomically_async(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let tmp_path = temp_path_for(path);
    tokio::fs::write(&tmp_path, bytes).await?;
    tokio::fs::rename(&tmp_path, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_bytes_atomically_replaces_file_without_leaving_tmp() {
        let dir = tempdir().expect("create tempdir");
        let path = dir.path().join("sample.txt");

        write_bytes_atomically(&path, b"first").expect("write first");
        write_bytes_atomically(&path, b"second").expect("write second");

        assert_eq!(fs::read_to_string(&path).expect("read file"), "second");
        assert!(!path.with_extension("txt.tmp").exists());
    }
}
