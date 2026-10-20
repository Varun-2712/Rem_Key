package dev.remkey.relay

sealed interface RelayConnectionState {
    data object Disconnected : RelayConnectionState
    data object Connecting : RelayConnectionState
    data object Connected : RelayConnectionState
    data class Failed(val message: String) : RelayConnectionState
}
