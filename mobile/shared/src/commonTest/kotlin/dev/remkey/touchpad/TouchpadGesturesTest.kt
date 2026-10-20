package dev.remkey.touchpad

import dev.remkey.relay.PointerButton
import dev.remkey.relay.RelayCommand
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class TouchpadGesturesTest {
    private val sent = mutableListOf<RelayCommand>()
    private val gestures = TouchpadGestures(TouchpadSettings(pointerSpeed = 2f, dpPerScrollNotch = 10f)) { sent += it }

    @Test
    fun oneFingerDragMovesThePointer() {
        gestures.down(1, 0f, 0f, 0)
        gestures.move(1, 10f, -5f)
        gestures.up(1, 500)
        assertEquals(listOf<RelayCommand>(RelayCommand.PointerDelta(20f, -10f)), sent)
    }

    @Test
    fun quickTapClicksLeft() {
        gestures.down(1, 0f, 0f, 0)
        gestures.move(1, 1f, 1f)
        gestures.up(1, 100)
        assertEquals(PointerButton.Left, (sent[1] as RelayCommand.Button).button)
        assertEquals(listOf(true, false), sent.filterIsInstance<RelayCommand.Button>().map { it.pressed })
    }

    @Test
    fun slowTouchIsNotATap() {
        gestures.down(1, 0f, 0f, 0)
        gestures.up(1, 400)
        assertTrue(sent.isEmpty())
    }

    @Test
    fun twoFingerTapClicksRight() {
        gestures.down(1, 0f, 0f, 0)
        gestures.down(2, 50f, 0f, 10)
        gestures.up(1, 90)
        gestures.up(2, 100)
        assertEquals(
            listOf(RelayCommand.Button(PointerButton.Right, true), RelayCommand.Button(PointerButton.Right, false)),
            sent,
        )
    }

    @Test
    fun twoFingersScrollNaturallyAndNeverMoveThePointer() {
        gestures.down(1, 0f, 0f, 0)
        gestures.down(2, 50f, 0f, 0)
        // Both fingers move down by 10: content follows, so the wheel scrolls up.
        gestures.move(1, 0f, 10f)
        gestures.move(2, 50f, 10f)
        gestures.up(2, 300)
        gestures.move(1, 0f, 40f)
        gestures.up(1, 400)

        val scroll = sent.filterIsInstance<RelayCommand.Scroll>()
        assertEquals(1f, scroll.sumOf { it.dy.toDouble() }.toFloat())
        assertTrue(sent.none { it is RelayCommand.PointerDelta })
    }
}
