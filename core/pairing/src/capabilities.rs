use pir_protocol::events::CapabilitySet;

pub fn capability_to_string(cap: CapabilitySet) -> Vec<String> {
    let mut result = Vec::new();
    if cap.contains(CapabilitySet::MOUSE) { result.push("mouse".to_string()); }
    if cap.contains(CapabilitySet::KEYBOARD) { result.push("keyboard".to_string()); }
    if cap.contains(CapabilitySet::TEXT) { result.push("text".to_string()); }
    if cap.contains(CapabilitySet::COMPOSITION) { result.push("composition".to_string()); }
    if cap.contains(CapabilitySet::CLIPBOARD) { result.push("clipboard".to_string()); }
    if cap.contains(CapabilitySet::VOICE) { result.push("voice".to_string()); }
    if cap.contains(CapabilitySet::CONSUMER_KEYS) { result.push("consumer_keys".to_string()); }
    if cap.contains(CapabilitySet::DIAGNOSTICS) { result.push("diagnostics".to_string()); }
    if cap.contains(CapabilitySet::BLUETOOTH_HID) { result.push("bluetooth_hid".to_string()); }
    result
}