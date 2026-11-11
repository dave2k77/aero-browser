//! E02: one controller per run serializes task control and browser input
//! ownership.
//!
//! Trusted callers send typed commands (start, pause, resume, cancel,
//! takeover, release, reply, approve); each call returns once the command has
//! taken effect. The controller drives an [`Automation`] through plan →
//! dispatch → verify steps and is the only caller of [`Automation::dispatch`]:
//!
//! - Pause drops a planning or read-only wait at once. An input sequence
//!   already in flight may finish for up to [`INPUT_GRACE`]; after that it is
//!   abandoned and reported uncertain. No dispatch starts once a pause,
//!   cancel or takeover has been requested.
//! - Cancel interrupts the same way, then ends the run.
//! - Takeover pauses, then hands exclusive input ownership to the human as a
//!   [`HumanInput`] token (not `Clone`). Automation cannot run until that
//!   token is released. The first resume after manual input invalidates the
//!   automation's observations and voids the run's unconsumed approvals
//!   through the [`RunGuard`], so automation re-observes what the user left.
//!
//! Status and ownership changes are published as gapless [`PublicEvent`]s.
//! Run status is held in memory: persisting transitions, flushing on close
//! and signal handling are E07/N09. The real automation (gate, executor,
//! planner) is E05; dispatch must go through the E01 gate there.
//!
//! ```compile_fail,E0599
//! fn twice(token: aero_agent::controller::HumanInput) { let _ = (token.clone(), token); }
//! ```

use crate::capabilities::Capabilities;
use crate::store::StoreError;
use aero_core::checkpoint::{RunContext, RunStatus};
use aero_core::events::{
    DispatchState, EVENT_SCHEMA_VERSION, EventKind, InputOwner, PublicEvent, VerificationStatus,
};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

/// How long an in-flight input sequence may finish after an interruption.
pub const INPUT_GRACE: Duration = Duration::from_millis(500);
/// Events kept for slow subscribers; a subscriber further behind sees a gap.
pub const EVENT_BUFFER: usize = 256;

/// What a planning step decided.
pub enum Plan<S> {
    /// Dispatch this step.
    Act(S),
    /// Wait for the user: a question, or a commit awaiting approval.
    AwaitUser,
    /// The automation proposes completion (verification is separate).
    Complete,
}

/// What the user supplied while the run awaited them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserInput {
    Reply(String),
    /// An approval the trusted user issued (its ID).
    Approved(String),
}

/// One run's planner, executor and verifier as the controller sees them.
/// Futures may be dropped at any await point except as documented.
pub trait Automation: Send + 'static {
    type Step: Send;
    /// Decide the next step. Dropped on pause, cancel or takeover.
    fn plan(&mut self) -> impl Future<Output = Plan<Self::Step>> + Send;
    /// Send one bounded input sequence. On interruption it may run for
    /// [`INPUT_GRACE`] more and is then dropped and reported uncertain.
    fn dispatch(&mut self, step: Self::Step) -> impl Future<Output = DispatchState> + Send;
    /// Read-only checks after a dispatch. Dropped on interruption.
    fn verify(&mut self) -> impl Future<Output = ()> + Send;
    /// The user's answer while the run awaited them.
    fn answer(&mut self, input: UserInput);
    /// Forget every observation and handle: the page may have changed.
    fn invalidate_observations(&mut self);
}

