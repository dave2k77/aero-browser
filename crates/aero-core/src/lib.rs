//! Pure versioned domain contracts. No browser, storage, or provider runtime.

pub mod ids;
pub mod types;

/// Version of the shared domain contract crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
