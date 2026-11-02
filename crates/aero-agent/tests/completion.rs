//! N05 evidence-backed completion gate (CH-6). Separate test binary: other
//! suites spawn child processes that can inherit journal lock descriptors.

use aero_agent::artifacts::{ArtifactPersistence, ArtifactStore};
use aero_agent::config::{PrivateState, StateConfig};
use aero_agent::store::StoreError;
use aero_agent::store::worker::{
    CommitStep, Journal, PrepareAction, PreparedAction, Resources, StepOutcome,
};
use aero_agent::tasks::TaskAuthority;
use aero_agent::verification::{CompletionReport, OutputBinding};
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::{RunContext, RunStatus};
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ArtifactId, ObservationId, RunId, TaskId};
use aero_core::task::{Criterion, StartPrerequisites, TaskContract};
use std::os::unix::fs::PermissionsExt;

const GOOD_CSV: &[u8] =
    b"supplier,price,lead_time,source_url\nacme,10,3,https://a.test/\n\"bolt, inc\",12,5,https://b.test/\n";

/// The documented fixture without its evidence-coverage criterion, which is
/// not machine-checkable yet (CH-7); its report output becomes optional.
fn contract(task: &str, revision: u64) -> TaskContract {
    let mut contract = fixture_contract(task, revision);
    contract
        .criteria
        .retain(|c| !matches!(c, Criterion::EvidenceCoverage { .. }));
    for output in &mut contract.outputs {
        if output.name == "supplier-report" {
            output.required = false;
        }
    }
    contract
}
fn fixture_contract(task: &str, revision: u64) -> TaskContract {
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
fn proposal(context: &RunContext) -> PrepareAction {
    PrepareAction {
        context: context.clone(),
        persistence: aero_agent::store::worker::Persistence::Redacted,
        decision: Decision {
            schema_version: 1,
            observation_id: ObservationId::from("observation"),
            rationale: "synthetic".into(),
            action: Action::Wait { milliseconds: 1 },
        },
        reserve: Resources {
            input_tokens: 10,
            output_tokens: 5,
            active_ms: 100,
        },
    }
}

struct Fixture {
    journal: Journal,
    authority: TaskAuthority,
    state: PrivateState,
    _home: tempfile::TempDir,
}
impl Fixture {
    fn new(contract: TaskContract) -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = StateConfig::resolve(Some(home.path()), None)
            .unwrap()
            .open()
            .unwrap();
        let journal = Journal::start(state.journal_directory(), 8).unwrap();
        let authority = TaskAuthority::new(&journal);
        let owner = authority.trusted_user();
        let task = contract.task_id.as_str().to_owned();
        owner
            .create_task(contract)
            .unwrap()
            .blocking_recv()
            .unwrap();
        owner
            .start_run(run(&task, "run", 1), prerequisites())
            .unwrap()
            .blocking_recv()
            .unwrap();
        Self {
            journal,
            authority,
            state,
            _home: home,
        }
    }
    fn prepare(&self, context: &RunContext) -> PreparedAction {
        self.journal
            .client()
            .prepare_action(proposal(context))
            .unwrap()
            .blocking_recv()
            .unwrap()
    }
    /// Commit one step whose artifact is `bytes`; returns its ID.
    fn produce(&self, store: &ArtifactStore, context: &RunContext, bytes: &[u8]) -> ArtifactId {
        let staged = store.stage(ArtifactPersistence::Public, bytes).unwrap();
        self.journal
            .client()
            .commit_step(CommitStep {
                prepared: self.prepare(context),
                outcome: StepOutcome {
                    schema_version: 1,
                    dispatch: DispatchState::NotDispatched,
                    verification: VerificationStatus::Passed,
                },
                actual: Some(Resources {
                    input_tokens: 1,
                    output_tokens: 1,
                    active_ms: 1,
                }),
                evidence: vec![],
                artifacts: vec![staged.record(context.clone())],
            })
            .unwrap()
            .blocking_recv()
            .unwrap();
        staged.id().clone()
    }
    fn status(&self, context: &RunContext) -> RunStatus {
        self.journal
            .client()
            .load_resume(context.clone())
            .unwrap()
            .blocking_recv()
            .unwrap()
            .current_status
    }
}
fn bind(artifact: &ArtifactId) -> Vec<OutputBinding> {
    vec![OutputBinding {
        output: "supplier-comparison".into(),
        artifact: artifact.clone(),
    }]
}
fn statuses(report: &CompletionReport) -> Vec<(String, VerificationStatus)> {
    let mut all: Vec<_> = report
        .results
        .iter()
        .map(|r| (r.criterion_id.clone(), r.status))
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

#[test]
fn all_required_criteria_verified_completes_the_run_once() {
    let f = Fixture::new(contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let artifact = f.produce(&store, &context, GOOD_CSV);

    let report = f
        .authority
        .complete_run(&store, context.clone(), bind(&artifact))
        .unwrap();
    assert!(report.completed);
    assert_eq!(
        statuses(&report),
        vec![
            ("csv-schema".to_owned(), VerificationStatus::Passed),
            ("two-suppliers".to_owned(), VerificationStatus::Passed),
        ]
    );
    assert_eq!(f.status(&context), RunStatus::Completed);

    // A completed run/task is final: no new run, revision, step or re-check.
    let owner = f.authority.trusted_user();
    assert_eq!(
        owner
            .start_run(run("task", "again", 1), prerequisites())
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::Conflict)
    );
    assert_eq!(
        owner
            .revise_contract(contract("task", 2))
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::Conflict)
    );
    assert!(matches!(
        f.journal
            .client()
            .prepare_action(proposal(&context))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        f.authority
            .complete_run(&store, context.clone(), bind(&artifact))
            .err(),
        Some(StoreError::Conflict)
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();

    // Durable results; completed runs, tasks and their results are immutable.
    let sql =
        rusqlite::Connection::open(f.state.journal_directory().join("journal.sqlite3")).unwrap();
    let passed: i64 = sql
        .query_row(
            "SELECT count(*) FROM criterion_results WHERE status='passed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(passed, 2);
    for statement in [
        "UPDATE sessions SET status='paused'",
        "UPDATE tasks SET status='paused'",
        "UPDATE criterion_results SET status='failed'",
        "DELETE FROM criterion_results",
    ] {
        assert!(sql.execute(statement, []).is_err(), "{statement}");
    }
}

#[test]
fn failed_or_unverifiable_criteria_leave_the_run_open() {
    let f = Fixture::new(contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();

    // Wrong row count fails; the run stays open for more work.
    let one_row = f.produce(
        &store,
        &context,
        b"supplier,price,lead_time,source_url\nacme,10,3,https://a.test/\n",
    );
    let report = f
        .authority
        .complete_run(&store, context.clone(), bind(&one_row))
        .unwrap();
    assert!(!report.completed);
    assert_eq!(
        statuses(&report),
        vec![
            ("csv-schema".to_owned(), VerificationStatus::Passed),
            ("two-suppliers".to_owned(), VerificationStatus::Failed),
        ]
    );
    assert_eq!(f.status(&context), RunStatus::Paused);

    // Wrong header fails; malformed CSV and modified bytes are inconclusive.
    let renamed = f.produce(
        &store,
        &context,
        b"supplier,cost,lead_time,source_url\na,1,1,x\nb,2,2,y\n",
    );
    let ragged = f.produce(
        &store,
        &context,
        b"supplier,price,lead_time,source_url\na,1,1\nb,2,2,y\n",
    );
    let tampered = f.produce(&store, &context, GOOD_CSV);
    let path = lease.directory().join("artifacts").join(tampered.as_str());
    std::fs::write(&path, GOOD_CSV.to_ascii_uppercase()).unwrap();
    for (artifact, expected) in [
        (&renamed, VerificationStatus::Failed),
        (&ragged, VerificationStatus::Inconclusive),
        (&tampered, VerificationStatus::Inconclusive),
    ] {
        let report = f
            .authority
            .complete_run(&store, context.clone(), bind(artifact))
            .unwrap();
        assert!(!report.completed);
        assert_eq!(
            report
                .results
                .iter()
                .find(|r| r.criterion_id == "csv-schema")
                .unwrap()
                .status,
            expected
        );
    }
    // No binding for the required output: unavailable, not complete.
    let report = f
        .authority
        .complete_run(&store, context.clone(), vec![])
        .unwrap();
    assert!(!report.completed);
    assert!(
        report
            .results
            .iter()
            .all(|r| r.status == VerificationStatus::Inconclusive)
    );
    // Bindings must name contract outputs once, with this run's artifacts.
    let mut duplicate = bind(&one_row);
    duplicate.extend(bind(&one_row));
    for bad in [
        vec![OutputBinding {
            output: "unknown-output".into(),
            artifact: one_row.clone(),
        }],
        duplicate,
        bind(&ArtifactId::from("not-an-artifact")),
    ] {
        assert!(
            f.authority
                .complete_run(&store, context.clone(), bad)
                .is_err()
        );
    }
    assert_eq!(f.status(&context), RunStatus::Paused);

    // Correct output later: re-evaluation completes the same run.
    let good = f.produce(&store, &context, GOOD_CSV);
    assert!(
        f.authority
            .complete_run(&store, context.clone(), bind(&good))
            .unwrap()
            .completed
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn unsupported_required_criteria_never_complete() {
    // DEC-009: only machine-verified criteria complete. Evidence coverage is
    // not machine-checkable yet, so the full documented fixture cannot pass.
    let f = Fixture::new(fixture_contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let csv = f.produce(&store, &context, GOOD_CSV);
    let report_md = f.produce(&store, &context, b"# Suppliers\n\nacme [1]\n");
    let report = f
        .authority
        .complete_run(
            &store,
            context.clone(),
            vec![
                OutputBinding {
                    output: "supplier-comparison".into(),
                    artifact: csv,
                },
                OutputBinding {
                    output: "supplier-report".into(),
                    artifact: report_md,
                },
            ],
        )
        .unwrap();
    assert!(!report.completed);
    assert_eq!(
        statuses(&report),
        vec![
            ("csv-schema".to_owned(), VerificationStatus::Passed),
            (
                "source-coverage".to_owned(),
                VerificationStatus::Inconclusive
            ),
            ("two-suppliers".to_owned(), VerificationStatus::Passed),
        ]
    );
    assert_eq!(f.status(&context), RunStatus::Paused);
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn unresolved_work_or_a_superseded_run_or_revision_blocks_completion() {
    let f = Fixture::new(contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let artifact = f.produce(&store, &context, GOOD_CSV);

    // An unresolved prepared action anywhere in the task blocks completion.
    let pending = f.prepare(&context);
    assert!(
        f.authority
            .complete_run(&store, context.clone(), bind(&artifact))
            .is_err()
    );
    f.journal
        .client()
        .commit_step(CommitStep {
            prepared: pending,
            outcome: StepOutcome {
                schema_version: 1,
                dispatch: DispatchState::NotDispatched,
                verification: VerificationStatus::Passed,
            },
            actual: Some(Resources {
                input_tokens: 1,
                output_tokens: 1,
                active_ms: 1,
            }),
            evidence: vec![],
            artifacts: vec![],
        })
        .unwrap()
        .blocking_recv()
        .unwrap();

    // A superseded run cannot be completed.
    let owner = f.authority.trusted_user();
    owner
        .start_run(run("task", "newer", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        f.authority
            .complete_run(&store, context.clone(), bind(&artifact))
            .err(),
        Some(StoreError::Conflict)
    );
    // Nor can a run of a superseded revision.
    let newer = run("task", "newer", 1);
    let newer_artifact = f.produce(&store, &newer, GOOD_CSV);
    owner
        .revise_contract(contract("task", 2))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        f.authority
            .complete_run(&store, newer.clone(), bind(&newer_artifact))
            .err(),
        Some(StoreError::Conflict)
    );
    // An artifact from another run cannot satisfy this run's outputs.
    let current = run("task", "current", 2);
    owner
        .start_run(current.clone(), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        f.authority
            .complete_run(&store, current.clone(), bind(&newer_artifact))
            .err(),
        Some(StoreError::OwnershipMismatch)
    );
    assert_eq!(f.status(&current), RunStatus::Paused);
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}
