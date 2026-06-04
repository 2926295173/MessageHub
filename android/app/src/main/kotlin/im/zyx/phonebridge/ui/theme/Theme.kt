package im.zyx.phonebridge.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val Light = lightColorScheme(
    primary = Color(0xFF1F6FEB),
    onPrimary = Color.White,
    background = Color(0xFFF7F8FA),
    surface = Color(0xFFFFFFFF),
    onSurface = Color(0xFF101319),
    onBackground = Color(0xFF101319)
)

private val Dark = darkColorScheme(
    primary = Color(0xFF58A6FF),
    onPrimary = Color(0xFF0D1117),
    background = Color(0xFF0D1117),
    surface = Color(0xFF161B22),
    onSurface = Color(0xFFE6EDF3),
    onBackground = Color(0xFFE6EDF3)
)

@Composable
fun PhoneBridgeTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit
) {
    MaterialTheme(
        colorScheme = if (darkTheme) Dark else Light,
        content = content
    )
}
