package dev.remkey.domain.model

/**
 * A desktop agent the phone can connect to. [fingerprint] is the SHA-256 of the agent's
 * certificate as hex, delivered by pairing; the connection is refused if it doesn't match.
 */
data class DesktopTarget(
    val host: String,
    val port: Int,
    val fingerprint: String,
) {
    val label: String get() = "$host:$port"
}
