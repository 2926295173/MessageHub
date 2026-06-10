// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Cryptographic primitives for PhoneBridge.
//!
//! - [`ecdh`]: ECDH P-256 keypair generation + shared-secret derivation (using `ring`).
//! - [`pairing_code`]: derive the 4-digit decimal pairing code via HKDF-SHA256.
//! - [`cert`]: generate a self-signed X.509 certificate for the daemon's long-term identity.
//! - [`fingerprint`]: SHA-256 fingerprint of a DER certificate.
//!
//! M1 implements the core building blocks + unit tests with RFC 6979-style
//! vectors where possible. M2 wires them into the actual pairing state
//! machine.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod cert;
pub mod ecdh;
pub mod fingerprint;
pub mod pairing_code;

pub use ecdh::{EphemeralKeyPair, PublicKey, SharedSecret};
pub use pairing_code::derive_pairing_code;
