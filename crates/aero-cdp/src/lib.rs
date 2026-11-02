//! Owned browser transport and lifecycle boundary.
//!
//! C01/C02 provide the CDP WebSocket transport. C03 ([`launcher`]) constructs
//! the Bubblewrap launch command as pure data; C04 ([`runtime`]) owns the
//! sandboxed process from spawn to verified teardown. Targets, frames and page
//! sessions are later packages; a connection proves nothing about which page a
//! session belongs to.

mod connection;
mod error;
pub mod launcher;
pub mod runtime;

pub use connection::{CdpConnection, CdpEvent, EventStream, TransportLimits};
pub use error::CdpError;
