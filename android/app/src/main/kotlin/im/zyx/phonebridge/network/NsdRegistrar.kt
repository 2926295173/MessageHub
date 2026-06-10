package im.zyx.phonebridge.network

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.util.Log
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.callbackFlow

private const val TAG = "Nsd"

/**
 * One PhoneBridge desktop discovered on the LAN.
 *
 * @param name Service name as advertised (e.g. the message-center's configured
 *   display name). May be `null` for the brief moment between
 *   `onServiceFound` and the first successful resolve; in that case we
 *   substitute a placeholder so the UI never sees a null label.
 * @param host Resolved IPv4/IPv6 address. `null` until the first
 *   successful `resolveService` for this entry.
 * @param port Resolved port. `0` until the first successful resolve.
 * @param isResolving `true` while the resolve callback is in flight.
 *   The list-merging logic keeps the entry in "resolving" state until
 *   the host/port arrive, so the UI can show a spinner / dimmed row.
 */
data class DiscoveredDesktop(
    val instanceName: String,
    val name: String,
    val host: String?,
    val port: Int,
    val isResolving: Boolean,
)

/**
 * Browses the LAN for `_phonebridge._tcp.` services.
 *
 * Two flows are exposed:
 *  - [discoverFirstDesktop]: emits just the first desktop it sees, then
 *    closes (used by the foreground service to auto-pick a default).
 *  - [discoverDesktops] (a [StateFlow]): accumulates all found
 *    desktops for the lifetime of the screen, deduplicating on
 *    `instanceName`. The screen calls this when the user pulls to
 *    refresh.
 *
 * NsdManager requires `CHANGE_WIFI_MULTICAST_STATE`. We require it in
 * the manifest and grant it at runtime through the main UI before the
 * user starts pairing.
 */
@Singleton
class NsdRegistrar @Inject constructor(
    @ApplicationContext private val context: Context
) {
    private val nsd: NsdManager by lazy {
        context.getSystemService(Context.NSD_SERVICE) as NsdManager
    }

    /** All desktops currently visible on the LAN, keyed by instance name. */
    private val _desktops = MutableStateFlow<Map<String, DiscoveredDesktop>>(emptyMap())
    val desktops: StateFlow<Map<String, DiscoveredDesktop>> = _desktops.asStateFlow()

    /**
     * Suspends until a PhoneBridge message-center is found, then emits its
     * (host, port). Closes if the user navigates away.
     */
    fun discoverFirstDesktop(): Flow<Pair<String, Int>> = callbackFlow {
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {
                Log.d(TAG, "discovery started for $regType")
            }
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "discovery start failed: $errorCode")
                close()
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "discovery stop failed: $errorCode")
            }
            override fun onServiceFound(service: NsdServiceInfo) {
                Log.d(TAG, "service found: ${service.serviceName}")
                if (!service.serviceType.startsWith(SERVICE_TYPE)) return
                nsd.resolveService(service, object : NsdManager.ResolveListener {
                    override fun onResolveFailed(s: NsdServiceInfo, code: Int) {
                        Log.w(TAG, "resolve failed: $code")
                    }
                    override fun onServiceResolved(s: NsdServiceInfo) {
                        val host = s.host?.hostAddress ?: return
                        val port = s.port
                        Log.d(TAG, "resolved $host:$port (${s.serviceName})")
                        trySend(host to port)
                    }
                })
            }
            override fun onServiceLost(service: NsdServiceInfo) {
                Log.d(TAG, "service lost: ${service.serviceName}")
            }
        }
        nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        awaitClose { nsd.stopServiceDiscovery(listener) }
    }

    /**
     * Start (or restart) a discovery session. While the caller's
     * coroutine is active, every desktop found on the LAN is merged
     * into [desktops]; when the coroutine is cancelled (e.g. the
     * user navigates away), NSD is stopped.
     *
     * Calling this multiple times restarts discovery — the result is
     * "the latest call wins". Entries already in [desktops] from a
     * prior session are kept (so a second pull-to-refresh doesn't
     * empty the list during the few hundred ms it takes for the
     * resolver to re-populate it).
     */
    fun discoverDesktops(): Flow<Unit> = callbackFlow {
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {
                Log.d(TAG, "discoverDesktops: started $regType")
                trySend(Unit)
            }
            override fun onDiscoveryStopped(serviceType: String) {}
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "discoverDesktops: start failed: $errorCode")
                close()
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.e(TAG, "discoverDesktops: stop failed: $errorCode")
            }
            override fun onServiceFound(service: NsdServiceInfo) {
                val key = service.serviceName
                Log.d(TAG, "discoverDesktops: found $key")
                if (!service.serviceType.startsWith(SERVICE_TYPE)) return
                // Mark as resolving immediately so the UI shows a
                // placeholder row.
                _desktops.value = _desktops.value + (key to DiscoveredDesktop(
                    instanceName = key,
                    name = key,
                    host = null,
                    port = 0,
                    isResolving = true,
                ))
                nsd.resolveService(service, object : NsdManager.ResolveListener {
                    override fun onResolveFailed(s: NsdServiceInfo, code: Int) {
                        Log.w(TAG, "resolve failed for $key: $code")
                        // Drop the placeholder on failure.
                        val cur = _desktops.value
                        if (cur[key]?.isResolving == true) {
                            _desktops.value = cur - key
                        }
                    }
                    override fun onServiceResolved(s: NsdServiceInfo) {
                        val host = s.host?.hostAddress ?: return
                        val port = s.port
                        Log.d(TAG, "discoverDesktops: resolved $key -> $host:$port")
                        _desktops.value = _desktops.value + (key to DiscoveredDesktop(
                            instanceName = key,
                            name = s.serviceName,
                            host = host,
                            port = port,
                            isResolving = false,
                        ))
                    }
                })
            }
            override fun onServiceLost(service: NsdServiceInfo) {
                Log.d(TAG, "discoverDesktops: lost ${service.serviceName}")
                _desktops.value = _desktops.value - service.serviceName
            }
        }
        nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        awaitClose { nsd.stopServiceDiscovery(listener) }
    }

    /**
     * Forget all currently-discovered desktops. Call this when the
     * user explicitly hits a "clear" button or when the screen is
     * torn down so a subsequent discovery starts with a clean slate.
     */
    fun clearDesktops() {
        _desktops.value = emptyMap()
    }

    companion object {
        // Must match the message-center's mDNS service type:
        // crates/phonebridge-net/src/mdns.rs -> _phonebridge._tcp
        const val SERVICE_TYPE = "_phonebridge._tcp."
    }
}
