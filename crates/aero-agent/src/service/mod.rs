//! N08: one in-process, role-scoped application interface over the run
//! controllers and the journal. CLI, desktop and model-facing adapters all
//! go through a [`Session`]; there is no alternate executor path, no network
//! socket and no daemon here.
//!
//! The host constructs [`LocalService`] and hands out sessions: trusted
//! sessions to the CLI/desktop after a trusted human channel exists, and
//! model sessions scoped to one task to model adapters. A role is a
//! host-constructed value, never a JSON string, and every task or run lookup
//! checks it; a model asking about another task gets `NotFound`.
//!
//! A model session may read its own task, subscribe to its runs, pause them
//! and request approvals; only a trusted session creates or revises tasks,
//! starts, resumes, cancels or takes over runs, replies, and grants
//! approvals. Methods with no backend yet return [`ServiceError::Unsupported`]
//! rather than pretend: `observe` and `propose_action` (E05),
//! `get_recovery_report` (F08).
//!
//! A model session cannot be widened into a trusted one:
//! ```compile_fail,E0451
//! fn widen<F>(s: aero_agent::service::Session<F>) -> aero_agent::service::Session<F> {
//!     aero_agent::service::Session { role: todo!(), ..s }
//! }
//! ```

mod local;

pub use local::{EventStream, LocalService, Session};

use crate::approval::Refusal;
use crate::controller::{Automation, ControlError, Snapshot};
use crate::store::StoreError;
use aero_core::checkpoint::RunContext;
use aero_core::events::PublicEvent;

/// Builds the automation for a run the service starts (E05 provides the
/// real one; tests use fakes).
pub trait AutomationFactory: Send + Sync + 'static {
    type Automation: Automation;
    fn automation(&self, context: &RunContext) -> Self::Automation;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// The caller's role may not do this.
    PermissionDenied,
    /// No such task, run or request in the caller's scope.
    NotFound,
    /// No backend exists for this method yet.
    Unsupported,
    Control(ControlError),
    Store(StoreError),
    Approval(Refusal),
}

impl From<StoreError> for ServiceError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::PermissionDenied => Self::PermissionDenied,
            error => Self::Store(error),
        }
    }
}

impl From<ControlError> for ServiceError {
    fn from(error: ControlError) -> Self {
        Self::Control(error)
    }
}

/// One item of a run's event stream.
pub enum StreamItem {
    Event(PublicEvent),
    /// Events were missed: the run's state now, from which the stream
    /// continues (the next event follows `snapshot.sequence`).
    Gap(Snapshot),
    /// The run's controller is gone.
    Closed,
}
