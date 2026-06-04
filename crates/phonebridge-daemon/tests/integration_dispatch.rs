//! Integration test: WS handler dispatches notification/sms/call
//! messages and the DaemonSink persists them to storage.

#![cfg(test)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use phonebridge_net::ws_handler::{self, NoopSink, WsContext, WsSink};
use phonebridge_proto::{
    DeviceHello, DeviceType, Envelope, MessageType, NotificationReceived,
};
use phonebridge_storage::Db;
use phonebridge_storage::models::{CallRow, NotificationRow, SmsDirection, SmsRow};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{client_async, WebSocketStream};
use uuid::Uuid;

use phonebridge_daemon::daemon_sink::DaemonSink;
use phonebridge_daemon::test_context;

type Ws = WebSocketStream<TcpStream>;

async fn listen_addr() -> (TcpListener, SocketAddr) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    (l, addr)
}

/// A `WsSink` that wraps `DaemonSink` and counts calls for assertions.
struct CountingSink {
    inner: Arc<DaemonSink>,
    counter: Arc<std::sync::Mutex<Counters>>,
}

#[derive(Default)]
struct Counters {
    notification: usize,
    sms_received: usize,
    call_incoming: usize,
    hello: usize,
    disconnect: usize,
}

#[async_trait]
impl WsSink for CountingSink {
    async fn on_notification(&self, device_id: Uuid, env: &NotificationReceived) {
        self.inner.on_notification(device_id, env).await;
        self.counter.lock().unwrap().notification += 1;
    }
    async fn on_notification_dismissed(
        &self,
        device_id: Uuid,
        env: &phonebridge_proto::NotificationDismissed,
    ) {
        self.inner.on_notification_dismissed(device_id, env).await;
    }
    async fn on_sms_received(&self, device_id: Uuid, env: &phonebridge_proto::SmsReceived) {
        self.inner.on_sms_received(device_id, env).await;
        self.counter.lock().unwrap().sms_received += 1;
    }
    async fn on_sms_send_result(&self, device_id: Uuid, env: &phonebridge_proto::SmsSendResult) {
        self.inner.on_sms_send_result(device_id, env).await;
    }
    async fn on_call_state(&self, device_id: Uuid, env: &phonebridge_proto::CallState) {
        self.inner.on_call_state(device_id, env).await;
    }
    async fn on_call_incoming(&self, device_id: Uuid, env: &phonebridge_proto::CallIncoming) {
        self.inner.on_call_incoming(device_id, env).await;
        self.counter.lock().unwrap().call_incoming += 1;
    }
    async fn on_call_history(&self, device_id: Uuid, env: &phonebridge_proto::CallHistory) {
        self.inner.on_call_history(device_id, env).await;
    }
    async fn on_sms_list_result(&self, device_id: Uuid, env: &phonebridge_proto::SmsListResult) {
        self.inner.on_sms_list_result(device_id, env).await;
    }
    async fn on_hello(&self, device_id: Uuid, env: &DeviceHello) {
        self.inner.on_hello(device_id, env).await;
        self.counter.lock().unwrap().hello += 1;
    }
    async fn on_unpair(&self, device_id: Uuid, env: &phonebridge_proto::Unpair) {
        self.inner.on_unpair(device_id, env).await;
    }
    async fn on_disconnect(&self, device_id: Uuid) {
        self.inner.on_disconnect(device_id).await;
        self.counter.lock().unwrap().disconnect += 1;
    }
}

