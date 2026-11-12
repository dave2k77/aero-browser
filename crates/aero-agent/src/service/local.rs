//! N08 in-process implementation.

use super::{AutomationFactory, ServiceError, StreamItem};
use crate::capabilities::{Capabilities, NetworkAccess};
use crate::controller::{Controller, HumanInput, Snapshot};
use crate::preview::Preview;
use crate::store::StoreError;
use crate::store::worker::{ArtifactRecord, EvidenceRecord, Journal, JournalClient};
use crate::tasks::{TaskAuthority, TaskClient};
use aero_core::actions::Decision;
use aero_core::checkpoint::RunContext;
use aero_core::events::PublicEvent;
use aero_core::ids::{EvidenceId, RunId, TaskId};
use aero_core::task::{StartPrerequisites, TaskContract};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Pending approval requests kept per service.
pub const MAX_PENDING_REQUESTS: usize = 256;

struct RunEntry {
    context: RunContext,
    controller: Controller,
}

struct Inner<F> {
    factory: F,
    authority: TaskAuthority,
    owner: TaskClient,
    journal: JournalClient,
    capabilities: Capabilities,
    runs: Mutex<HashMap<RunId, RunEntry>>,
    requests: Mutex<HashMap<String, Preview>>,
    next_request: AtomicU64,
}

#[derive(Clone, PartialEq, Eq)]
enum Role {
    Trusted,
    Model(TaskId),
}

/// The host's service. Never give it to a model adapter: it mints sessions.
pub struct LocalService<F> {
    inner: Arc<Inner<F>>,
}

/// A caller's handle with a fixed, host-chosen role.
pub struct Session<F> {
    inner: Arc<Inner<F>>,
    role: Role,
}

impl<F> Clone for Session<F> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            role: self.role.clone(),
        }
    }
}

/// One subscriber's view of a run's events.
pub struct EventStream {
    controller: Controller,
    receiver: broadcast::Receiver<PublicEvent>,
    /// After a gap: events up to this sequence are already reflected.
    resume_after: u64,
}

impl EventStream {
    /// The next event, or a gap with a snapshot to continue from.
    pub async fn next(&mut self) -> StreamItem {
        loop {
            match self.receiver.recv().await {
                Ok(event) if event.sequence <= self.resume_after => {}
                Ok(event) => return StreamItem::Event(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Subscribe afresh first: the old receiver still points
                    // into the overwritten buffer. Events published between
                    // this and the snapshot are reflected in the snapshot
                    // and skipped by sequence.
                    let fresh = self.controller.subscribe();
                    return match self.controller.snapshot().await {
                        Ok(snapshot) => {
                            self.receiver = fresh;
                            self.resume_after = snapshot.sequence;
                            StreamItem::Gap(snapshot)
                        }
                        Err(_) => StreamItem::Closed,
                    };
                }
                Err(broadcast::error::RecvError::Closed) => return StreamItem::Closed,
            }
        }
    }
}

impl<F: AutomationFactory> LocalService<F> {
    pub fn new(journal: &Journal, factory: F, network: NetworkAccess) -> Self {
        let authority = TaskAuthority::new(journal);
        let owner = authority.trusted_user();
        let capabilities = authority.capabilities(network);
        Self {
            inner: Arc::new(Inner {
                factory,
                authority,
                owner,
                journal: journal.client(),
                capabilities,
                runs: Mutex::default(),
                requests: Mutex::default(),
                next_request: AtomicU64::new(1),
            }),
        }
    }

    /// For the CLI or desktop, once a trusted human channel exists.
    pub fn trusted(&self) -> Session<F> {
        Session {
            inner: self.inner.clone(),
            role: Role::Trusted,
        }
    }

    /// For a model adapter: this task only.
    pub fn model(&self, task: TaskId) -> Result<Session<F>, ServiceError> {
        self.inner.authority.model_for_task(task.clone())?;
        Ok(Session {
            inner: self.inner.clone(),
            role: Role::Model(task),
        })
    }
}

