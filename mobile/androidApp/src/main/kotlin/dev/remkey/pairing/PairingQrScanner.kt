package dev.remkey.pairing

import android.content.Context
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

sealed interface ScanResult {
    /** The QR code's text; the bridge decides whether it is a pairing code. */
    data class Scanned(val text: String) : ScanResult
    data object Cancelled : ScanResult
    data class Failed(val message: String) : ScanResult
}

/**
 * Scans the pairing QR code the agent shows with `--pair`, using Google's code scanner:
 * Play services runs the camera screen, so the app needs no camera permission and never
 * sees a frame. Needs Google Play services; without them, paste the pairing data instead.
 */
object PairingQrScanner {
    private val options = GmsBarcodeScannerOptions.Builder()
        .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
        .build()

    /** [context] should be the current Activity. [onResult] runs on the main thread. */
    fun scan(context: Context, onResult: (ScanResult) -> Unit) {
        GmsBarcodeScanning.getClient(context, options)
            .startScan()
            .addOnSuccessListener { barcode ->
                val text = barcode.rawValue
                onResult(if (text.isNullOrBlank()) ScanResult.Failed("That QR code has no text") else ScanResult.Scanned(text))
            }
            .addOnCanceledListener { onResult(ScanResult.Cancelled) }
            .addOnFailureListener { e ->
                onResult(ScanResult.Failed(e.message ?: "The scanner isn't available; paste the pairing data instead"))
            }
    }
}
