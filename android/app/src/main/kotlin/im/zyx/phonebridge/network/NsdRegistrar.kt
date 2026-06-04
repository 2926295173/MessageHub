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
import kotlinx.coroutines.flow.callbackFlow

private const val TAG = "Nsd"

/**
 * Browses the LAN for `_phonebridge._tcp.` services and yields the
 * first one it finds. (Multi-desktop setups are out of scope for MVP.)
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

    /**
     * Suspends until a PhoneBridge daemon is found, then emits its
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

    companion object {
        // Must match the daemon's mDNS service type:
        // crates/phonebridge-net/src/mdns.rs -> _phonebridge._tcp
        const val SERVICE_TYPE = "_phonebridge._tcp."
    }
}
