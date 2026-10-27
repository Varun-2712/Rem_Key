package dev.remkey.relay

import dev.remkey.domain.model.DesktopTarget
import dev.remkey.domain.model.PairingCode
import kotlinx.coroutines.flow.StateFlow

/**
 * The app's view of the connection to one desktop. Implementations are platform adapters
 * over the Rust bridge; none of these calls block on the network.
 */
interface RelayClient {
    val state: StateFlow<RelayConnectionState>

    /** This phone's certificate fingerprint, hex; the desktop lists paired phones by it. */
    val deviceFingerprint: String

    /** Reads the text of a pairing QR code; fails with a message fit to show the user. */
    fun parsePairingCode(text: String): Result<PairingCode>

    /** Starts connecting to a desktop this phone is paired with; progress shows up in [state]. */
    fun connect(target: DesktopTarget)

    /**
     * Pairs with the desktop in [code] and stays connected; progress shows up in [state].
     * [deviceName] is how the desktop lists this phone.
     */
    fun pair(code: PairingCode, deviceName: String)

    /** Queues [command]. Ignored while not connected. */
    fun send(command: RelayCommand)

    /** Releases anything held on the desktop, then disconnects. */
    fun disconnect()

    /** Frees the native client. The instance can't be used afterwards. */
    fun close()
}