/// Host-side effects of control transitions on durable state.
pub trait RunGuard: Send + Sync + 'static {
    /// Void the run's unconsumed approvals (after manual input).
    fn void_approvals(
        &self,
        context: &RunContext,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl RunGuard for Capabilities {
    async fn void_approvals(&self, context: &RunContext) -> Result<(), StoreError> {
        self.void_run_approvals(context.clone())?.recv().await
    }
}

/// Exclusive human input ownership for one controller. Releasing it is the
/// only way to give input back to automation.
#[derive(Debug)]
pub struct HumanInput {
    controller: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlError {
    /// The run has already been started.
    AlreadyStarted,
    NotStarted,
    /// The run has ended (completed, cancelled or failed).
    Ended,
    /// The command does not apply in the current status.
    InvalidState(RunStatus),
    /// A human holds input ownership.
    HumanOwnsInput,
    /// The token belongs to another controller.
    WrongToken,
    /// Voiding approvals failed; the run stays paused.
    Store(StoreError),
    /// The controller task has stopped.
    Closed,
}

/// A consistent view for (re)synchronising subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// `None` until started.
    pub status: Option<RunStatus>,
    pub owner: InputOwner,
    /// The sequence of the last published event (0: none yet).
    pub sequence: u64,
}

enum Command {
    Start,
    Pause,
    Resume,
    Cancel,
    Takeover,
    Release(HumanInput),
    Input(UserInput),
    Snapshot,
}

enum Reply {
    Done,
    Token(HumanInput),
    Snapshot(Snapshot),
}

type Request = (Command, oneshot::Sender<Result<Reply, ControlError>>);

static NEXT_CONTROLLER: AtomicU64 = AtomicU64::new(1);

/// The handle trusted callers use. Cloning shares the same controller.
#[derive(Clone)]
pub struct Controller {
    requests: mpsc::Sender<Request>,
    events: broadcast::Sender<PublicEvent>,
}

impl Controller {
    /// Spawn the controller task for `context` on the current Tokio runtime.
    pub fn spawn<A: Automation, G: RunGuard>(context: RunContext, automation: A, guard: G) -> Self {
        let (requests, commands) = mpsc::channel(16);
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let id = NEXT_CONTROLLER.fetch_add(1, Ordering::Relaxed);
        let actor = Actor {
            core: Core {
                id,
                context,
                guard,
                commands,
                events: events.clone(),
                sequence: 0,
                step: 0,
                status: None,
                owner: InputOwner::Automation,
                manual_input: false,
            },
            automation,
        };
        tokio::spawn(actor.run());
        Self { requests, events }
    }

    async fn send(&self, command: Command) -> Result<Reply, ControlError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send((command, reply))
            .await
            .map_err(|_| ControlError::Closed)?;
        answer.await.map_err(|_| ControlError::Closed)?
    }

    pub async fn start(&self) -> Result<(), ControlError> {
        self.send(Command::Start).await.map(|_| ())
    }
    /// Returns once automation has stopped: no input is in flight and none
    /// will start.
    pub async fn pause(&self) -> Result<(), ControlError> {
        self.send(Command::Pause).await.map(|_| ())
    }
    pub async fn resume(&self) -> Result<(), ControlError> {
        self.send(Command::Resume).await.map(|_| ())
    }
    pub async fn cancel(&self) -> Result<(), ControlError> {
        self.send(Command::Cancel).await.map(|_| ())
    }
    /// Pause, then hand input to the human.
    pub async fn takeover(&self) -> Result<HumanInput, ControlError> {
        match self.send(Command::Takeover).await? {
            Reply::Token(token) => Ok(token),
            _ => Err(ControlError::Closed),
        }
    }
    /// Give input back; the run stays paused until resumed.
    pub async fn release(&self, token: HumanInput) -> Result<(), ControlError> {
        self.send(Command::Release(token)).await.map(|_| ())
    }
    pub async fn reply(&self, text: String) -> Result<(), ControlError> {
        self.send(Command::Input(UserInput::Reply(text)))
            .await
            .map(|_| ())
    }
    pub async fn approve(&self, approval_id: String) -> Result<(), ControlError> {
        self.send(Command::Input(UserInput::Approved(approval_id)))
            .await
            .map(|_| ())
    }
    pub async fn snapshot(&self) -> Result<Snapshot, ControlError> {
        match self.send(Command::Snapshot).await? {
            Reply::Snapshot(snapshot) => Ok(snapshot),
            _ => Err(ControlError::Closed),
        }
    }
    /// Events from now on. A `Lagged` error means events were missed: take a
    /// [`Self::snapshot`] and continue from its sequence.
    pub fn subscribe(&self) -> broadcast::Receiver<PublicEvent> {
        self.events.subscribe()
    }
}

/// What a command received while automation runs leads to.
enum During {
    /// Answered without stopping automation.
    Handled,
    /// Pause, cancel or takeover: stop automation, then apply.
    Interrupt(Request),
    /// Every handle is gone: stop automation and end.
    Closed,
}

struct Core<G> {
    id: u64,
    context: RunContext,
    guard: G,
    commands: mpsc::Receiver<Request>,
    events: broadcast::Sender<PublicEvent>,
    sequence: u64,
    step: u64,
    status: Option<RunStatus>,
    owner: InputOwner,
    /// A human held input since automation last ran.
    manual_input: bool,
}

struct Actor<A, G> {
    core: Core<G>,
    automation: A,
}

