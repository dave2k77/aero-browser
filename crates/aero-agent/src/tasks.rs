//! Host-issued task access, not caller-selected roles in JSON.
//!
//! Only the trusted application bootstrap owns TaskAuthority/Journal. Give a
//! model-facing adapter just its scoped TaskClient, never the factory, Journal,
//! raw storage client, filesystem access or an owner client. This is an in-process
//! capability boundary, not a sandbox for hostile Rust or a compromised same UID.
//! N08 owns adapter authentication; N06 owns effect grants; N13 owns data transfer.
//!
//! Clients cannot be decoded from model-selected role strings:
//! ```compile_fail,E0277
//! let _: aero_agent::tasks::TaskClient = serde_json::from_str("{\"role\":\"owner\"}").unwrap();
//! ```
//! Their authority and queue cannot be constructed or replaced externally:
//! ```compile_fail
//! let client = aero_agent::tasks::TaskClient {};
//! ```
//! Bare storage clients cannot bypass this task-write boundary:
//! ```compile_fail,E0624
//! fn bypass(client: &aero_agent::store::worker::JournalClient, contract: aero_core::task::TaskContract) {
//!     client.create_task(contract);
//! }
//! ```

use crate::store::StoreError;
use crate::store::worker::{AuditPage, Journal, JournalClient, Reply, ResumeState};
use aero_core::checkpoint::RunContext;
use aero_core::ids::TaskId;
use aero_core::task::{MAX_REFERENCE_BYTES, StartPrerequisites, TaskContract};

/// Privileged local-owner bootstrap. Never provide this object to model adapters.
/// The factory is bound to one journal, not a reusable global "trusted" token.
pub struct TaskAuthority {
    journal: JournalClient,
}
impl TaskAuthority {
    pub fn new(journal: &Journal) -> Self {
        Self {
            journal: journal.client(),
        }
    }
    /// Called only after the host has established a trusted human channel.
    pub fn trusted_user(&self) -> TaskClient {
        TaskClient {
            journal: self.journal.clone(),
            access: Access::Owner,
        }
    }
    /// Host-selected task scope. An ID does not authenticate a remote requester.
    pub fn model_for_task(&self, task_id: TaskId) -> Result<TaskClient, StoreError> {
        if task_id.as_str().trim().is_empty()
            || task_id.as_str().len() > MAX_REFERENCE_BYTES
            || task_id.as_str().contains('\0')
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(TaskClient {
            journal: self.journal.clone(),
            access: Access::Model(task_id),
        })
    }
}
#[derive(Clone)]
enum Access {
    Owner,
    Model(TaskId),
}
/// A fixed host-issued access scope. No Serialize/Deserialize/Debug, raw-client
/// getter, role setter, authority factory, grant or completion method is exposed.
/// Scope is journal + stable TaskId, not a user/revision/run or revocable login.
/// Do not reuse a delegated TaskId for unrelated work: delete/recreate in the
/// same journal lifetime would reuse that scope. Journal close ends admission.
#[derive(Clone)]
pub struct TaskClient {
    journal: JournalClient,
    access: Access,
}
impl TaskClient {
    fn require_owner(&self) -> Result<(), StoreError> {
        match self.access {
            Access::Owner => Ok(()),
            Access::Model(_) => Err(StoreError::PermissionDenied),
        }
    }
    fn require_task(&self, task: &TaskId) -> Result<(), StoreError> {
        match &self.access {
            Access::Owner => Ok(()),
            Access::Model(id) if id == task => Ok(()),
            _ => Err(StoreError::PermissionDenied),
        }
    }
    pub fn create_task(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.create_task(contract)
    }
    pub fn revise_contract(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.revise_contract(contract)
    }
    /// Records a paused run intent, not a launched browser, verified plan or effect grant.
    /// Prerequisites are observations supplied by the trusted host, never model JSON.
    pub fn start_run(
        &self,
        context: RunContext,
        prerequisites: StartPrerequisites,
    ) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.start_run(context, prerequisites)
    }
    /// Task-scoped data. The host must still apply privacy/data-flow policy before
    /// sending private intent/history to a provider or another recipient.
    pub fn load_resume(&self, context: RunContext) -> Result<Reply<ResumeState>, StoreError> {
        self.require_task(&context.task_id)?;
        self.journal.load_resume(context)
    }
    pub fn load_audit(
        &self,
        context: RunContext,
        after_step: u64,
        limit: usize,
    ) -> Result<Reply<AuditPage>, StoreError> {
        self.require_task(&context.task_id)?;
        self.journal.load_audit(context, after_step, limit)
    }
}
