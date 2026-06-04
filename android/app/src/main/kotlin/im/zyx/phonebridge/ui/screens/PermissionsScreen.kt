package im.zyx.phonebridge.ui.screens

import android.Manifest
import android.app.role.RoleManager
import android.content.Context
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import android.content.pm.PackageManager
import androidx.compose.material3.MaterialTheme

@Composable
fun PermissionsScreen(onContinue: () -> Unit) {
    val context = LocalContext.current
    val reqPerms = buildList {
        add(Manifest.permission.POST_NOTIFICATIONS)
        add(Manifest.permission.RECEIVE_SMS)
        add(Manifest.permission.READ_SMS)
        add(Manifest.permission.SEND_SMS)
        add(Manifest.permission.READ_PHONE_STATE)
        add(Manifest.permission.ANSWER_PHONE_CALLS)
        add(Manifest.permission.READ_CALL_LOG)
    }

    val launcher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions()
    ) { _ -> /* user decided; we still continue */ }

    Scaffold { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp)
        ) {
            Text(
                text = "PhoneBridge needs a few permissions",
                style = MaterialTheme.typography.headlineSmall
            )
            Text(
                text = "Grant notifications, SMS, and call access so this device can be " +
                    "controlled by your desktop daemon over LAN.",
                style = MaterialTheme.typography.bodyMedium
            )
            Spacer(Modifier.height(8.dp))
            PermissionCard(
                title = "Runtime permissions",
                body = "POST_NOTIFICATIONS, SMS, phone state, call log, answer calls.",
                granted = reqPerms.all { granted(context, it) }
            )
            PermissionCard(
                title = "Notification access",
                body = "Required to mirror Android notifications to the desktop.",
                granted = notificationListenerGranted(context)
            )
            Button(
                onClick = {
                    launcher.launch(reqPerms.toTypedArray())
                    openNotificationAccessSettings(context)
                },
                modifier = Modifier.fillMaxWidth()
            ) { Text("Grant") }
            Button(
                onClick = onContinue,
                modifier = Modifier.fillMaxWidth()
            ) { Text("Continue") }
        }
    }
}

@Composable
private fun PermissionCard(title: String, body: String, granted: Boolean) {
    Card(
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface
        )
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(text = title, style = MaterialTheme.typography.titleMedium)
            Text(text = body, style = MaterialTheme.typography.bodyMedium)
            Text(
                text = if (granted) "Granted" else "Pending",
                style = MaterialTheme.typography.labelMedium,
                color = if (granted) MaterialTheme.colorScheme.primary
                        else MaterialTheme.colorScheme.error
            )
        }
    }
}

private fun granted(ctx: Context, perm: String): Boolean =
    ContextCompat.checkSelfPermission(ctx, perm) == PackageManager.PERMISSION_GRANTED

private fun notificationListenerGranted(ctx: Context): Boolean {
    val flat = Settings.Secure.getString(ctx.contentResolver, "enabled_notification_listeners")
        ?: return false
    return flat.contains(ctx.packageName)
}

private fun openNotificationAccessSettings(ctx: Context) {
    val i = Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
    i.flags = Intent.FLAG_ACTIVITY_NEW_TASK
    try {
        ctx.startActivity(i)
    } catch (_: Throwable) { /* device without that settings page */ }
}
