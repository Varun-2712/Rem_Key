use std::path::Path;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use crate::fsutil::write_private;

/// A device's TLS certificate and private key (both DER), kept across restarts so that
/// its fingerprint, which paired devices pin, stays the same.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredIdentity {
    #[serde(with = "b64")]
    pub cert_der: Vec<u8>,
    #[serde(with = "b64")]
    pub key_der: Vec<u8>,
}

impl std::fmt::Debug for StoredIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredIdentity").field("cert_der", &self.cert_der.len()).finish_non_exhaustive()
    }
}

impl StoredIdentity {
    pub const FILE_NAME: &'static str = "identity.json";

    /// Loads the identity in `dir`, creating it with `generate` (returning certificate and
    /// key DER) the first time.
    pub fn load_or_create(dir: &Path, generate: impl FnOnce() -> Result<(Vec<u8>, Vec<u8>)>) -> Result<Self> {
        let path = dir.join(Self::FILE_NAME);
        if path.exists() {
            let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            return serde_json::from_slice(&data).with_context(|| format!("parsing {}", path.display()));
        }
        let (cert_der, key_der) = generate()?;
        let identity = Self { cert_der, key_der };
        write_private(&path, &serde_json::to_vec_pretty(&identity)?)?;
        Ok(identity)
    }
}

mod b64 {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&BASE64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        BASE64.decode(s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_once_then_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let first = StoredIdentity::load_or_create(dir.path(), || Ok((vec![1, 2, 3], vec![4, 5]))).unwrap();
        let second = StoredIdentity::load_or_create(dir.path(), || panic!("must not regenerate")).unwrap();
        assert_eq!(first.cert_der, second.cert_der);
        assert_eq!(first.key_der, second.key_der);
    }

    #[test]
    fn debug_does_not_print_key() {
        let identity = StoredIdentity { cert_der: vec![1], key_der: vec![0xAB; 4] };
        assert!(!format!("{identity:?}").contains("171"));
    }
}