#[tokio::test]
async fn ws_dispatch_persists_to_db() {
    let db = Db::open_memory().await.unwrap();
    db.migrate().await.unwrap();
    let db_arc = Arc::new(db);

    let counters = Arc::new(std::sync::Mutex::new(Counters::default()));
    let sink: Arc<dyn WsSink + Send + Sync> = Arc::new(CountingSink {
        inner: Arc::new(DaemonSink::new(db_arc.clone())),
        counter: counters.clone(),
    });

    // Set up a WS connection with the real (counting) sink.
    let (l, addr) = listen_addr().await;
    let server = tokio::spawn(async move {
        let (s, _) = l.accept().await.unwrap();
        s
    });
    let client = TcpStream::connect(addr).await.unwrap();
    let server_stream = server.await.unwrap();

    let ctx = WsContext::new(Uuid::new_v4(), sink, phonebridge_net::DeviceRegistry::new());
    let task = tokio::spawn(ws_handler::handle_connection(server_stream, addr, ctx));

    // === Connect + hello ===
    let mut ws: Ws = client_async("ws://localhost/", client).await.unwrap().0;
    let device_id = Uuid::new_v4();
    let hello = Envelope::new(
        MessageType::DeviceHello,
        device_id,
        DeviceHello {
            name: "TestPixel".into(),
            device_type: DeviceType::Android,
            protocol_version: 1,
            pubkey: "PUBKEY".into(),
            port: Some(8443),
            manufacturer: Some("Google".into()),
            model: Some("Pixel 8 Pro".into()),
        },
    )
    .unwrap();
    ws.send(Message::Text(hello.to_json())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counters.lock().unwrap().hello, 1, "hello should be processed once");

    // === Send a notification ===
    let n = Envelope::new(
        MessageType::NotificationReceived,
        device_id,
        NotificationReceived {
            id: "notif-1".into(),
            package: "com.test".into(),
            app_name: Some("Test".into()),
            title: "Hello".into(),
            content: "World".into(),
            posted_at: 1717000000,
            is_sensitive: false,
            category: None,
        },
    )
    .unwrap();
    ws.send(Message::Text(n.to_json())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counters.lock().unwrap().notification, 1);

    // === Send an SMS ===
    let s = Envelope::new(
        MessageType::SmsReceived,
        device_id,
        phonebridge_proto::SmsReceived {
            id: "sms-1".into(),
            address: "+8613800000000".into(),
            body: "hi".into(),
            received_at: 1717000001,
            sim_slot: Some(0),
            subscription_id: Some(1),
        },
    )
    .unwrap();
    ws.send(Message::Text(s.to_json())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counters.lock().unwrap().sms_received, 1);

    // === Send an incoming call ===
    let c = Envelope::new(
        MessageType::CallIncoming,
        device_id,
        phonebridge_proto::CallIncoming {
            phone_number: "+8613800000000".into(),
            contact_name: Some("Alice".into()),
            sim_slot: Some(0),
        },
    )
    .unwrap();
    ws.send(Message::Text(c.to_json())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counters.lock().unwrap().call_incoming, 1);

    // Close the connection.
    let _ = ws.send(Message::Close(None)).await;
    let _ = task.await;

    // === Verify storage ===
    let notifs = db_arc.list_notifications(None, 100, false, None).await.unwrap();
    assert_eq!(notifs.len(), 1);
    assert_eq!(notifs[0].id, "notif-1");
    assert_eq!(notifs[0].title, "Hello");

    let sms = db_arc.list_sms(None, None, 100).await.unwrap();
    assert_eq!(sms.len(), 1);
    assert_eq!(sms[0].phone_number, "+8613800000000");
    assert_eq!(sms[0].direction, SmsDirection::In.as_str());

    let calls = db_arc.list_calls(None, 100).await.unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].phone_number, "+8613800000000");
    assert_eq!(calls[0].state, "ringing");
    assert_eq!(calls[0].direction, "incoming");

    // Audit log should have ws.connected and ws.closed.
    let log = db_arc.list_audit_log(100).await.unwrap();
    let events: Vec<&str> = log.iter().map(|e| e.event.as_str()).collect();
    assert!(events.contains(&"ws.connected"), "missing ws.connected: {events:?}");
    assert!(events.contains(&"ws.closed"), "missing ws.closed: {events:?}");

    // Device row should exist.
    let dev = db_arc.get_device(device_id).await.unwrap();
    assert!(dev.is_some());
    let dev = dev.unwrap();
    assert_eq!(dev.name, "TestPixel");
}

/// Verify the test_context helper in the daemon crate produces a usable context.
#[tokio::test]
async fn test_context_helper_works() {
    let db = Db::open_memory().await.unwrap();
    db.migrate().await.unwrap();
    let our_id = Uuid::new_v4();
    let _ctx = test_context(our_id, Arc::new(db));
    // (No assertions needed; we just check it compiles and constructs.)
}
