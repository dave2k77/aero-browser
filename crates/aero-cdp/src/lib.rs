//! Owned browser transport and lifecycle boundary.
//!
//! C01/C02 provide the CDP WebSocket transport. C03 ([`launcher`]) constructs
//! the Bubblewrap launch command as pure data; spawning, readiness, targets,
//! frames and page sessions are later packages. A connection here proves
//! nothing about sandboxing or about which page a session belongs to.

mod connection;
mod error;
pub mod launcher;

pub use connection::{CdpConnection, CdpEvent, EventStream, TransportLimits};
pub use error::CdpError;
