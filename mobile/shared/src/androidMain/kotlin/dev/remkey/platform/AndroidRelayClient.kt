package dev.remkey.platform

import dev.remkey.bridge.ConnectionState
import dev.remkey.bridge.RelayException
import dev.remkey.bridge.RelayListener
import dev.remkey.bridge.SpecialKey
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.domain.model.PairingCode
import dev.remkey.relay.EditingKey
import dev.remkey.relay.PointerButton
import dev.remkey.relay.RelayClient
import dev.remkey.relay.RelayCommand
import dev.remkey.relay.RelayConnectionState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import dev.remkey.bridge.DesktopTarget as NativeTarget
import dev.remkey.bridge.PairingCode as NativeCode
import dev.remkey.bridge.PointerButton as NativeButton
import dev.remkey.bridge.RelayClient as NativeRelayClient
import dev.remkey.bridge.parsePairingCode as nativeParsePairingCode

/**
 * [RelayClient] over the Rust bridge (`libpir_mobile.so`, loaded by JNA on first use).
 * [dataDir] holds this phone's certificate and key; use app-private storage that isn't
 * backed up, so the identity never moves to another device.
 */
class AndroidRelayClient(dataDir: String) : RelayClient {
    private val native = NativeRelayClient(dataDir)
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

    override val deviceFingerprint: String = native.deviceFingerprint()

    override fun parsePairingCode(text: String): Result<PairingCode> = try {
        Result.success(nativeParsePairingCode(text).toCommon())
    } catch (e: RelayException) {
        Result.failure(IllegalArgumentException(e.describe()))
    }

    override fun connect(target: DesktopTarget) {
        try {
            native.connect(target.toNative(), listener)
        } catch (e: RelayException) {
            _state.value = RelayConnectionState.Failed(e.describe())
        }
    }

    override fun pair(code: PairingCode, deviceName: String) {
        try {
            native.pair(NativeCode(code.desktop.toNative(), code.secret), deviceName, listener)
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
    is RelayException.InvalidAddress -> "The pairing code has no usable address"
    is RelayException.InvalidFingerprint -> "The pairing code has a malformed fingerprint"
    is RelayException.InvalidPairingCode -> "That isn't a Rem Key pairing code"
    is RelayException.Identity -> "This phone's identity couldn't be loaded"
    is RelayException.AlreadyConnected -> "Already connected"
    is RelayException.NotConnected -> "Not connected"
    is RelayException.QueueFull -> "Connection is stalled"
}

private fun DesktopTarget.toNative() = NativeTarget(name, endpoints, fingerprint)

private fun NativeTarget.toCommon() = DesktopTarget(name, endpoints, fingerprint)

private fun NativeCode.toCommon() = PairingCode(desktop.toCommon(), secret)

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
