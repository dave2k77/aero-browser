//! N08: one role-scoped application interface (REQ-SERVICE-01).

use aero_agent::approval::{ClearedBy, Gate, Refusal, Resolved};
use aero_agent::capabilities::NetworkAccess;
use aero_agent::controller::{Automation, ControlError, Plan, UserInput};
use aero_agent::preview::{Amount, CommitTarget, PREVIEW_SCHEMA_VERSION, Preview};
use aero_agent::service::{AutomationFactory, LocalService, ServiceError, Session, StreamItem};
use aero_agent::store::worker::Journal;
use aero_agent::tasks::TaskAuthority;
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::{RunContext, RunStatus};
use aero_core::events::{DispatchState, EventKind, PublicEvent};
use aero_core::ids::{DocumentEpoch, EvidenceId, FrameId, ObservationId, RunId, TaskId};
use aero_core::task::{Effect, StartPrerequisites, TaskContract};
use std::collections::BTreeMap;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const TASK: &str = "service-task";
const OTHER: &str = "other-task";

fn contract(task: &str, revision: u64) -> TaskContract {
    let plan = include_str!("../../../docs/implementation-plan.md");
    let json = plan
        .split("### 7.10 TaskContract")
        .nth(1)
        .unwrap()
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let mut contract: TaskContract = serde_json::from_str(json).unwrap();
    contract.task_id = TaskId::from(task);
    contract.revision = revision;
    contract.allowed_effects = vec![Effect::Navigate, Effect::Extract, Effect::Submit];
    contract.forbidden_effects = vec![Effect::Purchase, Effect::Upload];
    contract
}

fn context(task: &str, run: &str) -> RunContext {
    RunContext {
        task_id: TaskId::from(task),
        run_id: RunId::from(run),
        contract_revision: 1,
    }
}

fn prerequisites() -> StartPrerequisites {
    StartPrerequisites {
        local_processor_available: true,
        background_service_available: false,
    }
}

