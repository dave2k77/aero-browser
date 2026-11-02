//! N13 gateway retirement wired to the owner's journal run lifecycle.
//! Separate test binary: the data_flow suite spawns child processes, which can
//! briefly inherit journal lock descriptors.

use aero_agent::config::StateConfig;
use aero_agent::data_flow::{
    Classification, DispatchError, GatewayHost, Grant, Payload, PayloadKind, Purpose, Recipient,
    RecipientKind, Transport, TransportError,
};
use aero_agent::store::StoreError;
use aero_agent::store::worker::Journal;
use aero_agent::tasks::TaskAuthority;
use aero_core::checkpoint::RunContext;
use aero_core::ids::{RunId, TaskId};
use aero_core::task::{ProcessingMode, StartPrerequisites, TaskContract};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Default)]
struct Recorder(Mutex<usize>);
impl Transport for Recorder {
    fn send(&self, _: &[u8]) -> Result<(), TransportError> {
        *self.0.lock().unwrap() += 1;
        Ok(())
    }
}
fn sent(r: &Recorder) -> usize {
    *r.0.lock().unwrap()
}

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
    contract
}
/// Run IDs are journal-wide unique, so fixtures prefix them with the task.
fn run(task: &str, run: &str, revision: u64) -> RunContext {
    RunContext {
        task_id: TaskId::from(task),
        run_id: RunId::from(format!("{task}-{run}")),
        contract_revision: revision,
    }
}
fn prerequisites() -> StartPrerequisites {
    StartPrerequisites {
        local_processor_available: true,
        background_service_available: false,
    }
}

/// A gateway for `context` with one recipient, payload and inference grant.
fn gateway(context: RunContext) -> (GatewayHost, Arc<Recorder>) {
    let host = GatewayHost::new(context.clone(), ProcessingMode::ApprovedCloud).unwrap();
    let recorder = Arc::new(Recorder::default());
    host.install_recipient(
        Recipient {
            id: "recipient".into(),
            kind: RecipientKind::Remote,
        },
        recorder.clone(),
    )
    .unwrap();
    host.install_payload(Payload {
        id: "payload".into(),
        source: "source".into(),
        classification: Some(Classification::Public),
        secret: false,
        kind: PayloadKind::Text,
        encoded_bytes: b"bytes".to_vec(),
    })
    .unwrap();
    host.install_grant(Grant {
        id: "grant".into(),
        context,
        source: "source".into(),
        classification: Classification::Public,
        payload_id: "payload".into(),
        recipient_id: "recipient".into(),
        purpose: Purpose::Inference,
        payload_kind: PayloadKind::Text,
        encoded_size: 5,
        expires_at: SystemTime::now() + Duration::from_secs(60),
    })
    .unwrap();
    (host, recorder)
}

struct Fixture {
    journal: Journal,
    _home: tempfile::TempDir,
}
fn fixture(tasks: &[&str]) -> (Fixture, TaskAuthority) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = Journal::start(state.journal_directory(), 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    let owner = authority.trusted_user();
    for task in tasks {
        owner
            .create_task(contract(task, 1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        owner
            .start_run(run(task, "run-1", 1), prerequisites())
            .unwrap()
            .blocking_recv()
            .unwrap();
    }
    (
        Fixture {
            journal,
            _home: home,
        },
        authority,
    )
}

#[test]
fn starting_a_new_run_retires_the_old_runs_gateways_before_commit() {
    let (f, authority) = fixture(&["task", "other"]);
    let owner = authority.trusted_user();
    let (old, old_sent) = gateway(run("task", "run-1", 1));
    let (other, other_sent) = gateway(run("other", "run-1", 1));
    authority.register_gateway(&old).unwrap();
    authority.register_gateway(&other).unwrap();
    let handle = old.dispatch_handle();
    handle.dispatch("payload", "grant").unwrap();
    assert_eq!(sent(&old_sent), 1);

    let reply = owner
        .start_run(run("task", "run-2", 1), prerequisites())
        .unwrap();
    // Retired synchronously when the request was admitted, before its commit
    // was awaited: the cancelled run can no longer transfer anything.
    assert_eq!(
        handle.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
    reply.blocking_recv().unwrap();
    assert_eq!(sent(&old_sent), 1);
    // Another task's gateway is unaffected; the new run needs a new gateway.
    other
        .dispatch_handle()
        .dispatch("payload", "grant")
        .unwrap();
    assert_eq!(sent(&other_sent), 1);
    let (new, new_sent) = gateway(run("task", "run-2", 1));
    authority.register_gateway(&new).unwrap();
    new.dispatch_handle().dispatch("payload", "grant").unwrap();
    assert_eq!(sent(&new_sent), 1);
    f.journal.close().unwrap();
}

#[test]
fn contract_revision_retires_the_tasks_gateways() {
    let (f, authority) = fixture(&["task"]);
    let (host, recorder) = gateway(run("task", "run-1", 1));
    authority.register_gateway(&host).unwrap();
    let handle = host.dispatch_handle();
    authority
        .trusted_user()
        .revise_contract(contract("task", 2))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        handle.dispatch("payload", "grant"),
        Err(DispatchError::Denied)
    );
    assert_eq!(sent(&recorder), 0);
    f.journal.close().unwrap();
}

#[test]
fn model_clients_cannot_retire_and_dead_gateways_are_pruned() {
    let (f, authority) = fixture(&["task"]);
    let (host, recorder) = gateway(run("task", "run-1", 1));
    authority.register_gateway(&host).unwrap();
    let model = authority.model_for_task(TaskId::from("task")).unwrap();
    assert!(matches!(
        model.start_run(run("task", "run-2", 1), prerequisites()),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.revise_contract(contract("task", 2)),
        Err(StoreError::PermissionDenied)
    ));
    // Denied model requests retire nothing.
    host.dispatch_handle().dispatch("payload", "grant").unwrap();
    assert_eq!(sent(&recorder), 1);
    // Registration is bounded, but dropped gateways do not count against it.
    drop(host);
    for _ in 0..100 {
        let (temporary, _) = gateway(run("task", "run-1", 1));
        authority.register_gateway(&temporary).unwrap();
    }
    let live: Vec<_> = (0..64)
        .map(|_| gateway(run("task", "run-1", 1)).0)
        .collect();
    for host in &live {
        authority.register_gateway(host).unwrap();
    }
    let (extra, _) = gateway(run("task", "run-1", 1));
    assert_eq!(
        authority.register_gateway(&extra).err(),
        Some(StoreError::BudgetExceeded)
    );
    f.journal.close().unwrap();
}
