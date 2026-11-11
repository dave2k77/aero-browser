//! E02: task control and input ownership, with a fake automation and paused
//! Tokio time (no sleep-based races).

use aero_agent::controller::{
    Automation, ControlError, Controller, INPUT_GRACE, Plan, RunGuard, UserInput,
};
use aero_agent::store::StoreError;
use aero_core::checkpoint::{RunContext, RunStatus};
use aero_core::events::{DispatchState, EventKind, InputOwner, PublicEvent};
use aero_core::ids::{RunId, TaskId};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{Instant, sleep};

fn context() -> RunContext {
    RunContext {
        task_id: TaskId::from("task"),
        run_id: RunId::from("run"),
        contract_revision: 1,
    }
}

#[derive(Default)]
struct Log {
    plans: usize,
    started: usize,
    finished: usize,
    abandoned: usize,
    verifies: usize,
    answers: Vec<UserInput>,
    invalidations: usize,
}

/// Plans `Act` (or `AwaitUser` while `await_user` is set), dispatches for
/// `dispatch` (forever if `None`), verifies for `verify`.
#[derive(Clone)]
struct Fake {
    log: Arc<Mutex<Log>>,
    plan: Duration,
    dispatch: Option<Duration>,
    verify: Duration,
    await_user: Arc<Mutex<bool>>,
    /// If set, planning also waits for this signal.
    gate: Option<Arc<tokio::sync::Notify>>,
}

impl Fake {
    fn new(plan: u64, dispatch: Option<u64>, verify: u64) -> Self {
        Self {
            log: Arc::default(),
            plan: Duration::from_millis(plan),
            dispatch: dispatch.map(Duration::from_millis),
            verify: Duration::from_millis(verify),
            await_user: Arc::default(),
            gate: None,
        }
    }
    fn log<T>(&self, read: impl FnOnce(&Log) -> T) -> T {
        read(&self.log.lock().unwrap())
    }
}

/// Counts an input sequence abandoned before it finished.
struct InFlight(Arc<Mutex<Log>>, bool);
impl Drop for InFlight {
    fn drop(&mut self) {
        if !self.1 {
            self.0.lock().unwrap().abandoned += 1;
        }
    }
}

impl Automation for Fake {
    type Step = ();
    fn plan(&mut self) -> impl Future<Output = Plan<()>> + Send {
        let (log, delay, await_user) = (self.log.clone(), self.plan, self.await_user.clone());
        let gate = self.gate.clone();
        async move {
            log.lock().unwrap().plans += 1;
            sleep(delay).await;
            if let Some(gate) = gate {
                gate.notified().await;
            }
            if *await_user.lock().unwrap() {
                Plan::AwaitUser
            } else {
                Plan::Act(())
            }
        }
    }
    fn dispatch(&mut self, (): ()) -> impl Future<Output = DispatchState> + Send {
        let (log, delay) = (self.log.clone(), self.dispatch);
        async move {
            log.lock().unwrap().started += 1;
            let mut flight = InFlight(log.clone(), false);
            match delay {
                Some(delay) => sleep(delay).await,
                None => std::future::pending::<()>().await,
            }
            flight.1 = true;
            log.lock().unwrap().finished += 1;
            DispatchState::Dispatched
        }
    }
    fn verify(&mut self) -> impl Future<Output = ()> + Send {
        let (log, delay) = (self.log.clone(), self.verify);
        async move {
            log.lock().unwrap().verifies += 1;
            sleep(delay).await;
        }
    }
    fn answer(&mut self, input: UserInput) {
        *self.await_user.lock().unwrap() = false;
        self.log.lock().unwrap().answers.push(input);
    }
    fn invalidate_observations(&mut self) {
        self.log.lock().unwrap().invalidations += 1;
    }
}

#[derive(Clone, Default)]
struct Guard(Arc<Mutex<Vec<RunContext>>>);
impl RunGuard for Guard {
    async fn void_approvals(&self, context: &RunContext) -> Result<(), StoreError> {
        self.0.lock().unwrap().push(context.clone());
        Ok(())
    }
}

