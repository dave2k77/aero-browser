//! Trusted host-side journal plumbing, not a model-callable authority service.
//! Accepted requests execute even if their reply is dropped. A lost prepare reply
//! is uncertain: consult resume state, never assume cancellation or retry dispatch.
//! Shutdown drains accepted work and joins synchronously; call start/close/drop on
//! a blocking host thread, not an async executor thread. No hard filesystem deadline.

use super::{Store, StoreError, database_error};
use crate::budget::{BudgetRequest, BudgetSnapshot, BudgetTicket, Charges};
#[path = "budget_store.rs"]
mod budget_store;
use aero_core::actions::Decision;
use aero_core::checkpoint::{CumulativeUsage, MAX_COUNTER, RunContext, RunStatus};
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ArtifactId, EvidenceId, TaskId};
use aero_core::task::{ProcessingMode, StartPrerequisites, TaskContract};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use tokio::sync::oneshot;

pub const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
pub const MAX_AUDIT_PAGE: usize = 100;
const MAX_RECORDS: usize = 64;
/// Upper bound on one task's artifact records returned for reconciliation.
pub const MAX_ARTIFACT_INVENTORY: usize = 1024;
// An artifact record is small typed JSON; longer stored rows are corrupt.
const ARTIFACT_ROW_LIMIT: i64 = 4096;
const READ_LIMIT: i64 = MAX_PAYLOAD_BYTES as i64 + 1;
type Job = Box<dyn FnOnce(&mut Store) + Send>;
type Queue = Arc<Mutex<Option<mpsc::SyncSender<Job>>>>;

