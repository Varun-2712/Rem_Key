package dev.remkey.touchpad

import dev.remkey.relay.PointerButton
import dev.remkey.relay.RelayCommand
import kotlin.math.abs

/** Distances are in dp so the touchpad feels the same on every screen density. */
data class TouchpadSettings(
    /** Desktop pixels per dp of finger travel. The desktop applies its own acceleration. */
    val pointerSpeed: Float = 1.5f,
    /** Two-finger travel that makes one wheel notch. */
    val dpPerScrollNotch: Float = 14f,
    /** Movement under this still counts as a tap. */
    val tapSlopDp: Float = 8f,
    /** Same tap timeout as libinput. */
    val tapTimeoutMillis: Long = 180,
)

/**
 * Turns raw touches into relay commands: one finger moves the pointer, two fingers
 * scroll naturally (content follows the fingers), and a quick tap with one or two fingers
 * is a left or right click.
 *
 * Platform UI code feeds it pointer events; it has no platform dependencies so Android
 * and iOS share the same feel.
 */
class TouchpadGestures(
    private val settings: TouchpadSettings = TouchpadSettings(),
    private val emit: (RelayCommand) -> Unit,
) {
    private class Finger(var x: Float, var y: Float)

    private val fingers = mutableMapOf<Long, Finger>()
    private var gestureStart = 0L
    private var maxFingers = 0
    private var travel = 0f

    fun down(id: Long, x: Float, y: Float, timeMillis: Long) {
        if (fingers.isEmpty()) {
            gestureStart = timeMillis
            maxFingers = 0
            travel = 0f
        }
        fingers[id] = Finger(x, y)
        maxFingers = maxOf(maxFingers, fingers.size)
    }

    fun move(id: Long, x: Float, y: Float) {
        val finger = fingers[id] ?: return
        val dx = x - finger.x
        val dy = y - finger.y
        finger.x = x
        finger.y = y
        travel += abs(dx) + abs(dy)

        when {
            // Once a second finger has been down, lifting it mustn't turn the rest of the
            // gesture into pointer motion.
            maxFingers == 1 -> emit(RelayCommand.PointerDelta(dx * settings.pointerSpeed, dy * settings.pointerSpeed))
            fingers.size == 2 -> {
                // Each finger reports its own move; halve so two fingers scroll like one.
                val notch = settings.dpPerScrollNotch * 2
                emit(RelayCommand.Scroll(dx = -dx / notch, dy = dy / notch))
            }
        }
    }

    fun up(id: Long, timeMillis: Long) {
        if (fingers.remove(id) == null || fingers.isNotEmpty()) return
        val quick = timeMillis - gestureStart <= settings.tapTimeoutMillis
        if (quick && travel < settings.tapSlopDp) {
            val button = if (maxFingers >= 2) PointerButton.Right else PointerButton.Left
            emit(RelayCommand.Button(button, pressed = true))
            emit(RelayCommand.Button(button, pressed = false))
        }
    }

    /** The system took the gesture away (e.g. a navigation swipe); forget it without clicking. */
    fun cancel() {
        fingers.clear()
    }
}
