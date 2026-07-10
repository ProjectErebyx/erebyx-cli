// SPDX-License-Identifier: MIT OR Apache-2.0
//! Local counterpart credential store.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCredentials {
    pub api_key: String,
    pub api_url: String,
    pub instance_id: String,
    pub passphrase: Option<String>,
}

impl StoredCredentials {
    pub fn normalized(mut self) -> Self {
        self.api_key = self.api_key.trim().to_string();
        self.api_url = self.api_url.trim().trim_end_matches('/').to_string();
        self.instance_id = self.instance_id.trim().to_string();
        self.passphrase = self
            .passphrase
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        self
    }
}

pub fn credentials_path() -> Result<PathBuf> {
    let base = dirs::config_dir()
        .or_else(dirs::home_dir)
        .context("Could not determine a config directory for EREBYX credentials")?;
    Ok(base.join("erebyx").join("credentials.json"))
}

pub fn portable_harness_dir() -> Result<PathBuf> {
    let creds = credentials_path()?;
    let parent = creds
        .parent()
        .context("Credential path did not include a parent directory")?;
    Ok(parent.join("portable-counterpart"))
}

pub fn save_credentials(credentials: &StoredCredentials) -> Result<PathBuf> {
    let path = credentials_path()?;
    save_credentials_to_path(&path, credentials)?;
    Ok(path)
}

pub fn load_credentials() -> Result<Option<StoredCredentials>> {
    let path = credentials_path()?;
    load_credentials_from_path(&path)
}

pub fn save_credentials_to_path(path: &Path, credentials: &StoredCredentials) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    let normalized = credentials.clone().normalized();
    let body =
        serde_json::to_vec_pretty(&normalized).context("Failed to serialize EREBYX credentials")?;

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("Failed to open {}", path.display()))?
    };

    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;

    file.write_all(&body)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("Failed to finish {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("Failed to sync {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

pub fn load_credentials_from_path(path: &Path) -> Result<Option<StoredCredentials>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let credentials: StoredCredentials = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(credentials.normalized()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_store_round_trips_counterpart_identity_material() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("credentials.json");
        let creds = StoredCredentials {
            api_key: "ebx_live_counterpart_key".to_string(),
            api_url: "https://core.erebyx.com".to_string(),
            instance_id: "inst_01JZ_REAL_COUNTERPART".to_string(),
            passphrase: Some("carry-the-self".to_string()),
        };

        save_credentials_to_path(&path, &creds).unwrap();
        let loaded = load_credentials_from_path(&path).unwrap().unwrap();

        assert_eq!(loaded, creds);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "credential file must be owner-only");
        }
    }
}