/// The sole owner of the worker lifetime. Clients cannot keep it alive after close.
pub struct Journal {
    client: JournalClient,
    thread: Option<JoinHandle<()>>,
}
#[derive(Clone)]
pub struct JournalClient {
    queue: Queue,
}
/// Reply loss says nothing about transaction completion. Reconcile before retry.
pub struct Reply<T>(oneshot::Receiver<Result<T, StoreError>>);
impl<T> Reply<T> {
    pub async fn recv(self) -> Result<T, StoreError> {
        self.0.await.map_err(|_| StoreError::WorkerClosed)?
    }
    /// For synchronous hosts/tests only; Tokio rejects this inside async contexts.
    pub fn blocking_recv(self) -> Result<T, StoreError> {
        self.0
            .blocking_recv()
            .map_err(|_| StoreError::WorkerClosed)?
    }
}
impl Journal {
    pub fn start(directory: &Path, capacity: usize) -> Result<Self, StoreError> {
        if !(1..=256).contains(&capacity) {
            return Err(StoreError::InvalidInput);
        }
        let mut store = Store::open(directory)?;
        // Exclusive startup, before exposing a client or permitting dispatch.
        store.validate_budget_holds()?;
        store.recover_prepared()?;
        store.recover_work()?;
        let (sender, receiver) = mpsc::sync_channel::<Job>(capacity);
        let thread = std::thread::Builder::new()
            .name("aero-journal".into())
            .spawn(move || {
                while let Ok(job) = receiver.recv() {
                    job(&mut store);
                }
            })
            .map_err(|_| StoreError::Io)?;
        Ok(Self {
            client: JournalClient {
                queue: Arc::new(Mutex::new(Some(sender))),
            },
            thread: Some(thread),
        })
    }
    pub fn client(&self) -> JournalClient {
        self.client.clone()
    }
    pub fn close(mut self) -> Result<(), StoreError> {
        self.stop()
    }
    fn stop(&mut self) -> Result<(), StoreError> {
        // Only a non-awaiting try_send occurs under this lock. All clones share
        // this sender slot, so close atomically prevents any later admission.
        self.client
            .queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| StoreError::WorkerClosed)?;
        }
        Ok(())
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
impl JournalClient {
    /// Trusted host accounting only; a reservation never authorizes dispatch.
    pub fn reserve_budget(
        &self,
        request: BudgetRequest,
    ) -> Result<Reply<BudgetTicket>, StoreError> {
        request.validate()?;
        self.submit(move |s| s.reserve_budget(request))
    }
    pub fn settle_budget(
        &self,
        ticket: BudgetTicket,
        actual: Option<Resources>,
    ) -> Result<Reply<()>, StoreError> {
        context_valid(&ticket.context)?;
        identifier(&ticket.id)?;
        self.submit(move |s| s.settle_budget(ticket, actual))
    }
    pub fn budget_status(&self, context: RunContext) -> Result<Reply<BudgetSnapshot>, StoreError> {
        context_valid(&context)?;
        self.submit(move |s| {
            read_run(&s.connection, &context)?;
            read_usage(&s.connection, &context.task_id)?.snapshot()
        })
    }
    pub fn budget_receipt(
        &self,
        ticket: BudgetTicket,
    ) -> Result<Reply<crate::budget::BudgetReceipt>, StoreError> {
        context_valid(&ticket.context)?;
        identifier(&ticket.id)?;
        self.submit(move |s| s.budget_receipt(ticket))
    }
    /// Bounded task-wide history for reconciling receipts after reconnect/restart.
    /// Cursor order is the caller-selected ID, not insertion order: concurrent
    /// inserts sorting at or before the cursor are missed. Pause admissions or
    /// restart traversal when a complete inventory is required.
    pub fn budget_history(
        &self,
        context: RunContext,
        after_id: Option<String>,
        limit: usize,
    ) -> Result<Reply<crate::budget::BudgetPage>, StoreError> {
        context_valid(&context)?;
        if let Some(id) = &after_id {
            identifier(id)?;
        }
        if !(1..=MAX_AUDIT_PAGE).contains(&limit) {
            return Err(StoreError::InvalidInput);
        }
        self.submit(move |s| s.budget_history(context, after_id, limit))
    }
    /// Quoted rates and purchase upper bound come from the trusted host, not a planner.
    pub fn prepare_charged_action(
        &self,
        request: PrepareAction,
        charges: Charges,
    ) -> Result<Reply<PreparedAction>, StoreError> {
        request.validate()?;
        charges.validate()?;
        encode(&charges)?;
        self.submit(move |s| s.prepare_charged_action(request, charges))
    }
    /// None purchase usage retains the hold if a nonzero purchase was reserved.
    pub fn commit_charged_step(
        &self,
        mut request: CommitStep,
        purchase_actual: Option<u64>,
    ) -> Result<Reply<()>, StoreError> {
        request.validate()?;
        let attachment_error = request.validate_attachments().err();
        if attachment_error.is_some() {
            request.evidence.clear();
            request.artifacts.clear();
        }
        self.submit(move |s| s.commit_checked(request, purchase_actual, attachment_error))
    }
    fn submit<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<Reply<T>, StoreError> {
        let (sender, receiver) = oneshot::channel();
        let job = Box::new(move |store: &mut Store| {
            let result = f(store);
            let _ = sender.send(result);
        });
        let queue = self.queue.lock().map_err(|_| StoreError::WorkerClosed)?;
        queue
            .as_ref()
            .ok_or(StoreError::WorkerClosed)?
            .try_send(job)
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => StoreError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => StoreError::WorkerClosed,
            })?;
        Ok(Reply(receiver))
    }
    pub(crate) fn create_task(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        validate_contract(&contract)?;
        self.submit(move |s| s.create_task(contract))
    }
    pub(crate) fn revise_contract(&self, contract: TaskContract) -> Result<Reply<()>, StoreError> {
        validate_contract(&contract)?;
        self.submit(move |s| s.revise_contract(contract))
    }
    pub(crate) fn start_run(
        &self,
        context: RunContext,
        prerequisites: StartPrerequisites,
    ) -> Result<Reply<()>, StoreError> {
        context_valid(&context)?;
        self.submit(move |s| s.create_run(context, prerequisites))
    }
    pub fn prepare_action(
        &self,
        request: PrepareAction,
    ) -> Result<Reply<PreparedAction>, StoreError> {
        request.validate()?;
        self.submit(move |s| s.prepare_action(request))
    }
    pub fn commit_step(&self, request: CommitStep) -> Result<Reply<()>, StoreError> {
        self.commit_charged_step(request, None)
    }
    /// Read-only resume view. Startup classifies old prepared intents; a live
    /// prepared request is rejected here rather than racing its executor.
    pub fn load_resume(&self, context: RunContext) -> Result<Reply<ResumeState>, StoreError> {
        context_valid(&context)?;
        self.submit(move |s| s.load_resume(context))
    }
    /// Read-only offline history. Cursor is exclusive; no executor/provider callbacks.
    pub fn load_audit(
        &self,
        context: RunContext,
        after_step: u64,
        limit: usize,
    ) -> Result<Reply<AuditPage>, StoreError> {
        context_valid(&context)?;
        if after_step > MAX_COUNTER || !(1..=MAX_AUDIT_PAGE).contains(&limit) {
            return Err(StoreError::InvalidInput);
        }
        self.submit(move |s| s.load_audit(context, after_step, limit))
    }
    /// Trusted host lookup of one committed artifact record for a task. The
    /// record is metadata only: use `ArtifactStore::verify` to read back the
    /// actual bytes before relying on it. Unknown or other-task IDs are NotFound.
    pub fn load_artifact(
        &self,
        task_id: TaskId,
        id: ArtifactId,
    ) -> Result<Reply<ArtifactRecord>, StoreError> {
        identifier(task_id.as_str())?;
        identifier(id.as_str())?;
        self.submit(move |s| load_artifact(&s.connection, &task_id, &id))
    }
    /// Trusted host lookup of one committed evidence record for a task. The
    /// record is metadata only: use `EvidenceStore::load` to read back and
    /// decode the captured fact. Unknown or other-task IDs are NotFound.
    pub fn load_evidence(
        &self,
        task_id: TaskId,
        id: EvidenceId,
    ) -> Result<Reply<EvidenceRecord>, StoreError> {
        identifier(task_id.as_str())?;
        identifier(id.as_str())?;
        self.submit(move |s| load_evidence(&s.connection, &task_id, &id))
    }
    /// Trusted host inventory of every committed artifact record for a task,
    /// ordered by ID, for reconciliation with `ArtifactStore::reconcile`. More
    /// than [`MAX_ARTIFACT_INVENTORY`] records is BudgetExceeded rather than a
    /// silently truncated (and therefore falsely consistent) inventory.
    pub fn load_artifacts(
        &self,
        task_id: TaskId,
    ) -> Result<Reply<Vec<ArtifactRecord>>, StoreError> {
        identifier(task_id.as_str())?;
        self.submit(move |s| load_artifacts(&s.connection, &task_id))
    }
    /// Deletes SQL-owned records only, not files/profiles/derived stores (N16).
    /// This operation does not acquire the task lease or stop an existing writer.
    /// It is not safe to expose as complete task deletion or a retention purge:
    /// private files can remain and a live ArtifactStore can continue writing.
    /// The host must not use it as a user-facing deletion operation until N16
    /// adds a coordinated admission gate, durable retry and derived-data cleanup.
    pub fn delete_task(&self, task_id: TaskId) -> Result<Reply<()>, StoreError> {
        identifier(task_id.as_str())?;
        self.submit(move |s| s.delete_task(task_id))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub active_ms: u64,
}
impl Resources {
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        if [self.input_tokens, self.output_tokens, self.active_ms]
            .into_iter()
            .any(|x| x > MAX_COUNTER)
            || self
                .input_tokens
                .checked_add(self.output_tokens)
                .is_none_or(|x| x > MAX_COUNTER)
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
}
/// Host classification, not deserializable planner permission or automatic redaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persistence {
    Redacted,
    RequiresEncryption,
}
pub struct PrepareAction {
    pub context: RunContext,
    pub persistence: Persistence,
    /// Must already be redacted by the trusted caller. Validation is not sanitization.
    pub decision: Decision,
    pub reserve: Resources,
}
impl PrepareAction {
    fn validate(&self) -> Result<(), StoreError> {
        if self.persistence != Persistence::Redacted {
            return Err(StoreError::EncryptionRequired);
        }
        context_valid(&self.context)?;
        self.decision
            .validate()
            .map_err(|_| StoreError::InvalidInput)?;
        self.reserve.validate()?;
        encode(&self.decision)?;
        Ok(())
    }
}
/// A durable location, NOT an execution capability or proof of fresh authorization.
#[derive(Clone)]
pub struct PreparedAction {
    pub context: RunContext,
    pub step: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub id: EvidenceId,
    pub context: RunContext,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    pub id: ArtifactId,
    pub context: RunContext,
    pub sha256: String,
    pub bytes: u64,
}
/// Typed non-snapshot metadata only. No forms, URLs, free text, arbitrary JSON or key material.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointMetadata {
    pub schema_version: u32,
    pub context: RunContext,
    pub step: u64,
    pub next_step: u64,
    pub usage: CumulativeUsage,
    pub status: RunStatus,
    pub evidence_ids: Vec<EvidenceId>,
    pub artifact_ids: Vec<ArtifactId>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Prepared,
    Committed,
    Uncertain,
    Failed,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepOutcome {
    pub schema_version: u32,
    pub dispatch: DispatchState,
    pub verification: VerificationStatus,
}
pub struct CommitStep {
    pub prepared: PreparedAction,
    pub outcome: StepOutcome,
    /// None means unknown consumption: keep the entire reservation and require review.
    pub actual: Option<Resources>,
    pub evidence: Vec<EvidenceRecord>,
    pub artifacts: Vec<ArtifactRecord>,
}
impl CommitStep {
    fn validate(&self) -> Result<(), StoreError> {
        context_valid(&self.prepared.context)?;
        if self.prepared.step == 0 || self.prepared.step >= MAX_COUNTER {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
    fn validate_outcome(&self) -> Result<(), StoreError> {
        if self.outcome.schema_version != 1
            || (self.outcome.dispatch == DispatchState::Uncertain
                && self.outcome.verification == VerificationStatus::Passed)
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
    fn validate_attachments(&self) -> Result<(), StoreError> {
        if self.evidence.len() > MAX_RECORDS || self.artifacts.len() > MAX_RECORDS {
            return Err(StoreError::InvalidInput);
        }
        let mut seen = std::collections::HashSet::new();
        for r in &self.evidence {
            r.context
                .validate_for(&self.prepared.context)
                .map_err(|_| StoreError::OwnershipMismatch)?;
            identifier(r.id.as_str())?;
            crate::artifacts::check_evidence_owner(&r.context.task_id, r.id.as_str())?;
            digest(&r.sha256)?;
            if r.bytes > MAX_COUNTER || !seen.insert(r.id.as_str()) {
                return Err(StoreError::InvalidInput);
            }
        }
        seen.clear();
        for r in &self.artifacts {
            r.context
                .validate_for(&self.prepared.context)
                .map_err(|_| StoreError::OwnershipMismatch)?;
            identifier(r.id.as_str())?;
            crate::artifacts::check_artifact_owner(&r.context.task_id, r.id.as_str())?;
            digest(&r.sha256)?;
            if r.bytes > MAX_COUNTER || !seen.insert(r.id.as_str()) {
                return Err(StoreError::InvalidInput);
            }
        }
        Ok(())
    }
}
pub struct ResumeState {
    pub contract: TaskContract,
    pub context: RunContext,
    pub next_step: u64,
    pub usage: CumulativeUsage,
    pub needs_review: bool,
    /// Current persisted run status, distinct from an older checkpoint's status.
    /// Neither this report nor needs_review=false grants execution authority.
    pub current_status: RunStatus,
    pub budget: BudgetSnapshot,
    pub checkpoint: Option<CheckpointMetadata>,
}
pub struct AuditEntry {
    pub step: u64,
    pub decision: Decision,
    pub state: ActionState,
    pub outcome: Option<StepOutcome>,
    pub reported_overrun: Option<crate::budget::OverrunReport>,
}
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    pub has_more: bool,
}

fn identifier(s: &str) -> Result<(), StoreError> {
    if s.trim().is_empty() || s.len() > 256 || s.contains('\0') {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn context_valid(c: &RunContext) -> Result<(), StoreError> {
    c.validate().map_err(|_| StoreError::InvalidInput)?;
    identifier(c.task_id.as_str())?;
    identifier(c.run_id.as_str())
}
fn digest(s: &str) -> Result<(), StoreError> {
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_contract(c: &TaskContract) -> Result<(), StoreError> {
    c.validate().map_err(|_| StoreError::InvalidInput)?;
    // No encrypted persistence path exists yet; do not silently downgrade sensitive mode.
    if c.processing_mode == ProcessingMode::Sensitive {
        return Err(StoreError::EncryptionRequired);
    }
    encode(c)?;
    Ok(())
}
fn encode<T: Serialize>(value: &T) -> Result<String, StoreError> {
    let bytes = serde_json::to_string(value).map_err(|_| StoreError::InvalidInput)?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(StoreError::InvalidInput);
    }
    Ok(bytes)
}
fn decode<T: DeserializeOwned + Serialize>(s: &str) -> Result<T, StoreError> {
    if s.len() > MAX_PAYLOAD_BYTES || !s.trim_start().starts_with('{') {
        return Err(StoreError::CorruptDatabase);
    }
    let decoded: T = serde_json::from_str(s).map_err(|_| StoreError::CorruptDatabase)?;
    // Our persisted format is the serializer's exact shape. Reject alternate
    // positional nested structs/defaulted fields even if a derive accepts them.
    let original: serde_json::Value =
        serde_json::from_str(s).map_err(|_| StoreError::CorruptDatabase)?;
    if original != serde_json::to_value(&decoded).map_err(|_| StoreError::CorruptDatabase)? {
        return Err(StoreError::CorruptDatabase);
    }
    Ok(decoded)
}
fn stamp() -> Result<String, StoreError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Io)?
        .as_millis()
        .to_string())
}
fn sql<T>(r: rusqlite::Result<T>) -> Result<T, StoreError> {
    r.map_err(database_error)
}
fn integer(n: u64) -> Result<i64, StoreError> {
    i64::try_from(n).map_err(|_| StoreError::InvalidInput)
}
fn counter(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

impl Store {
    fn create_task(&mut self, contract: TaskContract) -> Result<(), StoreError> {
        validate_contract(&contract)?;
        if contract.revision != 1 {
            return Err(StoreError::Conflict);
        }
        let json = encode(&contract)?;
        let now = stamp()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO tasks(id,status,created_at,updated_at) VALUES (?1,'draft',?2,?2)",
            params![contract.task_id.as_str(), now],
        ))?;
        sql(tx.execute(
            "INSERT INTO task_contracts VALUES (?1,1,1,?2,?3)",
            params![contract.task_id.as_str(), json, now],
        ))?;
        sql(tx.commit())
    }
    fn revise_contract(&mut self, contract: TaskContract) -> Result<(), StoreError> {
        validate_contract(&contract)?;
        let json = encode(&contract)?;
        let now = stamp()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        no_pending(&tx, &contract.task_id)?;
        let latest = latest_revision(&tx, &contract.task_id)?;
        if latest.checked_add(1) != Some(contract.revision) {
            return Err(StoreError::Conflict);
        }
        let prior = read_task_contract(&tx, &contract.task_id, latest)?;
        if prior.budgets.budget_currency != contract.budgets.budget_currency {
            return Err(StoreError::InvalidInput);
        }
        let usage = read_usage(&tx, &contract.task_id)?;
        usage.within(&contract)?;
        sql(tx.execute(
            "INSERT INTO task_contracts VALUES (?1,?2,1,?3,?4)",
            params![
                contract.task_id.as_str(),
                integer(contract.revision)?,
                json,
                now
            ],
        ))?;
        sql(tx.execute(
            "UPDATE capability_grants SET revoked=1 WHERE task_id=?1",
            [contract.task_id.as_str()],
        ))?;
        sql(tx.execute(
            "UPDATE approvals SET consumed=1 WHERE task_id=?1",
            [contract.task_id.as_str()],
        ))?;
        sql(tx.execute("UPDATE sessions SET status='paused',updated_at=?2 WHERE task_id=?1 AND status IN ('running','awaiting_user')",params![contract.task_id.as_str(),now]))?;
        sql(tx.execute(
            "UPDATE tasks SET status='draft',updated_at=?2 WHERE id=?1",
            params![contract.task_id.as_str(), now],
        ))?;
        sql(tx.commit())
    }
    fn create_run(
        &mut self,
        context: RunContext,
        prerequisites: StartPrerequisites,
    ) -> Result<(), StoreError> {
        context_valid(&context)?;
        let now = stamp()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        no_pending(&tx, &context.task_id)?;
        if latest_revision(&tx, &context.task_id)? != context.contract_revision {
            return Err(StoreError::Conflict);
        }
        let contract = read_contract(&tx, &context)?;
        contract
            .validate_for_start(&prerequisites)
            .map_err(|_| StoreError::StartBlocked)?;
        read_usage(&tx, &context.task_id)?.within(&contract)?;
        sql(tx.execute("UPDATE sessions SET status='cancelled',updated_at=?2 WHERE task_id=?1 AND status IN ('running','awaiting_user','paused')",params![context.task_id.as_str(),now]))?;
        sql(tx.execute("INSERT INTO sessions(id,task_id,contract_revision,goal,status,created_at,updated_at) VALUES (?1,?2,?3,?4,'paused',?5,?5)",params![context.run_id.as_str(),context.task_id.as_str(),integer(context.contract_revision)?,contract.objective,now]))?;
        sql(tx.execute(
            "UPDATE tasks SET status='paused',updated_at=?2 WHERE id=?1",
            params![context.task_id.as_str(), now],
        ))?;
        sql(tx.commit())
    }
    fn prepare_action(&mut self, request: PrepareAction) -> Result<PreparedAction, StoreError> {
        // The legacy mechanical path asserts no inference when it reserves no
        // tokens. Explicit unknown-priced calls must use charged/work APIs.
        let inference = if request.reserve.input_tokens == 0 && request.reserve.output_tokens == 0 {
            crate::budget::Pricing::NoInference
        } else {
            crate::budget::Pricing::Unknown
        };
        self.prepare_charged_action(
            request,
            Charges {
                inference,
                purchase: None,
            },
        )
    }
    fn prepare_charged_action(
        &mut self,
        request: PrepareAction,
        charges: Charges,
    ) -> Result<PreparedAction, StoreError> {
        request.validate()?;
        let decision = encode(&request.decision)?;
        let now = stamp()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (contract, next, status) = read_run(&tx, &request.context)?;
        no_pending(&tx, &request.context.task_id)?;
        if !matches!(status.as_str(), "running" | "paused")
            || latest_revision(&tx, &request.context.task_id)? != request.context.contract_revision
        {
            return Err(StoreError::Conflict);
        }
        // A newer run supersedes older runs even at the same contract revision.
        let other_running:bool=sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE task_id=?1 AND id<>?2 AND status='running')",params![request.context.task_id.as_str(),request.context.run_id.as_str()],|r|r.get(0)))?;
        if other_running || next >= MAX_COUNTER {
            return Err(StoreError::Conflict);
        }
        let mut usage = read_usage(&tx, &request.context.task_id)?;
        usage.attempts = add(usage.attempts, 1)?;
        usage.reserve(request.reserve, &charges, &contract)?;
        write_usage(&tx, &request.context.task_id, &usage, &now)?;
        sql(tx.execute("UPDATE sessions SET next_step=?2,attempts=attempts+1,status='running',updated_at=?3 WHERE id=?1",params![request.context.run_id.as_str(),integer(next+1)?,now]))?;
        sql(tx.execute(
            "UPDATE tasks SET status='running' WHERE id=?1",
            [request.context.task_id.as_str()],
        ))?;
        sql(tx.execute(
            "INSERT INTO actions VALUES (?1,?2,?3,?4,'prepared',?5,NULL,?6)",
            params![
                request.context.run_id.as_str(),
                integer(next)?,
                request.decision.observation_id.as_str(),
                decision,
                request.decision.rationale,
                now
            ],
        ))?;
        let reservation = Reservation {
            schema_version: 1,
            context: request.context.clone(),
            step: next,
            reserve: request.reserve,
            charges,
        };
        sql(tx.execute(
            "INSERT INTO budget_reservations(id,task_id,session_id,reservation_json,state,created_at) VALUES (?1,?2,?3,?4,'reserved',?5)",
            params![
                reservation_id(&request.context, next),
                request.context.task_id.as_str(),
                request.context.run_id.as_str(),
                encode(&reservation)?,
                now
            ],
        ))?;
        sql(tx.commit())?;
        Ok(PreparedAction {
            context: request.context,
            step: next,
        })
    }
    #[cfg(test)]
    fn commit_step(&mut self, request: CommitStep) -> Result<(), StoreError> {
        self.commit_charged_step(request, None)
    }
    #[cfg(test)]
    fn commit_charged_step(
        &mut self,
        mut request: CommitStep,
        purchase_actual: Option<u64>,
    ) -> Result<(), StoreError> {
        request.validate()?;
        let attachment_error = request.validate_attachments().err();
        if attachment_error.is_some() {
            request.evidence.clear();
            request.artifacts.clear();
        }
        self.commit_checked(request, purchase_actual, attachment_error)
    }
    fn commit_checked(
        &mut self,
        request: CommitStep,
        purchase_actual: Option<u64>,
        attachment_error: Option<StoreError>,
    ) -> Result<(), StoreError> {
        request.validate()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let overrun = apply_commit(&tx, request, purchase_actual, attachment_error)?;
        sql(tx.commit())?;
        if overrun {
            Err(StoreError::BudgetExceeded)
        } else {
            Ok(())
        }
    }
    fn load_resume(&mut self, context: RunContext) -> Result<ResumeState, StoreError> {
        context_valid(&context)?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Deferred))?;
        let (contract, next, status) = read_run(&tx, &context)?;
        let current_status: RunStatus = serde_json::from_value(serde_json::Value::String(status))
            .map_err(|_| StoreError::CorruptDatabase)?;
        let live:bool=sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM actions a JOIN sessions s ON s.id=a.session_id WHERE s.task_id=?1 AND a.state='prepared') OR EXISTS(SELECT 1 FROM work_reservations WHERE task_id=?1 AND state='reserved')",[context.task_id.as_str()],|r|r.get(0)))?;
        if live {
            return Err(StoreError::Conflict);
        }
        let needs_review = has_pending(&tx, &context.task_id)?;
        let counters = read_usage(&tx, &context.task_id)?;
        let budget = counters.snapshot()?;
        let usage = counters.public()?;
        let checkpoint = read_checkpoint(&tx, &context, next, &usage)?;
        sql(tx.commit())?;
        Ok(ResumeState {
            contract,
            context,
            next_step: next,
            usage,
            needs_review,
            current_status,
            budget,
            checkpoint,
        })
    }
    fn load_audit(
        &mut self,
        context: RunContext,
        after_step: u64,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        read_run(&self.connection, &context)?;
        let mut stmt = sql(self.connection.prepare(
            "SELECT step FROM actions WHERE session_id=?1 AND step>?2 ORDER BY step LIMIT ?3",
        ))?;
        let steps = sql(stmt.query_map(
            params![
                context.run_id.as_str(),
                integer(after_step)?,
                (limit + 1) as i64
            ],
            |r| counter(r, 0),
        ))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(database_error)?;
        let has_more = steps.len() > limit;
        let entries = steps
            .into_iter()
            .take(limit)
            .map(|step| read_action(&self.connection, &context, step))
            .collect::<Result<_, _>>()?;
        Ok(AuditPage { entries, has_more })
    }
    fn delete_task(&mut self, task_id: TaskId) -> Result<(), StoreError> {
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        no_pending(&tx, &task_id)?;
        if sql(tx.execute("DELETE FROM tasks WHERE id=?1", [task_id.as_str()]))? != 1 {
            return Err(StoreError::NotFound);
        }
        sql(tx.commit())
    }
}

