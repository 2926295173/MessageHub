package im.zyx.phonebridge.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import dagger.hilt.android.qualifiers.ApplicationContext
import im.zyx.phonebridge.core.crypto.CertGen
import im.zyx.phonebridge.core.crypto.Ecdh
import java.io.StringWriter
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.MessageDigest
import java.security.PrivateKey
import java.security.PublicKey
import java.security.cert.X509Certificate
import java.security.spec.ECGenParameterSpec
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.runBlocking
import org.bouncycastle.openssl.jcajce.JcaPEMWriter

private val Context.prefsDataStore by preferencesDataStore("phonebridge_prefs")

/**
 * Identity provider backed by the Android Keystore.
 *
 * The long-term ECDH P-256 keypair is generated and held inside
 * the Android Keystore (hardware-backed on devices with a TEE/
 * StrongBox; software-backed as a fallback). The private key is
 * therefore non-extractable: it never leaves the Keystore boundary.
 *
 * The X.509 self-signed certificate is built by BouncyCastle and
 * stored in DataStore (the cert is public; only the private key
 * needs Keystore protection). The cert + fingerprint are stable
 * across process restarts. After app uninstall, the Keystore
 * entry is wiped, and the next install generates a fresh
 * identity — that is the intended behavior.
 */
interface IdentityStore {
    /**
     * Synchronous get-or-create for the device id. Safe to call
     * from non-suspend contexts; uses runBlocking internally. The
     * result is stable across calls.
     */
    fun getOrCreateDeviceIdBlocking(): String

    /**
     * Get-or-create the long-term keypair. If the Keystore already
     * has an entry under [KEY_ALIAS], load it (regenerate the cert
     * from the current public key so it stays consistent). Otherwise
     * generate a new EC P-256 keypair in the Keystore and persist
     * a self-signed cert + fingerprint.
     */
    fun getOrCreateIdentityBlocking(commonName: String = "phonebridge-android"): IdentityWithKey
}

data class IdentityWithKey(
    val keyPair: KeyPair,
    val pem: String,
    val fingerprint: String
)

/**
 * Production implementation of [IdentityStore]:
 *  - private key   : Android Keystore (alias [KEY_ALIAS])
 *  - public cert   : PEM, persisted in DataStore
 *  - fingerprint   : colon-separated UPPERCASE hex SHA-256 of cert
 *  - device id     : UUIDv4 in DataStore
 */
