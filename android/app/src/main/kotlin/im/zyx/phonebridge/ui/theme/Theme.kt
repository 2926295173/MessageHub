package im.zyx.phonebridge.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * PhoneBridge palette, modeled on the KDE Connect reference: a
 * very pale warm pinkish-white surface, deep charcoal foreground,
 * muted brownish-gray for sub-text. The dark variant is a quiet
 * charcoal-tinted night mode that keeps the same hue family so
 * status icons (battery, network) don't jump hue between modes.
 */
private val Light = lightColorScheme(
    primary           = Color(0xFF2C292A),
    onPrimary         = Color(0xFFFFF2F4),
    primaryContainer  = Color(0xFFEAE0E2),
    onPrimaryContainer= Color(0xFF2C292A),
    secondary         = Color(0xFF2C292A),
    onSecondary       = Color(0xFFFFF2F4),
    background        = Color(0xFFFFF2F4),
    onBackground      = Color(0xFF2C292A),
    surface           = Color(0xFFFFF2F4),
    onSurface         = Color(0xFF2C292A),
    surfaceVariant    = Color(0xFFF3E7E9),
    onSurfaceVariant  = Color(0xFF8C8486),
    surfaceContainerLowest = Color(0xFFFFFFFF),
    surfaceContainerLow    = Color(0xFFFCF6F7),
    surfaceContainer       = Color(0xFFFFFFFF),
    surfaceContainerHigh   = Color(0xFFF8ECEE),
    surfaceContainerHighest= Color(0xFFF3E7E9),
    outline           = Color(0xFFB3A8AA),
    outlineVariant    = Color(0xFFE5D9DB),
    error             = Color(0xFFB3261E),
    onError           = Color(0xFFFFFFFF),
    scrim             = Color(0x4A4A4A4A),
)

private val Dark = darkColorScheme(
    primary           = Color(0xFFEAE0E2),
    onPrimary         = Color(0xFF1F1D1E),
    primaryContainer  = Color(0xFF3A3738),
    onPrimaryContainer= Color(0xFFEAE0E2),
    secondary         = Color(0xFFEAE0E2),
    onSecondary       = Color(0xFF1F1D1E),
    background        = Color(0xFF1B191A),
    onBackground      = Color(0xFFEAE0E2),
    surface           = Color(0xFF1F1D1E),
    onSurface         = Color(0xFFEAE0E2),
    surfaceVariant    = Color(0xFF2C292A),
    onSurfaceVariant  = Color(0xFFB3A8AA),
    surfaceContainerLowest = Color(0xFF100F10),
    surfaceContainerLow    = Color(0xFF161415),
    surfaceContainer       = Color(0xFF1F1D1E),
    surfaceContainerHigh   = Color(0xFF262324),
    surfaceContainerHighest= Color(0xFF2C292A),
    outline           = Color(0xFF5C5456),
    outlineVariant    = Color(0xFF3A3738),
    error             = Color(0xFFF2B8B5),
    onError           = Color(0xFF601410),
    scrim             = Color(0x99000000),
)

/** User-selectable theme modes. Persisted by [PrefsRepository]. */
enum class ThemeMode(val persisted: String) {
    System("system"),
    Light("light"),
    Dark("dark");

    companion object {
        fun fromPersisted(s: String?): ThemeMode = when (s) {
            "light" -> Light
            "dark" -> Dark
            else -> System
        }
    }
}

@Composable
fun PhoneBridgeTheme(
    mode: ThemeMode = ThemeMode.System,
    content: @Composable () -> Unit
) {
    val darkTheme = when (mode) {
        ThemeMode.System -> isSystemInDarkTheme()
        ThemeMode.Light -> false
        ThemeMode.Dark -> true
    }
    MaterialTheme(
        colorScheme = if (darkTheme) Dark else Light,
        content = content
    )
}
