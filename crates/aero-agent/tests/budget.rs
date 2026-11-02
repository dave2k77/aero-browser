use aero_agent::budget::{ActiveTimer, BudgetRequest, Clock, Pricing};
use aero_agent::store::StoreError;
use aero_agent::store::worker::{Journal, Resources};
use aero_agent::tasks::TaskAuthority;
use aero_core::checkpoint::RunContext;
use aero_core::ids::{RunId, TaskId};
use aero_core::task::{StartPrerequisites, TaskContract};
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

// Another test's process spawn can temporarily inherit close-on-exec journal
// lock fds before exec, so a just-closed journal may legitimately return Busy.
// Serialize independent test fixtures; concurrency within each test remains.
static TEST_ISOLATION: Mutex<()> = Mutex::new(());

fn action(c: RunContext, input: u64) -> aero_agent::store::worker::PrepareAction {
    aero_agent::store::worker::PrepareAction {
        context: c,
        persistence: aero_agent::store::worker::Persistence::Redacted,
        decision: aero_core::actions::Decision {
            schema_version: 1,
            observation_id: aero_core::ids::ObservationId::from("synthetic-observation"),
            rationale: "synthetic accounting fixture".into(),
            action: aero_core::actions::Action::Wait { milliseconds: 1 },
        },
        reserve: Resources {
            input_tokens: input,
            output_tokens: 0,
            active_ms: 100,
        },
    }
}
fn outcome(
    prepared: aero_agent::store::worker::PreparedAction,
    input: u64,
) -> aero_agent::store::worker::CommitStep {
    aero_agent::store::worker::CommitStep {
        prepared,
        outcome: aero_agent::store::worker::StepOutcome {
            schema_version: 1,
            dispatch: aero_core::events::DispatchState::NotDispatched,
            verification: aero_core::events::VerificationStatus::Passed,
        },
        actual: Some(Resources {
            input_tokens: input,
            output_tokens: 0,
            active_ms: 10,
        }),
        evidence: vec![],
        artifacts: vec![],
    }
}

