//! Pure versioned domain contracts. No browser, storage, or provider runtime.

pub mod actions;
pub mod checkpoint;
pub mod events;
pub mod form;
pub mod ids;
pub mod types;
mod wire;

pub use wire::ValidationError;

/// Version of the shared domain contract crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
