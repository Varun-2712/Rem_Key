use crate::identity::{DeviceIdentity, PairingInfo};
use pir_protocol::CapabilitySet;
use anyhow::Result;

pub struct PairingManager {
    local_identity: DeviceIdentity,
    trusted_devices: std::collections::HashMap<String, TrustedDevice>,
}

#[derive(Debug, Clone)]
pub struct TrustedDevice {
    pub device_id: String,
    pub public_key: ed25519_dalek::VerifyingKey,
    pub name: String,
    pub paired_at: chrono::DateTime<chrono::Utc>,
    pub capabilities: CapabilitySet,
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
        let public_key_bytes = base64::decode(&pairing_info.public_key)?;
        let public_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key_bytes.try_into()?)?;
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