fn spawn(fake: &Fake) -> (Controller, Guard, broadcast::Receiver<PublicEvent>) {
    let guard = Guard::default();
    let controller = Controller::spawn(context(), fake.clone(), guard.clone());
    let events = controller.subscribe();
    (controller, guard, events)
}

/// Every event so far: gapless from sequence 1, for this run.
fn drain(events: &mut broadcast::Receiver<PublicEvent>, next: &mut u64) -> Vec<String> {
    let mut kinds = Vec::new();
    while let Ok(event) = events.try_recv() {
        assert_eq!(event.sequence, *next, "gapless sequence");
        assert!(event.context == context());
        event.validate_for(&context()).unwrap();
        *next += 1;
        kinds.push(describe(&event.kind));
    }
    kinds
}

/// Event kinds have no `Debug` (they may carry private data): compare text.
fn describe(kind: &EventKind) -> String {
    match kind {
        EventKind::StatusChanged { status } => format!("status {status:?}"),
        EventKind::StepOutcome {
            step,
            dispatch_state,
            verification_status,
        } => format!("step {step} {dispatch_state:?} {verification_status:?}"),
        EventKind::InputOwnerChanged { owner } => format!("owner {owner:?}"),
        EventKind::UsageUpdated { .. } => "usage".into(),
    }
}

fn status(status: RunStatus) -> String {
    describe(&EventKind::StatusChanged { status })
}

fn outcome(step: u64, state: DispatchState) -> String {
    describe(&EventKind::StepOutcome {
        step,
        dispatch_state: state,
        verification_status: aero_core::events::VerificationStatus::NotChecked,
    })
}

#[tokio::test(start_paused = true)]
async fn controller_pause_is_acknowledged() {
    // During planning: at once, without waiting for a 90 s planner.
    let fake = Fake::new(90_000, Some(10), 0);
    let (controller, _, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(5)).await;
    let asked = Instant::now();
    controller.pause().await.unwrap();
    assert_eq!(asked.elapsed(), Duration::ZERO);
    sleep(Duration::from_secs(200)).await;
    assert_eq!(fake.log(|l| (l.plans, l.started)), (1, 0));
    let mut next = 1;
    assert_eq!(
        drain(&mut events, &mut next),
        [status(RunStatus::Running), status(RunStatus::Paused)]
    );
    assert_eq!(
        controller.snapshot().await.unwrap().status,
        Some(RunStatus::Paused)
    );

    // During input: once the sequence finishes, and nothing starts after.
    let fake = Fake::new(0, Some(200), 0);
    let (controller, _, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(50)).await;
    let asked = Instant::now();
    controller.pause().await.unwrap();
    assert_eq!(asked.elapsed(), Duration::from_millis(150));
    sleep(Duration::from_secs(10)).await;
    assert_eq!(
        fake.log(|l| (l.started, l.finished, l.abandoned)),
        (1, 1, 0)
    );
    let mut next = 1;
    assert_eq!(
        drain(&mut events, &mut next),
        [
            status(RunStatus::Running),
            outcome(1, DispatchState::Dispatched),
            status(RunStatus::Paused),
        ]
    );

    // Input that does not finish: abandoned at the grace limit, uncertain.
    let fake = Fake::new(0, None, 0);
    let (controller, _, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(50)).await;
    let asked = Instant::now();
    controller.pause().await.unwrap();
    assert_eq!(asked.elapsed(), INPUT_GRACE);
    sleep(Duration::from_secs(10)).await;
    assert_eq!(
        fake.log(|l| (l.started, l.finished, l.abandoned)),
        (1, 0, 1)
    );
    let mut next = 1;
    assert_eq!(
        drain(&mut events, &mut next),
        [
            status(RunStatus::Running),
            outcome(1, DispatchState::Uncertain),
            status(RunStatus::Paused),
        ]
    );

    // Answers are only taken while the run awaits the user.
    assert_eq!(
        controller.reply("late".into()).await,
        Err(ControlError::InvalidState(RunStatus::Paused))
    );
    // Pausing an already paused run is refused.
    assert_eq!(
        controller.pause().await,
        Err(ControlError::InvalidState(RunStatus::Paused))
    );
}

