package im.zyx.phonebridge.di

import android.content.Context
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.NsdRegistrar
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.telephony.CallController
import javax.inject.Singleton

/**
 * Hilt bindings for the things we want as @Inject classes. The module
 * also wires singletons that we *don't* want constructor-injected
 * (e.g. things that need an ApplicationContext).
 */
@Module
@InstallIn(SingletonComponent::class)
object AppModule {

    @Provides @Singleton
    fun provideNsdRegistrar(@ApplicationContext context: Context): NsdRegistrar =
        NsdRegistrar(context)

    @Provides @Singleton
    fun providePairingMachine(): PairingMachine = PairingMachine()

    @Provides @Singleton
    fun provideCallController(
        @ApplicationContext context: Context,
        client: BridgeClient
    ): CallController = CallController(context, client)
}
