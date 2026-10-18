use crate::identity::{DeviceIdentity, PairingInfo};
use pir_protocol::events::CapabilitySet;
use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ed25519_dalek::{Verifier, VerifyingKey};

mod pubkey_serde {
    use serde::{Serializer, Deserializer, Deserialize};
    use ed25519_dalek::VerifyingKey;
    
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Vec::deserialize(deserializer)?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| serde::de::Error::custom("Invalid key length"))?;
        VerifyingKey::from_bytes(&arr).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TrustedDevice {
    pub device_id: String,
    #[serde(with = "pubkey_serde")]
    pub public_key: ed25519_dalek::VerifyingKey,
    pub name: String,
    pub paired_at: chrono::DateTime<chrono::Utc>,
    pub capabilities: CapabilitySet,
}

pub struct PairingManager {
    local_identity: DeviceIdentity,
    trusted_devices: std::collections::HashMap<String, TrustedDevice>,
}

impl PairingManager {
    pub fn new(local_identity: DeviceIdentity) -> Self {
        Self {
            local_identity,
            trusted_devices: std::collections::HashMap::new(),
        }
    }

    pub fn generate_pairing_qr(&self, endpoint: String, name: String, capabilities: CapabilitySet) -> PairingInfo {
        PairingInfo {
            version: 1,
            device_id: self.local_identity.device_id(),
            public_key: self.local_identity.public_key_b64(),
            endpoint,
            name,
            capabilities: crate::capabilities::capability_to_string(capabilities),
        }
    }

    pub fn verify_pairing(&self, pairing_info: &PairingInfo, signature: &ed25519_dalek::Signature, challenge: &[u8]) -> Result<bool> {
        let public_key_bytes = BASE64_STANDARD.decode(&pairing_info.public_key)?;
        let public_key = VerifyingKey::from_bytes(&public_key_bytes[..].try_into().map_err(|_| anyhow::anyhow!("Invalid key length"))?)?;
        Ok(public_key.verify(challenge, signature).is_ok())
    }

    pub fn add_trusted_device(&mut self, device: TrustedDevice) {
        self.trusted_devices.insert(device.device_id.clone(), device);
    }

    pub fn is_trusted(&self, device_id: &str) -> bool {
        self.trusted_devices.contains_key(device_id)
    }

    pub fn get_trusted(&self, device_id: &str) -> Option<&TrustedDevice> {
        self.trusted_devices.get(device_id)
    }

    pub fn local_identity(&self) -> &DeviceIdentity {
        &self.local_identity
    }
}