#[tokio::test(start_paused = true)]
async fn controller_cancel_interrupts_wait() {
    // A read-only verification wait.
    let fake = Fake::new(0, Some(0), 60_000);
    let (controller, _, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(10)).await;
    assert_eq!(fake.log(|l| (l.started, l.verifies)), (1, 1));
    let asked = Instant::now();
    controller.cancel().await.unwrap();
    assert_eq!(asked.elapsed(), Duration::ZERO);
    sleep(Duration::from_secs(120)).await;
    assert_eq!(fake.log(|l| (l.plans, l.started)), (1, 1));
    let mut next = 1;
    assert_eq!(
        drain(&mut events, &mut next),
        [
            status(RunStatus::Running),
            outcome(1, DispatchState::Dispatched),
            status(RunStatus::Cancelled),
        ]
    );
    // A cancelled run is over.
    for result in [
        controller.start().await,
        controller.resume().await,
        controller.pause().await,
        controller.cancel().await,
        controller.takeover().await.map(drop),
    ] {
        assert_eq!(result, Err(ControlError::Ended));
    }

    // Planning is interrupted too, and an in-flight input gets its grace.
    let fake = Fake::new(90_000, Some(10), 0);
    let (controller, _, _) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(5)).await;
    let asked = Instant::now();
    controller.cancel().await.unwrap();
    assert_eq!(asked.elapsed(), Duration::ZERO);
    let fake = Fake::new(0, None, 0);
    let (controller, _, _) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(5)).await;
    let asked = Instant::now();
    controller.cancel().await.unwrap();
    assert_eq!(asked.elapsed(), INPUT_GRACE);
    assert_eq!(fake.log(|l| l.abandoned), 1);
}

#[tokio::test(start_paused = true)]
async fn controller_double_start_is_rejected() {
    let fake = Fake::new(1_000, Some(10), 0);
    let (controller, _, _) = spawn(&fake);
    for result in [
        controller.pause().await,
        controller.resume().await,
        controller.cancel().await,
        controller.takeover().await.map(drop),
    ] {
        assert_eq!(result, Err(ControlError::NotStarted));
    }
    controller.start().await.unwrap();
    assert_eq!(controller.start().await, Err(ControlError::AlreadyStarted));
    // A second handle is the same controller.
    assert_eq!(
        controller.clone().start().await,
        Err(ControlError::AlreadyStarted)
    );
    assert_eq!(
        controller.reply("x".into()).await,
        Err(ControlError::InvalidState(RunStatus::Running))
    );
    assert_eq!(
        controller.resume().await,
        Err(ControlError::InvalidState(RunStatus::Running))
    );
    sleep(Duration::from_secs(5)).await;
    assert_eq!(fake.log(|l| l.plans), 5, "one plan loop, not two");
}

