package dev.remkey.ui.screens

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import dev.remkey.domain.model.DesktopTarget
import dev.remkey.pairing.PairingQrScanner
import dev.remkey.pairing.ScanResult
import dev.remkey.relay.RelayConnectionState
import dev.remkey.ui.theme.RemKeyTheme
import dev.remkey.ui.theme.Space

/**
 * Connects to the paired computer, or pairs a new one by scanning the QR code the agent
 * shows with `--pair`. The same code can be pasted: the agent prints it as "Pairing data".
 */
@Composable
fun ConnectScreen(
    state: RelayConnectionState,
    desktop: DesktopTarget?,
    pairingCodeError: String?,
    onConnect: () -> Unit,
    onPair: (String) -> Unit,
) {
    var pairingNew by rememberSaveable { mutableStateOf(false) }
    var code by rememberSaveable { mutableStateOf("") }
    var scanError by rememberSaveable { mutableStateOf<String?>(null) }
    val context = LocalContext.current
    // A newly paired computer replaces the old one; show it rather than the pairing form.
    LaunchedEffect(desktop) { pairingNew = false }
    val connecting = state == RelayConnectionState.Connecting
    val showPairing = desktop == null || pairingNew

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
        if (!showPairing && desktop != null) {
            SectionLabel("COMPUTER")
            Spacer(Modifier.height(Space.s))
            Text(desktop.name, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onBackground)
            Text(
                desktop.endpoints.joinToString(", "),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(Space.xl))
            PrimaryButton(if (connecting) "Connecting…" else "Connect", enabled = !connecting, onClick = onConnect)
            Spacer(Modifier.height(Space.s))
            TextButton(onClick = { pairingNew = true }, enabled = !connecting) {
                Text("Pair a different computer", style = MaterialTheme.typography.labelLarge)
            }
        } else {
            SectionLabel("PAIR")
            Spacer(Modifier.height(Space.s))
            Text(
                "On the computer, start the agent with --pair, then scan the code it shows.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(Space.l))
            PrimaryButton(if (connecting) "Pairing…" else "Scan QR code", enabled = !connecting) {
                scanError = null
                PairingQrScanner.scan(context) { result ->
                    when (result) {
                        is ScanResult.Scanned -> onPair(result.text)
                        is ScanResult.Failed -> scanError = result.message
                        ScanResult.Cancelled -> Unit
                    }
                }
            }

            Spacer(Modifier.height(Space.xl))
            SectionLabel("OR PASTE THE PAIRING DATA")
            Spacer(Modifier.height(Space.s))
            QuietField(
                value = code,
                onValueChange = { code = it },
                placeholder = "{\"version\":2, …}",
                keyboardType = KeyboardType.Ascii,
                imeAction = ImeAction.Done,
                singleLine = false,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(Space.s))
            TextButton(onClick = { onPair(code) }, enabled = !connecting && code.isNotBlank()) {
                Text("Pair with pasted code", style = MaterialTheme.typography.labelLarge)
            }
            if (desktop != null) {
                Spacer(Modifier.height(Space.s))
                TextButton(onClick = { pairingNew = false }, enabled = !connecting) {
                    Text("Back to ${desktop.name}", style = MaterialTheme.typography.labelLarge)
                }
            }
        }

        val error = (scanError ?: pairingCodeError).takeIf { showPairing } ?: (state as? RelayConnectionState.Failed)?.message
        if (error != null) {
            Spacer(Modifier.height(Space.m))
            Text(error, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
        }
    }
}

@Composable
private fun PrimaryButton(text: String, enabled: Boolean, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        enabled = enabled,
        shape = MaterialTheme.shapes.medium,
        elevation = null,
        colors = ButtonDefaults.buttonColors(
            disabledContainerColor = MaterialTheme.colorScheme.surfaceVariant,
            disabledContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
        ),
        modifier = Modifier.fillMaxWidth().height(56.dp),
    ) {
        Text(text, style = MaterialTheme.typography.labelLarge)
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
        placeholder = { Text(placeholder, color = colors.onSurfaceVariant) },
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
    RemKeyTheme {
        ConnectScreen(
            state = RelayConnectionState.Disconnected,
            desktop = DesktopTarget("Studio PC", listOf("192.168.1.50:8080"), "ab".repeat(32)),
            pairingCodeError = null,
            onConnect = {},
            onPair = {},
        )
    }
}