fn preview(run: &str) -> Preview {
    Preview {
        schema_version: PREVIEW_SCHEMA_VERSION,
        task_id: TaskId::from(TASK),
        run_id: RunId::from(run),
        contract_revision: 1,
        effect: Some(Effect::Submit),
        target: CommitTarget {
            frame: FrameId::from("frame"),
            document: DocumentEpoch::from("loader"),
            backend_node_id: 7,
        },
        origin: "https://supplier-a.test".into(),
        account_ref: None,
        recipients: vec![],
        fields: BTreeMap::new(),
        artifacts: vec![],
        amount: Amount::NotApplicable,
        observation_id: ObservationId::from("obs"),
        summary: "Send the enquiry".into(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Plans forever (so events are control changes only), or first waits for
/// the user once if asked to; counts invalidations.
struct Idle(Arc<AtomicUsize>, Arc<AtomicBool>);
impl Automation for Idle {
    type Step = ();
    fn plan(&mut self) -> impl Future<Output = Plan<()>> + Send {
        let ask = self.1.swap(false, Ordering::SeqCst);
        async move {
            if ask {
                Plan::AwaitUser
            } else {
                std::future::pending().await
            }
        }
    }
    async fn dispatch(&mut self, (): ()) -> DispatchState {
        DispatchState::NotDispatched
    }
    async fn verify(&mut self) {}
    fn answer(&mut self, _: UserInput) {}
    fn invalidate_observations(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone, Default)]
struct Factory(Arc<AtomicUsize>, Arc<AtomicBool>);
impl AutomationFactory for Factory {
    type Automation = Idle;
    fn automation(&self, _: &RunContext) -> Idle {
        Idle(self.0.clone(), self.1.clone())
    }
}

struct Setup {
    _parent: tempfile::TempDir,
    journal: Journal,
    service: LocalService<Factory>,
    factory: Factory,
}

impl Setup {
    async fn new() -> Self {
        let parent = tempfile::tempdir().unwrap();
        std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let journal = Journal::start(&parent.path().join("journal"), 16).unwrap();
        let factory = Factory::default();
        let service = LocalService::new(&journal, factory.clone(), NetworkAccess::PublicOnly);
        let setup = Self {
            _parent: parent,
            journal,
            service,
            factory,
        };
        let trusted = setup.trusted();
        for task in [TASK, OTHER] {
            trusted.create_task(contract(task, 1)).await.unwrap();
        }
        trusted
            .start_run(context(TASK, "run-1"), prerequisites())
            .await
            .unwrap();
        setup
    }
    fn trusted(&self) -> Session<Factory> {
        self.service.trusted()
    }
    fn model(&self, task: &str) -> Session<Factory> {
        self.service.model(TaskId::from(task)).unwrap()
    }
}

fn run(id: &str) -> RunId {
    RunId::from(id)
}

/// Event kinds have no `Debug`: compare text.
fn describe(event: &PublicEvent) -> String {
    let kind = match &event.kind {
        EventKind::StatusChanged { status } => format!("status {status:?}"),
        EventKind::InputOwnerChanged { owner } => format!("owner {owner:?}"),
        EventKind::StepOutcome {
            step,
            dispatch_state,
            ..
        } => format!("step {step} {dispatch_state:?}"),
        EventKind::UsageUpdated { .. } => "usage".into(),
    };
    format!("{} {}", event.sequence, kind)
}

async fn next_event(stream: &mut aero_agent::service::EventStream) -> String {
    match stream.next().await {
        StreamItem::Event(event) => describe(&event),
        StreamItem::Gap(snapshot) => format!("gap {snapshot:?}"),
        StreamItem::Closed => "closed".into(),
    }
}

#[tokio::test]
async fn trusted_clients_share_one_run_and_event_sequence() {
    let setup = Setup::new().await;
    let (cli, gui) = (setup.trusted(), setup.trusted());
    let mut cli_events = cli.subscribe(&run("run-1")).unwrap();
    let mut gui_events = gui.subscribe(&run("run-1")).unwrap();
    cli.pause(&run("run-1")).await.unwrap();
    // The other client sees the same state and acts on the same run.
    assert_eq!(
        gui.snapshot(&run("run-1")).await.unwrap().status,
        Some(RunStatus::Paused)
    );
    gui.resume(&run("run-1")).await.unwrap();
    let token = cli.takeover(&run("run-1")).await.unwrap();
    gui.release(&run("run-1"), token).await.unwrap();
    gui.cancel(&run("run-1")).await.unwrap();
    let mut seen = [Vec::new(), Vec::new()];
    for (events, seen) in [&mut cli_events, &mut gui_events]
        .into_iter()
        .zip(&mut seen)
    {
        for _ in 0..6 {
            seen.push(next_event(events).await);
        }
    }
    assert_eq!(
        seen[0],
        [
            "2 status Paused",
            "3 status Running",
            "4 status Paused",
            "5 owner Human",
            "6 owner Automation",
            "7 status Cancelled",
        ]
    );
    assert_eq!(seen[0], seen[1]);
}

#[tokio::test]
async fn model_role_is_scoped_to_its_task() {
    let setup = Setup::new().await;
    let model = setup.model(TASK);
    let r1 = run("run-1");
    // Trusted-only operations.
    assert_eq!(
        model.create_task(contract("new", 1)).await,
        Err(ServiceError::PermissionDenied)
    );
    assert_eq!(
        model.revise_contract(contract(TASK, 2)).await,
        Err(ServiceError::PermissionDenied)
    );
    assert_eq!(
        model
            .start_run(context(TASK, "run-2"), prerequisites())
            .await,
        Err(ServiceError::PermissionDenied)
    );
    assert_eq!(model.resume(&r1).await, Err(ServiceError::PermissionDenied));
    assert_eq!(model.cancel(&r1).await, Err(ServiceError::PermissionDenied));
    assert_eq!(
        model.takeover(&r1).await.err(),
        Some(ServiceError::PermissionDenied)
    );
    assert_eq!(
        model.reply_user(&r1, "yes".into()).await,
        Err(ServiceError::PermissionDenied)
    );
    let request = model.request_approval(preview("run-1")).await.unwrap();
    assert_eq!(
        model
            .grant_approval(&request, "self-approved".into(), now_ms() + 60_000)
            .await,
        Err(ServiceError::PermissionDenied)
    );
    // Its own task: read, subscribe and pause.
    assert_eq!(
        model
            .list_artifacts(&TaskId::from(TASK))
            .await
            .map(|a| a.len()),
        Ok(0)
    );
    model.subscribe(&r1).unwrap();
    model.pause(&r1).await.unwrap();
    assert_eq!(
        setup.trusted().snapshot(&r1).await.unwrap().status,
        Some(RunStatus::Paused)
    );
    // Another task's model sees nothing of this one.
    let other = setup.model(OTHER);
    assert_eq!(other.pause(&r1).await, Err(ServiceError::NotFound));
    assert_eq!(other.snapshot(&r1).await, Err(ServiceError::NotFound));
    assert!(matches!(other.subscribe(&r1), Err(ServiceError::NotFound)));
    assert_eq!(
        other.list_artifacts(&TaskId::from(TASK)).await.err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        other
            .get_evidence(&TaskId::from(TASK), EvidenceId::from("e1"))
            .await
            .err(),
        Some(ServiceError::NotFound)
    );
    assert_eq!(
        other.request_approval(preview("run-1")).await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(other.resume(&r1).await, Err(ServiceError::PermissionDenied));
    // Unknown runs are not found for anyone.
    assert_eq!(
        setup.trusted().pause(&run("nope")).await,
        Err(ServiceError::NotFound)
    );
}

#[tokio::test]
async fn only_trusted_callers_grant_requested_approvals() {
    let setup = Setup::new().await;
    let model = setup.model(TASK);
    let trusted = setup.trusted();
    let request = model.request_approval(preview("run-1")).await.unwrap();
    // The grant approves exactly the requested preview, once.
    trusted
        .grant_approval(&request, "approval-1".into(), now_ms() + 60_000)
        .await
        .unwrap();
    assert_eq!(
        trusted
            .grant_approval(&request, "approval-2".into(), now_ms() + 60_000)
            .await,
        Err(ServiceError::NotFound)
    );
    assert_eq!(
        trusted
            .grant_approval("made-up", "approval-3".into(), now_ms() + 60_000)
            .await,
        Err(ServiceError::NotFound)
    );
    let capabilities = TaskAuthority::new(&setup.journal).capabilities(NetworkAccess::PublicOnly);
    let click = Action::Click {
        target: "@e7".into(),
    };
    let gate = capabilities
        .gate(
            &click,
            Resolved::Commit {
                approval_id: Some("approval-1".into()),
                preview: preview("run-1"),
            },
        )
        .await
        .unwrap();
    assert!(
        matches!(gate, Gate::Cleared(c) if *c.cleared_by() == ClearedBy::Approval("approval-1".into()))
    );
    // A request for a run the caller cannot see, or naming another task
    // through one of its own runs.
    let mut foreign = preview("run-1");
    foreign.run_id = RunId::from("nope");
    assert_eq!(
        model.request_approval(foreign).await,
        Err(ServiceError::NotFound)
    );
    let mut crossed = preview("run-1");
    crossed.task_id = TaskId::from(OTHER);
    assert_eq!(
        model.request_approval(crossed).await,
        Err(ServiceError::NotFound)
    );
    // A grant the journal refuses leaves the request pending for a retry.
    let request = model.request_approval(preview("run-1")).await.unwrap();
    assert_eq!(
        trusted
            .grant_approval(&request, "expired".into(), now_ms() - 1)
            .await,
        Err(ServiceError::Store(
            aero_agent::store::StoreError::InvalidInput
        ))
    );
    trusted
        .grant_approval(&request, "retried".into(), now_ms() + 60_000)
        .await
        .unwrap();
}

#[tokio::test]
async fn duplicate_start_is_rejected() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    let mut first = trusted.subscribe(&run("run-1")).unwrap();
    assert!(matches!(
        trusted
            .start_run(context(TASK, "run-1"), prerequisites())
            .await,
        Err(ServiceError::Store(_))
    ));
    // A new run supersedes the task's earlier one.
    trusted
        .start_run(context(TASK, "run-2"), prerequisites())
        .await
        .unwrap();
    assert_eq!(next_event(&mut first).await, "2 status Cancelled");
    assert_eq!(
        trusted.resume(&run("run-1")).await,
        Err(ServiceError::Control(ControlError::Ended))
    );
    assert_eq!(
        trusted.snapshot(&run("run-2")).await.unwrap().status,
        Some(RunStatus::Running)
    );
}

#[tokio::test]
async fn reconnect_after_event_gap() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    let r1 = run("run-1");
    let mut events = trusted.subscribe(&r1).unwrap();
    // More events than the buffer holds, unread.
    for _ in 0..150 {
        trusted.pause(&r1).await.unwrap();
        trusted.resume(&r1).await.unwrap();
    }
    let StreamItem::Gap(snapshot) = events.next().await else {
        panic!("expected an explicit gap");
    };
    assert_eq!(snapshot.sequence, 301);
    assert_eq!(snapshot.status, Some(RunStatus::Running));
    // The stream continues after the snapshot, without replaying old events.
    trusted.pause(&r1).await.unwrap();
    assert_eq!(next_event(&mut events).await, "302 status Paused");
}

#[tokio::test]
async fn resume_after_manual_input_reobserves_and_voids_approvals() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    let r1 = run("run-1");
    let request = trusted.request_approval(preview("run-1")).await.unwrap();
    trusted
        .grant_approval(&request, "before-takeover".into(), now_ms() + 60_000)
        .await
        .unwrap();
    let token = trusted.takeover(&r1).await.unwrap();
    trusted.release(&r1, token).await.unwrap();
    trusted.resume(&r1).await.unwrap();
    assert_eq!(setup.factory.0.load(Ordering::SeqCst), 1);
    let capabilities = TaskAuthority::new(&setup.journal).capabilities(NetworkAccess::PublicOnly);
    let gate = capabilities
        .gate(
            &Action::Click {
                target: "@e7".into(),
            },
            Resolved::Commit {
                approval_id: Some("before-takeover".into()),
                preview: preview("run-1"),
            },
        )
        .await
        .unwrap();
    assert!(matches!(gate, Gate::Refused(Refusal::Consumed)));
}

#[tokio::test]
async fn methods_without_a_backend_say_so() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    let r1 = run("run-1");
    let decision: Decision = serde_json::from_str(
        r#"{"schema_version":1,"observation_id":"obs","rationale":"r","action":{"action":"wait","milliseconds":1}}"#,
    )
    .unwrap();
    assert_eq!(trusted.observe(&r1).await, Err(ServiceError::Unsupported));
    assert_eq!(
        trusted.propose_action(&r1, decision.clone()).await,
        Err(ServiceError::Unsupported)
    );
    assert_eq!(
        trusted.get_recovery_report(&r1).await,
        Err(ServiceError::Unsupported)
    );
    // Scope is still checked first.
    let other = setup.model(OTHER);
    assert_eq!(other.observe(&r1).await, Err(ServiceError::NotFound));
    assert_eq!(
        other.propose_action(&r1, decision).await,
        Err(ServiceError::NotFound)
    );
}