#[tokio::test(start_paused = true)]
async fn controller_waits_for_the_user() {
    let fake = Fake::new(0, Some(10), 0);
    *fake.await_user.lock().unwrap() = true;
    let (controller, _, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_secs(5)).await;
    assert_eq!(
        controller.snapshot().await.unwrap().status,
        Some(RunStatus::AwaitingUser)
    );
    assert_eq!(fake.log(|l| (l.plans, l.started)), (1, 0));
    controller.approve("approval-1".into()).await.unwrap();
    sleep(Duration::from_millis(15)).await;
    assert_eq!(
        fake.log(|l| l.answers.clone()),
        [UserInput::Approved("approval-1".into())]
    );
    assert!(fake.log(|l| l.started) >= 1);
    let mut next = 1;
    assert_eq!(
        drain(&mut events, &mut next)[..3],
        [
            status(RunStatus::Running),
            status(RunStatus::AwaitingUser),
            status(RunStatus::Running),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn controller_takeover_has_single_input_owner() {
    let fake = Fake::new(0, Some(10), 0);
    let (controller, guard, mut events) = spawn(&fake);
    controller.start().await.unwrap();
    sleep(Duration::from_millis(35)).await;
    let token = controller.takeover().await.unwrap();
    let snapshot = controller.snapshot().await.unwrap();
    assert_eq!(
        (snapshot.status, snapshot.owner),
        (Some(RunStatus::Paused), InputOwner::Human)
    );
    let dispatched = fake.log(|l| l.started);
    assert_eq!(fake.log(|l| l.started - l.finished - l.abandoned), 0);
    // While the human owns input, automation cannot run.
    sleep(Duration::from_secs(10)).await;
    assert_eq!(fake.log(|l| l.started), dispatched);
    assert_eq!(controller.resume().await, Err(ControlError::HumanOwnsInput));
    assert_eq!(
        controller.takeover().await.map(drop),
        Err(ControlError::HumanOwnsInput)
    );
    // Only this controller's token releases it.
    let other_fake = Fake::new(1_000, Some(10), 0);
    let (other, _, _) = spawn(&other_fake);
    other.start().await.unwrap();
    let foreign = other.takeover().await.unwrap();
    assert_eq!(
        controller.release(foreign).await,
        Err(ControlError::WrongToken)
    );
    controller.release(token).await.unwrap();
    let snapshot = controller.snapshot().await.unwrap();
    assert_eq!(
        (snapshot.status, snapshot.owner),
        (Some(RunStatus::Paused), InputOwner::Automation)
    );
    // Nothing is voided or forgotten until automation resumes.
    assert_eq!(
        (guard.0.lock().unwrap().len(), fake.log(|l| l.invalidations)),
        (0, 0)
    );
    controller.resume().await.unwrap();
    assert!(*guard.0.lock().unwrap() == [context()]);
    assert_eq!(fake.log(|l| l.invalidations), 1);
    sleep(Duration::from_millis(50)).await;
    assert!(fake.log(|l| l.started) > dispatched);
    // A plain pause and resume, with no manual input, voids nothing more.
    controller.pause().await.unwrap();
    controller.resume().await.unwrap();
    assert_eq!(guard.0.lock().unwrap().len(), 1);
    assert_eq!(fake.log(|l| l.invalidations), 1);
    let mut next = 1;
    let kinds = drain(&mut events, &mut next);
    let owners: Vec<_> = kinds
        .iter()
        .filter(|kind| kind.starts_with("owner"))
        .cloned()
        .collect();
    assert_eq!(owners, ["owner Human", "owner Automation"]);
}

#[tokio::test(start_paused = true)]
async fn a_failed_void_keeps_the_run_paused() {
    #[derive(Clone)]
    struct Failing;
    impl RunGuard for Failing {
        async fn void_approvals(&self, _: &RunContext) -> Result<(), StoreError> {
            Err(StoreError::WorkerClosed)
        }
    }
    let fake = Fake::new(0, Some(10), 0);
    let controller = Controller::spawn(context(), fake.clone(), Failing);
    controller.start().await.unwrap();
    let token = controller.takeover().await.unwrap();
    controller.release(token).await.unwrap();
    let dispatched = fake.log(|l| l.started);
    assert_eq!(
        controller.resume().await,
        Err(ControlError::Store(StoreError::WorkerClosed))
    );
    sleep(Duration::from_secs(1)).await;
    assert_eq!(fake.log(|l| l.started), dispatched);
    assert_eq!(
        controller.snapshot().await.unwrap().status,
        Some(RunStatus::Paused)
    );
}

#[tokio::test(start_paused = true)]
async fn no_dispatch_starts_after_a_requested_pause() {
    assert_eq!(INPUT_GRACE, Duration::from_millis(500), "plan row E02");
    // The pause and the end of planning reach the controller together, in
    // either order: input must not start. Repeated because the controller
    // may see either first.
    for _ in 0..32 {
        let mut fake = Fake::new(0, Some(10), 0);
        let gate = Arc::new(tokio::sync::Notify::new());
        fake.gate = Some(gate.clone());
        let (controller, _, _) = spawn(&fake);
        controller.start().await.unwrap();
        sleep(Duration::from_millis(1)).await;
        assert_eq!(fake.log(|l| l.plans), 1);
        let (paused, ()) = tokio::join!(controller.pause(), async { gate.notify_one() });
        paused.unwrap();
        sleep(Duration::from_secs(1)).await;
        assert_eq!(fake.log(|l| l.started), 0);
    }
}
