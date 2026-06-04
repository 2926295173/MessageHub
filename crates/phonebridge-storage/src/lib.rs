//! SQLite storage layer.
//!
//! - [`migrations`]: SQL files run on startup.
//! - [`models`]: row structs and DTOs.
//! - [`pool`]: the [`Db`] connection pool wrapper.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod models;
pub mod pool;

pub use pool::Db;