@Singleton
class PrefsRepository @Inject constructor(
    @ApplicationContext private val context: Context
) : IdentityStore {

    private val keyDesktopHost = stringPreferencesKey("desktop_host")
    private val keyDesktopPort = stringPreferencesKey("desktop_port")
    private val keyFingerprint = stringPreferencesKey("fingerprint")
    private val keyDeviceId = stringPreferencesKey("device_id")
    private val keyIdentityPem = stringPreferencesKey("identity_pem")
    private val keyIdentityFingerprint = stringPreferencesKey("identity_fingerprint")
    // Settings
    private val keyDeviceName = stringPreferencesKey("device_name")
    private val keyTheme = stringPreferencesKey("theme_mode")
    private val keyPersistentNotif = booleanPreferencesKey("persistent_notification")
    private val keyTrustedSsids = stringPreferencesKey("trusted_ssids_csv")
    private val keyManualDesktops = stringPreferencesKey("manual_desktops_json")
    private val keyOnboarded = booleanPreferencesKey("onboarded_v1")
    // Keep-alive / hardening
    private val keyBatteryOptPrompted = booleanPreferencesKey("battery_opt_prompted_v1")
    private val keyFloatingEnabled = booleanPreferencesKey("floating_console_enabled")
    private val keyFloatingX = intPreferencesKey("floating_console_x")
    private val keyFloatingY = intPreferencesKey("floating_console_y")
    private val keySuppressedAlerts = stringPreferencesKey("suppressed_alerts_csv")

    val desktopHost: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopHost] }
    val desktopPort: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopPort] }
    val fingerprint: Flow<String?> = context.prefsDataStore.data.map { it[keyFingerprint] }
    val deviceId: Flow<String?> = context.prefsDataStore.data.map { it[keyDeviceId] }
    val identityFingerprint: Flow<String?> = context.prefsDataStore.data.map { it[keyIdentityFingerprint] }
    val deviceName: Flow<String?> = context.prefsDataStore.data.map { it[keyDeviceName] }
    val themeMode: Flow<String?> = context.prefsDataStore.data.map { it[keyTheme] }
    val persistentNotif: Flow<Boolean> = context.prefsDataStore.data.map { it[keyPersistentNotif] ?: true }
    val trustedSsidsCsv: Flow<String?> = context.prefsDataStore.data.map { it[keyTrustedSsids] }
    val manualDesktopsJson: Flow<String?> = context.prefsDataStore.data.map { it[keyManualDesktops] }
    val onboarded: Flow<Boolean> = context.prefsDataStore.data.map { it[keyOnboarded] ?: false }
    val batteryOptPrompted: Flow<Boolean> = context.prefsDataStore.data.map { it[keyBatteryOptPrompted] ?: false }
    val floatingEnabled: Flow<Boolean> = context.prefsDataStore.data.map { it[keyFloatingEnabled] ?: false }
    val floatingPos: Flow<Pair<Int, Int>> = context.prefsDataStore.data.map { p ->
        (p[keyFloatingX] ?: -1) to (p[keyFloatingY] ?: -1)
    }
    val suppressedAlertsCsv: Flow<String?> = context.prefsDataStore.data.map { it[keySuppressedAlerts] }

    suspend fun setDesktop(host: String, port: Int) {
        context.prefsDataStore.edit {
            it[keyDesktopHost] = host
            it[keyDesktopPort] = port.toString()
        }
    }

    suspend fun setFingerprint(hex: String) {
        context.prefsDataStore.edit { it[keyFingerprint] = hex }
    }

    suspend fun setDeviceName(name: String?) {
        context.prefsDataStore.edit {
            val v = name?.trim().orEmpty()
            if (v.isEmpty()) it.remove(keyDeviceName) else it[keyDeviceName] = v
        }
    }

    suspend fun setThemeMode(mode: String) {
        context.prefsDataStore.edit { it[keyTheme] = mode }
    }

    suspend fun setPersistentNotif(enabled: Boolean) {
        context.prefsDataStore.edit { it[keyPersistentNotif] = enabled }
    }

    suspend fun setTrustedSsidsCsv(csv: String?) {
        context.prefsDataStore.edit {
            val v = csv?.trim().orEmpty()
            if (v.isEmpty()) it.remove(keyTrustedSsids) else it[keyTrustedSsids] = v
        }
    }

    suspend fun setManualDesktopsJson(json: String?) {
        context.prefsDataStore.edit {
            val v = json?.trim().orEmpty()
            if (v.isEmpty()) it.remove(keyManualDesktops) else it[keyManualDesktops] = v
        }
    }

    suspend fun setOnboarded(v: Boolean) {
        context.prefsDataStore.edit { it[keyOnboarded] = v }
    }

    suspend fun setBatteryOptPrompted(v: Boolean) {
        context.prefsDataStore.edit { it[keyBatteryOptPrompted] = v }
    }

    suspend fun setFloatingEnabled(enabled: Boolean) {
        context.prefsDataStore.edit { it[keyFloatingEnabled] = enabled }
    }

    suspend fun setFloatingPos(x: Int, y: Int) {
        context.prefsDataStore.edit {
            it[keyFloatingX] = x
            it[keyFloatingY] = y
        }
    }

    suspend fun setSuppressedAlertsCsv(csv: String?) {
        context.prefsDataStore.edit {
            val v = csv?.trim().orEmpty()
            if (v.isEmpty()) it.remove(keySuppressedAlerts) else it[keySuppressedAlerts] = v
        }
    }

    private suspend fun saveCert(pem: String, fingerprint: String) {
        context.prefsDataStore.edit {
            it[keyIdentityPem] = pem
            it[keyIdentityFingerprint] = fingerprint
        }
    }

    private suspend fun loadCert(): Identity? {
        val data = context.prefsDataStore.data.first()
        val pem = data[keyIdentityPem] ?: return null
        val fp = data[keyIdentityFingerprint] ?: return null
        return Identity(pem, fp)
    }

    private data class Identity(val pem: String, val fingerprint: String)

    @Volatile private var cachedDeviceId: String? = null
    override fun getOrCreateDeviceIdBlocking(): String {
        cachedDeviceId?.let { return it }
        val id = runBlocking {
            val existing = context.prefsDataStore.data.map { it[keyDeviceId] }.first()
            if (existing != null) existing
            else {
                val n = UUID.randomUUID().toString()
                context.prefsDataStore.edit { it[keyDeviceId] = n }
                n
            }
        }
        cachedDeviceId = id
        return id
    }

    @Volatile private var cachedIdentity: IdentityWithKey? = null
    override fun getOrCreateIdentityBlocking(commonName: String): IdentityWithKey {
        cachedIdentity?.let { return it }

        val ks = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val result = if (ks.containsAlias(KEY_ALIAS)) {
            loadFromKeystore(ks, commonName)
        } else {
            generateIntoKeystore(ks, commonName)
        }
        cachedIdentity = result
        return result
    }

    /**
     * Read the cert we previously persisted in DataStore and only
     * regenerate it if its public key no longer matches the current
     * Keystore public key (which would only happen if the Keystore
     * entry was overwritten with a different key — e.g., restored
     * from a backup).
     *
     * The cert is stable across process restarts because we never
     * regenerate it; `notBefore` is a fresh timestamp on every call
     * to [CertGen.generateSelfSigned], so re-signing would change
     * the cert bytes and hence the SHA-256 fingerprint.
     */
    private fun loadFromKeystore(ks: KeyStore, commonName: String): IdentityWithKey {
        val entry = ks.getEntry(KEY_ALIAS, null) as KeyStore.PrivateKeyEntry
        val pub = entry.certificate.publicKey
        val priv = entry.privateKey
        val kp = KeyPair(pub, priv)

        val stored = runBlocking { loadCert() }
        if (stored != null && certMatchesPub(stored.pem, pub)) {
            return IdentityWithKey(kp, stored.pem, stored.fingerprint)
        }
        // Stored cert is missing or stale (pubkey mismatch). Regenerate
        // and overwrite.
        val cert = CertGen.generateSelfSigned(commonName, kp, validityDays = 3650)
        runBlocking { saveCert(cert.pem, cert.fingerprint) }
        return IdentityWithKey(kp, cert.pem, cert.fingerprint)
    }

    /**
     * Verify that the public key in [pem] matches [expectedPub]. We
     * do this by parsing the PEM, extracting the SubjectPublicKeyInfo,
     * and comparing the EC point coordinates.
     */
    private fun certMatchesPub(pem: String, expectedPub: PublicKey): Boolean {
        return runCatching {
            val cert = readCertFromPem(pem) ?: return false
            val actual = cert.publicKey as? java.security.interfaces.ECPublicKey
                ?: return false
            val e = expectedPub as? java.security.interfaces.ECPublicKey
                ?: return false
            actual.w.affineX == e.w.affineX && actual.w.affineY == e.w.affineY
        }.getOrDefault(false)
    }

    private fun readCertFromPem(pem: String): X509Certificate? {
        return runCatching {
            val cf = java.security.cert.CertificateFactory.getInstance("X.509")
            cf.generateCertificate(pem.byteInputStream()) as? X509Certificate
        }.getOrNull()
    }

    private fun generateIntoKeystore(ks: KeyStore, commonName: String): IdentityWithKey {
        // Generate the keypair INSIDE the Keystore. The private key
        // is non-extractable.
        val kpg = KeyPairGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_EC,
            ANDROID_KEYSTORE
        )
        val spec = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY or
                KeyProperties.PURPOSE_AGREE_KEY
        )
            .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .build()
        kpg.initialize(spec)
        val kp = kpg.generateKeyPair()

        // Sign the cert with the Keystore-backed private key. Bouncy
        // Castle hands the signing op back to the JCA, which routes
        // to the AndroidKeyStore provider. The resulting signature
        // is produced inside the TEE where available.
        val cert = CertGen.generateSelfSigned(commonName, kp, validityDays = 3650)
        runBlocking { saveCert(cert.pem, cert.fingerprint) }
        return IdentityWithKey(kp, cert.pem, cert.fingerprint)
    }

    companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEY_ALIAS = "phonebridge.identity.v1"
    }
}