impl Store {
    fn recover_prepared(&mut self) -> Result<(), StoreError> {
        // One bounded record per transaction; no allocation proportional to the DB.
        loop {
            let tx = sql(self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            let row:Option<(String,String,u64,u64)>=sql(tx.query_row("SELECT s.task_id,s.id,s.contract_revision,a.step FROM actions a JOIN sessions s ON s.id=a.session_id WHERE a.state='prepared' ORDER BY s.id,a.step LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,counter(r,2)?,counter(r,3)?))).optional())?;
            let Some((task, run, revision, step)) = row else {
                sql(tx.commit())?;
                return Ok(());
            };
            let context = RunContext {
                task_id: TaskId::from(task),
                run_id: aero_core::ids::RunId::from(run),
                contract_revision: revision,
            };
            context_valid(&context).map_err(|_| StoreError::CorruptDatabase)?;
            let (contract, next, _) = read_run(&tx, &context)?;
            let reservation = read_reservation(&tx, &context, step)?;
            read_action(&tx, &context, step)?;
            let usage = read_usage(&tx, &context.task_id)?;
            usage.contains(reservation.reserve, &reservation.charges, &contract)?;
            if next != step.checked_add(1).ok_or(StoreError::CorruptDatabase)?
                || usage.attempts < step
                || usage.input_reserved < reservation.reserve.input_tokens
                || usage.output_reserved < reservation.reserve.output_tokens
                || usage.active_reserved < reservation.reserve.active_ms
            {
                return Err(StoreError::CorruptDatabase);
            }
            let now = stamp()?;
            sql(tx.execute(
                "UPDATE actions SET state='uncertain' WHERE session_id=?1 AND step=?2",
                params![context.run_id.as_str(), integer(step)?],
            ))?;
            sql(tx.execute(
                "UPDATE budget_reservations SET state='uncertain' WHERE id=?1",
                [reservation_id(&context, step)],
            ))?;
            sql(tx.execute(
                "UPDATE tasks SET status='needs_review',updated_at=?2 WHERE id=?1",
                params![context.task_id.as_str(), now],
            ))?;
            sql(tx.execute(
                "UPDATE sessions SET status='needs_review',updated_at=?2 WHERE id=?1",
                params![context.run_id.as_str(), now],
            ))?;
            sql(tx.commit())?;
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionSettlement {
    schema_version: u32,
    actual: Resources,
    purchase_actual: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    schema_version: u32,
    context: RunContext,
    step: u64,
    reserve: Resources,
    charges: Charges,
}
fn reservation_id(c: &RunContext, step: u64) -> String {
    format!("{}:{step}", c.run_id.as_str())
}
fn add(a: u64, b: u64) -> Result<u64, StoreError> {
    a.checked_add(b)
        .filter(|n| *n <= MAX_COUNTER)
        .ok_or(StoreError::BudgetExceeded)
}
fn sub(a: u64, b: u64) -> Result<u64, StoreError> {
    a.checked_sub(b).ok_or(StoreError::CorruptDatabase)
}
struct Usage {
    uncertain_inference: bool,
    currency: String,
    attempts: u64,
    input_spent: u64,
    output_spent: u64,
    input_reserved: u64,
    output_reserved: u64,
    active_spent: u64,
    active_reserved: u64,
    recovery_count: u64,
    inference_spent: u64,
    inference_reserved: u64,
    purchase_spent: u64,
    purchase_reserved: u64,
    unknown_spent: u64,
    unknown_reserved: u64,
}
impl Usage {
    fn within(&self, c: &TaskContract) -> Result<(), StoreError> {
        if self.attempts > c.budgets.max_action_attempts
            || add(self.input_spent, self.input_reserved)? > c.budgets.max_input_tokens
            || add(self.output_spent, self.output_reserved)? > c.budgets.max_output_tokens
            || add(self.active_spent, self.active_reserved)? > c.budgets.max_active_ms
        {
            return Err(StoreError::BudgetExceeded);
        }
        self.public()?;
        self.money_within(c)?;
        Ok(())
    }
    fn public(&self) -> Result<CumulativeUsage, StoreError> {
        let result = CumulativeUsage {
            attempts_spent: self.attempts,
            tokens_spent: add(self.input_spent, self.output_spent)?,
            tokens_reserved: add(self.input_reserved, self.output_reserved)?,
            active_ms_spent: self.active_spent,
            recovery_count: self.recovery_count,
        };
        result.validate().map_err(|_| StoreError::CorruptDatabase)?;
        Ok(result)
    }
}
fn read_usage(c: &Connection, task: &TaskId) -> Result<Usage, StoreError> {
    let currency = read_task_contract(c, task, latest_revision(c, task)?)?
        .budgets
        .budget_currency;
    let mut value=sql(c.query_row("SELECT attempts_spent,input_tokens_spent,output_tokens_spent,input_tokens_reserved,output_tokens_reserved,active_ms_spent,active_ms_reserved,recovery_count,inference_spent,inference_reserved,purchase_spent,purchase_reserved,unknown_inference_spent,unknown_inference_reserved FROM tasks WHERE id=?1",[task.as_str()],|r|Ok(Usage{attempts:counter(r,0)?,input_spent:counter(r,1)?,output_spent:counter(r,2)?,input_reserved:counter(r,3)?,output_reserved:counter(r,4)?,active_spent:counter(r,5)?,active_reserved:counter(r,6)?,recovery_count:counter(r,7)?,inference_spent:counter(r,8)?,inference_reserved:counter(r,9)?,purchase_spent:counter(r,10)?,purchase_reserved:counter(r,11)?,unknown_spent:counter(r,12)?,unknown_reserved:counter(r,13)?,currency:currency.clone(),uncertain_inference:false})).optional())?.ok_or(StoreError::NotFound)?;
    value.public()?;
    value.snapshot()?;
    value.uncertain_inference = budget_store::validate_holds(c, task, &value)?;
    budget_store::validate_spent(c, task, &value)?;
    Ok(value)
}
fn write_usage(c: &Connection, task: &TaskId, u: &Usage, now: &str) -> Result<(), StoreError> {
    let p = u.public()?;
    let rows=sql(c.execute("UPDATE tasks SET attempts_spent=?2,tokens_spent=?3,tokens_reserved=?4,active_ms_spent=?5,input_tokens_spent=?6,output_tokens_spent=?7,input_tokens_reserved=?8,output_tokens_reserved=?9,active_ms_reserved=?10,updated_at=?11 WHERE id=?1",params![task.as_str(),integer(u.attempts)?,integer(p.tokens_spent)?,integer(p.tokens_reserved)?,integer(u.active_spent)?,integer(u.input_spent)?,integer(u.output_spent)?,integer(u.input_reserved)?,integer(u.output_reserved)?,integer(u.active_reserved)?,now]))?;
    if rows != 1 {
        return Err(StoreError::NotFound);
    }
    sql(c.execute("UPDATE tasks SET inference_spent=?2,inference_reserved=?3,purchase_spent=?4,purchase_reserved=?5,unknown_inference_spent=?6,unknown_inference_reserved=?7 WHERE id=?1",params![task.as_str(),integer(u.inference_spent)?,integer(u.inference_reserved)?,integer(u.purchase_spent)?,integer(u.purchase_reserved)?,integer(u.unknown_spent)?,integer(u.unknown_reserved)?]))?;
    Ok(())
}
fn latest_revision(c: &Connection, task: &TaskId) -> Result<u64, StoreError> {
    let n = sql(c.query_row(
        "SELECT max(revision) FROM task_contracts WHERE task_id=?1",
        [task.as_str()],
        |r| r.get::<_, Option<i64>>(0),
    ))?
    .ok_or(StoreError::NotFound)?;
    u64::try_from(n).map_err(|_| StoreError::CorruptDatabase)
}
fn read_contract(c: &Connection, context: &RunContext) -> Result<TaskContract, StoreError> {
    read_task_contract(c, &context.task_id, context.contract_revision)
}
fn read_task_contract(
    c: &Connection,
    task: &TaskId,
    revision: u64,
) -> Result<TaskContract, StoreError> {
    let json:String=sql(c.query_row("SELECT substr(CAST(contract_json AS BLOB),1,?3) FROM task_contracts WHERE task_id=?1 AND revision=?2",params![task.as_str(),integer(revision)?,READ_LIMIT],|r|r.get::<_,Vec<u8>>(0)).optional())?.map(|v|String::from_utf8(v).map_err(|_|StoreError::CorruptDatabase)).transpose()?.ok_or(StoreError::NotFound)?;
    let contract: TaskContract = decode(&json)?;
    contract
        .validate()
        .map_err(|_| StoreError::CorruptDatabase)?;
    if contract.task_id != *task || contract.revision != revision {
        return Err(StoreError::CorruptDatabase);
    }
    if contract.processing_mode == ProcessingMode::Sensitive {
        return Err(StoreError::EncryptionRequired);
    }
    Ok(contract)
}
fn read_run(
    c: &Connection,
    context: &RunContext,
) -> Result<(TaskContract, u64, String), StoreError> {
    let (task, revision, next, status): (String, u64, u64, String) = sql(c
        .query_row(
            "SELECT task_id,contract_revision,next_step,status FROM sessions WHERE id=?1",
            [context.run_id.as_str()],
            |r| Ok((r.get(0)?, counter(r, 1)?, counter(r, 2)?, r.get(3)?)),
        )
        .optional())?
    .ok_or(StoreError::NotFound)?;
    if task != context.task_id.as_str() || revision != context.contract_revision {
        return Err(StoreError::OwnershipMismatch);
    }
    Ok((read_contract(c, context)?, next, status))
}
fn has_pending(c: &Connection, task: &TaskId) -> Result<bool, StoreError> {
    sql(c.query_row("SELECT EXISTS(SELECT 1 FROM actions a JOIN sessions s ON s.id=a.session_id WHERE s.task_id=?1 AND a.state IN ('prepared','uncertain')) OR EXISTS(SELECT 1 FROM budget_reservations WHERE task_id=?1 AND state IN ('reserved','uncertain')) OR EXISTS(SELECT 1 FROM work_reservations WHERE task_id=?1 AND state IN ('reserved','uncertain')) OR EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND status='needs_review')",[task.as_str()],|r|r.get(0)))
}
fn no_pending(c: &Connection, task: &TaskId) -> Result<(), StoreError> {
    if has_pending(c, task)? {
        Err(StoreError::NeedsReview)
    } else {
        Ok(())
    }
}
fn read_reservation(
    c: &Connection,
    context: &RunContext,
    step: u64,
) -> Result<Reservation, StoreError> {
    let (json, state): (String, String) = sql(c
        .query_row(
            "SELECT substr(reservation_json,1,?2),state FROM budget_reservations WHERE id=?1 AND task_id=?3 AND session_id=?4",
            params![reservation_id(context, step), READ_LIMIT, context.task_id.as_str(), context.run_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?
    .ok_or(StoreError::CorruptDatabase)?;
    let r: Reservation = decode(&json)?;
    if r.schema_version != 1 || r.context != *context || r.step != step || state != "reserved" {
        return Err(StoreError::CorruptDatabase);
    }
    r.reserve
        .validate()
        .map_err(|_| StoreError::CorruptDatabase)?;
    Ok(r)
}
fn read_action(c: &Connection, context: &RunContext, step: u64) -> Result<AuditEntry, StoreError> {
    let (json,state,outcome,observation,rationale):(String,String,Option<String>,String,String)=sql(c.query_row("SELECT substr(action_json,1,?3),state,substr(outcome_json,1,?3),observation_id,public_rationale FROM actions WHERE session_id=?1 AND step=?2",params![context.run_id.as_str(),integer(step)?,READ_LIMIT],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional())?.ok_or(StoreError::NotFound)?;
    let decision: Decision = decode(&json)?;
    decision
        .validate()
        .map_err(|_| StoreError::CorruptDatabase)?;
    if decision.observation_id.as_str() != observation || decision.rationale != rationale {
        return Err(StoreError::CorruptDatabase);
    }
    let state = match state.as_str() {
        "prepared" => ActionState::Prepared,
        "committed" => ActionState::Committed,
        "uncertain" => ActionState::Uncertain,
        "failed" => ActionState::Failed,
        _ => return Err(StoreError::CorruptDatabase),
    };
    let outcome: Option<StepOutcome> = outcome.map(|s| decode(&s)).transpose()?;
    if let Some(o) = &outcome {
        let unresolved_effect = o.dispatch == DispatchState::Uncertain
            || (o.dispatch == DispatchState::Dispatched
                && matches!(
                    o.verification,
                    VerificationStatus::NotChecked | VerificationStatus::Inconclusive
                ));
        let contradictory = match state {
            ActionState::Prepared => true,
            ActionState::Committed => {
                unresolved_effect || o.verification == VerificationStatus::Failed
            }
            ActionState::Failed => {
                unresolved_effect || o.verification != VerificationStatus::Failed
            }
            // Unknown usage may retain any otherwise valid outcome as uncertain.
            ActionState::Uncertain => false,
        };
        if o.schema_version != 1
            || (o.dispatch == DispatchState::Uncertain
                && o.verification == VerificationStatus::Passed)
            || contradictory
        {
            return Err(StoreError::CorruptDatabase);
        }
    } else if matches!(state, ActionState::Committed | ActionState::Failed) {
        return Err(StoreError::CorruptDatabase);
    }
    let reported_overrun = if state == ActionState::Uncertain {
        let row: Option<(String, String)> = sql(c.query_row(
            "SELECT substr(reservation_json,1,?2),substr(overrun_json,1,?2) FROM budget_reservations WHERE id=?1 AND task_id=?3 AND session_id=?4 AND overrun_json IS NOT NULL",
            params![reservation_id(context, step), READ_LIMIT, context.task_id.as_str(), context.run_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional())?;
        if let Some((reservation, report)) = row {
            let reservation: Reservation = decode(&reservation)?;
            let report: crate::budget::OverrunReport = decode(&report)?;
            let (contract, _, _) = read_run(c, context)?;
            let (_, purchase) = reservation
                .charges
                .quote(reservation.reserve, &contract.budgets.budget_currency)
                .map_err(|_| StoreError::CorruptDatabase)?;
            if reservation.schema_version != 1
                || reservation.context != *context
                || reservation.step != step
                || report.schema_version != 1
                || !(report.actual.is_some_and(|actual| {
                    actual.input_tokens > reservation.reserve.input_tokens
                        || actual.output_tokens > reservation.reserve.output_tokens
                        || actual.active_ms > reservation.reserve.active_ms
                }) || report
                    .purchase_actual
                    .is_some_and(|actual| actual > purchase))
            {
                return Err(StoreError::CorruptDatabase);
            }
            Some(report)
        } else {
            None
        }
    } else {
        None
    };
    Ok(AuditEntry {
        step,
        decision,
        state,
        outcome,
        reported_overrun,
    })
}
fn read_checkpoint(
    c: &Connection,
    context: &RunContext,
    next: u64,
    usage: &CumulativeUsage,
) -> Result<Option<CheckpointMetadata>, StoreError> {
    let row:Option<(u64,String,bool)>=sql(c.query_row("SELECT step,substr(metadata_json,1,?2),snapshot_ciphertext IS NOT NULL FROM checkpoints WHERE session_id=?1 ORDER BY step DESC LIMIT 1",params![context.run_id.as_str(),READ_LIMIT],|r|Ok((counter(r,0)?,r.get(1)?,r.get(2)?))).optional())?;
    let Some((step, json, encrypted)) = row else {
        return Ok(None);
    };
    if encrypted {
        return Err(StoreError::EncryptionRequired);
    }
    let cp: CheckpointMetadata = decode(&json)?;
    cp.usage
        .validate()
        .map_err(|_| StoreError::CorruptDatabase)?;
    if cp.schema_version != 1
        || cp.context != *context
        || cp.step != step
        || step == 0
        || cp.next_step <= step
        || cp.next_step > next
        || cp.usage.attempts_spent < cp.next_step - 1
        || cp.usage.attempts_spent > usage.attempts_spent
        || cp.usage.tokens_spent > usage.tokens_spent
        || cp.usage.active_ms_spent > usage.active_ms_spent
        || cp.usage.recovery_count > usage.recovery_count
        || !matches!(cp.status, RunStatus::Paused | RunStatus::NeedsReview)
        || cp.evidence_ids.len() > MAX_RECORDS
        || cp.artifact_ids.len() > MAX_RECORDS
    {
        return Err(StoreError::CorruptDatabase);
    }
    for (table, ids) in [
        (
            "evidence",
            cp.evidence_ids
                .iter()
                .map(EvidenceId::as_str)
                .collect::<Vec<_>>(),
        ),
        (
            "artifacts",
            cp.artifact_ids.iter().map(ArtifactId::as_str).collect(),
        ),
    ] {
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            identifier(id).map_err(|_| StoreError::CorruptDatabase)?;
            let json:String=sql(c.query_row(&format!("SELECT substr(metadata_json,1,?4) FROM {table} WHERE id=?1 AND task_id=?2 AND session_id=?3"),params![id,context.task_id.as_str(),context.run_id.as_str(),READ_LIMIT],|r|r.get(0)).optional())?.ok_or(StoreError::CorruptDatabase)?;
            let (record_id, record_context, sha256) = if table == "evidence" {
                let record: EvidenceRecord = decode(&json)?;
                if record.bytes > MAX_COUNTER
                    || crate::artifacts::check_evidence_owner(&context.task_id, id).is_err()
                {
                    return Err(StoreError::CorruptDatabase);
                }
                (record.id.as_str().to_owned(), record.context, record.sha256)
            } else {
                let record: ArtifactRecord = decode(&json)?;
                if record.bytes > MAX_COUNTER
                    || crate::artifacts::check_artifact_owner(&context.task_id, id).is_err()
                {
                    return Err(StoreError::CorruptDatabase);
                }
                (record.id.as_str().to_owned(), record.context, record.sha256)
            };
            digest(&sha256).map_err(|_| StoreError::CorruptDatabase)?;
            if record_id != id || record_context != *context || !seen.insert(id) {
                return Err(StoreError::CorruptDatabase);
            }
        }
    }
    Ok(Some(cp))
}
fn load_artifact(
    c: &Connection,
    task_id: &TaskId,
    id: &ArtifactId,
) -> Result<ArtifactRecord, StoreError> {
    let json: String = sql(c
        .query_row(
            "SELECT substr(metadata_json,1,?3) FROM artifacts WHERE id=?1 AND task_id=?2",
            params![id.as_str(), task_id.as_str(), READ_LIMIT],
            |r| r.get(0),
        )
        .optional())?
    .ok_or(StoreError::NotFound)?;
    checked_artifact(&json, task_id, id)
}
fn load_evidence(
    c: &Connection,
    task_id: &TaskId,
    id: &EvidenceId,
) -> Result<EvidenceRecord, StoreError> {
    let json: String = sql(c
        .query_row(
            "SELECT substr(metadata_json,1,?3) FROM evidence WHERE id=?1 AND task_id=?2",
            params![id.as_str(), task_id.as_str(), ARTIFACT_ROW_LIMIT],
            |r| r.get(0),
        )
        .optional())?
    .ok_or(StoreError::NotFound)?;
    let record: EvidenceRecord = decode(&json)?;
    digest(&record.sha256).map_err(|_| StoreError::CorruptDatabase)?;
    if record.id != *id
        || record.context.task_id != *task_id
        || record.bytes > MAX_COUNTER
        || crate::artifacts::check_evidence_owner(task_id, id.as_str()).is_err()
    {
        return Err(StoreError::CorruptDatabase);
    }
    Ok(record)
}
fn load_artifacts(c: &Connection, task_id: &TaskId) -> Result<Vec<ArtifactRecord>, StoreError> {
    let mut statement = sql(c.prepare(
        "SELECT id, substr(metadata_json,1,?2) FROM artifacts WHERE task_id=?1 ORDER BY id LIMIT ?3",
    ))?;
    let rows = sql(statement.query_map(
        params![
            task_id.as_str(),
            ARTIFACT_ROW_LIMIT,
            integer(MAX_ARTIFACT_INVENTORY as u64 + 1)?
        ],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ))?;
    let mut records = Vec::new();
    for row in rows {
        let (id, json) = sql(row)?;
        if records.len() == MAX_ARTIFACT_INVENTORY {
            return Err(StoreError::BudgetExceeded);
        }
        records.push(checked_artifact(&json, task_id, &ArtifactId::from(id))?);
    }
    Ok(records)
}
/// A stored row must decode to a record for exactly this task and ID.
fn checked_artifact(
    json: &str,
    task_id: &TaskId,
    id: &ArtifactId,
) -> Result<ArtifactRecord, StoreError> {
    let record: ArtifactRecord = decode(json)?;
    digest(&record.sha256).map_err(|_| StoreError::CorruptDatabase)?;
    if record.id != *id
        || record.context.task_id != *task_id
        || record.bytes > MAX_COUNTER
        || crate::artifacts::check_artifact_owner(task_id, id.as_str()).is_err()
    {
        return Err(StoreError::CorruptDatabase);
    }
    Ok(record)
}
fn apply_commit(
    c: &Connection,
    mut request: CommitStep,
    purchase_actual: Option<u64>,
    attachment_error: Option<StoreError>,
) -> Result<bool, StoreError> {
    let context = &request.prepared.context;
    let step = request.prepared.step;
    let now = stamp()?;
    let (contract, next, _) = read_run(c, context)?;
    if next != step + 1 {
        return Err(StoreError::Conflict);
    }
    let action = read_action(c, context, step)?;
    if action.state != ActionState::Prepared {
        return Err(StoreError::Conflict);
    }
    let reservation = read_reservation(c, context, step)?;
    let (_, purchase) = reservation
        .charges
        .quote(reservation.reserve, &contract.budgets.budget_currency)
        .map_err(|_| StoreError::CorruptDatabase)?;
    // A trusted report of excess consumption is an unresolved effect, not a
    // rejected typo that can later be replaced by a smaller successful report.
    let overrun = purchase_actual.is_some_and(|v| v > purchase)
        || request.actual.is_some_and(|actual| {
            actual.input_tokens > reservation.reserve.input_tokens
                || actual.output_tokens > reservation.reserve.output_tokens
                || actual.active_ms > reservation.reserve.active_ms
        });
    let outcome_error = request.validate_outcome().err();
    if overrun {
        // Attachment conflicts cannot undo the conservative safety transition.
        request.evidence.clear();
        request.artifacts.clear();
    } else {
        if let Some(error) = outcome_error {
            return Err(error);
        }
        if let Some(error) = attachment_error {
            return Err(error);
        }
    }
    let uncertain = overrun
        || request.actual.is_none()
        || (purchase > 0 && purchase_actual.is_none())
        || request.outcome.dispatch == DispatchState::Uncertain
        || (request.outcome.dispatch == DispatchState::Dispatched
            && matches!(
                request.outcome.verification,
                VerificationStatus::NotChecked | VerificationStatus::Inconclusive
            ));
    let state = if uncertain {
        "uncertain"
    } else if request.outcome.verification == VerificationStatus::Failed {
        "failed"
    } else {
        "committed"
    };
    let status = if uncertain { "needs_review" } else { "paused" };
    let mut usage = read_usage(c, &context.task_id)?;
    usage.contains(reservation.reserve, &reservation.charges, &contract)?;
    sql(c.execute(
        "UPDATE actions SET state=?3,outcome_json=?4 WHERE session_id=?1 AND step=?2",
        params![
            context.run_id.as_str(),
            integer(step)?,
            state,
            if outcome_error.is_none() {
                Some(encode(&request.outcome)?)
            } else {
                None
            }
        ],
    ))?;
    if !uncertain {
        let actual = request.actual.ok_or(StoreError::InvalidInput)?;
        usage.settle(
            reservation.reserve,
            &reservation.charges,
            actual,
            purchase_actual.unwrap_or(0),
            &contract,
        )?;
    }
    write_usage(c, &context.task_id, &usage, &now)?;
    let settlement = if uncertain {
        None
    } else {
        Some(encode(&ActionSettlement {
            schema_version: 1,
            actual: request.actual.ok_or(StoreError::InvalidInput)?,
            purchase_actual: purchase_actual.unwrap_or(0),
        })?)
    };
    sql(c.execute(
        "UPDATE budget_reservations SET state=?2,settlement_json=?3,overrun_json=?4 WHERE id=?1",
        params![
            reservation_id(context, step),
            if uncertain { "uncertain" } else { "settled" },
            settlement,
            if overrun {
                Some(encode(&crate::budget::OverrunReport {
                    schema_version: 1,
                    actual: request.actual,
                    purchase_actual,
                })?)
            } else {
                None
            },
        ],
    ))?;
    for r in &request.evidence {
        sql(c.execute(
            "INSERT INTO evidence VALUES (?1,?2,?3,?4,?5)",
            params![
                r.id.as_str(),
                context.task_id.as_str(),
                context.run_id.as_str(),
                encode(r)?,
                now
            ],
        ))?;
    }
    for r in &request.artifacts {
        sql(c.execute(
            "INSERT INTO artifacts VALUES (?1,?2,?3,?4,?5)",
            params![
                r.id.as_str(),
                context.task_id.as_str(),
                context.run_id.as_str(),
                encode(r)?,
                now
            ],
        ))?;
    }
    let cp = CheckpointMetadata {
        schema_version: 1,
        context: context.clone(),
        step,
        next_step: next,
        usage: usage.public()?,
        status: if uncertain {
            RunStatus::NeedsReview
        } else {
            RunStatus::Paused
        },
        evidence_ids: request.evidence.into_iter().map(|r| r.id).collect(),
        artifact_ids: request.artifacts.into_iter().map(|r| r.id).collect(),
    };
    sql(c.execute(
        "INSERT INTO checkpoints VALUES (?1,?2,1,?3,NULL,NULL,NULL,?4)",
        params![context.run_id.as_str(), integer(step)?, encode(&cp)?, now],
    ))?;
    sql(c.execute(
        "UPDATE sessions SET status=?2,updated_at=?3 WHERE id=?1",
        params![context.run_id.as_str(), status, now],
    ))?;
    sql(c.execute(
        "UPDATE tasks SET status=?2,updated_at=?3 WHERE id=?1",
        params![context.task_id.as_str(), status, now],
    ))?;
    Ok(overrun)
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
