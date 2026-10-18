use std::collections::HashMap;
use std::path::PathBuf;
use crate::identity::TrustedDevice;
use anyhow::Result;

pub struct TrustStore {
    path: PathBuf,
    devices: HashMap<String, TrustedDevice>,
}

impl TrustStore {
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut store = Self { path, devices: HashMap::new() };
        store.load()?;
        Ok(store)
    }

    fn load(&mut self) -> Result<()> {
        if self.path.exists() {
            let data = std::fs::read_to_string(&self.path)?;
            self.devices = serde_json::from_str(&data)?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let data = serde_json::to_string_pretty(&self.devices)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    pub fn add(&mut self, device: TrustedDevice) -> Result<()> {
        self.devices.insert(device.device_id.clone(), device);
        self.save()
    }

    pub fn remove(&mut self, device_id: &str) -> Result<()> {
        self.devices.remove(device_id);
        self.save()
    }

    pub fn get(&self, device_id: &str) -> Option<&TrustedDevice> {
        self.devices.get(device_id)
    }

    pub fn list(&self) -> Vec<&TrustedDevice> {
        self.devices.values().collect()
    }
}