package dev.remkey

import android.app.Application
import android.content.Context
import androidx.lifecycle.AndroidViewModel
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.platform.AndroidRelayClient
import dev.remkey.relay.RelayClient
import dev.remkey.relay.RelayCommand
import dev.remkey.relay.RelayConnectionState
import dev.remkey.touchpad.TouchpadGestures
import kotlinx.coroutines.flow.StateFlow

/**
 * Owns the connection so it survives rotation; composables only observe [state] and
 * forward input. Pairing isn't wired up yet, so the last manually entered desktop is
 * remembered instead.
 */
class RelayViewModel(app: Application) : AndroidViewModel(app) {
    private val client: RelayClient = AndroidRelayClient()
    private val prefs = app.getSharedPreferences("desktop", Context.MODE_PRIVATE)

    val state: StateFlow<RelayConnectionState> = client.state
    val gestures = TouchpadGestures(emit = client::send)

    var lastTarget: DesktopTarget? = prefs.getString(KEY_HOST, null)?.let { host ->
        DesktopTarget(host, prefs.getInt(KEY_PORT, DEFAULT_PORT), prefs.getString(KEY_FINGERPRINT, "").orEmpty())
    }
        private set

    fun connect(target: DesktopTarget) {
        lastTarget = target
        prefs.edit()
            .putString(KEY_HOST, target.host)
            .putInt(KEY_PORT, target.port)
            .putString(KEY_FINGERPRINT, target.fingerprint)
            .apply()
        client.connect(target)
    }

    fun send(command: RelayCommand) = client.send(command)

    fun disconnect() = client.disconnect()

    override fun onCleared() {
        client.disconnect()
        client.close()
    }

    companion object {
        const val DEFAULT_PORT = 8080
        private const val KEY_HOST = "host"
        private const val KEY_PORT = "port"
        private const val KEY_FINGERPRINT = "fingerprint"
    }
}
