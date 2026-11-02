//! Private failure injection and lifecycle tests. No test hooks in production API.
use super::*;
use aero_core::actions::Action;
use aero_core::ids::{ObservationId, RunId};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

fn directory() -> (tempfile::TempDir, std::path::PathBuf) {
    let parent = tempfile::tempdir().unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = parent.path().join("journal");
    (parent, path)
}
fn contract() -> TaskContract {
    let plan = include_str!("../../../../docs/implementation-plan.md");
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
    let mut c: TaskContract = serde_json::from_str(json).unwrap();
    c.task_id = TaskId::from("task");
    c
}
fn context() -> RunContext {
    RunContext {
        task_id: TaskId::from("task"),
        run_id: RunId::from("run"),
        contract_revision: 1,
    }
}
fn prepare() -> PrepareAction {
    PrepareAction {
        context: context(),
        persistence: Persistence::Redacted,
        decision: Decision {
            schema_version: 1,
            observation_id: ObservationId::from("obs"),
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
fn commit() -> CommitStep {
    CommitStep {
        prepared: PreparedAction {
            context: context(),
            step: 1,
        },
        outcome: StepOutcome {
            schema_version: 1,
            dispatch: DispatchState::Dispatched,
            verification: VerificationStatus::Passed,
        },
        actual: Some(Resources {
            input_tokens: 7,
            output_tokens: 3,
            active_ms: 80,
        }),
        evidence: vec![EvidenceRecord {
            id: EvidenceId::from(format!(
                "e1-{}-{}",
                crate::config::path_key(b"task", "task").unwrap(),
                "0".repeat(32)
            )),
            context: context(),
            sha256: "a".repeat(64),
            bytes: 1,
        }],
        artifacts: vec![ArtifactRecord {
            id: ArtifactId::from(format!(
                "a1-{}-{}",
                crate::config::path_key(b"task", "task").unwrap(),
                "0".repeat(32)
            )),
            context: context(),
            sha256: "b".repeat(64),
            bytes: 10,
        }],
    }
}
fn seeded(path: &Path) -> Store {
    let mut store = Store::open(path).unwrap();
    store.create_task(contract()).unwrap();
    store
        .create_run(
            context(),
            StartPrerequisites {
                local_processor_available: true,
                background_service_available: false,
            },
        )
        .unwrap();
    store
}

#[test]
fn settled_work_counter_corruption_cannot_restore_budget() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    let ticket = store
        .reserve_budget(BudgetRequest {
            context: context(),
            id: "work".into(),
            reserve: prepare().reserve,
            pricing: crate::budget::Pricing::Unknown,
        })
        .unwrap();
    store.settle_budget(ticket, commit().actual).unwrap();
    store
        .connection
        .execute(
            "UPDATE tasks SET input_tokens_spent=0, tokens_spent=output_tokens_spent WHERE id=?1",
            [context().task_id.as_str()],
        )
        .unwrap();
    assert!(matches!(
        read_usage(&store.connection, &context().task_id),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn settled_work_receipts_are_immutable_and_reconciled_on_read() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    let ticket = store
        .reserve_budget(BudgetRequest {
            context: context(),
            id: "work".into(),
            reserve: prepare().reserve,
            pricing: crate::budget::Pricing::Unknown,
        })
        .unwrap();
    store
        .settle_budget(ticket.clone(), commit().actual)
        .unwrap();
    let mutation =
        "UPDATE work_reservations SET actual_json=json_set(actual_json,'$.input_tokens',1)";
    assert!(store.connection.execute_batch(mutation).is_err());
    store
        .connection
        .execute_batch("DROP TRIGGER immutable_work_settlement")
        .unwrap();
    store.connection.execute_batch(mutation).unwrap();
    assert!(matches!(
        store.budget_receipt(ticket),
        Err(StoreError::CorruptDatabase)
    ));
    assert!(matches!(
        store.budget_history(context(), None, 10),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn work_history_and_overrun_reports_cannot_be_erased() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    let request = BudgetRequest {
        context: context(),
        id: "work".into(),
        reserve: Resources {
            input_tokens: 0,
            output_tokens: 0,
            active_ms: 1,
        },
        pricing: crate::budget::Pricing::NoInference,
    };
    let ticket = store.reserve_budget(request).unwrap();
    store
        .settle_budget(
            ticket,
            Some(Resources {
                input_tokens: 0,
                output_tokens: 0,
                active_ms: 0,
            }),
        )
        .unwrap();
    assert!(
        store
            .connection
            .execute("DELETE FROM work_reservations", [])
            .is_err()
    );

    let ticket = store
        .reserve_budget(BudgetRequest {
            context: context(),
            id: "overrun".into(),
            reserve: Resources {
                input_tokens: 0,
                output_tokens: 0,
                active_ms: 1,
            },
            pricing: crate::budget::Pricing::NoInference,
        })
        .unwrap();
    assert_eq!(
        store.settle_budget(
            ticket,
            Some(Resources {
                input_tokens: 1,
                output_tokens: 0,
                active_ms: 0
            })
        ),
        Err(StoreError::BudgetExceeded)
    );
    assert!(
        store
            .connection
            .execute(
                "UPDATE work_reservations SET overrun_json=NULL WHERE id='overrun'",
                []
            )
            .is_err()
    );
}

#[test]
fn settled_action_usage_and_receipt_corruption_fail_closed() {
    for mutation in [
        "UPDATE tasks SET input_tokens_spent=0,tokens_spent=output_tokens_spent",
        "UPDATE tasks SET output_tokens_spent=0,tokens_spent=input_tokens_spent",
        "UPDATE tasks SET active_ms_spent=0",
        "UPDATE tasks SET unknown_inference_spent=0",
        "UPDATE tasks SET attempts_spent=0",
        "UPDATE budget_reservations SET settlement_json=json_set(settlement_json,'$.schema_version',2)",
        "UPDATE budget_reservations SET settlement_json=json_set(settlement_json,'$.actual.input_tokens',8)",
        "UPDATE budget_reservations SET settlement_json=json_set(settlement_json,'$.actual',json('[7,3,80]'))",
        "UPDATE budget_reservations SET settlement_json=json_set(settlement_json,'$.purchase_actual',1)",
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        store.prepare_action(prepare()).unwrap();
        store.commit_step(commit()).unwrap();
        store
            .connection
            .execute_batch("DROP TRIGGER immutable_action_settlement")
            .unwrap();
        store.connection.execute_batch(mutation).unwrap();
        assert!(
            matches!(
                read_usage(&store.connection, &context().task_id),
                Err(StoreError::CorruptDatabase)
            ),
            "{mutation}"
        );
    }
}

#[test]
fn action_reservation_cannot_be_replaced_when_recursive_triggers_are_disabled() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    let mut report = commit();
    report.actual.as_mut().unwrap().input_tokens = 11;
    assert_eq!(store.commit_step(report), Err(StoreError::BudgetExceeded));
    store
        .connection
        .pragma_update(None, "recursive_triggers", false)
        .unwrap();
    assert!(store.connection.execute("INSERT OR REPLACE INTO budget_reservations(id,task_id,session_id,reservation_json,state,created_at) VALUES ('run:1','task','run','{}','reserved','now')", []).is_err());
    store
        .connection
        .pragma_update(None, "recursive_triggers", true)
        .unwrap();
    assert!(
        read_action(&store.connection, &context(), 1)
            .unwrap()
            .reported_overrun
            .is_some()
    );
}

#[test]
fn invalid_outcome_cannot_cancel_an_overrun_quarantine() {
    for invalid_version in [true, false] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        store.prepare_action(prepare()).unwrap();
        let mut invalid = commit();
        invalid.outcome.schema_version = 2;
        assert_eq!(store.commit_step(invalid), Err(StoreError::InvalidInput));
        assert_eq!(
            read_action(&store.connection, &context(), 1).unwrap().state,
            ActionState::Prepared
        );
        let mut report = commit();
        report.actual.as_mut().unwrap().input_tokens = 11;
        if invalid_version {
            report.outcome.schema_version = 2;
        } else {
            report.outcome.dispatch = DispatchState::Uncertain;
            report.outcome.verification = VerificationStatus::Passed;
        }
        assert_eq!(store.commit_step(report), Err(StoreError::BudgetExceeded));
        let audit = read_action(&store.connection, &context(), 1).unwrap();
        assert_eq!(audit.state, ActionState::Uncertain);
        assert!(audit.outcome.is_none());
        assert!(audit.reported_overrun.is_some());
        assert_eq!(store.commit_step(commit()), Err(StoreError::Conflict));
    }
}

#[test]
fn malformed_attachment_cannot_cancel_an_overrun_quarantine() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    let mut report = commit();
    report.actual.as_mut().unwrap().input_tokens = 11;
    report.evidence[0].sha256 = "invalid".into();
    assert_eq!(store.commit_step(report), Err(StoreError::BudgetExceeded));
    assert_eq!(
        read_action(&store.connection, &context(), 1).unwrap().state,
        ActionState::Uncertain
    );
    assert!(store.load_resume(context()).unwrap().needs_review);
}

#[test]
fn overrun_quarantine_does_not_depend_on_attachment_insertion() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    store.commit_step(commit()).unwrap();
    store.prepare_action(prepare()).unwrap();
    let mut overrun = commit();
    overrun.prepared.step = 2;
    overrun.actual.as_mut().unwrap().input_tokens = 11;
    // Same evidence/artifact IDs as step 1 would violate SQL uniqueness.
    assert_eq!(store.commit_step(overrun), Err(StoreError::BudgetExceeded));
    assert!(store.connection.execute("UPDATE budget_reservations SET overrun_json=NULL WHERE session_id='run' AND state='uncertain'", []).is_err());
    assert_eq!(
        read_action(&store.connection, &context(), 2).unwrap().state,
        ActionState::Uncertain
    );
    assert!(store.load_resume(context()).unwrap().needs_review);
}

#[test]
fn action_overrun_write_failure_rolls_back_entire_review_transition() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    store.connection.execute_batch("CREATE TEMP TRIGGER fail_overrun BEFORE UPDATE ON budget_reservations BEGIN SELECT RAISE(ABORT,'synthetic'); END").unwrap();
    let mut reported = commit();
    reported.actual.as_mut().unwrap().input_tokens = 11;
    assert_eq!(store.commit_step(reported), Err(StoreError::Database));
    assert_eq!(
        read_action(&store.connection, &context(), 1).unwrap().state,
        ActionState::Prepared
    );
    let usage = read_usage(&store.connection, &context().task_id).unwrap();
    assert_eq!(usage.input_reserved, 10);
    assert_eq!(usage.input_spent, 0);
    assert_eq!(
        read_run(&store.connection, &context()).unwrap().2,
        "running"
    );
}

#[test]
fn aggregate_pending_holds_must_match_task_counters() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);

    for id in ["one", "two"] {
        store
            .reserve_budget(BudgetRequest {
                context: context(),
                id: id.into(),
                reserve: Resources {
                    input_tokens: 10,
                    output_tokens: 0,
                    active_ms: 10,
                },
                pricing: crate::budget::Pricing::Unknown,
            })
            .unwrap();
    }
    // Every individual hold fits 15, but the combined 20-token obligation does not.
    store
        .connection
        .execute(
            "UPDATE tasks SET input_tokens_reserved=15,tokens_reserved=15",
            [],
        )
        .unwrap();
    assert_eq!(store.recover_work(), Err(StoreError::CorruptDatabase));
}

