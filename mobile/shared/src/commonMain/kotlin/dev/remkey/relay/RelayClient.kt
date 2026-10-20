package dev.remkey.relay

import dev.remkey.domain.model.DesktopTarget
import kotlinx.coroutines.flow.StateFlow

/**
 * The app's view of the connection to one desktop. Implementations are platform adapters
 * over the Rust bridge; none of these calls block on the network.
 */
interface RelayClient {
    val state: StateFlow<RelayConnectionState>

    /** Starts connecting; progress shows up in [state]. */
    fun connect(target: DesktopTarget)

    /** Queues [command]. Ignored while not connected. */
    fun send(command: RelayCommand)

    /** Releases anything held on the desktop, then disconnects. */
    fun disconnect()

    /** Frees the native client. The instance can't be used afterwards. */
    fun close()
}