#[tokio::test]
async fn revising_the_contract_pauses_the_task_runs() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    trusted
        .start_run(context(OTHER, "other-run"), prerequisites())
        .await
        .unwrap();
    trusted.revise_contract(contract(TASK, 2)).await.unwrap();
    assert_eq!(
        trusted.snapshot(&run("run-1")).await.unwrap().status,
        Some(RunStatus::Paused)
    );
    // Another task's runs are untouched.
    assert_eq!(
        trusted.snapshot(&run("other-run")).await.unwrap().status,
        Some(RunStatus::Running)
    );
}

#[tokio::test]
async fn granting_an_approval_resumes_a_waiting_run() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    setup.factory.1.store(true, Ordering::SeqCst);
    trusted
        .start_run(context(TASK, "run-2"), prerequisites())
        .await
        .unwrap();
    let r2 = run("run-2");
    while trusted.snapshot(&r2).await.unwrap().status != Some(RunStatus::AwaitingUser) {
        tokio::task::yield_now().await;
    }
    let request = trusted.request_approval(preview("run-2")).await.unwrap();
    trusted
        .grant_approval(&request, "approval-1".into(), now_ms() + 60_000)
        .await
        .unwrap();
    assert_eq!(
        trusted.snapshot(&r2).await.unwrap().status,
        Some(RunStatus::Running)
    );
}

#[tokio::test]
async fn a_gap_snapshot_is_not_followed_by_events_it_already_covers() {
    let setup = Setup::new().await;
    let trusted = setup.trusted();
    let r1 = run("run-1");
    let mut events = trusted.subscribe(&r1).unwrap();
    for _ in 0..150 {
        trusted.pause(&r1).await.unwrap();
        trusted.resume(&r1).await.unwrap();
    }
    // The pause reaches the controller just before the stream's catch-up
    // snapshot, so its event is in the snapshot and in the new subscription.
    let (paused, item) = tokio::join!(trusted.pause(&r1), events.next());
    paused.unwrap();
    let StreamItem::Gap(snapshot) = item else {
        panic!("expected an explicit gap");
    };
    assert_eq!(
        (snapshot.sequence, snapshot.status),
        (302, Some(RunStatus::Paused))
    );
    trusted.resume(&r1).await.unwrap();
    assert_eq!(next_event(&mut events).await, "303 status Running");
}
