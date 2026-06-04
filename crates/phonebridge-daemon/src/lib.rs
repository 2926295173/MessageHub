//! Library surface for the daemon. Re-exports modules needed by integration
//! tests and the main binary.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod app_state;
pub mod cert_loader;
pub mod daemon_sink;
pub mod identity;
pub mod mdns_service;
pub mod pair_cli;
pub mod rest;
pub mod static_files;
pub mod tls;
pub mod ws;

pub use ws::test_context;

pub use app_state::AppState;
pub use daemon_sink::DaemonSink;
