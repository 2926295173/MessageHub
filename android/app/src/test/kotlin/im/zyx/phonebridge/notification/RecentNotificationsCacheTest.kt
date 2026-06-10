package im.zyx.phonebridge.notification

import im.zyx.phonebridge.core.protocol.NotificationReceivedPayload
import org.junit.Assert.assertEquals
import org.junit.Test

class RecentNotificationsCacheTest {

    @Test
    fun `push then snapshot returns in order, oldest first`() {
        val cache = RecentNotificationsCache().apply { capacity = 4 }
        cache.push(item("a"))
        cache.push(item("b"))
        cache.push(item("c"))
        assertEquals(listOf("a", "b", "c"), cache.snapshot().map { it.id })
    }

    @Test
    fun `push beyond capacity evicts the oldest entries`() {
        val cache = RecentNotificationsCache().apply { capacity = 2 }
        cache.push(item("a"))
        cache.push(item("b"))
        cache.push(item("c"))
        cache.push(item("d"))
        assertEquals(listOf("c", "d"), cache.snapshot().map { it.id })
    }

    @Test
    fun `clear empties the cache`() {
        val cache = RecentNotificationsCache()
        cache.push(item("a"))
        cache.clear()
        assertEquals(emptyList<NotificationReceivedPayload>(), cache.snapshot())
    }

    @Test
    fun `snapshot returns an immutable copy`() {
        val cache = RecentNotificationsCache()
        cache.push(item("a"))
        val s = cache.snapshot()
        cache.push(item("b"))
        // The earlier snapshot is unaffected by later pushes.
        assertEquals(1, s.size)
        assertEquals("a", s.first().id)
    }

    private fun item(id: String) = NotificationReceivedPayload(
        id = id,
        package_name = "pkg",
        app_name = "App",
        title = "title $id",
        content = "content $id",
        posted_at = 0L,
    )
}
