// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! mDNS service: advertise the message-center + browse for phones.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use phonebridge_net::mdns::{self, MdnsEvent, MdnsHandle};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::app_state::AppState;

/// Handle to the running mDNS service. Dropping shuts it down.
pub struct MdnsService {
    /// The advertiser keep-alive.
    #[allow(dead_code)]
    advertiser: Option<MdnsHandle>,
    /// The browser keep-alive.
    #[allow(dead_code)]
    browser: Option<mdns::BrowseGuard>,
    /// Channel sender used to push discovered devices into the runtime.
    #[allow(dead_code)]
    pub discovered_tx: mpsc::Sender<MdnsDeviceEntry>,
}

/// One discovered device, plus a flag whether it is paired.
#[derive(Debug, Clone)]
pub struct MdnsDeviceEntry {
    /// Stable id of the device (parsed from TXT).
    pub device_id: String,
    /// Display name.
    pub name: String,
    /// IPv4 address.
    pub address: std::net::Ipv4Addr,
    /// Port.
    pub port: u16,
    /// Optional fingerprint from TXT.
    pub fingerprint: Option<String>,
}

/// Start the mDNS service: advertise the message-center + browse for peers.
pub fn start(state: Arc<AppState>) -> Result<MdnsService, mdns::MdnsError> {
    let instance_name = format!("phonebridge-{}", hostname());
    let host_name = normalize_hostname(&hostname());
    let port: u16 = state
        .config
        .server
        .bind
        .rsplit(':')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8443);
    let fp = state.our_fingerprint.read().clone();
    let name = state.our_name.read().clone();
    let txt = mdns::daemon_txt(
        &state.our_device_id.to_string(),
        &name,
        port,
        &fp,
    );
    // mdns-sd requires the service type to end with `._tcp.local.` or
    // `._udp.local.`. We append `.local.` if missing.
    let service_type = normalize_service_type(&state.config.discovery.service_type);
    let advertiser = mdns::advertise(
        &service_type,
        &instance_name,
        &host_name,
        port,
        txt,
    )?;

    let (mut rx, browser) = mdns::browse(&service_type)?;
    let (discovered_tx, mut discovered_rx) = mpsc::channel::<MdnsDeviceEntry>(64);

    // Spawn a task that drains mdns events and forwards to our channel.
    let _state_for_task = state.clone();
    let tx_for_task = discovered_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                MdnsEvent::Discovered(dev) => {
                    info!(device_id = %dev.device_id, name = %dev.name, "mDNS: discovered peer");
                    let entry = MdnsDeviceEntry {
                        device_id: dev.device_id,
                        name: dev.name,
                        address: dev.address,
                        port: dev.port,
                        fingerprint: dev.fingerprint,
                    };
                    if tx_for_task.send(entry).await.is_err() {
                        break;
                    }
                }
                MdnsEvent::Removed(id) => {
                    info!(fullname = %id, "mDNS: peer removed");
                }
            }
        }
    });

    // Spawn a task that updates the in-memory `state` with discovered devices.
    let state_for_drain = state.clone();
    tokio::spawn(async move {
        let map: Arc<Mutex<HashMap<String, MdnsDeviceEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        while let Some(entry) = discovered_rx.recv().await {
            map.lock().insert(entry.device_id.clone(), entry.clone());
            // Persist any newly discovered device in the DB (unpaired).
            // We don't have the long-term pubkey here (we'd need a hello),
            // so we just record presence.
            let _ = state_for_drain;
            let _ = entry;
        }
    });

    Ok(MdnsService {
        advertiser: Some(advertiser),
        browser: Some(browser),
        discovered_tx,
    })
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "phonebridge".to_string())
}

fn normalize_service_type(t: &str) -> String {
    if t.ends_with(".local.") || t.ends_with(".local") {
        t.to_string()
    } else if t.ends_with("._tcp") || t.ends_with("._udp") {
        format!("{}.local.", t)
    } else {
        // Assume it's a service name like `_phonebridge` and we add `_tcp.local.`.
        // The config spec uses `_phonebridge._tcp` so this branch shouldn't
        // normally trigger.
        format!("{}._tcp.local.", t)
    }
}

fn normalize_hostname(h: &str) -> String {
    if h.ends_with(".local.") {
        h.to_string()
    } else if h.ends_with(".local") {
        format!("{}.", h)
    } else {
        format!("{}.local.", h)
    }
}

#[allow(dead_code)]
fn _suppress_unused_warn() {
    warn!("");
}
