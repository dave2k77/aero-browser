use aero_agent::evidence::{CapturedFact, StructuredClaim, assess_structured_claim};
use aero_core::checkpoint::RunContext;
use aero_core::events::VerificationStatus;
use aero_core::ids::{DocumentEpoch, EvidenceId, RunId, TaskId};

fn context() -> RunContext {
    RunContext {
        task_id: TaskId::from("task"),
        run_id: RunId::from("run"),
        contract_revision: 1,
    }
}
fn fact() -> CapturedFact {
    CapturedFact {
        id: EvidenceId::from("capture"),
        context: context(),
        document: DocumentEpoch::from("document-1"),
        origin: "https://supplier.test".into(),
        effective_url: "https://supplier.test/catalog".into(),
        captured_at_ms: 1,
        entity: "supplier-a".into(),
        field: "price".into(),
        value: "42 USD".into(),
        excerpt: "price: 42 USD".into(),
    }
}
fn claim() -> StructuredClaim {
    StructuredClaim {
        evidence_id: EvidenceId::from("capture"),
        entity: "supplier-a".into(),
        field: "price".into(),
        value: "42 USD".into(),
    }
}

#[test]
fn absent_excerpt_and_contradictory_owned_facts_are_inconclusive() {
    let doc = DocumentEpoch::from("document-1");
    let mut missing_excerpt = fact();
    missing_excerpt.excerpt.clear();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[missing_excerpt]),
        VerificationStatus::Inconclusive
    );
    let mut contradictory = fact();
    contradictory.id = EvidenceId::from("other-capture");
    contradictory.value = "43 USD".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[fact(), contradictory]),
        VerificationStatus::Inconclusive
    );
    // Empty may be an actual source field value if an excerpt is present.
    let mut empty = fact();
    empty.value.clear();
    empty.excerpt = "price: [empty]".into();
    let mut empty_claim = claim();
    empty_claim.value.clear();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &empty_claim, &[empty]),
        VerificationStatus::Passed
    );
}

#[test]
fn caller_can_forge_matching_capture_so_result_is_not_authority() {
    // These publicly constructible inputs are synthetic. A passing comparison
    // must never be interpreted as proof that a real source was captured.
    let forged = fact();
    assert_eq!(
        assess_structured_claim(
            &context(),
            &DocumentEpoch::from("document-1"),
            &claim(),
            &[forged]
        ),
        VerificationStatus::Passed
    );
}

#[test]
fn malformed_ambiguous_or_stale_inputs_cannot_pass() {
    let doc = DocumentEpoch::from("document-1");
    let valid = fact();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[valid.clone(), valid.clone()]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.context.run_id = RunId::from("foreign");
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.context.contract_revision = 2;
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.captured_at_ms = 0;
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.effective_url = "https://supplier.test.attacker/catalog".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.effective_url = "https://user@supplier.test/catalog".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.excerpt = "x".repeat(aero_core::task::MAX_TEXT_BYTES + 1);
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let oversized = vec![valid; aero_core::task::MAX_ITEMS + 1];
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &oversized),
        VerificationStatus::Inconclusive
    );
}

#[test]
fn claims_require_owned_captured_sources() {
    let current = DocumentEpoch::from("document-1");
    let capture = fact();
    assert_eq!(
        assess_structured_claim(
            &context(),
            &current,
            &claim(),
            std::slice::from_ref(&capture)
        ),
        VerificationStatus::Passed
    );
    let mut wrong = claim();
    wrong.value = "43 USD".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Failed
    );
    wrong = claim();
    wrong.field = "delivery".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    wrong = claim();
    wrong.entity = "supplier-b".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    wrong = claim();
    wrong.evidence_id = EvidenceId::from("unknown");
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    let mut foreign = context();
    foreign.task_id = TaskId::from("other");
    assert_eq!(
        assess_structured_claim(&foreign, &current, &claim(), std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    assert_eq!(
        assess_structured_claim(
            &context(),
            &DocumentEpoch::from("document-2"),
            &claim(),
            &[capture]
        ),
        VerificationStatus::Inconclusive
    );
}
