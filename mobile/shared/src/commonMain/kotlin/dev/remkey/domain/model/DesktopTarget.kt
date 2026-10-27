package dev.remkey.domain.model

/**
 * A desktop agent the phone is paired with, as learned from its pairing code.
 * [endpoints] are `host:port` addresses tried in order; [fingerprint] is the SHA-256 of the
 * agent's certificate as hex, and the connection is refused if it doesn't match.
 */
data class DesktopTarget(
    val name: String,
    val endpoints: List<String>,
    val fingerprint: String,
) {
    val label: String get() = name
}

/** The content of a desktop's pairing QR code: the desktop plus its one-time [secret]. */
data class PairingCode(
    val desktop: DesktopTarget,
    val secret: String,
)
