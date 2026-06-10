package im.zyx.phonebridge.ui.screens

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.Devices
import androidx.compose.material.icons.filled.LightMode
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.OpenInNew
import androidx.compose.material.icons.filled.Shield
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import im.zyx.phonebridge.R
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.ui.components.PermissionsSection
import im.zyx.phonebridge.ui.theme.ThemeMode
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import javax.inject.Inject
import org.json.JSONArray
import org.json.JSONObject

data class ManualDesktop(val host: String, val port: Int, val label: String)

@HiltViewModel
class SettingsViewModel @Inject constructor(
    private val prefs: PrefsRepository
) : ViewModel() {

    val host = prefs.desktopHost.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    val port = prefs.desktopPort.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    val fingerprint = prefs.fingerprint.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    val deviceId = prefs.deviceId.stateIn(viewModelScope, SharingStarted.Eagerly, null)

    val deviceName = prefs.deviceName.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    val themePref = prefs.themeMode.stateIn(viewModelScope, SharingStarted.Eagerly, null)
    val persistentNotif = prefs.persistentNotif.stateIn(viewModelScope, SharingStarted.Eagerly, true)
    val floatingEnabled = prefs.floatingEnabled.stateIn(viewModelScope, SharingStarted.Eagerly, false)
    val batteryOptPrompted = prefs.batteryOptPrompted.stateIn(viewModelScope, SharingStarted.Eagerly, false)
    val trustedSsids = prefs.trustedSsidsCsv
        .map { it.orEmpty().split(',').map(String::trim).filter(String::isNotEmpty) }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())
    val manualDesktops = prefs.manualDesktopsJson
        .map { json ->
            if (json.isNullOrBlank()) emptyList()
            else runCatching {
                val arr = JSONArray(json)
                (0 until arr.length()).map { i ->
                    val o = arr.getJSONObject(i)
                    ManualDesktop(
                        host = o.getString("host") ?: "",
                        port = o.optInt("port", 8443),
                        label = o.optString("label", ""),
                    )
                }
            }.getOrDefault(emptyList())
        }
        .stateIn(viewModelScope, SharingStarted.Eagerly, emptyList())

    fun setDeviceName(name: String?) { viewModelScope.launch { prefs.setDeviceName(name) } }
    fun setThemeMode(mode: ThemeMode) { viewModelScope.launch { prefs.setThemeMode(mode.persisted) } }
    fun setPersistentNotif(enabled: Boolean) { viewModelScope.launch { prefs.setPersistentNotif(enabled) } }
    fun setFloatingEnabled(enabled: Boolean) { viewModelScope.launch { prefs.setFloatingEnabled(enabled) } }

    fun addTrustedSsid(ssid: String) {
        val s = ssid.trim()
        if (s.isEmpty()) return
        viewModelScope.launch {
            val current = prefs.trustedSsidsCsv.first().orEmpty()
            val list = current.split(',').map(String::trim).filter(String::isNotEmpty).toMutableSet()
            list.add(s)
            prefs.setTrustedSsidsCsv(list.joinToString(","))
        }
    }
    fun removeTrustedSsid(ssid: String) {
        viewModelScope.launch {
            val current = prefs.trustedSsidsCsv.first().orEmpty()
            val list = current.split(',').map(String::trim).filter(String::isNotEmpty).toMutableSet()
            list.remove(ssid)
            prefs.setTrustedSsidsCsv(list.joinToString(","))
        }
    }

    fun addManualDesktop(host: String, port: Int, label: String) {
        if (host.isBlank()) return
        viewModelScope.launch {
            val current = prefs.manualDesktopsJson.first().orEmpty()
            val arr = if (current.isBlank()) JSONArray() else JSONArray(current)
            arr.put(JSONObject().apply {
                put("host", host.trim())
                put("port", port)
                put("label", label.trim().ifEmpty { "$host:$port" })
            })
            prefs.setManualDesktopsJson(arr.toString())
        }
    }
    fun removeManualDesktop(host: String, port: Int) {
        viewModelScope.launch {
            val current = prefs.manualDesktopsJson.first().orEmpty()
            if (current.isBlank()) return@launch
            val arr = JSONArray(current)
            val out = JSONArray()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                if (o.optString("host") == host && o.optInt("port", 8443) == port) continue
                out.put(o)
            }
            prefs.setManualDesktopsJson(out.toString())
        }
    }

    /** Current Wi-Fi SSID, or null if not connected / not permitted. */
    fun currentSsid(ctx: android.content.Context): String? {
        return try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                // Android 12+: SSID is restricted; we still get the
                // connection info but not the human-readable SSID
                // without location permission. Best-effort return null.
                null
            } else {
                val wm = ctx.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                @Suppress("DEPRECATION")
                val info = wm.connectionInfo
                info?.ssid?.removeSurrounding("\"")
            }
        } catch (_: Throwable) { null }
    }

    fun exportLogsToClipboard(ctx: android.content.Context) {
        val dump = buildString {
            appendLine("=== PhoneBridge diagnostics ===")
            appendLine("captured: ${java.util.Date()}")
            appendLine()
            appendLine("--- device ---")
            appendLine("model: ${Build.MANUFACTURER} ${Build.MODEL}")
            appendLine("android: ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT})")
            appendLine("device_id: ${deviceId.value ?: "(none)"}")
            appendLine("device_name: ${deviceName.value ?: Build.MODEL}")
            appendLine("fingerprint: ${fingerprint.value ?: "(unpinned)"}")
            appendLine()
            appendLine("--- desktop ---")
            appendLine("host: ${host.value ?: "-"}")
            appendLine("port: ${port.value ?: "-"}")
            appendLine("ssid: ${currentSsid(ctx) ?: "(unknown)"}")
            appendLine("trusted_ssids: ${trustedSsids.value.joinToString(",").ifEmpty { "(none)" }}")
            appendLine()
            appendLine("--- prefs ---")
            appendLine("theme: ${themePref.value ?: "system"}")
            appendLine("persistent_notif: ${persistentNotif.value}")
            appendLine("manual_desktops: ${manualDesktops.value.size} entry(ies)")
        }
        val cb = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        cb.setPrimaryClip(ClipData.newPlainText("PhoneBridge logs", dump))
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    onOpenDrawer: () -> Unit = {},
    vm: SettingsViewModel = hiltViewModel()
) {
    val host by vm.host.collectAsState()
    val port by vm.port.collectAsState()
    val fp by vm.fingerprint.collectAsState()
    val dev by vm.deviceId.collectAsState()
    val devName by vm.deviceName.collectAsState()
    val themePref by vm.themePref.collectAsState()
    val notifOn by vm.persistentNotif.collectAsState()
    val trusted by vm.trustedSsids.collectAsState()
    val manuals by vm.manualDesktops.collectAsState()
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }

    var showNameDialog by remember { mutableStateOf(false) }
    var showAddSsidDialog by remember { mutableStateOf(false) }
    var showAddDesktopDialog by remember { mutableStateOf(false) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings_title)) },
                navigationIcon = {
                    IconButton(onClick = onOpenDrawer) {
                        Icon(
                            imageVector = Icons.Filled.Menu,
                            contentDescription = stringResource(R.string.menu_open),
                        )
                    }
                },
            )
        },
        snackbarHost = { SnackbarHost(snackbar) },
    ) { pad ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {

            // ───── 设备名 ─────
            SettingsCard {
                SettingsRow(
                    icon = Icons.Filled.Devices,
                    title = stringResource(R.string.setting_device_name_title),
                    subtitle = devName?.takeIf { it.isNotBlank() } ?: stringResource(R.string.setting_device_name_default, Build.MODEL),
                    onClick = { showNameDialog = true },
                )
            }

            // ───── 主题 ─────
            SettingsCard {
                Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.DarkMode, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                        Spacer(Modifier.padding(start = 12.dp))
                        Text(stringResource(R.string.setting_theme_title), style = MaterialTheme.typography.bodyLarge)
                    }
                    Spacer(Modifier.height(8.dp))
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        listOf(
                            Triple(ThemeMode.System, R.string.setting_theme_system, Icons.Filled.Devices),
                            Triple(ThemeMode.Light, R.string.setting_theme_light, Icons.Filled.LightMode),
                            Triple(ThemeMode.Dark, R.string.setting_theme_dark, Icons.Filled.DarkMode),
                        ).forEach { (mode, labelRes, icon) ->
                            val selected = ThemeMode.fromPersisted(themePref) == mode
                            FilterChip(
                                selected = selected,
                                onClick = { vm.setThemeMode(mode) },
                                leadingIcon = { Icon(icon, contentDescription = null) },
                                label = { Text(stringResource(labelRes)) },
                                modifier = Modifier.weight(1f),
                            )
                        }
                    }
                }
            }

            // ───── 持久通知 ─────
            SettingsCard {
                SettingsSwitchRow(
                    icon = Icons.Filled.Notifications,
                    title = stringResource(R.string.setting_notif_title),
                    subtitle = stringResource(R.string.setting_notif_subtitle),
                    checked = notifOn,
                    onCheckedChange = { vm.setPersistentNotif(it) },
                )
            }

            // ───── 悬浮控制台 ─────
            val floatingOn by vm.floatingEnabled.collectAsState()
            SettingsCard {
                SettingsSwitchRow(
                    icon = Icons.Filled.OpenInNew,
                    title = stringResource(R.string.setting_floating_title),
                    subtitle = stringResource(R.string.setting_floating_subtitle),
                    checked = floatingOn,
                    onCheckedChange = { enabled ->
                        vm.setFloatingEnabled(enabled)
                        if (enabled) {
                            if (android.provider.Settings.canDrawOverlays(ctx)) {
                                ctx.startService(
                                    android.content.Intent(
                                        ctx,
                                        im.zyx.phonebridge.keepalive.FloatingConsoleService::class.java,
                                    )
                                )
                            } else {
                                val i = android.content.Intent(
                                    android.provider.Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                                    android.net.Uri.parse("package:${ctx.packageName}"),
                                ).addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
                                ctx.startActivity(i)
                            }
                        } else {
                            ctx.stopService(
                                android.content.Intent(
                                    ctx,
                                    im.zyx.phonebridge.keepalive.FloatingConsoleService::class.java,
                                )
                            )
                        }
                    },
                )
            }

            // ───── 信任的网络 ─────
            SettingsCard {
                Column(modifier = Modifier.padding(vertical = 4.dp)) {
                    SettingsRow(
                        icon = Icons.Filled.Wifi,
                        title = stringResource(R.string.setting_trusted_title),
                        subtitle = if (trusted.isEmpty())
                            stringResource(R.string.setting_trusted_empty)
                        else
                            stringResource(R.string.setting_trusted_count, trusted.size),
                        onClick = { showAddSsidDialog = true },
                        trailing = {
                            IconButton(onClick = { showAddSsidDialog = true }) {
                                Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.setting_trusted_add))
                            }
                        },
                    )
                    if (trusted.isNotEmpty()) {
                        HorizontalDivider(modifier = Modifier.padding(horizontal = 16.dp))
                        trusted.forEach { ssid ->
                            Row(
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Icon(
                                    Icons.Filled.Shield,
                                    contentDescription = null,
                                    tint = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier.padding(end = 12.dp),
                                )
                                Text(ssid, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                                TextButton(onClick = { vm.removeTrustedSsid(ssid) }) {
                                    Text(stringResource(R.string.action_remove))
                                }
                            }
                        }
                    }
                }
            }

            // ───── 通过 IP 添加设备 ─────
            SettingsCard {
                Column(modifier = Modifier.padding(vertical = 4.dp)) {
                    SettingsRow(
                        icon = Icons.Filled.Add,
                        title = stringResource(R.string.setting_manual_title),
                        subtitle = stringResource(R.string.setting_manual_count, manuals.size),
                        onClick = { showAddDesktopDialog = true },
                        trailing = {
                            IconButton(onClick = { showAddDesktopDialog = true }) {
                                Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.setting_manual_add))
                            }
                        },
                    )
                    if (manuals.isNotEmpty()) {
                        HorizontalDivider(modifier = Modifier.padding(horizontal = 16.dp))
                        manuals.forEach { d ->
                            Row(
                                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(modifier = Modifier.weight(1f)) {
                                    Text(d.label, style = MaterialTheme.typography.bodyMedium)
                                    Text(
                                        "${d.host}:${d.port}",
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    )
                                }
                                TextButton(onClick = { vm.removeManualDesktop(d.host, d.port) }) {
                                    Text(stringResource(R.string.action_remove))
                                }
                            }
                        }
                    }
                }
            }

            // ───── 导出日志 ─────
            SettingsCard {
                SettingsRow(
                    icon = Icons.Filled.BugReport,
                    title = stringResource(R.string.setting_logs_title),
                    subtitle = stringResource(R.string.setting_logs_subtitle),
                    onClick = {
                        vm.exportLogsToClipboard(ctx)
                        scope.launch { snackbar.showSnackbar(ctx.getString(R.string.logs_copied)) }
                    },
                )
            }

            // ───── 桥接配置 ─────
            SettingsCard {
                Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    val missing = stringResource(R.string.settings_value_missing)
                    Text(stringResource(R.string.settings_bridge_config), style = MaterialTheme.typography.titleSmall)
                    Text(stringResource(R.string.settings_last_desktop, host ?: missing, (port ?: missing).toString()))
                    Text(stringResource(R.string.settings_cert_fp, fp ?: stringResource(R.string.settings_unpinned)))
                    Text(stringResource(R.string.settings_device_id, dev ?: stringResource(R.string.settings_unassigned)))
                }
            }

            // ───── 权限 ─────
            // Bottom (not top) — the user only comes here when something
            // has gone wrong (OS revoked access, fresh ROM, fresh
            // install) so it should be out of the way of the day-to-day
            // settings (device name, theme, notification, floating
            // console, trusted networks, manual desktops, log export).
            PermissionsSection(modifier = Modifier.fillMaxWidth())
        }
    }

    // ─── Dialogs ───
    if (showNameDialog) {
        NameEditDialog(
            initial = devName.orEmpty(),
            onDismiss = { showNameDialog = false },
            onSave = { newName ->
                vm.setDeviceName(newName.takeIf { it.isNotBlank() })
                showNameDialog = false
                scope.launch { snackbar.showSnackbar(ctx.getString(R.string.device_name_saved)) }
            },
        )
    }
    if (showAddSsidDialog) {
        AddSsidDialog(
            currentSsid = vm.currentSsid(ctx).orEmpty(),
            onDismiss = { showAddSsidDialog = false },
            onAdd = { ssid ->
                vm.addTrustedSsid(ssid)
                showAddSsidDialog = false
            },
        )
    }
    if (showAddDesktopDialog) {
        AddDesktopDialog(
            onDismiss = { showAddDesktopDialog = false },
            onAdd = { host, port, label ->
                vm.addManualDesktop(host, port, label)
                showAddDesktopDialog = false
            },
        )
    }
}

