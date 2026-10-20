package dev.remkey.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import dev.remkey.RelayViewModel
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.relay.RelayConnectionState
import dev.remkey.ui.theme.RemKeyTheme
import dev.remkey.ui.theme.Space

/**
 * Manual connection until discovery and QR pairing land: the agent prints its address
 * and certificate fingerprint when it starts.
 */
@Composable
fun ConnectScreen(
    state: RelayConnectionState,
    initial: DesktopTarget?,
    onConnect: (DesktopTarget) -> Unit,
) {
    var host by rememberSaveable { mutableStateOf(initial?.host.orEmpty()) }
    var port by rememberSaveable { mutableStateOf((initial?.port ?: RelayViewModel.DEFAULT_PORT).toString()) }
    var fingerprint by rememberSaveable { mutableStateOf(initial?.fingerprint.orEmpty()) }
    val connecting = state == RelayConnectionState.Connecting
    val portNumber = port.toIntOrNull()?.takeIf { it in 1..65535 }
    val canConnect = !connecting && host.isNotBlank() && portNumber != null && fingerprint.isNotBlank()

    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .imePadding()
            .padding(horizontal = Space.l, vertical = Space.xl),
    ) {
        Spacer(Modifier.height(Space.xl))
        Text("Rem Key", style = MaterialTheme.typography.displaySmall, color = MaterialTheme.colorScheme.onBackground)
        Spacer(Modifier.height(Space.s))
        Text(
            "Use this phone as a touchpad and keyboard for your computer.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(Modifier.height(Space.xl + Space.m))
        SectionLabel("COMPUTER")
        Spacer(Modifier.height(Space.s))
        Row(horizontalArrangement = Arrangement.spacedBy(Space.s)) {
            QuietField(
                value = host,
                onValueChange = { host = it },
                placeholder = "192.168.1.50",
                keyboardType = KeyboardType.Uri,
                modifier = Modifier.weight(1f),
            )
            QuietField(
                value = port,
                onValueChange = { port = it.filter(Char::isDigit).take(5) },
                placeholder = "Port",
                keyboardType = KeyboardType.Number,
                modifier = Modifier.width(96.dp),
            )
        }

        Spacer(Modifier.height(Space.l))
        SectionLabel("FINGERPRINT")
        Spacer(Modifier.height(Space.s))
        QuietField(
            value = fingerprint,
            onValueChange = { fingerprint = it.trim() },
            placeholder = "Shown by the agent when it starts",
            keyboardType = KeyboardType.Ascii,
            imeAction = ImeAction.Done,
            singleLine = false,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(Space.xl))
        Button(
            onClick = { onConnect(DesktopTarget(host.trim(), portNumber!!, fingerprint)) },
            enabled = canConnect,
            shape = MaterialTheme.shapes.medium,
            elevation = null,
            colors = ButtonDefaults.buttonColors(
                disabledContainerColor = MaterialTheme.colorScheme.surfaceVariant,
                disabledContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            ),
            modifier = Modifier.fillMaxWidth().height(56.dp),
        ) {
            Text(if (connecting) "Connecting…" else "Connect", style = MaterialTheme.typography.labelLarge)
        }

        if (state is RelayConnectionState.Failed) {
            Spacer(Modifier.height(Space.m))
            Text(state.message, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
        }
    }
}

@Composable
internal fun SectionLabel(text: String) {
    Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
}

/** A flat, borderless field: tone separates it from the page, not an outline. */
@Composable
internal fun QuietField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    modifier: Modifier = Modifier,
    keyboardType: KeyboardType = KeyboardType.Text,
    imeAction: ImeAction = ImeAction.Next,
    singleLine: Boolean = true,
    autoCorrect: Boolean = false,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
) {
    val colors = MaterialTheme.colorScheme
    TextField(
        value = value,
        onValueChange = onValueChange,
        placeholder = { Text(placeholder, color = colors.onSurfaceVariant.copy(alpha = 0.7f)) },
        singleLine = singleLine,
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType, imeAction = imeAction, autoCorrectEnabled = autoCorrect),
        keyboardActions = keyboardActions,
        shape = MaterialTheme.shapes.small,
        textStyle = MaterialTheme.typography.bodyLarge,
        colors = TextFieldDefaults.colors(
            focusedContainerColor = colors.surfaceVariant,
            unfocusedContainerColor = colors.surfaceVariant,
            focusedIndicatorColor = Color.Transparent,
            unfocusedIndicatorColor = Color.Transparent,
            cursorColor = colors.primary,
        ),
        modifier = modifier,
    )
}

@Preview(showBackground = true)
@Composable
private fun ConnectScreenPreview() {
    RemKeyTheme { ConnectScreen(RelayConnectionState.Disconnected, null) {} }
}
