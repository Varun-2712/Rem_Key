use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use rand::rngs::OsRng;
use rand::Rng;
use zeroize::Zeroize;
use serde::{Deserialize, Serialize};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;

#[derive(Clone)]
pub struct DeviceIdentity {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl DeviceIdentity {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> anyhow::Result<Self> {
        let signing_key = SigningKey::from_bytes(bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self { signing_key, verifying_key })
    }

    pub fn public_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    pub fn public_key_b64(&self) -> String {
        BASE64_STANDARD.encode(self.verifying_key.as_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> Signature {
        self.signing_key.sign(msg)
    }

    pub fn verify(&self, msg: &[u8], sig: &Signature) -> bool {
        self.verifying_key.verify(msg, sig).is_ok()
    }

    pub fn device_id(&self) -> String {
        format!("pir-{}", hex::encode(&self.verifying_key.as_bytes()[..8]))
    }
}

impl Drop for DeviceIdentity {
    fn drop(&mut self) {
        self.signing_key.to_bytes().zeroize();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInfo {
    pub version: u32,
    pub device_id: String,
    pub public_key: String,
    pub endpoint: String,
    pub name: String,
    pub capabilities: Vec<String>,
}

impl PairingInfo {
    pub fn to_qr_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn from_qr_json(json: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}