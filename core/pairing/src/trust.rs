use std::collections::BTreeMap;
use std::path::PathBuf;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::fsutil::write_private;
use crate::{fingerprint_hex, Fingerprint};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDevice {
    /// Hex SHA-256 of the device's certificate.
    pub fingerprint: String,
    pub name: String,
    pub paired_at: DateTime<Utc>,
}

/// Devices allowed to connect, persisted as JSON and keyed by certificate fingerprint.
pub struct TrustStore {
    path: PathBuf,
    devices: BTreeMap<String, TrustedDevice>,
}

impl TrustStore {
    pub const FILE_NAME: &'static str = "trusted_devices.json";

    pub fn open(path: PathBuf) -> Result<Self> {
        let devices = if path.exists() {
            let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()))?
        } else {
            BTreeMap::new()
        };
        Ok(Self { path, devices })
    }

    fn save(&self) -> Result<()> {
        write_private(&self.path, &serde_json::to_vec_pretty(&self.devices)?)
    }

    pub fn is_trusted(&self, fingerprint: &Fingerprint) -> bool {
        self.devices.contains_key(&fingerprint_hex(fingerprint))
    }

    pub fn get(&self, fingerprint: &Fingerprint) -> Option<&TrustedDevice> {
        self.devices.get(&fingerprint_hex(fingerprint))
    }

    pub fn add(&mut self, fingerprint: &Fingerprint, name: &str) -> Result<TrustedDevice> {
        let device = TrustedDevice {
            fingerprint: fingerprint_hex(fingerprint),
            name: name.to_string(),
            paired_at: Utc::now(),
        };
        self.devices.insert(device.fingerprint.clone(), device.clone());
        self.save()?;
        Ok(device)
    }

    /// Removes the device whose fingerprint starts with `prefix` (at least 8 hex
    /// characters, and only if exactly one device matches).
    pub fn remove_by_prefix(&mut self, prefix: &str) -> Result<TrustedDevice> {
        let prefix = prefix.trim().to_ascii_lowercase();
        anyhow::ensure!(prefix.len() >= 8, "give at least 8 characters of the fingerprint");
        let matches: Vec<_> = self.devices.keys().filter(|k| k.starts_with(&prefix)).cloned().collect();
        match matches.as_slice() {
            [key] => {
                let device = self.devices.remove(key).expect("key was just listed");
                self.save()?;
                Ok(device)
            }
            [] => anyhow::bail!("no paired device matches {prefix}"),
            _ => anyhow::bail!("{prefix} matches more than one device; give more characters"),
        }
    }

    pub fn list(&self) -> impl Iterator<Item = &TrustedDevice> {
        self.devices.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> TrustStore {
        TrustStore::open(dir.path().join(TrustStore::FILE_NAME)).unwrap()
    }

    #[test]
    fn persists_trusted_devices() {
        let dir = tempfile::tempdir().unwrap();
        let fp = [7u8; 32];
        store(&dir).add(&fp, "Pixel").unwrap();

        let reopened = store(&dir);
        assert!(reopened.is_trusted(&fp));
        assert_eq!(reopened.get(&fp).unwrap().name, "Pixel");
        assert!(!reopened.is_trusted(&[8u8; 32]));
    }

    #[test]
    fn removes_by_unique_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = store(&dir);
        s.add(&[0xAA; 32], "a").unwrap();
        s.add(&[0xAB; 32], "b").unwrap();

        assert!(s.remove_by_prefix("aa").is_err(), "too short");
        assert!(s.remove_by_prefix("cccccccc").is_err());
        assert_eq!(s.remove_by_prefix("ABABABAB").unwrap().name, "b");
        assert!(!store(&dir).is_trusted(&[0xAB; 32]));
        assert!(store(&dir).is_trusted(&[0xAA; 32]));
    }
}
