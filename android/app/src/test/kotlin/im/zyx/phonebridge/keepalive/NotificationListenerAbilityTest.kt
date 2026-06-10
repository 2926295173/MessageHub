package im.zyx.phonebridge.keepalive

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationListenerAbilityTest {

    @Test
    fun `isGranted returns true when our entry is in the colon list`() {
        val flat = "com.other.app/Cls:im.zyx.phonebridge/.notification.NotificationRelayService"
        assertTrue(NotificationListenerAbility.isGranted(flat, "im.zyx.phonebridge"))
    }

    @Test
    fun `isGranted returns false when only other apps are listed`() {
        assertFalse(NotificationListenerAbility.isGranted("com.other.app/Cls", "im.zyx.phonebridge"))
    }

    @Test
    fun `isGranted returns false when the value is null`() {
        assertFalse(NotificationListenerAbility.isGranted(null, "im.zyx.phonebridge"))
    }

    @Test
    fun `isGranted returns false when the value is empty`() {
        assertFalse(NotificationListenerAbility.isGranted("", "im.zyx.phonebridge"))
    }

    @Test
    fun `isGranted does not match a package whose name is a prefix of another`() {
        // "im.zyx.phonebridge.debug/" should not match a query for
        // "im.zyx.phonebridge/" — note the trailing slash in the
        // prefix; a query for "im.zyx.phonebridgeX" must miss.
        val flat = "im.zyx.phonebridge.debug/.notification.NotificationRelayService"
        assertFalse(NotificationListenerAbility.isGranted(flat, "im.zyx.phonebridge"))
    }

    @Test
    fun `isGranted handles trailing colon gracefully`() {
        val flat = "im.zyx.phonebridge/.notification.NotificationRelayService:"
        assertTrue(NotificationListenerAbility.isGranted(flat, "im.zyx.phonebridge"))
    }
}
