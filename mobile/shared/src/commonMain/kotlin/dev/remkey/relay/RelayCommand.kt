package dev.remkey.relay

/** What the UI asks the desktop to do. Mapped onto the wire protocol by the Rust bridge. */
sealed interface RelayCommand {
    /** Relative movement in desktop pixels; fractions are kept and accumulated. */
    data class PointerDelta(val dx: Float, val dy: Float) : RelayCommand

    /** Scroll in wheel notches, wheel convention: positive [dy] scrolls up, positive [dx] right. */
    data class Scroll(val dx: Float, val dy: Float) : RelayCommand

    data class Button(val button: PointerButton, val pressed: Boolean) : RelayCommand

    data class Key(val key: EditingKey, val pressed: Boolean) : RelayCommand

    /** Text from the phone keyboard or dictation, typed on the desktop as-is. */
    data class CommitText(val text: String) : RelayCommand
}

enum class PointerButton { Left, Right, Middle }

/** Keys that have no text of their own. Printable characters go through [RelayCommand.CommitText]. */
enum class EditingKey {
    Enter, Backspace, Delete, Tab, Escape,
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,
}
