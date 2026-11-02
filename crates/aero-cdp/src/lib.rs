//! Owned browser transport and lifecycle boundary.
//!
//! C01/C02 provide the CDP WebSocket transport. C03 ([`launcher`]) constructs
//! the Bubblewrap launch command as pure data; C04 ([`runtime`]) owns the
//! sandboxed process from spawn to verified teardown, and its
//! [`runtime::BrowserRuntime`] owns process, connection and targets together.
//! C05 ([`target_registry`], [`page`]) discovers targets and attaches tabs with
//! flattened sessions. Frames and execution contexts are later packages.

mod connection;
mod error;
pub mod launcher;
pub mod page;
pub mod runtime;
pub mod target_registry;

pub use connection::{CdpConnection, CdpEvent, EventStream, TransportLimits};
pub use error::CdpError;
