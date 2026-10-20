package dev.remkey.platform

import dev.remkey.bridge.ConnectionState
import dev.remkey.bridge.RelayException
import dev.remkey.bridge.RelayListener
import dev.remkey.bridge.SpecialKey
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.relay.EditingKey
import dev.remkey.relay.PointerButton
import dev.remkey.relay.RelayClient
import dev.remkey.relay.RelayCommand
import dev.remkey.relay.RelayConnectionState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import dev.remkey.bridge.PointerButton as NativeButton
import dev.remkey.bridge.RelayClient as NativeRelayClient

/** [RelayClient] over the Rust bridge (`libpir_mobile.so`, loaded by JNA on first use). */
class AndroidRelayClient : RelayClient {
    private val native = NativeRelayClient()
    private val _state = MutableStateFlow<RelayConnectionState>(RelayConnectionState.Disconnected)
    override val state: StateFlow<RelayConnectionState> = _state.asStateFlow()

    // Called from a Rust worker thread; StateFlow is thread-safe.
    private val listener = object : RelayListener {
        override fun onStateChanged(state: ConnectionState) {
            _state.value = when (state) {
                ConnectionState.Disconnected -> RelayConnectionState.Disconnected
                ConnectionState.Connecting -> RelayConnectionState.Connecting
                ConnectionState.Connected -> RelayConnectionState.Connected
                is ConnectionState.Failed -> RelayConnectionState.Failed(state.message)
            }
        }
    }

    override fun connect(target: DesktopTarget) {
        try {
            native.connect(target.host.trim(), target.port.toUShort(), target.fingerprint, listener)
        } catch (e: RelayException) {
            _state.value = RelayConnectionState.Failed(e.describe())
        }
    }

    override fun send(command: RelayCommand) {
        try {
            when (command) {
                is RelayCommand.PointerDelta -> native.pointerDelta(command.dx, command.dy)
                is RelayCommand.Scroll -> native.scroll(command.dx, command.dy)
                is RelayCommand.Button -> native.mouseButton(command.button.toNative(), command.pressed)
                is RelayCommand.Key -> native.specialKey(command.key.toNative(), command.pressed)
                is RelayCommand.CommitText -> native.commitText(command.text)
            }
        } catch (_: RelayException.NotConnected) {
            // The UI can race a disconnect; there's nobody to send to.
        } catch (_: RelayException.QueueFull) {
            // Only reliable commands get here; the network is badly stalled and the
            // session will fail on its own shortly.
        }
    }

    override fun disconnect() = native.disconnect()

    override fun close() = native.close()
}

private fun RelayException.describe(): String = when (this) {
    is RelayException.InvalidAddress -> "That address doesn't look right"
    is RelayException.InvalidFingerprint -> "The fingerprint should be 64 hex characters"
    is RelayException.AlreadyConnected -> "Already connected"
    is RelayException.NotConnected -> "Not connected"
    is RelayException.QueueFull -> "Connection is stalled"
}

private fun PointerButton.toNative(): NativeButton = when (this) {
    PointerButton.Left -> NativeButton.LEFT
    PointerButton.Right -> NativeButton.RIGHT
    PointerButton.Middle -> NativeButton.MIDDLE
}

private fun EditingKey.toNative(): SpecialKey = when (this) {
    EditingKey.Enter -> SpecialKey.ENTER
    EditingKey.Backspace -> SpecialKey.BACKSPACE
    EditingKey.Delete -> SpecialKey.DELETE
    EditingKey.Tab -> SpecialKey.TAB
    EditingKey.Escape -> SpecialKey.ESCAPE
    EditingKey.ArrowUp -> SpecialKey.ARROW_UP
    EditingKey.ArrowDown -> SpecialKey.ARROW_DOWN
    EditingKey.ArrowLeft -> SpecialKey.ARROW_LEFT
    EditingKey.ArrowRight -> SpecialKey.ARROW_RIGHT
    EditingKey.Home -> SpecialKey.HOME
    EditingKey.End -> SpecialKey.END
    EditingKey.PageUp -> SpecialKey.PAGE_UP
    EditingKey.PageDown -> SpecialKey.PAGE_DOWN
}
