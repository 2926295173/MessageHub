package im.zyx.phonebridge.notification

import im.zyx.phonebridge.core.protocol.NotificationReceivedPayload
import java.util.ArrayDeque
import java.util.concurrent.locks.ReentrantLock
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.concurrent.withLock

/**
 * In-memory ring buffer of the last [capacity] notifications seen by
 * [NotificationRelayService]. The floating console ([FloatingConsoleService])
 * reads from this to populate its "recent" panel without having to
 * re-introspect the system shade.
 *
 * Singleton-scoped, thread-safe, deliberately cheap: pushing a
 * notification is O(1) amortised, reading is a snapshot of the
 * current deque.
 */
@Singleton
class RecentNotificationsCache @Inject constructor() {

    private val lock = ReentrantLock()
    private val buffer = ArrayDeque<NotificationReceivedPayload>()

    fun push(payload: NotificationReceivedPayload) = lock.withLock {
        if (buffer.size >= capacity) buffer.removeFirst()
        buffer.addLast(payload)
    }

    fun snapshot(): List<NotificationReceivedPayload> = lock.withLock {
        buffer.toList()
    }

    fun clear() = lock.withLock { buffer.clear() }

    var capacity: Int = 16
}