@Composable
private fun SettingsCard(content: @Composable () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) { content() }
}

@Composable
private fun SettingsRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String,
    onClick: () -> Unit,
    trailing: @Composable (() -> Unit)? = null,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickableSafe(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.padding(start = 12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(
                subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        trailing?.invoke()
    }
}

@Composable
private fun SettingsSwitchRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    subtitle: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Spacer(Modifier.padding(start = 12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(
                subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
private fun NameEditDialog(
    initial: String,
    onDismiss: () -> Unit,
    onSave: (String) -> Unit,
) {
    var v by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.setting_device_name_title)) },
        text = {
            OutlinedTextField(
                value = v,
                onValueChange = { v = it.take(64) },
                singleLine = true,
                label = { Text(stringResource(R.string.setting_device_name_hint)) },
            )
        },
        confirmButton = {
            TextButton(onClick = { onSave(v) }) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) }
        },
    )
}

@Composable
private fun AddSsidDialog(
    currentSsid: String,
    onDismiss: () -> Unit,
    onAdd: (String) -> Unit,
) {
    var v by remember { mutableStateOf(currentSsid) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.setting_trusted_add)) },
        text = {
            Column {
                OutlinedTextField(
                    value = v,
                    onValueChange = { v = it.take(64) },
                    singleLine = true,
                    label = { Text(stringResource(R.string.setting_trusted_ssid_hint)) },
                )
                if (currentSsid.isNotBlank()) {
                    Spacer(Modifier.height(6.dp))
                    Text(
                        stringResource(R.string.setting_trusted_current_ssid, currentSsid),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onAdd(v) },
                enabled = v.isNotBlank(),
            ) { Text(stringResource(R.string.action_add)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) }
        },
    )
}

