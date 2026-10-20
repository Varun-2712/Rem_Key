package dev.remkey.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/*
 * Scandinavian: warm off-whites and soft greys instead of pure white and black, one quiet
 * accent, flat surfaces separated by tone rather than shadow, light type with generous
 * spacing, and nothing on screen that doesn't do a job.
 *
 * Quiet doesn't mean faint: every text pairing here clears WCAG AA (4.5:1), so secondary
 * text uses darker tones rather than transparency.
 */

private object Palette {
    val Linen = Color(0xFFF6F4F0)       // page
    val Birch = Color(0xFFEDE9E3)       // touchpad and fields
    val Oat = Color(0xFFE2DCD3)         // hairlines, pressed surfaces
    val Charcoal = Color(0xFF2E2D2B)    // text, never pure black
    val Stone = Color(0xFF625D57)       // secondary text
    val Fjord = Color(0xFF4E6B7C)       // the one accent
    val Moss = Color(0xFF7D9478)        // connected
    val Clay = Color(0xFFA0553F)        // errors, muted

    val Night = Color(0xFF1C1D1B)
    val Slate = Color(0xFF262725)
    val Graphite = Color(0xFF34352F)
    val Bone = Color(0xFFE9E5DF)
    val Ash = Color(0xFFA6A197)
    val FjordLight = Color(0xFF8FAABA)
    val MossLight = Color(0xFF9DB497)
    val ClayLight = Color(0xFFD08A74)
}

private val LightColors = lightColorScheme(
    primary = Palette.Fjord,
    onPrimary = Palette.Linen,
    secondary = Palette.Moss,
    onSecondary = Palette.Linen,
    background = Palette.Linen,
    onBackground = Palette.Charcoal,
    surface = Palette.Linen,
    onSurface = Palette.Charcoal,
    surfaceVariant = Palette.Birch,
    onSurfaceVariant = Palette.Stone,
    surfaceContainerHighest = Palette.Birch,
    outline = Palette.Oat,
    outlineVariant = Palette.Oat,
    error = Palette.Clay,
    onError = Palette.Linen,
)

private val DarkColors = darkColorScheme(
    primary = Palette.FjordLight,
    onPrimary = Palette.Night,
    secondary = Palette.MossLight,
    onSecondary = Palette.Night,
    background = Palette.Night,
    onBackground = Palette.Bone,
    surface = Palette.Night,
    onSurface = Palette.Bone,
    surfaceVariant = Palette.Slate,
    onSurfaceVariant = Palette.Ash,
    surfaceContainerHighest = Palette.Slate,
    outline = Palette.Graphite,
    outlineVariant = Palette.Graphite,
    error = Palette.ClayLight,
    onError = Palette.Night,
)

private val Sans = FontFamily.SansSerif

private val RemKeyTypography = Typography(
    displaySmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Light, fontSize = 34.sp, lineHeight = 40.sp, letterSpacing = (-0.5).sp),
    titleMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 17.sp, lineHeight = 24.sp),
    bodyLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 16.sp, lineHeight = 24.sp),
    bodyMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 14.sp, lineHeight = 20.sp),
    labelLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Medium, fontSize = 15.sp, letterSpacing = 0.3.sp),
    labelMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Medium, fontSize = 12.sp, letterSpacing = 1.2.sp),
)

private val RemKeyShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(16.dp),
    large = RoundedCornerShape(24.dp),
    extraLarge = RoundedCornerShape(28.dp),
)

/** Spacing scale. Generous by default; whitespace is what keeps the screens calm. */
object Space {
    val xs = 4.dp
    val s = 8.dp
    val m = 16.dp
    val l = 24.dp
    val xl = 40.dp
}

/** Colour for "connected", outside Material's roles. */
val ColorScheme.connected: Color get() = secondary

@Composable
fun RemKeyTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = if (dark) DarkColors else LightColors,
        typography = RemKeyTypography,
        shapes = RemKeyShapes,
        content = content,
    )
}