#[test]
fn budget_writes_are_atomic_at_each_resource_money_and_receipt_boundary() {
    fn request() -> BudgetRequest {
        BudgetRequest {
            context: context(),
            id: "work".into(),
            reserve: prepare().reserve,
            pricing: crate::budget::Pricing::UpperBound {
                currency: "USD".into(),
                input_minor: 1,
                output_minor: 1,
                tokens_per_unit: 1,
            },
        }
    }
    for point in [
        "AFTER UPDATE OF tokens_reserved ON tasks",
        "AFTER UPDATE OF inference_reserved ON tasks",
        "AFTER INSERT ON work_reservations",
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        let before = read_usage(&store.connection, &context().task_id)
            .unwrap()
            .snapshot()
            .unwrap();
        store.connection.execute_batch(&format!("CREATE TEMP TRIGGER fail_budget {point} BEGIN SELECT RAISE(ABORT,'synthetic'); END;")).unwrap();
        assert!(store.reserve_budget(request()).is_err());
        assert_eq!(
            read_usage(&store.connection, &context().task_id)
                .unwrap()
                .snapshot()
                .unwrap(),
            before
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM work_reservations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    for (point, actual) in [
        (
            "AFTER UPDATE OF tokens_reserved ON tasks",
            Some(commit().actual.unwrap()),
        ),
        (
            "AFTER UPDATE OF inference_reserved ON tasks",
            Some(commit().actual.unwrap()),
        ),
        (
            "AFTER UPDATE ON work_reservations",
            Some(commit().actual.unwrap()),
        ),
        ("AFTER UPDATE ON sessions", None),
        ("AFTER UPDATE ON work_reservations", None),
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        let ticket = store.reserve_budget(request()).unwrap();
        let before = read_usage(&store.connection, &context().task_id)
            .unwrap()
            .snapshot()
            .unwrap();
        store.connection.execute_batch(&format!("CREATE TEMP TRIGGER fail_budget {point} BEGIN SELECT RAISE(ABORT,'synthetic'); END;")).unwrap();
        assert!(store.settle_budget(ticket.clone(), actual).is_err());
        assert_eq!(
            read_usage(&store.connection, &context().task_id)
                .unwrap()
                .snapshot()
                .unwrap(),
            before
        );
        assert_eq!(
            store.budget_receipt(ticket.clone()).unwrap().state,
            crate::budget::ReservationState::Reserved
        );
        assert_eq!(read_run(&store.connection, &context()).unwrap().2, "paused");
        store
            .connection
            .execute_batch("DROP TRIGGER fail_budget")
            .unwrap();
        store.settle_budget(ticket, actual).unwrap();
    }
}

#[test]
fn budget_receipts_reject_corrupt_identity_shapes_prices_and_settlement() {
    for mutation in [
        "UPDATE work_reservations SET request_json=json_set(request_json,'$.schema_version',2)",
        "UPDATE work_reservations SET request_json=json_set(request_json,'$.request.id','wrong')",
        "UPDATE work_reservations SET request_json=json_set(request_json,'$.request.context.task_id','wrong')",
        "UPDATE work_reservations SET request_json=json_set(request_json,'$.request.reserve',json('[10,5,100]'))",
        "UPDATE work_reservations SET request_json=json_set(request_json,'$.request.pricing.tokens_per_unit',0)",
        "UPDATE work_reservations SET actual_json='{}'",
        "UPDATE work_reservations SET state='settled'",
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        let ticket = store
            .reserve_budget(BudgetRequest {
                context: context(),
                id: "work".into(),
                reserve: prepare().reserve,
                pricing: crate::budget::Pricing::UpperBound {
                    currency: "USD".into(),
                    input_minor: 1,
                    output_minor: 1,
                    tokens_per_unit: 1,
                },
            })
            .unwrap();
        store
            .connection
            .execute_batch("DROP TRIGGER immutable_work_reservation")
            .unwrap();
        store.connection.execute_batch(mutation).unwrap();
        assert!(
            matches!(
                store.budget_receipt(ticket.clone()),
                Err(StoreError::CorruptDatabase)
            ),
            "{mutation}"
        );
        assert_eq!(
            store.settle_budget(ticket, commit().actual),
            Err(StoreError::CorruptDatabase),
            "{mutation}"
        );
        assert_eq!(
            store
                .connection
                .query_row(
                    "SELECT input_tokens_spent FROM tasks WHERE id='task'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn every_prepare_and_commit_subwrite_rolls_back() {
    let prepare_points = [
        "AFTER UPDATE ON tasks",
        "AFTER UPDATE ON sessions",
        "AFTER INSERT ON actions",
        "AFTER INSERT ON budget_reservations",
    ];
    for point in prepare_points {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        store.connection.execute_batch(&format!("CREATE TEMP TRIGGER inject_failure {point} BEGIN SELECT RAISE(ABORT,'synthetic injection'); END;")).unwrap();
        assert!(store.prepare_action(prepare()).is_err(), "{point}");
        assert_eq!(read_run(&store.connection, &context()).unwrap().1, 1);
        assert_eq!(
            read_usage(&store.connection, &context().task_id)
                .unwrap()
                .attempts,
            0
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM actions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    for point in [
        "AFTER UPDATE ON actions",
        "AFTER UPDATE ON tasks",
        "AFTER UPDATE ON budget_reservations",
        "AFTER INSERT ON evidence",
        "AFTER INSERT ON artifacts",
        "AFTER INSERT ON checkpoints",
        "AFTER UPDATE ON sessions",
        "AFTER UPDATE ON tasks WHEN NEW.status <> OLD.status",
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        store.prepare_action(prepare()).unwrap();
        store.connection.execute_batch(&format!("CREATE TEMP TRIGGER inject_failure {point} BEGIN SELECT RAISE(ABORT,'synthetic injection'); END;")).unwrap();
        assert!(store.commit_step(commit()).is_err(), "{point}");
        assert_eq!(
            read_action(&store.connection, &context(), 1).unwrap().state,
            ActionState::Prepared
        );
        let usage = read_usage(&store.connection, &context().task_id)
            .unwrap()
            .public()
            .unwrap();
        assert_eq!(usage.attempts_spent, 1);
        assert_eq!(usage.tokens_spent, 0);
        assert_eq!(usage.tokens_reserved, 15);
        assert_eq!(usage.active_ms_spent, 0);
        assert_eq!(read_run(&store.connection, &context()).unwrap().1, 2);
        for table in ["evidence", "artifacts", "checkpoints"] {
            assert_eq!(
                store
                    .connection
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        read_reservation(&store.connection, &context(), 1).unwrap();
        store
            .connection
            .execute_batch("DROP TRIGGER inject_failure")
            .unwrap();
        store.commit_step(commit()).unwrap();
        assert_eq!(read_run(&store.connection, &context()).unwrap().1, 2);
    }
}

#[test]
fn bounded_queue_async_reply_and_drop_drain_accepted_work() {
    use std::future::Future;
    use std::task::{Context, Poll, Wake, Waker};
    struct Notify(mpsc::Sender<()>);
    impl Wake for Notify {
        fn wake(self: Arc<Self>) {
            let _ = self.0.send(());
        }
    }
    let (_parent, path) = directory();
    let owner = Journal::start(&path, 1).unwrap();
    let client = owner.client();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first = client
        .submit(move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(7)
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let queued = client.submit(|s| s.create_task(contract())).unwrap();
    let rejected = client.submit(|_| Ok(()));
    let model = crate::tasks::TaskAuthority::new(&owner)
        .model_for_task(TaskId::from("task"))
        .unwrap();
    let denied = model.revise_contract(contract());
    let (wake_tx, wake_rx) = mpsc::channel();
    let waker = Waker::from(Arc::new(Notify(wake_tx)));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(first.recv());
    let pending = matches!(future.as_mut().poll(&mut cx), Poll::Pending);
    release_tx.send(()).unwrap(); // Release before assertions so failed tests cannot deadlock Drop.
    assert!(pending);
    assert!(matches!(rejected, Err(StoreError::QueueFull)));
    assert!(matches!(denied, Err(StoreError::PermissionDenied)));
    wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(Ok(7))));
    drop(queued);
    drop(owner);
    assert!(matches!(
        client.submit(|_| Ok(())),
        Err(StoreError::WorkerClosed)
    ));
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .connection
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn concurrent_close_admits_only_drained_requests() {
    use std::sync::Barrier;
    let (_parent, path) = directory();
    let owner = Journal::start(&path, 16).unwrap();
    let client = owner.client();
    let gate = Arc::new(Barrier::new(9));
    let mut threads = Vec::new();
    for i in 0..8 {
        let client = client.clone();
        let gate = gate.clone();
        threads.push(std::thread::spawn(move || {
            gate.wait();
            match client.submit(move |s| {sql(s.connection.execute("INSERT INTO tasks(id,status,created_at,updated_at) VALUES (?1,'draft','now','now')",[format!("synthetic-{i}")]))?;Ok(())}) {
                Ok(reply)=>{reply.blocking_recv().unwrap();true},
                Err(StoreError::WorkerClosed|StoreError::QueueFull)=>false,
                Err(error)=>panic!("unexpected static error {error}"),
            }
        }));
    }
    gate.wait();
    owner.close().unwrap();
    let accepted = threads
        .into_iter()
        .map(|t| usize::from(t.join().unwrap()))
        .sum::<usize>();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .connection
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        accepted as i64
    );
}

#[test]
fn worker_panic_disconnects_replies_and_releases_owner() {
    let (_parent, path) = directory();
    let owner = Journal::start(&path, 2).unwrap();
    let client = owner.client();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first: Reply<()> = client
        .submit(move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            panic!("synthetic worker failure")
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let queued = client.submit(|_| Ok(())).unwrap();
    release_tx.send(()).unwrap();
    assert_eq!(first.blocking_recv(), Err(StoreError::WorkerClosed));
    assert_eq!(queued.blocking_recv(), Err(StoreError::WorkerClosed));
    assert_eq!(owner.close(), Err(StoreError::WorkerClosed));
    assert!(Store::open(&path).is_ok());
}

#[test]
fn persisted_payloads_reject_wrong_shapes_versions_bounds_and_context() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    let original: String = store
        .connection
        .query_row(
            "SELECT reservation_json FROM budget_reservations",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut positional: serde_json::Value = serde_json::from_str(&original).unwrap();
    positional["reserve"] = serde_json::json!([10, 5, 100]);
    store
        .connection
        .execute(
            "UPDATE budget_reservations SET reservation_json=?1",
            [positional.to_string()],
        )
        .unwrap();
    assert!(matches!(
        store.commit_step(commit()),
        Err(StoreError::CorruptDatabase)
    ));
    store
        .connection
        .execute(
            "UPDATE budget_reservations SET reservation_json=?1",
            [original],
        )
        .unwrap();
    store.commit_step(commit()).unwrap();
    let original: String = store
        .connection
        .query_row("SELECT metadata_json FROM checkpoints", [], |r| r.get(0))
        .unwrap();
    for mode in ["version", "owner", "cursor", "size", "unknown", "null"] {
        let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
        match mode {
            "version" => value["schema_version"] = 2.into(),
            "owner" => value["context"]["task_id"] = "other".into(),
            "cursor" => value["next_step"] = 1.into(),
            "size" => value["unknown"] = "x".repeat(MAX_PAYLOAD_BYTES + 1).into(),
            "unknown" => value["unknown"] = true.into(),
            _ => value["usage"] = serde_json::Value::Null,
        }
        store
            .connection
            .execute(
                "UPDATE checkpoints SET metadata_json=?1",
                [value.to_string()],
            )
            .unwrap();
        assert!(
            matches!(
                store.load_resume(context()),
                Err(StoreError::CorruptDatabase)
            ),
            "{mode}"
        );
    }
    store
        .connection
        .execute("UPDATE checkpoints SET metadata_json=?1", [original])
        .unwrap();
    store
        .connection
        .execute("UPDATE actions SET action_json='{}'", [])
        .unwrap();
    assert!(matches!(
        store.load_audit(context(), 0, 10),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn persisted_reservation_owner_must_match_its_payload() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    let mut other = contract();
    other.task_id = TaskId::from("other");
    store.create_task(other).unwrap();
    store
        .create_run(
            RunContext {
                task_id: TaskId::from("other"),
                run_id: RunId::from("other-run"),
                contract_revision: 1,
            },
            StartPrerequisites {
                local_processor_available: true,
                background_service_available: false,
            },
        )
        .unwrap();
    store.prepare_action(prepare()).unwrap();
    store
        .connection
        .execute(
            "UPDATE budget_reservations SET task_id='other',session_id='other-run'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.commit_step(commit()),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn checkpoint_usage_must_cover_its_committed_cursor() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    store.commit_step(commit()).unwrap();
    store.connection.execute("UPDATE checkpoints SET metadata_json=json_set(metadata_json,'$.usage.attempts_spent',0)",[]).unwrap();
    assert!(matches!(
        store.load_resume(context()),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn audit_rejects_contradictory_persisted_state_and_outcome() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    store.prepare_action(prepare()).unwrap();
    store.commit_step(commit()).unwrap();
    store
        .connection
        .execute(
            "UPDATE actions SET outcome_json=json_set(outcome_json,'$.verification','failed')",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.load_audit(context(), 0, 10),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn checkpoint_references_validate_evidence_and_artifact_payload_identity() {
    for table in ["evidence", "artifacts"] {
        for field in ["owner", "id", "digest", "shape"] {
            let (_parent, path) = directory();
            let mut store = seeded(&path);
            store.prepare_action(prepare()).unwrap();
            store.commit_step(commit()).unwrap();
            let sql = match field {
                "owner" => format!(
                    "UPDATE {table} SET metadata_json=json_set(metadata_json,'$.context.task_id','other-task')"
                ),
                "id" => format!(
                    "UPDATE {table} SET metadata_json=json_set(metadata_json,'$.id','different')"
                ),
                "digest" => format!(
                    "UPDATE {table} SET metadata_json=json_set(metadata_json,'$.sha256','invalid')"
                ),
                _ => format!("UPDATE {table} SET metadata_json='{{}}'"),
            };
            // Simulate corruption below the SQL immutability guards; the
            // triggers are covered by the artifacts and evidence suites.
            let trigger = if table == "artifacts" {
                "immutable_artifact_record"
            } else {
                "immutable_evidence_record"
            };
            store
                .connection
                .execute(&format!("DROP TRIGGER {trigger}"), [])
                .unwrap();
            store.connection.execute(&sql, []).unwrap();
            assert!(
                matches!(
                    store.load_resume(context()),
                    Err(StoreError::CorruptDatabase)
                ),
                "{table}/{field}"
            );
        }
    }
}

#[test]
fn counter_boundaries_and_not_dispatched_reports_remain_safe() {
    let (_parent, path) = directory();
    let mut store = Store::open(&path).unwrap();
    let mut c = contract();
    c.budgets.max_input_tokens = MAX_COUNTER;
    c.budgets.max_output_tokens = 0;
    c.budgets.max_active_ms = MAX_COUNTER;
    store.create_task(c).unwrap();
    store
        .create_run(
            context(),
            StartPrerequisites {
                local_processor_available: true,
                background_service_available: false,
            },
        )
        .unwrap();
    let mut request = prepare();
    request.reserve = Resources {
        input_tokens: MAX_COUNTER,
        output_tokens: 0,
        active_ms: MAX_COUNTER,
    };
    store.prepare_action(request).unwrap();
    let mut result = commit();
    result.actual = Some(Resources {
        input_tokens: MAX_COUNTER,
        output_tokens: 0,
        active_ms: MAX_COUNTER,
    });
    store.commit_step(result).unwrap();
    let state = store.load_resume(context()).unwrap();
    assert_eq!(state.usage.tokens_spent, MAX_COUNTER);
    assert!(matches!(
        store.prepare_action(prepare()),
        Err(StoreError::BudgetExceeded)
    ));
    for status in [
        VerificationStatus::NotChecked,
        VerificationStatus::Passed,
        VerificationStatus::Failed,
        VerificationStatus::Inconclusive,
    ] {
        let (_parent, path) = directory();
        let mut store = seeded(&path);
        store.prepare_action(prepare()).unwrap();
        let mut result = commit();
        result.outcome.dispatch = DispatchState::NotDispatched;
        result.outcome.verification = status;
        store.commit_step(result).unwrap();
        assert!(!store.load_resume(context()).unwrap().needs_review);
        assert_eq!(
            store
                .connection
                .query_row("SELECT status FROM tasks", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "paused"
        );
    }
}

#[test]
fn encryption_required_payloads_and_validation_errors_never_leak() {
    let (_parent, path) = directory();
    let mut store = seeded(&path);
    let mut request = prepare();
    request.persistence = Persistence::RequiresEncryption;
    request.decision.action = Action::Type {
        target: "field".into(),
        text: "SYNTHETIC_PRIVATE_SENTINEL".into(),
        clear: true,
        press_enter: false,
    };
    let error = store.prepare_action(request).err().unwrap();
    assert_eq!(error, StoreError::EncryptionRequired);
    assert!(!format!("{error:?} {error}").contains("SENTINEL"));
    let mut sensitive = contract();
    sensitive.processing_mode = ProcessingMode::Sensitive;
    assert_eq!(
        store.create_task(sensitive),
        Err(StoreError::EncryptionRequired)
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM actions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(store);
    for name in ["journal.sqlite3", "journal.sqlite3-wal"] {
        if let Ok(bytes) = std::fs::read(path.join(name)) {
            assert!(
                !bytes
                    .windows(b"SYNTHETIC_PRIVATE_SENTINEL".len())
                    .any(|w| w == b"SYNTHETIC_PRIVATE_SENTINEL")
            );
        }
    }
}
