package im.zyx.phonebridge.data

import android.content.Context
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.prefsDataStore by preferencesDataStore("phonebridge_prefs")

/**
 * Persisted settings. We keep it deliberately small for the MVP:
 *   - last known desktop host:port (so we can reconnect after a restart)
 *   - last pinned certificate SHA-256 fingerprint (hex, no colons)
 *   - last device id we registered with the daemon
 */
@Singleton
class PrefsRepository @Inject constructor(
    @ApplicationContext private val context: Context
) {
    private val keyDesktopHost = stringPreferencesKey("desktop_host")
    private val keyDesktopPort = stringPreferencesKey("desktop_port")
    private val keyFingerprint = stringPreferencesKey("fingerprint")
    private val keyDeviceId = stringPreferencesKey("device_id")

    val desktopHost: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopHost] }
    val desktopPort: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopPort] }
    val fingerprint: Flow<String?> = context.prefsDataStore.data.map { it[keyFingerprint] }
    val deviceId: Flow<String?> = context.prefsDataStore.data.map { it[keyDeviceId] }

    suspend fun setDesktop(host: String, port: Int) {
        context.prefsDataStore.edit {
            it[keyDesktopHost] = host
            it[keyDesktopPort] = port.toString()
        }
    }

    suspend fun setFingerprint(hex: String) {
        context.prefsDataStore.edit { it[keyFingerprint] = hex }
    }

    suspend fun setDeviceId(id: String) {
        context.prefsDataStore.edit { it[keyDeviceId] = id }
    }
}
