//! Owned browser transport and lifecycle boundary.
//!
//! C01/C02 provide the CDP WebSocket transport only. Process launch, targets,
//! frames and page sessions are later packages; a connection here proves
//! nothing about sandboxing or about which page a session belongs to.

mod connection;
mod error;

pub use connection::{CdpConnection, CdpEvent, EventStream, TransportLimits};
pub use error::CdpError;
