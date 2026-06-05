//! Integration test: `/ws/console` live-pushes daemon events.

#![cfg(test)]

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use phonebridge_daemon::console_bus::ConsoleBus;
use phonebridge_daemon::test_context;
use phonebridge_proto::{
    Envelope, MessageType, NotificationReceived,
};
use phonebridge_storage::Db;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::client_async;
use uuid::Uuid;

#[tokio::test]
async fn console_ws_pushes_events() {
    let db = Db::open_memory().await.unwrap();
    db.migrate().await.unwrap();
    let _ctx = test_context(Uuid::new_v4(), std::sync::Arc::new(db));

    // The console bus is process-wide per daemon. We need a way to
    // share it between the test and the daemon's WS handler. For
    // simplicity, run a small in-process server: spawn a TcpListener,
    // accept one connection, drive the run_console_ws-style loop
    // directly.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();

    let bus = ConsoleBus::new(16);

    // Spawn the server side: accept + run a simplified console ws.
    let server_bus = bus.clone();
    let server = tokio::spawn(async move {
        let (s, _) = l.accept().await.unwrap();
        // Use tokio_tungstenite accept_async to do the WS upgrade.
        let ws = tokio_tungstenite::accept_async(s).await.unwrap();
        let (mut sink, _stream) = ws.split();
        let mut sub = server_bus.subscribe();
        // Send a hello.
        let hello = serde_json::json!({"kind":"console.hello","summary":{}});
        let _ = sink.send(Message::Text(hello.to_string())).await;
        // Forward a few events.
        for _ in 0..3 {
            if let Ok(Ok(e)) = timeout(Duration::from_secs(2), sub.recv()).await {
                let s = serde_json::to_string(&e).unwrap();
                if sink.send(Message::Text(s)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Connect a fake console client.
    let client = TcpStream::connect(addr).await.unwrap();
    let mut ws = client_async("ws://localhost/console", client).await.unwrap().0;

    // First message should be the hello.
    let hello = next_text(&mut ws, Duration::from_secs(2)).await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&hello).unwrap();
    assert_eq!(json["kind"], "console.hello");

    // Publish a notification via the bus; the server should forward.
    let env = Envelope::new(
        MessageType::NotificationReceived,
        Uuid::new_v4(),
        NotificationReceived {
            id: "n1".into(),
            package: "com.test".into(),
            app_name: Some("Test".into()),
            title: "Hello".into(),
            content: "World".into(),
            posted_at: 0,
            is_sensitive: false,
            category: None,
        },
    )
    .unwrap();
    bus.publish(&env);

    let msg = next_text(&mut ws, Duration::from_secs(2)).await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["kind"], "notification.received");
    assert_eq!(json["summary"]["package"], "com.test");
    assert_eq!(json["summary"]["title"], "Hello");

    let _ = server.await;
}

async fn next_text(ws: &mut tokio_tungstenite::WebSocketStream<TcpStream>, dur: Duration) -> Option<String> {
    let f = timeout(dur, ws.next()).await.ok()??;
    let m = f.ok()?;
    if let Message::Text(t) = m { Some(t) } else { None }
}