fn ended(status: Option<RunStatus>) -> bool {
    matches!(
        status,
        Some(RunStatus::Completed | RunStatus::Cancelled | RunStatus::Failed)
    )
}

impl<G: RunGuard> Core<G> {
    fn publish(&mut self, kind: EventKind) {
        self.sequence += 1;
        // No subscribers is fine: snapshots carry the state.
        let _ = self.events.send(PublicEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            context: self.context.clone(),
            sequence: self.sequence,
            kind,
            evidence_ids: vec![],
            artifact_ids: vec![],
        });
    }

    fn set_status(&mut self, status: RunStatus) {
        if self.status != Some(status) {
            self.status = Some(status);
            self.publish(EventKind::StatusChanged { status });
        }
    }

    fn set_owner(&mut self, owner: InputOwner) {
        if self.owner != owner {
            self.owner = owner;
            self.publish(EventKind::InputOwnerChanged { owner });
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            status: self.status,
            owner: self.owner,
            sequence: self.sequence,
        }
    }

    /// The current status, if lifecycle commands apply to it.
    fn lifecycle(&self) -> Result<RunStatus, ControlError> {
        match self.status {
            None => Err(ControlError::NotStarted),
            status if ended(status) => Err(ControlError::Ended),
            Some(status) => Ok(status),
        }
    }

    /// A command that arrived while automation runs.
    fn during_run(&mut self, request: Option<Request>) -> During {
        let Some((command, reply)) = request else {
            return During::Closed;
        };
        let answer = match command {
            Command::Pause | Command::Cancel | Command::Takeover => {
                return During::Interrupt((command, reply));
            }
            Command::Snapshot => Ok(Reply::Snapshot(self.snapshot())),
            Command::Start => Err(ControlError::AlreadyStarted),
            Command::Release(token) if token.controller != self.id => Err(ControlError::WrongToken),
            Command::Resume | Command::Input(_) | Command::Release(_) => {
                Err(ControlError::InvalidState(RunStatus::Running))
            }
        };
        let _ = reply.send(answer);
        During::Handled
    }

    /// Apply interruptions once automation has stopped, in arrival order.
    /// `None`: every handle is gone, which ends the run.
    fn interrupt(&mut self, interruptions: Vec<Option<Request>>) {
        for interruption in interruptions {
            match interruption {
                Some((command, reply)) => {
                    let answer = self.stop(command);
                    let _ = reply.send(answer);
                }
                None => self.set_status(RunStatus::Cancelled),
            }
        }
    }

    /// Pause, cancel or takeover, with automation stopped.
    fn stop(&mut self, command: Command) -> Result<Reply, ControlError> {
        let status = self.lifecycle()?;
        match command {
            Command::Pause if status == RunStatus::Paused => {
                Err(ControlError::InvalidState(status))
            }
            Command::Pause => {
                self.set_status(RunStatus::Paused);
                Ok(Reply::Done)
            }
            Command::Cancel => {
                self.set_status(RunStatus::Cancelled);
                Ok(Reply::Done)
            }
            Command::Takeover if self.owner == InputOwner::Human => {
                Err(ControlError::HumanOwnsInput)
            }
            Command::Takeover => {
                self.set_status(RunStatus::Paused);
                self.set_owner(InputOwner::Human);
                self.manual_input = true;
                Ok(Reply::Token(HumanInput {
                    controller: self.id,
                }))
            }
            _ => unreachable!("only pause, cancel and takeover stop automation"),
        }
    }
}

impl<A: Automation, G: RunGuard> Actor<A, G> {
    async fn run(mut self) {
        loop {
            if self.core.status == Some(RunStatus::Running)
                && self.core.owner == InputOwner::Automation
            {
                if !self.step().await {
                    return;
                }
            } else {
                let Some((command, reply)) = self.core.commands.recv().await else {
                    return;
                };
                let answer = self.idle(command).await;
                let _ = reply.send(answer);
            }
        }
    }