impl<F: AutomationFactory> Session<F> {
    fn require_trusted(&self) -> Result<(), ServiceError> {
        match self.role {
            Role::Trusted => Ok(()),
            Role::Model(_) => Err(ServiceError::PermissionDenied),
        }
    }

    fn sees(&self, task: &TaskId) -> bool {
        match &self.role {
            Role::Trusted => true,
            Role::Model(own) => own == task,
        }
    }

    /// The run, if it exists in the caller's scope.
    fn run(&self, run: &RunId) -> Result<(RunContext, Controller), ServiceError> {
        let runs = self.inner.runs.lock().map_err(|_| ServiceError::NotFound)?;
        let entry = runs.get(run).ok_or(ServiceError::NotFound)?;
        if !self.sees(&entry.context.task_id) {
            return Err(ServiceError::NotFound);
        }
        Ok((entry.context.clone(), entry.controller.clone()))
    }

    /// The controllers of the task's runs other than `except`.
    fn task_controllers(&self, task: &TaskId, except: Option<&RunId>) -> Vec<Controller> {
        self.inner
            .runs
            .lock()
            .map(|runs| {
                runs.iter()
                    .filter(|(id, entry)| entry.context.task_id == *task && Some(*id) != except)
                    .map(|(_, entry)| entry.controller.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn create_task(&self, contract: TaskContract) -> Result<(), ServiceError> {
        self.require_trusted()?;
        Ok(self.inner.owner.create_task(contract)?.recv().await?)
    }

    /// Revising pauses the task's runs (the journal records them paused and
    /// voids their grants and approvals); start a new run to continue.
    pub async fn revise_contract(&self, contract: TaskContract) -> Result<(), ServiceError> {
        self.require_trusted()?;
        let task = contract.task_id.clone();
        self.inner.owner.revise_contract(contract)?.recv().await?;
        for controller in self.task_controllers(&task, None) {
            let _ = controller.pause().await;
        }
        Ok(())
    }

    /// Record the run, end the task's earlier runs and start automation.
    pub async fn start_run(
        &self,
        context: RunContext,
        prerequisites: StartPrerequisites,
    ) -> Result<(), ServiceError> {
        self.require_trusted()?;
        self.inner
            .owner
            .start_run(context.clone(), prerequisites)?
            .recv()
            .await?;
        // The journal cancelled the task's earlier runs; so do their controllers.
        for controller in self.task_controllers(&context.task_id, Some(&context.run_id)) {
            let _ = controller.cancel().await;
        }
        let controller = Controller::spawn(
            context.clone(),
            self.inner.factory.automation(&context),
            self.inner.capabilities.clone(),
        );
        controller.start().await?;
        if let Ok(mut runs) = self.inner.runs.lock() {
            runs.insert(
                context.run_id.clone(),
                RunEntry {
                    context,
                    controller,
                },
            );
        }
        Ok(())
    }

    /// E05: no observation backend yet.
    pub async fn observe(&self, run: &RunId) -> Result<(), ServiceError> {
        self.run(run)?;
        Err(ServiceError::Unsupported)
    }

    /// E05: no planner/executor path yet; proposals never bypass the gate.
    pub async fn propose_action(
        &self,
        run: &RunId,
        decision: Decision,
    ) -> Result<(), ServiceError> {
        self.run(run)?;
        let _ = decision;
        Err(ServiceError::Unsupported)
    }

    /// Any caller in scope may stop its run.
    pub async fn pause(&self, run: &RunId) -> Result<(), ServiceError> {
        Ok(self.run(run)?.1.pause().await?)
    }

    pub async fn resume(&self, run: &RunId) -> Result<(), ServiceError> {
        self.require_trusted()?;
        Ok(self.run(run)?.1.resume().await?)
    }

    pub async fn cancel(&self, run: &RunId) -> Result<(), ServiceError> {
        self.require_trusted()?;
        Ok(self.run(run)?.1.cancel().await?)
    }

    pub async fn takeover(&self, run: &RunId) -> Result<HumanInput, ServiceError> {
        self.require_trusted()?;
        Ok(self.run(run)?.1.takeover().await?)
    }

    pub async fn release(&self, run: &RunId, token: HumanInput) -> Result<(), ServiceError> {
        self.require_trusted()?;
        Ok(self.run(run)?.1.release(token).await?)
    }

    /// Ask the trusted user to approve this preview; returns a request ID.
    /// Nothing is approved until a trusted caller grants it.
    pub async fn request_approval(&self, preview: Preview) -> Result<String, ServiceError> {
        let (context, _) = self.run(&preview.run_id)?;
        if context.task_id != preview.task_id {
            return Err(ServiceError::NotFound);
        }
        preview
            .validate()
            .map_err(|_| ServiceError::Store(StoreError::InvalidInput))?;
        let mut requests = self
            .inner
            .requests
            .lock()
            .map_err(|_| ServiceError::Store(StoreError::Io))?;
        if requests.len() >= MAX_PENDING_REQUESTS {
            return Err(ServiceError::Store(StoreError::QueueFull));
        }
        let id = format!(
            "request-{}",
            self.inner.next_request.fetch_add(1, Ordering::Relaxed)
        );
        requests.insert(id.clone(), preview);
        Ok(id)
    }

    /// Trusted only: approve a pending request exactly as previewed, and
    /// hand the approval to its run if the run is waiting for it.
    pub async fn grant_approval(
        &self,
        request: &str,
        approval_id: String,
        expires_at_ms: u64,
    ) -> Result<(), ServiceError> {
        self.require_trusted()?;
        let preview = self
            .inner
            .requests
            .lock()
            .map_err(|_| ServiceError::Store(StoreError::Io))?
            .remove(request)
            .ok_or(ServiceError::NotFound)?;
        let run = preview.run_id.clone();
        let approved =
            match self
                .inner
                .owner
                .approve(approval_id.clone(), preview.clone(), expires_at_ms)
            {
                Ok(reply) => reply.recv().await,
                Err(error) => Err(error),
            };
        if let Err(error) = approved {
            // Not approved: the request stays pending.
            if let Ok(mut requests) = self.inner.requests.lock() {
                requests.insert(request.to_owned(), preview);
            }
            return Err(error.into());
        }
        if let Ok((_, controller)) = self.run(&run) {
            // Only a run awaiting the user takes it now; otherwise the
            // automation finds the approval when it next needs it.
            let _ = controller.approve(approval_id).await;
        }
        Ok(())
    }

    pub async fn reply_user(&self, run: &RunId, text: String) -> Result<(), ServiceError> {
        self.require_trusted()?;
        Ok(self.run(run)?.1.reply(text).await?)
    }

    pub async fn list_artifacts(&self, task: &TaskId) -> Result<Vec<ArtifactRecord>, ServiceError> {
        if !self.sees(task) {
            return Err(ServiceError::NotFound);
        }
        Ok(self
            .inner
            .journal
            .load_artifacts(task.clone())?
            .recv()
            .await?)
    }

    pub async fn get_evidence(
        &self,
        task: &TaskId,
        id: EvidenceId,
    ) -> Result<EvidenceRecord, ServiceError> {
        if !self.sees(task) {
            return Err(ServiceError::NotFound);
        }
        Ok(self
            .inner
            .journal
            .load_evidence(task.clone(), id)?
            .recv()
            .await?)
    }

    /// F08: no recovery report yet.
    pub async fn get_recovery_report(&self, run: &RunId) -> Result<(), ServiceError> {
        self.run(run)?;
        Err(ServiceError::Unsupported)
    }

    pub async fn snapshot(&self, run: &RunId) -> Result<Snapshot, ServiceError> {
        Ok(self.run(run)?.1.snapshot().await?)
    }

    /// Events from now on, with explicit gaps (see [`EventStream::next`]).
    pub fn subscribe(&self, run: &RunId) -> Result<EventStream, ServiceError> {
        let (_, controller) = self.run(run)?;
        Ok(EventStream {
            receiver: controller.subscribe(),
            controller,
            resume_after: 0,
        })
    }
}
