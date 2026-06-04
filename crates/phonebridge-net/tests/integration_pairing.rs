//! End-to-end pairing integration test.
//!
//! Drives a full pairing handshake between an `Initiator` and a `Responder`
//! over a tokio mpsc channel. This isolates the state-machine behavior
//! from the WebSocket/TLS stack (which is covered by separate tests in the
//! daemon and by the e2e smoke script).
//!
//! This test is the wire-level smoke for the protocol: every message type
//! in the pairing flow is exercised, and the final outcome is verified
//! to carry a valid cert PEM + 32-pair fingerprint.

#![cfg(test)]

use std::time::Duration;

use phonebridge_net::pairing::{Initiator, Responder};
use phonebridge_proto::{Envelope, MessageType};
use tokio::sync::mpsc;
use tokio::time::timeout;

#[tokio::test]
async fn full_pairing_handshake_over_mpsc() {
    // Two mpsc channels: one for each direction.
    let (tx_init, mut rx_resp) = mpsc::channel::<Envelope>(8);
    let (tx_resp, mut rx_init) = mpsc::channel::<Envelope>(8);

    let android_id = uuid::Uuid::new_v4();
    let daemon_id = uuid::Uuid::new_v4();

    // === Initiator side (desktop) ===
    let initiator_task = tokio::spawn(async move {
        let mut initiator = Initiator::start(android_id, "TestAndroid").unwrap();
        // Build pair.request.
        let req_env = initiator.build_request_envelope(daemon_id).unwrap();
        tx_init.send(req_env).await.unwrap();

        // Receive pair.challenge.
        let env = recv_envelope(&mut rx_init).await;
        assert_eq!(env.message_type, MessageType::DevicePairChallenge);
        initiator.on_challenge(&env, daemon_id).unwrap();

        // Send accept.
        let accept = initiator.build_accept_envelope(daemon_id).unwrap();
        tx_init.send(accept).await.unwrap();

        // Receive pair.confirm.
        let env = recv_envelope(&mut rx_init).await;
        assert_eq!(env.message_type, MessageType::DevicePairConfirm);
        let confirm: phonebridge_proto::PairConfirm = env.parse_payload().unwrap();
        assert!(confirm.accepted);

        // Send complete.
        let complete = initiator.build_complete_envelope(daemon_id).unwrap();
        tx_init.send(complete).await.unwrap();

        // Receive responder's complete.
        let env = recv_envelope(&mut rx_init).await;
        assert_eq!(env.message_type, MessageType::DevicePairComplete);
        let outcome = initiator.on_complete(&env).unwrap();
        (initiator, outcome)
    });

    // === Responder side (android) ===
    let responder_task = tokio::spawn(async move {
        let mut responder = Responder::start(daemon_id).unwrap();

        // Receive pair.request.
        let env = recv_envelope(&mut rx_resp).await;
        assert_eq!(env.message_type, MessageType::DevicePairRequest);
        responder.on_request(&env).unwrap();
        let code = responder.code().unwrap().to_string();
        assert_eq!(code.len(), 6);

        // Send pair.challenge.
        let challenge = responder.build_challenge_envelope(android_id).unwrap();
        tx_resp.send(challenge).await.unwrap();

        // Receive pair.accept.
        let env = recv_envelope(&mut rx_resp).await;
        assert_eq!(env.message_type, MessageType::DevicePairAccept);

        // Send pair.confirm.
        let confirm = responder.build_confirm_envelope(android_id, true).unwrap();
        tx_resp.send(confirm).await.unwrap();

        // Receive pair.complete (from initiator).
        let env = recv_envelope(&mut rx_resp).await;
        assert_eq!(env.message_type, MessageType::DevicePairComplete);
        let outcome = responder.on_complete(&env).unwrap();

        // Send our own complete (so the initiator also gets a cert from us).
        let our_complete = responder.build_complete_envelope(android_id).unwrap();
        tx_resp.send(our_complete).await.unwrap();

        (responder, outcome)
    });

    let (initiator, init_outcome) = timeout(Duration::from_secs(5), initiator_task)
        .await
        .expect("initiator task did not finish in time")
        .unwrap();
    let (responder, resp_outcome) = timeout(Duration::from_secs(5), responder_task)
        .await
        .expect("responder task did not finish in time")
        .unwrap();

    // Both sides should agree on the peer's cert.
    // The initiator's peer is the android (the responder in this test).
    // The responder's peer is the daemon (the initiator in this test).
    assert_eq!(init_outcome.peer_device_id, android_id);
    assert!(init_outcome.peer_cert_pem.contains("BEGIN CERTIFICATE"));
    assert_eq!(init_outcome.peer_fingerprint.matches(':').count(), 31);

    assert_eq!(resp_outcome.peer_device_id, daemon_id);
    assert!(resp_outcome.peer_cert_pem.contains("BEGIN CERTIFICATE"));
    assert_eq!(resp_outcome.peer_fingerprint.matches(':').count(), 31);

    // The two fingerprints should differ (each side generated its own cert).
    assert_ne!(init_outcome.peer_fingerprint, resp_outcome.peer_fingerprint);

    // The init stored the code the resp put in the challenge.
    // The resp's code() returns the same value (computed in on_request).
    // They should match — that's the whole point of the parity check.
    let init_code = initiator.challenge_code().map(String::from);
    let resp_code = responder.code().map(String::from);
    assert_eq!(init_code, resp_code, "init challenge_code != resp code");
}

async fn recv_envelope(rx: &mut mpsc::Receiver<Envelope>) -> Envelope {
    timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("recv timed out")
        .expect("channel closed")
}

/// Sanity: the PairingMap can be shared across tasks and round-trips a
/// PairedSession.
#[tokio::test]
async fn pairing_map_basic() {
    use phonebridge_net::ws_handler::{DeviceSession, PairingMap, UnpairedSession};

    let map = PairingMap::new();
    let dev = uuid::Uuid::new_v4();
    let r = Responder::start(dev).unwrap();
    map.insert(dev, DeviceSession::Unpaired(UnpairedSession::Responder(r)));
    let got = map.get(&dev).unwrap();
    matches!(got, DeviceSession::Unpaired(UnpairedSession::Responder(_)));

    let paired = phonebridge_net::ws_handler::PairedSession {
        device_id: dev,
        name: "x".into(),
        cert_fingerprint: "DE:AD".into(),
    };
    map.insert(dev, DeviceSession::Paired(paired.clone()));
    let got = map.get(&dev).unwrap();
    matches!(got, DeviceSession::Paired(_));
    let list = map.list_paired();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].0, dev);
}
