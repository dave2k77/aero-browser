//! N05 evidence-backed completion gate (CH-6). Separate test binary: other
//! suites spawn child processes that can inherit journal lock descriptors.

use aero_agent::artifacts::{ArtifactPersistence, ArtifactStore};
use aero_agent::config::{PrivateState, StateConfig, TaskLease};
use aero_agent::evidence::{EvidenceStore, FactCapture};
use aero_agent::store::StoreError;
use aero_agent::store::worker::{
    CommitStep, Journal, PrepareAction, PreparedAction, Resources, StepOutcome,
};
use aero_agent::tasks::TaskAuthority;
use aero_agent::verification::{CompletionReport, OutputBinding};
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::{RunContext, RunStatus};
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ArtifactId, DocumentEpoch, EvidenceId, ObservationId, RunId, TaskId};
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
    /// Capture `(entity, field, value)` facts and commit them with one step.
    fn capture(
        &self,
        lease: &TaskLease,
        context: &RunContext,
        facts: &[(&str, &str, &str)],
    ) -> Vec<EvidenceId> {
        let store = EvidenceStore::open(lease).unwrap();
        let staged: Vec<_> = facts
            .iter()
            .map(|(entity, field, value)| {
                store
                    .capture(
                        ArtifactPersistence::Public,
                        context,
                        FactCapture {
                            document: DocumentEpoch::from("catalog-1"),
                            origin: "https://supplier.test".into(),
                            effective_url: "https://supplier.test/catalog".into(),
                            captured_at_ms: 1,
                            entity: (*entity).into(),
                            field: (*field).into(),
                            value: (*value).into(),
                            excerpt: format!("{entity} {field}: {value}"),
                        },
                    )
                    .unwrap()
            })
            .collect();
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
                evidence: staged.iter().map(|e| e.record(context.clone())).collect(),
                artifacts: vec![],
            })
            .unwrap()
            .blocking_recv()
            .unwrap();
        staged.iter().map(|e| e.id().clone()).collect()
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
        .complete_run(&lease, context.clone(), bind(&artifact))
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
            .complete_run(&lease, context.clone(), bind(&artifact))
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
        .complete_run(&lease, context.clone(), bind(&one_row))
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
            .complete_run(&lease, context.clone(), bind(artifact))
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
        .complete_run(&lease, context.clone(), vec![])
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
                .complete_run(&lease, context.clone(), bad)
                .is_err()
        );
    }
    assert_eq!(f.status(&context), RunStatus::Paused);

    // Correct output later: re-evaluation completes the same run.
    let good = f.produce(&store, &context, GOOD_CSV);
    assert!(
        f.authority
            .complete_run(&lease, context.clone(), bind(&good))
            .unwrap()
            .completed
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn unsupported_required_criteria_never_complete() {
    // DEC-009: only machine-verified criteria complete. A report without a
    // structured claims block cannot be checked for evidence coverage.
    let f = Fixture::new(fixture_contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let csv = f.produce(&store, &context, GOOD_CSV);
    let report_md = f.produce(&store, &context, b"# Suppliers\n\nacme [1]\n");
    let report = f
        .authority
        .complete_run(
            &lease,
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
            .complete_run(&lease, context.clone(), bind(&artifact))
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
            .complete_run(&lease, context.clone(), bind(&artifact))
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
            .complete_run(&lease, newer.clone(), bind(&newer_artifact))
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
            .complete_run(&lease, current.clone(), bind(&newer_artifact))
            .err(),
        Some(StoreError::OwnershipMismatch)
    );
    assert_eq!(f.status(&current), RunStatus::Paused);
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

/// A Markdown report whose structured claims block is `claims_json`.
fn report(claims_json: &str) -> Vec<u8> {
    format!("# Suppliers\n\nAcme and Bolt prices.\n\n```aero-claims\n{claims_json}\n```\n")
        .into_bytes()
}
fn claim(entity: &str, value: &str, evidence: Option<&EvidenceId>) -> String {
    let evidence = evidence.map_or("null".to_owned(), |e| format!("\"{}\"", e.as_str()));
    format!(r#"{{"entity":"{entity}","field":"price","value":"{value}","evidence_id":{evidence}}}"#)
}
fn claims(items: &[String]) -> String {
    format!(r#"{{"schema_version":1,"claims":[{}]}}"#, items.join(","))
}
fn both(csv: &ArtifactId, report: &ArtifactId) -> Vec<OutputBinding> {
    let mut bindings = bind(csv);
    bindings.push(OutputBinding {
        output: "supplier-report".into(),
        artifact: report.clone(),
    });
    bindings
}
fn coverage(report: &CompletionReport) -> VerificationStatus {
    report
        .results
        .iter()
        .find(|r| r.criterion_id == "source-coverage")
        .unwrap()
        .status
}

#[test]
fn evidence_coverage_passes_with_matching_owned_captures() {
    let f = Fixture::new(fixture_contract("task", 1));
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let ids = f.capture(
        &lease,
        &context,
        &[("acme", "price", "10"), ("bolt", "price", "12")],
    );
    let csv = f.produce(&store, &context, GOOD_CSV);
    let md = f.produce(
        &store,
        &context,
        &report(&claims(&[
            claim("acme", "10", Some(&ids[0])),
            claim("bolt", "12", Some(&ids[1])),
        ])),
    );
    let result = f
        .authority
        .complete_run(&lease, context.clone(), both(&csv, &md))
        .unwrap();
    assert!(result.completed);
    let source = result
        .results
        .iter()
        .find(|r| r.criterion_id == "source-coverage")
        .unwrap();
    assert_eq!(source.status, VerificationStatus::Passed);
    let mut cited = source.evidence_ids.clone();
    cited.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let mut expected = ids.clone();
    expected.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert_eq!(cited, expected);
    assert_eq!(f.status(&context), RunStatus::Completed);
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn evidence_coverage_rejects_unknown_contradicted_foreign_or_malformed_claims() {
    let f = Fixture::new(fixture_contract("task", 1));
    let context = run("task", "run", 1);
    // Another task's committed capture must not count for this task.
    let owner = f.authority.trusted_user();
    owner
        .create_task(fixture_contract("other", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let other = run("other", "run", 1);
    owner
        .start_run(other.clone(), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let other_lease = f.state.acquire_task(&TaskId::from("other")).unwrap();
    let foreign = f.capture(&other_lease, &other, &[("acme", "price", "10")]);
    drop(other_lease);

    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let ids = f.capture(
        &lease,
        &context,
        &[("acme", "price", "10"), ("bolt", "price", "12")],
    );
    let csv = f.produce(&store, &context, GOOD_CSV);
    let acme = claim("acme", "10", Some(&ids[0]));
    let bolt = claim("bolt", "12", Some(&ids[1]));
    let cases: Vec<(&str, Vec<u8>, VerificationStatus)> = vec![
        (
            "contradicted value",
            report(&claims(&[claim("acme", "11", Some(&ids[0])), bolt.clone()])),
            VerificationStatus::Failed,
        ),
        (
            "unknown claim below required fraction",
            report(&claims(&[acme.clone(), claim("bolt", "12", None)])),
            VerificationStatus::Failed,
        ),
        (
            "cites another entity's capture",
            report(&claims(&[acme.clone(), claim("bolt", "12", Some(&ids[0]))])),
            VerificationStatus::Failed,
        ),
        (
            "cites another task's capture",
            report(&claims(&[
                claim("acme", "10", Some(&foreign[0])),
                bolt.clone(),
            ])),
            VerificationStatus::Failed,
        ),
        (
            "cites a nonexistent capture",
            report(&claims(&[
                acme.clone(),
                claim("bolt", "12", Some(&EvidenceId::from("e1-missing"))),
            ])),
            VerificationStatus::Failed,
        ),
        (
            "no claims block",
            b"# Suppliers\n\nAcme 10, Bolt 12.\n".to_vec(),
            VerificationStatus::Inconclusive,
        ),
        (
            "empty claims",
            report(&claims(&[])),
            VerificationStatus::Inconclusive,
        ),
        (
            "array root",
            report(&format!("[{acme}]")),
            VerificationStatus::Inconclusive,
        ),
        (
            "unknown key",
            report(&format!(
                r#"{{"schema_version":1,"claims":[{acme}],"extra":1}}"#
            )),
            VerificationStatus::Inconclusive,
        ),
        (
            "duplicate key",
            report(&format!(
                r#"{{"schema_version":1,"schema_version":1,"claims":[{acme}]}}"#
            )),
            VerificationStatus::Inconclusive,
        ),
        (
            "positional claim",
            report(r#"{"schema_version":1,"claims":[["acme","price","10",null]]}"#),
            VerificationStatus::Inconclusive,
        ),
        (
            "unsupported version",
            report(&format!(r#"{{"schema_version":2,"claims":[{acme}]}}"#)),
            VerificationStatus::Inconclusive,
        ),
        (
            "two claims blocks",
            [
                report(&claims(std::slice::from_ref(&acme))),
                report(&claims(std::slice::from_ref(&bolt))),
            ]
            .concat(),
            VerificationStatus::Inconclusive,
        ),
    ];
    for (name, bytes, expected) in cases {
        let md = f.produce(&store, &context, &bytes);
        let result = f
            .authority
            .complete_run(&lease, context.clone(), both(&csv, &md))
            .unwrap();
        assert!(!result.completed, "{name}");
        assert_eq!(coverage(&result), expected, "{name}");
    }
    assert_eq!(f.status(&context), RunStatus::Paused);
    // The run stayed open: a fully supported report completes it.
    let md = f.produce(&store, &context, &report(&claims(&[acme, bolt])));
    assert!(
        f.authority
            .complete_run(&lease, context.clone(), both(&csv, &md))
            .unwrap()
            .completed
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn coverage_fraction_counts_unknowns_but_a_contradiction_always_fails() {
    let mut c = fixture_contract("task", 1);
    for criterion in &mut c.criteria {
        if let Criterion::EvidenceCoverage {
            required_fraction, ..
        } = criterion
        {
            *required_fraction = aero_core::task::CoverageFraction::new(0.5).unwrap();
        }
    }
    let f = Fixture::new(c);
    let context = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let ids = f.capture(
        &lease,
        &context,
        &[("acme", "price", "10"), ("bolt", "price", "12")],
    );
    let csv = f.produce(&store, &context, GOOD_CSV);
    // Half matched, half contradicted: the fraction is met but it still fails.
    let md = f.produce(
        &store,
        &context,
        &report(&claims(&[
            claim("acme", "10", Some(&ids[0])),
            claim("bolt", "99", Some(&ids[1])),
        ])),
    );
    let result = f
        .authority
        .complete_run(&lease, context.clone(), both(&csv, &md))
        .unwrap();
    assert!(!result.completed);
    assert_eq!(coverage(&result), VerificationStatus::Failed);
    // Half matched, half explicitly unknown: meets 0.5 and completes.
    let md = f.produce(
        &store,
        &context,
        &report(&claims(&[
            claim("acme", "10", Some(&ids[0])),
            claim("bolt", "12", None),
        ])),
    );
    assert!(
        f.authority
            .complete_run(&lease, context.clone(), both(&csv, &md))
            .unwrap()
            .completed
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}

#[test]
fn captures_from_another_run_do_not_count() {
    let f = Fixture::new(fixture_contract("task", 1));
    let first = run("task", "run", 1);
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let old = f.capture(
        &lease,
        &first,
        &[("acme", "price", "10"), ("bolt", "price", "12")],
    );
    let current = run("task", "second", 1);
    f.authority
        .trusted_user()
        .start_run(current.clone(), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let csv = f.produce(&store, &current, GOOD_CSV);
    let stale = f.produce(
        &store,
        &current,
        &report(&claims(&[
            claim("acme", "10", Some(&old[0])),
            claim("bolt", "12", Some(&old[1])),
        ])),
    );
    let result = f
        .authority
        .complete_run(&lease, current.clone(), both(&csv, &stale))
        .unwrap();
    assert!(!result.completed);
    assert_eq!(coverage(&result), VerificationStatus::Failed);
    // Fresh captures in the current run complete it.
    let fresh = f.capture(
        &lease,
        &current,
        &[("acme", "price", "10"), ("bolt", "price", "12")],
    );
    let md = f.produce(
        &store,
        &current,
        &report(&claims(&[
            claim("acme", "10", Some(&fresh[0])),
            claim("bolt", "12", Some(&fresh[1])),
        ])),
    );
    assert!(
        f.authority
            .complete_run(&lease, current.clone(), both(&csv, &md))
            .unwrap()
            .completed
    );
    drop(store);
    drop(lease);
    f.journal.close().unwrap();
}
