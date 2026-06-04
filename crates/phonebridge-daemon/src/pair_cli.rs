//! One-shot CLI test client: acts as a fake Android. Opens a TLS-WS
//! connection to the daemon, sends `device.hello`, then waits up to 10s
//! for any further messages. Used by `scripts/e2e-smoke.sh` and for
//! manual debugging. The full pairing handshake requires the web console
//! "click pair" UX (M3); this client only validates the WS layer.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use phonebridge_core::Config;
use phonebridge_proto::{DeviceHello, DeviceType, Envelope, MessageType};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use tokio::time::timeout;
use tokio_tungstenite::client_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;
use tracing::{info, warn};

use crate::identity::DaemonIdentity;

pub async fn run(peer: SocketAddr, identity: DaemonIdentity, _config: Arc<Config>) -> Result<()> {
    info!(%peer, "pair_cli: connecting as fake android client");

    let mut tls_cfg = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyVerifier))
        .with_no_client_auth();
    let _ = rustls::crypto::ring::default_provider();
    tls_cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let connector = Connector::Rustls(Arc::new(tls_cfg));

    let host = peer.ip().to_string();
    let port = peer.port();
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port))
        .await
        .with_context(|| format!("tcp connect to {peer}"))?;
    let req: http::Uri = format!("wss://{}/ws", peer).parse().expect("valid url");
    let (mut ws, _resp) = client_async_tls_with_config(req, tcp, None, Some(connector))
        .await
        .with_context(|| "ws upgrade")?;
    info!("pair_cli: ws connected");

    // 1. Send our hello.
    let hello = Envelope::new(
        MessageType::DeviceHello,
        identity.device_id,
        DeviceHello {
            name: identity.name.clone(),
            device_type: DeviceType::Android,
            protocol_version: 1,
            pubkey: identity.public_key_b64.clone(),
            port: None,
            manufacturer: None,
            model: None,
        },
    )?;
    ws.send(Message::Text(hello.to_json())).await?;
    info!("pair_cli: sent device.hello");

    // 2. Wait for any incoming messages for up to 10 seconds, then exit.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut got_pair_request = false;
    while std::time::Instant::now() < deadline {
        let frame = match timeout(Duration::from_secs(2), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => t,
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                warn!("pair_cli: ws closed by peer");
                break;
            }
            Ok(Some(Ok(other))) => {
                info!(?other, "pair_cli: ignoring frame");
                continue;
            }
            Ok(Some(Err(e))) => return Err(e.into()),
            Err(_) => break, // no frame in 2s
        };

        let env: Envelope = serde_json::from_str(&frame)?;
        match env.message_type {
            MessageType::DevicePairRequest => {
                info!("pair_cli: received device.pair.request (daemon is initiator); exiting");
                got_pair_request = true;
                break;
            }
            other => {
                info!(message_type = %other, "pair_cli: received message");
            }
        }
    }

    if got_pair_request {
        println!("WS_OK: hello + pair.request round-tripped");
    } else {
        println!("WS_OK: hello accepted (daemon logged session); no pair.request observed (expected without web console trigger)");
    }
    let _ = ws.close(None).await;
    Ok(())
}

/// Accepts any server cert. **MVP only.** M3 will introduce a CA / fingerprint flow.
#[derive(Debug)]
struct AcceptAnyVerifier;

impl ServerCertVerifier for AcceptAnyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}