    /// A command while automation is not running.
    async fn idle(&mut self, command: Command) -> Result<Reply, ControlError> {
        let core = &mut self.core;
        match command {
            Command::Snapshot => Ok(Reply::Snapshot(core.snapshot())),
            Command::Start => match core.status {
                None => {
                    core.set_status(RunStatus::Running);
                    Ok(Reply::Done)
                }
                status if ended(status) => Err(ControlError::Ended),
                Some(_) => Err(ControlError::AlreadyStarted),
            },
            Command::Pause | Command::Cancel | Command::Takeover => core.stop(command),
            Command::Release(token) => {
                if token.controller != core.id {
                    return Err(ControlError::WrongToken);
                }
                let status = core.lifecycle()?;
                if core.owner != InputOwner::Human {
                    return Err(ControlError::InvalidState(status));
                }
                core.set_owner(InputOwner::Automation);
                Ok(Reply::Done)
            }
            Command::Resume => {
                let status = core.lifecycle()?;
                if core.owner == InputOwner::Human {
                    return Err(ControlError::HumanOwnsInput);
                }
                if status != RunStatus::Paused {
                    return Err(ControlError::InvalidState(status));
                }
                if core.manual_input {
                    core.guard
                        .void_approvals(&core.context)
                        .await
                        .map_err(ControlError::Store)?;
                    self.automation.invalidate_observations();
                    core.manual_input = false;
                }
                core.set_status(RunStatus::Running);
                Ok(Reply::Done)
            }
            Command::Input(input) => {
                let status = core.lifecycle()?;
                if status != RunStatus::AwaitingUser {
                    return Err(ControlError::InvalidState(status));
                }
                self.automation.answer(input);
                core.set_status(RunStatus::Running);
                Ok(Reply::Done)
            }
        }
    }

    /// One plan → dispatch → verify step. `false`: every handle is gone.
    async fn step(&mut self) -> bool {
        let Self { core, automation } = self;
        // Plan: dropped on any interruption.
        let plan = {
            let planning = automation.plan();
            tokio::pin!(planning);
            loop {
                tokio::select! {
                    plan = &mut planning => break plan,
                    request = core.commands.recv() => match core.during_run(request) {
                        During::Handled => {}
                        During::Interrupt(request) => {
                            core.interrupt(vec![Some(request)]);
                            return true;
                        }
                        During::Closed => return false,
                    },
                }
            }
        };
        let step = match plan {
            Plan::Act(step) => step,
            Plan::AwaitUser => {
                core.set_status(RunStatus::AwaitingUser);
                return true;
            }
            Plan::Complete => {
                core.set_status(RunStatus::Completed);
                return true;
            }
        };
        // Nothing starts once an interruption is queued.
        loop {
            match core.commands.try_recv() {
                Ok(request) => match core.during_run(Some(request)) {
                    During::Handled => {}
                    During::Interrupt(request) => {
                        core.interrupt(vec![Some(request)]);
                        return true;
                    }
                    During::Closed => return false,
                },
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => return false,
            }
        }
        // Dispatch: an interruption gives the input sequence INPUT_GRACE to
        // finish; then it is dropped and reported uncertain.
        core.step += 1;
        let number = core.step;
        let mut pending: Vec<Option<Request>> = Vec::new();
        let mut deadline: Option<Instant> = None;
        let state = {
            let dispatching = automation.dispatch(step);
            tokio::pin!(dispatching);
            loop {
                // A placeholder while no interruption has set a deadline.
                let limit = deadline.unwrap_or_else(|| Instant::now() + INPUT_GRACE);
                let closed = pending.iter().any(Option::is_none);
                tokio::select! {
                    state = &mut dispatching => break state,
                    request = core.commands.recv(), if !closed => {
                        match core.during_run(request) {
                            During::Handled => {}
                            During::Interrupt(request) => pending.push(Some(request)),
                            During::Closed => pending.push(None),
                        }
                        if !pending.is_empty() && deadline.is_none() {
                            deadline = Some(Instant::now() + INPUT_GRACE);
                        }
                    }
                    () = sleep_until(limit), if deadline.is_some() => break DispatchState::Uncertain,
                }
            }
        };
        core.publish(EventKind::StepOutcome {
            step: number,
            dispatch_state: state,
            verification_status: VerificationStatus::NotChecked,
        });
        if !pending.is_empty() {
            let closed = pending.iter().any(Option::is_none);
            core.interrupt(pending);
            return !closed;
        }
        // Verify: read-only, dropped on any interruption.
        let verifying = automation.verify();
        tokio::pin!(verifying);
        loop {
            tokio::select! {
                () = &mut verifying => return true,
                request = core.commands.recv() => match core.during_run(request) {
                    During::Handled => {}
                    During::Interrupt(request) => {
                        core.interrupt(vec![Some(request)]);
                        return true;
                    }
                    During::Closed => return false,
                },
            }
        }
    }
}
