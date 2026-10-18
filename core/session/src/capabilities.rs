use pir_protocol::CapabilitySet;

pub fn negotiate_capabilities(client: CapabilitySet, server: CapabilitySet) -> CapabilitySet {
    client & server
}

pub fn get_default_phone_capabilities() -> CapabilitySet {
    CapabilitySet::MOUSE
        | CapabilitySet::KEYBOARD
        | CapabilitySet::TEXT
        | CapabilitySet::COMPOSITION
        | CapabilitySet::VOICE
        | CapabilitySet::CLIPBOARD
        | CapabilitySet::CONSUMER_KEYS
        | CapabilitySet::DIAGNOSTICS
}

pub fn get_default_desktop_capabilities() -> CapabilitySet {
    CapabilitySet::MOUSE
        | CapabilitySet::KEYBOARD
        | CapabilitySet::TEXT
        | CapabilitySet::COMPOSITION
        | CapabilitySet::CONSUMER_KEYS
        | CapabilitySet::DIAGNOSTICS
}

pub fn capability_to_string(cap: CapabilitySet) -> Vec<&'static str> {
    let mut result = Vec::new();
    if cap.contains(CapabilitySet::MOUSE) { result.push("mouse"); }
    if cap.contains(CapabilitySet::KEYBOARD) { result.push("keyboard"); }
    if cap.contains(CapabilitySet::TEXT) { result.push("text"); }
    if cap.contains(CapabilitySet::COMPOSITION) { result.push("composition"); }
    if cap.contains(CapabilitySet::CLIPBOARD) { result.push("clipboard"); }
    if cap.contains(CapabilitySet::VOICE) { result.push("voice"); }
    if cap.contains(CapabilitySet::CONSUMER_KEYS) { result.push("consumer_keys"); }
    if cap.contains(CapabilitySet::DIAGNOSTICS) { result.push("diagnostics"); }
    if cap.contains(CapabilitySet::BLUETOOTH_HID) { result.push("bluetooth_hid"); }
    result
}