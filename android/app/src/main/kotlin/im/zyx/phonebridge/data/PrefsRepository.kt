package im.zyx.phonebridge.data

import android.content.Context
import android.util.Base64
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import dagger.hilt.android.qualifiers.ApplicationContext
import im.zyx.phonebridge.core.crypto.CertGen
import im.zyx.phonebridge.core.crypto.Ecdh
import java.security.KeyFactory
import java.security.KeyPair
import java.security.spec.PKCS8EncodedKeySpec
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.runBlocking

private val Context.prefsDataStore by preferencesDataStore("phonebridge_prefs")

/**
 * Identity provider. The real implementation is [PrefsRepository]
 * (DataStore-backed). The interface lets unit tests inject an
 * in-memory implementation without needing a real Android Context.
 */
interface IdentityStore {
    /**
     * Synchronous get-or-create for the device id. Safe to call from
     * non-suspend contexts; uses runBlocking internally. The result
     * is stable across calls.
     */
    fun getOrCreateDeviceIdBlocking(): String

    /**
     * Get-or-create the long-term keypair. If an existing identity
     * is persisted, load it. Otherwise generate a new self-signed
     * P-256 cert and persist it.
     */
    fun getOrCreateIdentityBlocking(commonName: String = "phonebridge-android"): IdentityWithKey
}

data class IdentityWithKey(
    val keyPair: KeyPair,
    val pem: String,
    val fingerprint: String
)

/**
 * Persisted settings + the device's long-term identity.
 *
 * Schema (DataStore):
 *   - `desktop_host`, `desktop_port`: last successful NSD/manual target
 *   - `fingerprint`: SHA-256 fingerprint of the daemon's TLS cert
 *   - `device_id`: stable UUIDv4 assigned at first start, persisted
 *   - `identity_pem`, `identity_pkcs8_b64`, `identity_spki_b64`,
 *     `identity_fingerprint`: the long-term X.509 cert (PEM) plus
 *     PKCS#8 private key and X.509 SPKI public key (base64), plus
 *     the cert's SHA-256 fingerprint. Persisted so the device's
 *     `device.hello.pubkey` is stable across restarts.
 *
 * The long-term private key is stored in plain base64 in DataStore
 * (encrypted at rest by the OS file system). M5+ future work: move
 * the private key to Android Keystore (hardware-backed where
 * available).
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
    private val keyIdentityPkcs8 = stringPreferencesKey("identity_pkcs8_b64")
    private val keyIdentitySpki = stringPreferencesKey("identity_spki_b64")
    private val keyIdentityFingerprint = stringPreferencesKey("identity_fingerprint")

    val desktopHost: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopHost] }
    val desktopPort: Flow<String?> = context.prefsDataStore.data.map { it[keyDesktopPort] }
    val fingerprint: Flow<String?> = context.prefsDataStore.data.map { it[keyFingerprint] }
    val deviceId: Flow<String?> = context.prefsDataStore.data.map { it[keyDeviceId] }
    val identityFingerprint: Flow<String?> = context.prefsDataStore.data.map { it[keyIdentityFingerprint] }

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

    private suspend fun saveIdentity(pem: String, pkcs8: ByteArray, spki: ByteArray, fingerprint: String) {
        context.prefsDataStore.edit {
            it[keyIdentityPem] = pem
            it[keyIdentityPkcs8] = Base64.encodeToString(pkcs8, Base64.NO_WRAP)
            it[keyIdentitySpki] = Base64.encodeToString(spki, Base64.NO_WRAP)
            it[keyIdentityFingerprint] = fingerprint
        }
    }

    private suspend fun loadIdentity(): Identity? {
        val data = context.prefsDataStore.data.first()
        val pem = data[keyIdentityPem] ?: return null
        val pkcs8B64 = data[keyIdentityPkcs8] ?: return null
        val spkiB64 = data[keyIdentitySpki] ?: return null
        val fp = data[keyIdentityFingerprint] ?: return null
        val pkcs8 = Base64.decode(pkcs8B64, Base64.NO_WRAP)
        val spki = Base64.decode(spkiB64, Base64.NO_WRAP)
        return Identity(pem, pkcs8, spki, fp)
    }

    private data class Identity(
        val pem: String,
        val pkcs8: ByteArray,
        val spki: ByteArray,
        val fingerprint: String
    )

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
        val result = runBlocking {
            val existing = loadIdentity()
            if (existing != null) {
                val kf = KeyFactory.getInstance("EC")
                val priv = kf.generatePrivate(PKCS8EncodedKeySpec(existing.pkcs8))
                val pub = kf.generatePublic(java.security.spec.X509EncodedKeySpec(existing.spki))
                IdentityWithKey(KeyPair(pub, priv), existing.pem, existing.fingerprint)
            } else {
                val kp = Ecdh.generateKeyPair()
                val cert = CertGen.generateSelfSigned(commonName, kp, validityDays = 3650)
                val spki = kp.public.encoded
                val pkcs8 = kp.private.encoded
                saveIdentity(cert.pem, pkcs8, spki, cert.fingerprint)
                IdentityWithKey(kp, cert.pem, cert.fingerprint)
            }
        }
        cachedIdentity = result
        return result
    }
}
