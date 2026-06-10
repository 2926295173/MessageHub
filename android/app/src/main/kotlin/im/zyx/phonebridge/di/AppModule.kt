package im.zyx.phonebridge.di

import android.content.Context
import dagger.Binds
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import dagger.multibindings.IntoSet
import im.zyx.phonebridge.data.IdentityStore
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.keepalive.BridgeAbility
import im.zyx.phonebridge.keepalive.NotificationListenerAbility
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.NsdRegistrar
import im.zyx.phonebridge.telephony.CallController
import javax.inject.Singleton

/**
 * Hilt bindings.
 *
 * The interface [IdentityStore] is bound to its concrete
 * [PrefsRepository] implementation. Hilt creates the singleton
 * (PrefsRepository has `@Inject constructor(@ApplicationContext)`)
 * and PairingMachine / BridgeService get the IdentityStore view of
 * the same instance.
 *
 * PairingMachine is constructor-injected via @Inject (depends on
 * IdentityStore, which is provided here).
 */
@Module
@InstallIn(SingletonComponent::class)
abstract class AppBindModule {
    @Binds
    @Singleton
    abstract fun bindIdentityStore(impl: PrefsRepository): IdentityStore

    @Binds
    @IntoSet
    abstract fun bindNotificationListenerAbility(
        impl: NotificationListenerAbility
    ): BridgeAbility
}

@Module
@InstallIn(SingletonComponent::class)
object AppModule {

    @Provides @Singleton
    fun provideNsdRegistrar(@ApplicationContext context: Context): NsdRegistrar =
        NsdRegistrar(context)

    @Provides @Singleton
    fun provideCallController(
        @ApplicationContext context: Context,
        client: BridgeClient,
        pairing: im.zyx.phonebridge.pairing.PairingMachine
    ): CallController = CallController(context, client, pairing)
}