@Composable
private fun AddDesktopDialog(
    onDismiss: () -> Unit,
    onAdd: (host: String, port: Int, label: String) -> Unit,
) {
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("8443") }
    var label by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.setting_manual_add)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = host, onValueChange = { host = it.trim() },
                    singleLine = true,
                    label = { Text(stringResource(R.string.pairing_host)) },
                )
                OutlinedTextField(
                    value = port, onValueChange = { port = it.filter(Char::isDigit).take(5) },
                    singleLine = true,
                    label = { Text(stringResource(R.string.pairing_port)) },
                )
                OutlinedTextField(
                    value = label, onValueChange = { label = it.take(32) },
                    singleLine = true,
                    label = { Text(stringResource(R.string.setting_manual_label_hint)) },
                )
            }
        },
        confirmButton = {
            val p = port.toIntOrNull() ?: 8443
            TextButton(
                onClick = { onAdd(host, p, label) },
                enabled = host.isNotBlank(),
            ) { Text(stringResource(R.string.action_add)) }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) }
        },
    )
}

/** Clickable modifier that works without an extra import file. */
@Composable
private fun Modifier.clickableSafe(onClick: () -> Unit): Modifier =
    this.then(
        Modifier.clickable(
            interactionSource = remember { MutableInteractionSource() },
            indication = androidx.compose.foundation.LocalIndication.current,
            onClick = onClick,
        )
    )
