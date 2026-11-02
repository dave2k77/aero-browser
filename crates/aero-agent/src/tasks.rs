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

use crate::config::PrivateState;
use crate::data_flow::{GatewayHost, GatewayLink};
use crate::store::StoreError;
use crate::store::worker::{AuditPage, Journal, JournalClient, Reply, ResumeState};
use aero_core::checkpoint::RunContext;
use aero_core::ids::TaskId;
use aero_core::task::{MAX_REFERENCE_BYTES, StartPrerequisites, TaskContract};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Live registered gateways per task; dead (dropped) gateways do not count.
pub const MAX_GATEWAYS_PER_TASK: usize = 64;

/// Weak links to registered N13 gateways, keyed by task. Owner-only.
#[derive(Clone, Default)]
struct Gateways(Arc<Mutex<HashMap<TaskId, Vec<GatewayLink>>>>);
impl Gateways {
    fn register(&self, link: GatewayLink) -> Result<(), StoreError> {
        // A panic mid-push cannot leave a link half-registered; recover.
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let links = map.entry(link.task().clone()).or_default();
        links.retain(GatewayLink::is_live);
        if links.len() >= MAX_GATEWAYS_PER_TASK {
            return Err(StoreError::BudgetExceeded);
        }
        links.push(link);
        Ok(())
    }
    /// Retire every registered gateway for `task`, outside the registry lock:
    /// retirement waits for in-flight sends and drops transports.
    fn retire(&self, task: &TaskId) {
        let links = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(task)
            .unwrap_or_default();
        for link in links {
            link.retire();
        }
    }
}