#[test]
fn priced_caps_fail_closed_and_share_action_accounting() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut c = contract();
    c.budgets.max_inference_minor_units = Some(7);
    let (_dir, journal) = setup(c);
    let client = journal.client();
    assert!(matches!(
        client
            .reserve_budget(request("unknown", 1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::UnknownPricing)
    ));
    assert!(matches!(
        client
            .prepare_action(action(context("run", 1), 1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::UnknownPricing)
    ));
    let mut r = request("priced", 60);
    r.pricing = price();
    let ticket = client.reserve_budget(r).unwrap().blocking_recv().unwrap();
    let mut r = request("exceeds", 20);
    r.pricing = price();
    assert!(matches!(
        client.reserve_budget(r).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
    client
        .settle_budget(
            ticket,
            Some(Resources {
                input_tokens: 51,
                output_tokens: 0,
                active_ms: 70,
            }),
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    let prepared = client
        .prepare_charged_action(
            action(context("run", 1), 10),
            aero_agent::budget::Charges {
                inference: price(),
                purchase: None,
            },
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(outcome(prepared, 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let s = client
        .budget_status(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(s.total_inference_upper_bound, Some(7));
    assert_eq!(s.spent.input_tokens, 52);
    assert_eq!(s.action_attempts, 1);
    let mut r = request("last", 1);
    r.pricing = price();
    assert!(matches!(
        client.reserve_budget(r).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
}

#[test]
fn unknown_price_survives_history_and_cannot_be_relabelled_as_capped() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let owner = TaskAuthority::new(&journal).trusted_user();
    let ticket = client
        .reserve_budget(request("unknown", 60))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client
            .budget_status(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .total_inference_upper_bound,
        None
    );
    client
        .settle_budget(
            ticket,
            Some(Resources {
                input_tokens: 60,
                output_tokens: 0,
                active_ms: 75,
            }),
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut c = contract();
    c.revision = 2;
    c.budgets.max_inference_minor_units = Some(100);
    assert_eq!(
        owner.revise_contract(c).unwrap().blocking_recv(),
        Err(StoreError::UnknownPricing)
    );
    owner
        .start_run(context("next", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let s = client
        .budget_status(context("next", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(s.spent.input_tokens, 60);
    assert_eq!(s.spent.active_ms, 75);
    assert_eq!(s.total_inference_upper_bound, None);
    assert!(matches!(
        client
            .prepare_action(action(context("next", 1), 41))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
    assert!(matches!(
        client
            .reserve_budget(request("stale", 1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    ));
    let mut c = contract();
    c.revision = 2;
    c.budgets.budget_currency = "EUR".into();
    assert_eq!(
        owner.revise_contract(c).unwrap().blocking_recv(),
        Err(StoreError::InvalidInput)
    );
}

#[test]
fn purchase_caps_are_separate_and_unknown_settlement_keeps_the_hold() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use aero_agent::budget::{Charges, Money};
    let (_dir, journal) = setup(contract());
    let owner = TaskAuthority::new(&journal).trusted_user();
    let client = journal.client();
    let charge = |amount| Charges {
        inference: Pricing::NoInference,
        purchase: Some(Money {
            currency: "USD".into(),
            minor_units: amount,
        }),
    };
    assert!(matches!(
        client
            .prepare_charged_action(action(context("run", 1), 0), charge(1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
    assert_eq!(
        client
            .budget_status(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .action_attempts,
        0
    );
    let mut c = contract();
    c.revision = 2;
    c.budgets.max_purchase_minor_units = 10;
    owner.revise_contract(c).unwrap().blocking_recv().unwrap();
    owner
        .start_run(context("new", 2), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let prepared = client
        .prepare_charged_action(action(context("new", 2), 0), charge(7))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_charged_step(outcome(prepared, 0), Some(5))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(matches!(
        client
            .prepare_charged_action(action(context("new", 2), 0), charge(6))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
    let prepared = client
        .prepare_charged_action(action(context("new", 2), 0), charge(5))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(outcome(prepared, 0))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let s = client
        .budget_status(context("new", 2))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(s.purchase_spent, 5);
    assert_eq!(s.purchase_reserved, 5);
    assert_eq!(s.total_inference_upper_bound, Some(0));
    assert!(
        client
            .load_resume(context("new", 2))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .needs_review
    );
}

#[test]
fn orphan_holds_survive_restart_and_block_reset_or_refund() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (dir, journal) = setup(contract());
    let client = journal.client();
    let owner = TaskAuthority::new(&journal).trusted_user();
    let ticket = client
        .reserve_budget(request("lost", 60))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut revised = contract();
    revised.revision = 2;
    assert_eq!(
        owner.revise_contract(revised).unwrap().blocking_recv(),
        Err(StoreError::NeedsReview)
    );
    assert_eq!(
        owner
            .start_run(context("new", 1), prerequisites())
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview)
    );
    assert_eq!(
        client
            .delete_task(context("run", 1).task_id)
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview)
    );
    journal.close().unwrap();
    for _ in 0..2 {
        let journal = Journal::start(&dir.path().join("journal"), 8).unwrap();
        let client = journal.client();
        let s = client
            .budget_status(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert_eq!(s.reserved.input_tokens, 60);
        assert_eq!(s.reserved.active_ms, 100);
        assert_eq!(s.total_inference_upper_bound, None);
        assert_eq!(
            client
                .budget_receipt(ticket.clone())
                .unwrap()
                .blocking_recv()
                .unwrap()
                .state,
            aero_agent::budget::ReservationState::Uncertain
        );
        assert_eq!(
            client
                .settle_budget(
                    ticket.clone(),
                    Some(Resources {
                        input_tokens: 0,
                        output_tokens: 0,
                        active_ms: 0
                    })
                )
                .unwrap()
                .blocking_recv(),
            Err(StoreError::Conflict)
        );
        assert!(
            client
                .load_resume(context("run", 1))
                .unwrap()
                .blocking_recv()
                .unwrap()
                .needs_review
        );
        journal.close().unwrap();
    }
}

#[test]
fn receipts_reject_cross_context_reuse_and_overruns_without_mutation() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let r = request("id", 60);
    let ticket = client
        .reserve_budget(r.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(matches!(
        client.reserve_budget(r).unwrap().blocking_recv(),
        Err(StoreError::Conflict)
    ));
    for wrong in [
        context("wrong-run", 1),
        context("run", 2),
        RunContext {
            task_id: TaskId::from("foreign"),
            ..context("run", 1)
        },
    ] {
        let forged = aero_agent::budget::BudgetTicket {
            context: wrong,
            id: "id".into(),
        };
        assert!(
            client
                .settle_budget(
                    forged,
                    Some(Resources {
                        input_tokens: 0,
                        output_tokens: 0,
                        active_ms: 0
                    })
                )
                .unwrap()
                .blocking_recv()
                .is_err()
        );
    }
    for actual in [
        Resources {
            input_tokens: 61,
            output_tokens: 0,
            active_ms: 0,
        },
        Resources {
            input_tokens: 0,
            output_tokens: 1,
            active_ms: 0,
        },
        Resources {
            input_tokens: 0,
            output_tokens: 0,
            active_ms: 101,
        },
    ] {
        let (_dir, journal) = setup(contract());
        let client = journal.client();
        let overrun_ticket = client
            .reserve_budget(request("overrun", 60))
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert_eq!(
            client
                .settle_budget(overrun_ticket, Some(actual))
                .unwrap()
                .blocking_recv(),
            Err(StoreError::BudgetExceeded)
        );
    }
    let before = client
        .budget_status(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(before.reserved.input_tokens, 60);
    assert_eq!(before.spent.input_tokens, 0);
    client
        .settle_budget(
            ticket.clone(),
            Some(Resources {
                input_tokens: 0,
                output_tokens: 0,
                active_ms: 0,
            }),
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client
            .settle_budget(
                ticket,
                Some(Resources {
                    input_tokens: 0,
                    output_tokens: 0,
                    active_ms: 0
                })
            )
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        client
            .budget_status(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .total_inference_upper_bound,
        None
    );
}

#[test]
fn invalid_prices_overflow_and_clock_regressions_fail_closed() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use aero_core::checkpoint::MAX_COUNTER;
    let r = Resources {
        input_tokens: MAX_COUNTER,
        output_tokens: 0,
        active_ms: 0,
    };
    assert_eq!(
        Pricing::UpperBound {
            currency: "USD".into(),
            input_minor: MAX_COUNTER,
            output_minor: 0,
            tokens_per_unit: 1
        }
        .quote(r, "USD"),
        Err(StoreError::BudgetExceeded)
    );
    for (currency, unit) in [("usd", 1), ("USD", 0), ("EUR", 1)] {
        assert_eq!(
            Pricing::UpperBound {
                currency: currency.into(),
                input_minor: 1,
                output_minor: 1,
                tokens_per_unit: unit
            }
            .quote(r, "USD"),
            Err(StoreError::InvalidInput)
        );
    }
    assert_eq!(
        Pricing::UpperBound {
            currency: "USD".into(),
            input_minor: MAX_COUNTER,
            output_minor: 0,
            tokens_per_unit: MAX_COUNTER
        }
        .quote(r, "USD")
        .unwrap(),
        Some(MAX_COUNTER)
    );
    let clock = FakeClock(Arc::new(AtomicU64::new(10)));
    let mut timer = ActiveTimer::new(clock.clone());
    timer.resume().unwrap();
    assert_eq!(timer.resume(), Err(StoreError::Conflict));
    clock.0.store(5, Ordering::SeqCst);
    assert_eq!(timer.elapsed_ms(), Err(StoreError::InvalidInput));
    clock.0.store(20, Ordering::SeqCst);
    timer.pause().unwrap();
    assert_eq!(timer.pause(), Err(StoreError::Conflict));
    assert_eq!(timer.elapsed_ms().unwrap(), 1);
}
fn contract() -> TaskContract {
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
    let mut c: TaskContract = serde_json::from_str(json).unwrap();
    c.task_id = TaskId::from("budget-task");
    c.budgets.max_input_tokens = 100;
    c.budgets.max_output_tokens = 100;
    c.budgets.max_active_ms = 1000;
    c
}
fn context(run: &str, revision: u64) -> RunContext {
    RunContext {
        task_id: TaskId::from("budget-task"),
        run_id: RunId::from(run),
        contract_revision: revision,
    }
}
fn prerequisites() -> StartPrerequisites {
    StartPrerequisites {
        local_processor_available: true,
        background_service_available: false,
    }
}
fn setup(c: TaskContract) -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let j = Journal::start(&dir.path().join("journal"), 16).unwrap();
    let owner = TaskAuthority::new(&j).trusted_user();
    owner.create_task(c).unwrap().blocking_recv().unwrap();
    owner
        .start_run(context("run", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    (dir, j)
}
fn request(id: &str, input: u64) -> BudgetRequest {
    BudgetRequest {
        context: context("run", 1),
        id: id.into(),
        reserve: Resources {
            input_tokens: input,
            output_tokens: 0,
            active_ms: 100,
        },
        pricing: Pricing::Unknown,
    }
}
fn price() -> Pricing {
    Pricing::UpperBound {
        currency: "USD".into(),
        input_minor: 1,
        output_minor: 3,
        tokens_per_unit: 10,
    }
}
#[test]
fn reported_action_overruns_remain_uncertain_and_cannot_be_resettled() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use aero_agent::budget::{Charges, Money};
    use aero_agent::store::worker::ActionState;

    for purchase_overrun in [false, true] {
        let mut c = contract();
        c.budgets.max_purchase_minor_units = 10;
        let (dir, journal) = setup(c);
        let client = journal.client();
        let prepared = client
            .prepare_charged_action(
                action(context("run", 1), 60),
                Charges {
                    inference: price(),
                    purchase: Some(Money {
                        currency: "USD".into(),
                        minor_units: 5,
                    }),
                },
            )
            .unwrap()
            .blocking_recv()
            .unwrap();
        let reported = outcome(prepared.clone(), if purchase_overrun { 20 } else { 61 });
        let purchase_actual = Some(if purchase_overrun { 6 } else { 5 });
        assert_eq!(
            client
                .commit_charged_step(reported, purchase_actual)
                .unwrap()
                .blocking_recv(),
            Err(StoreError::BudgetExceeded)
        );
        let audit = client
            .load_audit(context("run", 1), 0, 10)
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert_eq!(audit.entries[0].state, ActionState::Uncertain);
        let resume = client
            .load_resume(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(resume.needs_review);
        assert_eq!(resume.budget.reserved.input_tokens, 60);
        assert_eq!(resume.budget.purchase_reserved, 5);
        assert!(resume.budget.uncertain_inference);
        assert_eq!(
            audit.entries[0]
                .reported_overrun
                .as_ref()
                .unwrap()
                .purchase_actual,
            purchase_actual
        );
        assert_eq!(
            audit.entries[0]
                .reported_overrun
                .as_ref()
                .unwrap()
                .actual
                .unwrap()
                .input_tokens,
            if purchase_overrun { 20 } else { 61 }
        );
        assert_eq!(
            client
                .commit_charged_step(outcome(prepared, 20), Some(5))
                .unwrap()
                .blocking_recv(),
            Err(StoreError::Conflict)
        );
        journal.close().unwrap();
        let reopened = Journal::start(&dir.path().join("journal"), 8).unwrap();
        let recovered = reopened
            .client()
            .load_resume(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(recovered.needs_review);
        assert_eq!(recovered.budget.reserved.input_tokens, 60);
        assert_eq!(recovered.budget.purchase_reserved, 5);
        assert!(recovered.budget.uncertain_inference);
        let recovered_audit = reopened
            .client()
            .load_audit(context("run", 1), 0, 10)
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert_eq!(
            recovered_audit.entries[0]
                .reported_overrun
                .as_ref()
                .unwrap()
                .purchase_actual,
            purchase_actual
        );
    }
}

#[test]
fn oversized_trusted_usage_reports_freeze_only_their_owned_work() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    use aero_core::checkpoint::MAX_COUNTER;
    let (dir, journal) = setup(contract());
    let client = journal.client();
    let ticket = client
        .reserve_budget(request("overflow", 10))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let oversized = Resources {
        input_tokens: MAX_COUNTER + 1,
        output_tokens: 0,
        active_ms: 0,
    };
    let mut wrong = ticket.clone();
    wrong.context.run_id = RunId::from("other");
    assert!(
        client
            .settle_budget(wrong, Some(oversized))
            .unwrap()
            .blocking_recv()
            .is_err()
    );
    assert_eq!(
        client
            .budget_receipt(ticket.clone())
            .unwrap()
            .blocking_recv()
            .unwrap()
            .state,
        aero_agent::budget::ReservationState::Reserved
    );
    assert_eq!(
        client
            .settle_budget(ticket.clone(), Some(oversized))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    let receipt = client
        .budget_receipt(ticket.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        receipt.state,
        aero_agent::budget::ReservationState::Uncertain
    );
    assert_eq!(receipt.reported_overrun, Some(oversized));
    journal.close().unwrap();
    let reopened = Journal::start(&dir.path().join("journal"), 8).unwrap();
    let receipt = reopened
        .client()
        .budget_receipt(ticket)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(receipt.reported_overrun, Some(oversized));

    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let prepared = client
        .prepare_action(action(context("run", 1), 10))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut report = outcome(prepared.clone(), 1);
    report.actual = Some(oversized);
    assert_eq!(
        client.commit_step(report).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    assert!(
        client
            .load_resume(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .needs_review
    );
}

#[test]
fn reported_work_overrun_freezes_further_admission() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let ticket = client
        .reserve_budget(request("overrun", 60))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client
            .settle_budget(
                ticket,
                Some(Resources {
                    input_tokens: 61,
                    output_tokens: 0,
                    active_ms: 0
                })
            )
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    assert!(matches!(
        client
            .reserve_budget(request("more", 30))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview) | Err(StoreError::Conflict)
    ));
}

#[test]
fn concurrent_settlement_keeps_review_sticky_and_history_is_bounded() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for reverse in [false, true] {
        let (_dir, journal) = setup(contract());
        let client = journal.client();
        let a = client
            .reserve_budget(request("a", 40))
            .unwrap()
            .blocking_recv()
            .unwrap();
        let b = client
            .reserve_budget(request("b", 40))
            .unwrap()
            .blocking_recv()
            .unwrap();
        let actual = Some(Resources {
            input_tokens: 30,
            output_tokens: 0,
            active_ms: 30,
        });
        if reverse {
            client
                .settle_budget(b, actual)
                .unwrap()
                .blocking_recv()
                .unwrap();
            client
                .settle_budget(a, None)
                .unwrap()
                .blocking_recv()
                .unwrap();
        } else {
            client
                .settle_budget(a, None)
                .unwrap()
                .blocking_recv()
                .unwrap();
            client
                .settle_budget(b, actual)
                .unwrap()
                .blocking_recv()
                .unwrap();
        }
        let resume = client
            .load_resume(context("run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(resume.needs_review);
        assert_eq!(
            resume.current_status,
            aero_core::checkpoint::RunStatus::NeedsReview
        );
        assert_eq!(resume.budget.reserved.input_tokens, 40);
        assert_eq!(resume.budget.spent.input_tokens, 30);
        assert_eq!(resume.budget.currency, "USD");
        assert_eq!(resume.budget.unknown_inference_spent_count, 1);
        assert_eq!(resume.budget.unknown_inference_reserved_count, 1);
        let first = client
            .budget_history(context("run", 1), None, 1)
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(first.has_more);
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].ticket.id, "a");
        let second = client
            .budget_history(context("run", 1), Some("a".into()), 1)
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(!second.has_more);
        assert_eq!(second.entries[0].ticket.id, "b");
        assert_eq!(
            second.entries[0].receipt.state,
            aero_agent::budget::ReservationState::Settled
        );
        assert!(matches!(
            client.budget_history(context("run", 1), None, 0),
            Err(StoreError::InvalidInput)
        ));
    }
}

#[test]
fn concurrent_reservations_cannot_oversubscribe_task() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let handles: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|id| {
            let client = client.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                client
                    .reserve_budget(request(id, 60))
                    .unwrap()
                    .blocking_recv()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::BudgetExceeded)))
            .count(),
        1
    );
}
#[test]
fn unexpected_inference_overrun_never_claims_free() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let prepared = client
        .prepare_action(action(context("run", 1), 0))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client
            .commit_step(outcome(prepared, 1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    let budget = client
        .budget_status(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(budget.uncertain_inference);
    assert_eq!(budget.total_inference_upper_bound, None);

    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let mut r = request("mechanical", 0);
    r.pricing = Pricing::NoInference;
    let ticket = client.reserve_budget(r).unwrap().blocking_recv().unwrap();
    assert_eq!(
        client
            .settle_budget(
                ticket,
                Some(Resources {
                    input_tokens: 1,
                    output_tokens: 0,
                    active_ms: 0
                })
            )
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    let budget = client
        .budget_status(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(budget.uncertain_inference);
    assert_eq!(budget.total_inference_upper_bound, None);
}

#[test]
fn unknown_zero_tokens_never_claim_free_inference() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert_eq!(
        Pricing::Unknown
            .quote(
                Resources {
                    input_tokens: 0,
                    output_tokens: 0,
                    active_ms: 0
                },
                "USD"
            )
            .unwrap(),
        None
    );
}

#[test]
fn price_rounds_up_and_unknown_is_not_free() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let r = Resources {
        input_tokens: 1,
        output_tokens: 1,
        active_ms: 0,
    };
    assert_eq!(price().quote(r, "USD").unwrap(), Some(2));
    assert_eq!(Pricing::Unknown.quote(r, "USD").unwrap(), None);
}
#[test]
fn hard_process_exit_preserves_reservations_without_drop_cleanup() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(path) = std::env::var_os("AERO_N03_CHILD") {
        let journal = Journal::start(std::path::Path::new(&path), 8).unwrap();
        journal
            .client()
            .reserve_budget(request("crash", 60))
            .unwrap()
            .blocking_recv()
            .unwrap();
        // Bypass Rust destructors, with no active provider/browser or remote effect.
        std::process::exit(0);
    }
    let (dir, journal) = setup(contract());
    journal.close().unwrap();
    let path = dir.path().join("journal");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "hard_process_exit_preserves_reservations_without_drop_cleanup",
        ])
        .env("AERO_N03_CHILD", &path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let journal = Journal::start(&path, 8).unwrap();
    let view = journal
        .client()
        .load_resume(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(view.needs_review);
    assert_eq!(view.budget.reserved.input_tokens, 60);
    assert_eq!(view.budget.reserved.active_ms, 100);
}

#[test]
fn lost_work_and_settlement_replies_are_reconciled_by_durable_id() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let (dir, journal) = setup(contract());
    let client = journal.client();
    drop(client.reserve_budget(request("lost-reply", 30)).unwrap());
    let ticket = aero_agent::budget::BudgetTicket {
        context: context("run", 1),
        id: "lost-reply".into(),
    };
    assert_eq!(
        client
            .budget_receipt(ticket.clone())
            .unwrap()
            .blocking_recv()
            .unwrap()
            .state,
        aero_agent::budget::ReservationState::Reserved
    );
    drop(
        client
            .settle_budget(
                ticket.clone(),
                Some(Resources {
                    input_tokens: 20,
                    output_tokens: 0,
                    active_ms: 20,
                }),
            )
            .unwrap(),
    );
    journal.close().unwrap();
    let journal = Journal::start(&dir.path().join("journal"), 8).unwrap();
    let client = journal.client();
    assert_eq!(
        client
            .budget_receipt(ticket)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .state,
        aero_agent::budget::ReservationState::Settled
    );
    let s = client
        .budget_status(context("run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(s.spent.input_tokens, 20);
    assert_eq!(s.reserved.input_tokens, 0);
    assert_eq!(s.total_inference_upper_bound, None);
}
#[derive(Clone)]
struct FakeClock(Arc<AtomicU64>);
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(self.0.load(Ordering::SeqCst))
    }
}
#[test]
fn active_clock_excludes_pauses_and_preserves_submillisecond_work() {
    let _isolation = TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let clock = FakeClock(Arc::new(AtomicU64::new(0)));
    let mut timer = ActiveTimer::new(clock.clone());
    clock.0.store(10_000_000_000, Ordering::SeqCst);
    assert_eq!(timer.elapsed_ms().unwrap(), 0);
    timer.resume().unwrap();
    clock.0.fetch_add(400_000, Ordering::SeqCst);
    timer.pause().unwrap();
    clock.0.fetch_add(50_000_000_000, Ordering::SeqCst);
    assert_eq!(timer.elapsed_ms().unwrap(), 1);
    timer.resume().unwrap();
    clock.0.fetch_add(700_000, Ordering::SeqCst);
    timer.pause().unwrap();
    assert_eq!(timer.elapsed_ms().unwrap(), 2);
    let (_dir, journal) = setup(contract());
    let client = journal.client();
    let mut r = request("timed", 0);
    r.pricing = Pricing::NoInference;
    let ticket = client.reserve_budget(r).unwrap().blocking_recv().unwrap();
    client
        .settle_budget(
            ticket,
            Some(Resources {
                input_tokens: 0,
                output_tokens: 0,
                active_ms: timer.elapsed_ms().unwrap(),
            }),
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    TaskAuthority::new(&journal)
        .trusted_user()
        .start_run(context("timed-next", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut r = request("too-long", 0);
    r.context = context("timed-next", 1);
    r.pricing = Pricing::NoInference;
    r.reserve.active_ms = 999;
    assert!(matches!(
        client.reserve_budget(r).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
}
