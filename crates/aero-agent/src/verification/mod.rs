//! Host-owned, read-only independent checks. No dispatch or result writer.
//!
//! A model-facing TaskClient has no criterion-result write API:
//! ```compile_fail,E0599
//! fn planner_cannot_write_passed_criteria(client: &aero_agent::tasks::TaskClient, result: aero_core::verification::CriterionResult) {
//!     client.record_criterion(result);
//! }
//! ```
//! This is an in-process host capability separation, not a Rust sandbox. The
//! trusted host must keep its registry, sources and raw JournalClient away from
//! model adapters. JournalClient still accepts trusted host StepOutcome reports;
//! N04 does not prove their provenance or connect them to this registry.
mod registry;
pub use registry::{Readback, ReadbackError, ReadbackQuery, ReadbackSource, VerifierRegistry};