/// Privileged local-owner bootstrap. Never provide this object to model adapters.
/// The factory is bound to one journal, not a reusable global "trusted" token.
pub struct TaskAuthority {
    journal: JournalClient,
    gateways: Gateways,
}
impl TaskAuthority {
    pub fn new(journal: &Journal) -> Self {
        Self {
            journal: journal.client(),
            gateways: Gateways::default(),
        }
    }
    /// Called only after the host has established a trusted human channel.
    pub fn trusted_user(&self) -> TaskClient {
        TaskClient {
            journal: self.journal.clone(),
            access: Access::Owner(self.gateways.clone()),
        }
    }
    /// Bind a run's data-flow gateway to this authority's run lifecycle. The
    /// owner's `start_run` and `revise_contract` for the gateway's task retire
    /// it (irreversibly) before submitting the journal change, so an old run
    /// or revision cannot transfer data once the change can commit. Holds only
    /// a weak reference. At most [`MAX_GATEWAYS_PER_TASK`] live registrations.
    pub fn register_gateway(&self, host: &GatewayHost) -> Result<(), StoreError> {
        self.gateways.register(host.link())
    }
    /// Coordinated task deletion (N16). Blocking: call from a host thread, not
    /// an async executor.
    ///
    /// 1. Retire the task's registered gateways (not undone if later refused).
    /// 2. Take the task lock without waiting: a live `TaskLease`, store or
    ///    runtime handle makes this Busy and nothing else changes.
    /// 3. Journal transaction: refuse on a hold (Held) or unresolved work
    ///    (NeedsReview); otherwise write the permanent `deleting` tombstone and
    ///    delete the task's records, closing all task admission.
    /// 4. Remove the task's files, keeping only the directory and its lock.
    /// 5. Mark the tombstone complete.
    ///
    /// A failure after step 3 leaves the tombstone `deleting`; call
    /// [`Self::resume_deletions`] (for example at startup) to finish. The ID can
    /// never be reused in this state root. Exported or remote copies are not
    /// recalled, and unlinking is not secure erasure.
    pub fn delete_task(&self, state: &PrivateState, task: &TaskId) -> Result<(), StoreError> {
        self.gateways.retire(task);
        let lease = state.acquire_task(task)?;
        self.journal.delete_task(task.clone())?.blocking_recv()?;
        crate::retention::purge_task_files(&lease)?;
        self.journal
            .complete_deletion(crate::config::path_key(b"task", task.as_str())?)?
            .blocking_recv()
    }
    /// N05 completion gate. `outputs` only says which committed artifact of
    /// this run is which contract output; the gate reads each back against its
    /// immutable journal record and evaluates the accepted contract's criteria
    /// itself, then the journal rechecks run, revision, unresolved work, budget
    /// and record identity atomically with recording results and completing.
    /// `lease` must be this task's lease; both its artifact and evidence stores
    /// are read. Blocking (host thread).
    ///
    /// `completed` is false when any required criterion is not Passed or a
    /// required output is unbound or failed readback; the run stays open.
    /// A completed run and task are final. Unsupported criteria (evidence
    /// coverage, non-CSV checks) are Inconclusive and block completion.
    pub fn complete_run(
        &self,
        lease: &crate::config::TaskLease,
        context: RunContext,
        outputs: Vec<crate::verification::OutputBinding>,
    ) -> Result<crate::verification::CompletionReport, StoreError> {
        let resume = self.journal.load_resume(context.clone())?.blocking_recv()?;
        if !matches!(
            resume.current_status,
            aero_core::checkpoint::RunStatus::Running | aero_core::checkpoint::RunStatus::Paused
        ) {
            return Err(StoreError::Conflict);
        }
        let evaluation = crate::verification::evaluate(
            &resume.contract,
            &context,
            lease,
            &self.journal,
            outputs,
        )?;
        let results = evaluation.results.clone();
        let completed = self
            .journal
            .finalize_run(context, evaluation)?
            .blocking_recv()?;
        Ok(crate::verification::CompletionReport { completed, results })
    }
    /// Finish deletions whose tombstone is still `deleting` (e.g. after a
    /// crash). Returns how many were completed. Busy if a task is in use.
    pub fn resume_deletions(&self, state: &PrivateState) -> Result<usize, StoreError> {
        let keys = self.journal.pending_deletions()?.blocking_recv()?;
        for key in &keys {
            let lease = state.acquire_task_key(key.clone())?;
            crate::retention::purge_task_files(&lease)?;
            self.journal
                .complete_deletion(key.clone())?
                .blocking_recv()?;
        }
        Ok(keys.len())
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
    Owner(Gateways),
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
    fn require_owner(&self) -> Result<&Gateways, StoreError> {
        match &self.access {
            Access::Owner(gateways) => Ok(gateways),
            Access::Model(_) => Err(StoreError::PermissionDenied),
        }
    }
    fn require_task(&self, task: &TaskId) -> Result<(), StoreError> {
        match &self.access {
            Access::Owner(_) => Ok(()),
            Access::Model(id) if id == task => Ok(()),
            _ => Err(StoreError::PermissionDenied),
        }
    }
    pub fn create_task(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.create_task(contract)
    }
    /// Retires the task's registered gateways first. Retirement is not undone
    /// if the journal then rejects the revision; create new gateways after it.
    pub fn revise_contract(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        self.require_owner()?.retire(&contract.task_id);
        self.journal.revise_contract(contract)
    }
    /// Records a paused run intent, not a launched browser, verified plan or effect grant.
    /// Prerequisites are observations supplied by the trusted host, never model JSON.
    /// A new run cancels the task's earlier runs, so every registered gateway for
    /// the task is retired before submission (not undone if the start is then
    /// rejected); register a new gateway for the new run.
    pub fn start_run(
        &self,
        context: RunContext,
        prerequisites: StartPrerequisites,
    ) -> Result<Reply<()>, StoreError> {
        self.require_owner()?.retire(&context.task_id);
        self.journal.start_run(context, prerequisites)
    }
    /// Place a named retention hold. While any hold exists, deletion cannot
    /// start; holds cannot stop a deletion already started (DEC-011).
    pub fn place_hold(&self, task: TaskId, hold: String) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.place_hold(task, hold)
    }
    pub fn release_hold(&self, task: TaskId, hold: String) -> Result<Reply<()>, StoreError> {
        self.require_owner()?;
        self.journal.release_hold(task, hold)
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
