package dev.remkey

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.animation.Crossfade
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.remkey.relay.RelayConnectionState
import dev.remkey.ui.screens.ConnectScreen
import dev.remkey.ui.screens.TouchpadScreen
import dev.remkey.ui.theme.RemKeyTheme

class MainActivity : ComponentActivity() {
    private val relay: RelayViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            RemKeyTheme {
                Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
                    val state by relay.state.collectAsStateWithLifecycle()
                    val desktop by relay.pairedDesktop.collectAsStateWithLifecycle()
                    val codeError by relay.pairingCodeError.collectAsStateWithLifecycle()
                    Box(Modifier.safeDrawingPadding()) {
                        Crossfade(targetState = state == RelayConnectionState.Connected, label = "screen") { connected ->
                            if (connected) {
                                TouchpadScreen(
                                    desktopLabel = desktop?.label.orEmpty(),
                                    gestures = relay.gestures,
                                    onCommand = relay::send,
                                    onDisconnect = relay::disconnect,
                                )
                            } else {
                                ConnectScreen(
                                    state = state,
                                    desktop = desktop,
                                    pairingCodeError = codeError,
                                    onConnect = relay::connect,
                                    onPair = relay::pair,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}
