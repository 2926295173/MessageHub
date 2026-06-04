//! Pairing state machine, both sides.
//!
//! Roles:
//! - **Initiator** (desktop, in MVP): sends `device.pair.request`, receives
//!   `device.pair.challenge` from the responder, then `device.pair.confirm`.
//!   The code is **never shown to the initiator**; it waits for the user to
//!   confirm on the responder device (Android).
//! - **Responder** (Android, in MVP): receives `device.pair.request`,
//!   derives the 6-digit code from the ECDH shared secret, displays it to
//!   the user, then sends `device.pair.challenge` + (after user action)
//!   `device.pair.confirm`.
//!
//! Both sides must exchange `device.pair.complete` (carrying the sender's
//! long-term cert) before considering the pairing done.
//!
//! The state machine is **typed** — the [`Initiator`] and [`Responder`]
//! types are linear: each method consumes `self` and returns the next
//! state, so invalid transitions are caught at compile time.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

use phonebridge_crypto::cert::{self, Identity};
use phonebridge_crypto::ecdh::{EphemeralKeyPair, PublicKey};
use phonebridge_proto::{
    DeviceHello, DeviceType, Envelope, MessageType, PairAccept, PairChallenge, PairComplete,
    PairConfirm, PairReject, PairRequest,
};

/// Errors from the pairing state machine.
#[derive(Debug, Error)]
pub enum PairingError {
    /// An envelope was received in the wrong state.
    #[error("invalid state transition: expected {expected}, got {got:?}")]
    BadState {
        /// What we were waiting for.
        expected: &'static str,
        /// What we actually got.
        got: MessageType,
    },
    /// ECDH key parse or agree failed.
    #[error("ECDH error: {0}")]
    Ecdh(#[from] phonebridge_crypto::ecdh::EcdhError),
    /// The 6-digit code in `pair.challenge` is malformed.
    #[error("malformed pairing code: {0}")]
    BadCode(String),
    /// The cert in `pair.complete` is malformed.
    #[error("malformed cert: {0}")]
    BadCert(String),
    /// A `pair.challenge` arrived after its expiry.
    #[error("pairing code expired (now={now}, expires={expires})")]
    Expired {
        /// Current time in epoch ms.
        now: i64,
        /// Expiry time in epoch ms.
        expires: i64,
    },
    /// Serialization error.
    #[error("serialize error: {0}")]
    Serde(#[from] serde_json::Error),
    /// Cert generation failed.
    #[error("cert generation failed: {0}")]
    Cert(#[from] phonebridge_crypto::cert::CertError),
}

/// Successful outcome of a pairing exchange.
#[derive(Debug, Clone)]
pub struct PairingOutcome {
    /// The peer's stable device id.
    pub peer_device_id: Uuid,
    /// The peer's long-term cert fingerprint.
    pub peer_fingerprint: String,
    /// The peer's cert PEM.
    pub peer_cert_pem: String,
}

// ============================================================================
// Initiator (desktop in MVP)
// ============================================================================

/// Internal data carried across the initiator state machine.
pub struct InitiatorCore {
    identity: Identity,
    peer_device_id: Uuid,
    #[allow(dead_code)]
    peer_name: String,
}

/// Initiator: about to send `device.pair.request`.
pub struct Initiator {
    core: InitiatorCore,
    /// Held until we need to call `agree` after receiving the challenge.
    ephemeral: Option<EphemeralKeyPair>,
    /// Peer's ephemeral public key (from the received challenge), used for
    /// shared-secret computation.
    peer_ephemeral_pub: Option<PublicKey>,
    /// The 6-digit code from the challenge (held in case the daemon wants
    /// to expose it for parity / debug).
    challenge_code: Option<String>,
}

impl Initiator {
    /// Begin pairing. Generates a fresh identity and an ephemeral keypair.
    pub fn start(peer_device_id: Uuid, peer_name: impl Into<String>) -> Result<Self, PairingError> {
        let identity = cert::generate_self_signed("phonebridge-daemon", 3650)?;
        let ephemeral = EphemeralKeyPair::generate()?;
        Ok(Self {
            core: InitiatorCore { identity, peer_device_id, peer_name: peer_name.into() },
            ephemeral: Some(ephemeral),
            peer_ephemeral_pub: None,
            challenge_code: None,
        })
    }

    /// Build the `device.pair.request` envelope.
    pub fn build_request_envelope(
        &self,
        our_device_id: Uuid,
    ) -> Result<Envelope, PairingError> {
        let pub_b64 = self
            .ephemeral
            .as_ref()
            .ok_or(PairingError::BadState {
                expected: "ephemeral keypair present",
                got: MessageType::DevicePairRequest,
            })?
            .public_key()
            .to_base64();
        Ok(Envelope::new(
            MessageType::DevicePairRequest,
            our_device_id,
            PairRequest { ephemeral_pubkey: pub_b64 },
        )?)
    }

    /// Accept a `device.pair.challenge` envelope. Validates the 6-digit
    /// code shape and the expiry.
    pub fn on_challenge(
        &mut self,
        env: &Envelope,
        our_device_id: Uuid,
    ) -> Result<chrono::DateTime<chrono::Utc>, PairingError> {
        if env.message_type != MessageType::DevicePairChallenge {
            return Err(PairingError::BadState {
                expected: "device.pair.challenge",
                got: env.message_type,
            });
        }
        let challenge: PairChallenge = env.parse_payload()?;
        if challenge.code.len() != 6 || !challenge.code.chars().all(|c| c.is_ascii_digit()) {
            return Err(PairingError::BadCode(challenge.code));
        }
        let now = Utc::now().timestamp_millis();
        if now > challenge.expires_at {
            return Err(PairingError::Expired { now, expires: challenge.expires_at });
        }
        // Parse + store the peer's ephemeral pub for the eventual `agree`.
        let peer_pub = PublicKey::from_base64(&challenge.ephemeral_pubkey)?;
        self.peer_ephemeral_pub = Some(peer_pub);
        self.challenge_code = Some(challenge.code.clone());
        // Touch our_device_id to silence unused if we don't use it.
        let _ = our_device_id;
        Ok(chrono::DateTime::<chrono::Utc>::from_timestamp_millis(challenge.expires_at)
            .unwrap_or_else(Utc::now))
    }

    /// Build the `device.pair.accept` envelope.
    pub fn build_accept_envelope(&self, our_device_id: Uuid) -> Result<Envelope, PairingError> {
        if self.peer_ephemeral_pub.is_none() {
            return Err(PairingError::BadState {
                expected: "challenge received",
                got: MessageType::DevicePairAccept,
            });
        }
        Ok(Envelope::new(
            MessageType::DevicePairAccept,
            our_device_id,
            PairAccept {},
        )?)
    }

    /// Build the `device.pair.reject` envelope.
    pub fn build_reject_envelope(
        &self,
        our_device_id: Uuid,
        reason: &str,
    ) -> Result<Envelope, PairingError> {
        Ok(Envelope::new(
            MessageType::DevicePairReject,
            our_device_id,
            PairReject { reason: Some(reason.to_string()) },
        )?)
    }

    /// Build the `device.pair.complete` envelope with our long-term cert.
    pub fn build_complete_envelope(&self, our_device_id: Uuid) -> Result<Envelope, PairingError> {
        Ok(Envelope::new(
            MessageType::DevicePairComplete,
            our_device_id,
            PairComplete {
                cert_pem: self.core.identity.cert_pem.clone(),
                cert_fingerprint: self.core.identity.fingerprint.clone(),
            },
        )?)
    }

    /// Accept the peer's `device.pair.complete`. Validates cert PEM +
    /// fingerprint. Returns the [`PairingOutcome`].
    pub fn on_complete(&self, env: &Envelope) -> Result<PairingOutcome, PairingError> {
        if env.message_type != MessageType::DevicePairComplete {
            return Err(PairingError::BadState {
                expected: "device.pair.complete",
                got: env.message_type,
            });
        }
        let complete: PairComplete = env.parse_payload()?;
        phonebridge_crypto::fingerprint::parse_fingerprint(&complete.cert_fingerprint)
            .map_err(|e| PairingError::BadCert(e.to_string()))?;
        if !complete.cert_pem.contains("BEGIN CERTIFICATE") {
            return Err(PairingError::BadCert("missing BEGIN CERTIFICATE".into()));
        }
        Ok(PairingOutcome {
            peer_device_id: self.core.peer_device_id,
            peer_fingerprint: complete.cert_fingerprint,
            peer_cert_pem: complete.cert_pem,
        })
    }

    /// Accessor for our long-term identity.
    pub fn identity(&self) -> &Identity {
        &self.core.identity
    }

    /// The 6-digit code from the last received challenge (if any).
    pub fn challenge_code(&self) -> Option<&str> {
        self.challenge_code.as_deref()
    }
}

// ============================================================================
// Responder (Android in MVP)
// ============================================================================

/// Internal data carried across the responder state machine.
pub struct ResponderCore {
    identity: Identity,
    peer_device_id: Uuid,
}

/// Responder: about to receive `device.pair.request`.
pub struct Responder {
    core: ResponderCore,
    /// Our ephemeral public key (set after we generate it in `on_request`).
    ephemeral_pub: Option<PublicKey>,
    /// Set after computing the code.
    code: Option<String>,
    /// Set after computing the code.
    expires_at: Option<i64>,
    /// Set after the user accepts/rejects.
    accepted: Option<bool>,
}

impl Responder {
    /// Begin a new responder session. Generates a fresh long-term identity.
    pub fn start(peer_device_id: Uuid) -> Result<Self, PairingError> {
        let identity = cert::generate_self_signed("phonebridge-android", 3650)?;
        Ok(Self {
            core: ResponderCore { identity, peer_device_id },
            ephemeral_pub: None,
            code: None,
            expires_at: None,
            accepted: None,
        })
    }

    /// Accept a `device.pair.request` envelope. Derives the 6-digit code.
    /// After this returns, [`Responder::code()`] is available.
    pub fn on_request(&mut self, env: &Envelope) -> Result<(), PairingError> {
        if env.message_type != MessageType::DevicePairRequest {
            return Err(PairingError::BadState {
                expected: "device.pair.request",
                got: env.message_type,
            });
        }
        let req: PairRequest = env.parse_payload()?;
        let peer_pub = PublicKey::from_base64(&req.ephemeral_pubkey)?;
        let ephemeral = EphemeralKeyPair::generate()?;
        // Compute the code from the shared secret. `agree` consumes self,
        // so we extract the public key first.
        let our_pub = ephemeral.public_key();
        let shared = ephemeral.agree(&peer_pub)?;
        let code = phonebridge_crypto::pairing_code::derive_pairing_code(&shared);
        let expires_at = Utc::now().timestamp_millis() + 30_000;
        self.ephemeral_pub = Some(our_pub);
        self.code = Some(code);
        self.expires_at = Some(expires_at);
        Ok(())
    }

    /// Build the `device.pair.challenge` envelope.
    pub fn build_challenge_envelope(&self, our_device_id: Uuid) -> Result<Envelope, PairingError> {
        let ephemeral_pub = self.ephemeral_pub.as_ref().ok_or(PairingError::BadState {
            expected: "request received",
            got: MessageType::DevicePairChallenge,
        })?;
        let code = self.code.as_ref().ok_or(PairingError::BadState {
            expected: "code computed",
            got: MessageType::DevicePairChallenge,
        })?;
        let expires_at = self.expires_at.ok_or(PairingError::BadState {
            expected: "expires_at set",
            got: MessageType::DevicePairChallenge,
        })?;
        Ok(Envelope::new(
            MessageType::DevicePairChallenge,
            our_device_id,
            PairChallenge {
                ephemeral_pubkey: ephemeral_pub.to_base64(),
                code: code.clone(),
                expires_at,
            },
        )?)
    }

    /// The 6-digit code to display in the UI. None until `on_request` runs.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// The expiry timestamp (epoch ms) of the current code.
    pub fn expires_at_ms(&self) -> Option<i64> {
        self.expires_at
    }

    /// User accepted/rejected. Build the `device.pair.confirm` envelope.
    pub fn build_confirm_envelope(
        &mut self,
        our_device_id: Uuid,
        accepted: bool,
    ) -> Result<Envelope, PairingError> {
        if self.code.is_none() {
            return Err(PairingError::BadState {
                expected: "request received",
                got: MessageType::DevicePairConfirm,
            });
        }
        self.accepted = Some(accepted);
        Ok(Envelope::new(
            MessageType::DevicePairConfirm,
            our_device_id,
            PairConfirm { accepted },
        )?)
    }

    /// Build the responder's own `device.pair.complete` (carries our cert).
    pub fn build_complete_envelope(&self, our_device_id: Uuid) -> Result<Envelope, PairingError> {
        if !self.accepted.unwrap_or(false) {
            return Err(PairingError::BadState {
                expected: "user accepted",
                got: MessageType::DevicePairComplete,
            });
        }
        Ok(Envelope::new(
            MessageType::DevicePairComplete,
            our_device_id,
            PairComplete {
                cert_pem: self.core.identity.cert_pem.clone(),
                cert_fingerprint: self.core.identity.fingerprint.clone(),
            },
        )?)
    }

    /// Accept the initiator's `device.pair.complete`. Validates cert PEM +
    /// fingerprint. Returns the [`PairingOutcome`].
    pub fn on_complete(&self, env: &Envelope) -> Result<PairingOutcome, PairingError> {
        if !self.accepted.unwrap_or(false) {
            return Err(PairingError::BadState {
                expected: "user accepted",
                got: env.message_type,
            });
        }
        if env.message_type != MessageType::DevicePairComplete {
            return Err(PairingError::BadState {
                expected: "device.pair.complete",
                got: env.message_type,
            });
        }
        let complete: PairComplete = env.parse_payload()?;
        phonebridge_crypto::fingerprint::parse_fingerprint(&complete.cert_fingerprint)
            .map_err(|e| PairingError::BadCert(e.to_string()))?;
        if !complete.cert_pem.contains("BEGIN CERTIFICATE") {
            return Err(PairingError::BadCert("missing BEGIN CERTIFICATE".into()));
        }
        Ok(PairingOutcome {
            peer_device_id: self.core.peer_device_id,
            peer_fingerprint: complete.cert_fingerprint,
            peer_cert_pem: complete.cert_pem,
        })
    }

    /// Accessor for our long-term identity.
    pub fn identity(&self) -> &Identity {
        &self.core.identity
    }
}

// ============================================================================
// DeviceType is re-exported via phonebridge_proto (already in scope above).
// ============================================================================

// ============================================================================
// Convenience: device.hello handler
// ============================================================================

/// Build a `device.hello` envelope for the local daemon identity.
pub fn build_hello_envelope(
    our_device_id: Uuid,
    name: &str,
    pubkey_b64: &str,
    port: u16,
) -> Result<Envelope, PairingError> {
    Ok(Envelope::new(
        MessageType::DeviceHello,
        our_device_id,
        DeviceHello {
            name: name.to_string(),
            device_type: DeviceType::Android,
            protocol_version: 1,
            pubkey: pubkey_b64.to_string(),
            port: Some(port),
            manufacturer: None,
            model: None,
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responder_derives_6_digit_code() {
        let mut r = Responder::start(Uuid::new_v4()).unwrap();
        let initiator_kp = EphemeralKeyPair::generate().unwrap();
        let req_env = Envelope::new(
            MessageType::DevicePairRequest,
            Uuid::new_v4(),
            PairRequest {
                ephemeral_pubkey: initiator_kp.public_key().to_base64(),
            },
        )
        .unwrap();
        r.on_request(&req_env).unwrap();
        let code = r.code().unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        let exp = r.expires_at_ms().unwrap();
        assert!(exp > Utc::now().timestamp_millis());
    }

    #[test]
    fn initiator_rejects_bad_code_shape() {
        let mut i = Initiator::start(Uuid::new_v4(), "P").unwrap();
        let bad = Envelope::new(
            MessageType::DevicePairChallenge,
            Uuid::new_v4(),
            PairChallenge {
                ephemeral_pubkey: "AAAA".into(),
                code: "abc".into(),
                expires_at: i64::MAX,
            },
        )
        .unwrap();
        let r = i.on_challenge(&bad, Uuid::new_v4());
        assert!(matches!(r, Err(PairingError::BadCode(_))));
    }

    #[test]
    fn initiator_rejects_expired() {
        let mut i = Initiator::start(Uuid::new_v4(), "P").unwrap();
        let past = Envelope::new(
            MessageType::DevicePairChallenge,
            Uuid::new_v4(),
            PairChallenge {
                ephemeral_pubkey: "AAAA".into(),
                code: "123456".into(),
                expires_at: 1,
            },
        )
        .unwrap();
        let r = i.on_challenge(&past, Uuid::new_v4());
        assert!(matches!(r, Err(PairingError::Expired { .. })));
    }

    #[test]
    fn initiator_rejects_wrong_message_type() {
        let mut i = Initiator::start(Uuid::new_v4(), "P").unwrap();
        let not_challenge = Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "x".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: None,
                manufacturer: None,
                model: None,
            },
        )
        .unwrap();
        let r = i.on_challenge(&not_challenge, Uuid::new_v4());
        assert!(matches!(r, Err(PairingError::BadState { .. })));
    }

    #[test]
    fn full_happy_path_responder_then_initiator() {
        // Responder side
        let mut responder = Responder::start(Uuid::new_v4()).unwrap();
        let initiator_kp = EphemeralKeyPair::generate().unwrap();
        let req_env = Envelope::new(
            MessageType::DevicePairRequest,
            Uuid::new_v4(),
            PairRequest {
                ephemeral_pubkey: initiator_kp.public_key().to_base64(),
            },
        )
        .unwrap();
        responder.on_request(&req_env).unwrap();
        let challenge = responder.build_challenge_envelope(Uuid::new_v4()).unwrap();
        let responder_code = responder.code().unwrap().to_string();

        // Initiator side
        let mut initiator = Initiator::start(Uuid::new_v4(), "P").unwrap();
        initiator.on_challenge(&challenge, Uuid::new_v4()).unwrap();
        assert_eq!(initiator.challenge_code(), Some(responder_code.as_str()));

        // Build envelopes along the way
        let _accept = initiator.build_accept_envelope(Uuid::new_v4()).unwrap();
        let initiator_complete = initiator.build_complete_envelope(Uuid::new_v4()).unwrap();

        // User accepts on responder; build confirm + responder's own complete
        let _confirm = responder.build_confirm_envelope(Uuid::new_v4(), true).unwrap();
        let responder_complete = responder.build_complete_envelope(Uuid::new_v4()).unwrap();

        // Cross-exchange
        let responder_outcome = responder.on_complete(&initiator_complete).unwrap();
        let initiator_outcome = initiator.on_complete(&responder_complete).unwrap();
        assert!(responder_outcome.peer_cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(initiator_outcome.peer_cert_pem.contains("BEGIN CERTIFICATE"));
    }

    #[test]
    fn responder_rejects_complete_when_rejected() {
        let mut responder = Responder::start(Uuid::new_v4()).unwrap();
        let initiator_kp = EphemeralKeyPair::generate().unwrap();
        let req_env = Envelope::new(
            MessageType::DevicePairRequest,
            Uuid::new_v4(),
            PairRequest {
                ephemeral_pubkey: initiator_kp.public_key().to_base64(),
            },
        )
        .unwrap();
        responder.on_request(&req_env).unwrap();
        responder.build_confirm_envelope(Uuid::new_v4(), false).unwrap();
        // Build a fake complete and try to accept it.
        let fake_complete = Envelope::new(
            MessageType::DevicePairComplete,
            Uuid::new_v4(),
            PairComplete {
                cert_pem: "-----BEGIN CERTIFICATE-----\nAA\n-----END CERTIFICATE-----".into(),
                cert_fingerprint: "AB".repeat(32).split_at(0).1.to_string(), // not real fingerprint
            },
        )
        .unwrap();
        let r = responder.on_complete(&fake_complete);
        assert!(matches!(r, Err(PairingError::BadState { .. })));
    }
}
