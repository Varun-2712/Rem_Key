package dev.remkey

import android.app.Application
import android.content.Context
import android.os.Build
import android.provider.Settings
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.domain.model.PairingCode
import dev.remkey.platform.AndroidRelayClient
import dev.remkey.relay.RelayClient
import dev.remkey.relay.RelayCommand
import dev.remkey.relay.RelayConnectionState
import dev.remkey.touchpad.TouchpadGestures
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * Owns the connection so it survives rotation; composables only observe [state] and
 * forward input. Remembers the desktop this phone last paired with.
 */
class RelayViewModel(app: Application) : AndroidViewModel(app) {
    // noBackupFilesDir: the phone's private key must never be restored onto another device.
    private val client: RelayClient = AndroidRelayClient(File(app.noBackupFilesDir, "relay").path)
    private val prefs = app.getSharedPreferences("desktop", Context.MODE_PRIVATE)

    val state: StateFlow<RelayConnectionState> = client.state
    val gestures = TouchpadGestures(emit = client::send)

    private val _pairedDesktop = MutableStateFlow(loadDesktop())
    val pairedDesktop: StateFlow<DesktopTarget?> = _pairedDesktop.asStateFlow()

    private val _pairingCodeError = MutableStateFlow<String?>(null)
    /** Why the last pasted pairing code was rejected, before any connection was tried. */
    val pairingCodeError: StateFlow<String?> = _pairingCodeError.asStateFlow()

    /** Saved only once pairing actually succeeds. */
    private var pairing: DesktopTarget? = null

    init {
        viewModelScope.launch {
            client.state.collect { state ->
                if (state == RelayConnectionState.Connecting) return@collect
                if (state == RelayConnectionState.Connected) pairing?.let(::saveDesktop)
                pairing = null
            }
        }
    }

    fun connect() {
        _pairedDesktop.value?.let(client::connect)
    }

    /** [text] is the pairing QR code's content, or the "Pairing data" line the agent prints. */
    fun pair(text: String) {
        client.parsePairingCode(text)
            .onSuccess { code: PairingCode ->
                _pairingCodeError.value = null
                pairing = code.desktop
                client.pair(code, deviceName())
            }
            .onFailure { _pairingCodeError.value = it.message }
    }

    fun send(command: RelayCommand) = client.send(command)

    fun disconnect() = client.disconnect()

    override fun onCleared() {
        client.disconnect()
        client.close()
    }

    private fun deviceName(): String =
        Settings.Global.getString(getApplication<Application>().contentResolver, Settings.Global.DEVICE_NAME)
            ?.takeIf { it.isNotBlank() }
            ?: Build.MODEL

    private fun loadDesktop(): DesktopTarget? {
        val fingerprint = prefs.getString(KEY_FINGERPRINT, null) ?: return null
        val endpoints = prefs.getString(KEY_ENDPOINTS, null)?.split('\n')?.filter(String::isNotBlank)
        if (endpoints.isNullOrEmpty()) return null
        return DesktopTarget(prefs.getString(KEY_NAME, null) ?: endpoints.first(), endpoints, fingerprint)
    }

    private fun saveDesktop(desktop: DesktopTarget) {
        prefs.edit()
            .clear()
            .putString(KEY_NAME, desktop.name)
            .putString(KEY_ENDPOINTS, desktop.endpoints.joinToString("\n"))
            .putString(KEY_FINGERPRINT, desktop.fingerprint)
            .apply()
        _pairedDesktop.value = desktop
    }

    private companion object {
        const val KEY_NAME = "name"
        const val KEY_ENDPOINTS = "endpoints"
        const val KEY_FINGERPRINT = "fingerprint"
    }
}
