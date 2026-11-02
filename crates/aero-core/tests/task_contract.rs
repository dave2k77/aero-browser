use aero_core::ids::{RunId, TargetId, TaskId};
use aero_core::task::{CoverageFraction, StartPrerequisites, TaskContract};
use serde_json::{Value, json};
use std::any::TypeId;

fn fixture() -> Value {
    json!({
        "schema_version": 1,
        "task_id": "supplier-research-fixture",
        "revision": 1,
        "title": "Compare approved suppliers",
        "objective": "Compare the two approved fixture suppliers and identify unknown lead times.",
        "profile_ref": "ephemeral-public-research",
        "account_refs": [],
        "allowed_origins": ["https://supplier-a.test", "https://supplier-b.test"],
        "allowed_routes": [
            {"origin":"https://supplier-a.test","path_prefix":"/"},
            {"origin":"https://supplier-b.test","path_prefix":"/"}
        ],
        "allowed_effects": ["navigate", "extract", "stage_artifact"],
        "forbidden_effects": ["account_create", "submit", "purchase", "upload"],
        "processing_mode": "local_only",
        "processor_grants": [],
        "source_classifications": [],
        "export_grants": [],
        "screenshot_transfer_allowed": false,
        "budgets": {
            "max_action_attempts": 30, "max_active_ms": 300000,
            "max_input_tokens": 20000, "max_output_tokens": 4000,
            "max_purchase_minor_units": 0, "budget_currency": "USD",
            "max_inference_minor_units": null
        },
        "criteria": [
            {"id":"csv-schema","kind":"artifact_schema","required":true,
             "artifact":"supplier-comparison","columns":["supplier","price","lead_time","source_url"]},
            {"id":"two-suppliers","kind":"row_count","required":true,
             "artifact":"supplier-comparison","expected":2},
            {"id":"source-coverage","kind":"evidence_coverage","required":true,
             "artifact":"supplier-report","required_fraction":1.0}
        ],
        "outputs": [
            {"name":"supplier-comparison","format":"csv","required":true},
            {"name":"supplier-report","format":"markdown","required":true}
        ],
        "dependencies": [],
        "stop_conditions": ["budget_exhausted", "permission_required", "uncertain_effect"],
        "unresolved_questions": [],
        "close_behavior": "pause",
        "retention": {"snapshot_days":7,"artifact_days":30,"memory_opt_in":false}
    })
}

fn parsed(value: Value) -> TaskContract {
    serde_json::from_value(value).expect("structurally valid synthetic contract")
}

#[test]
fn contract_is_strict_and_roundtrips() {
    let value = fixture();
    let contract = parsed(value.clone());
    assert_eq!(contract.task_id, TaskId::from("supplier-research-fixture"));
    assert_eq!(contract.revision, 1);
    assert_eq!(contract.validate(), Ok(()));
    assert_eq!(serde_json::to_value(&contract).unwrap(), value);
    let mut bad = value.clone();
    bad["role"] = json!("owner");
    assert!(serde_json::from_value::<TaskContract>(bad).is_err());
    for list in ["criteria", "outputs"] {
        let mut bad = value.clone();
        let duplicate = bad[list][0].clone();
        bad[list].as_array_mut().unwrap().push(duplicate);
        assert!(parsed(bad).validate().is_err(), "duplicate {list}");
    }
}

#[test]
fn task_and_run_ids_are_distinct() {
    assert_ne!(TypeId::of::<TaskId>(), TypeId::of::<RunId>());
    assert_ne!(TypeId::of::<TaskId>(), TypeId::of::<TargetId>());
    let contract = parsed(fixture());
    let id: TaskId = contract.task_id;
    assert_eq!(id.as_str(), "supplier-research-fixture");
}

fn rejects(value: Value) {
    match serde_json::from_value::<TaskContract>(value) {
        Err(_) => {}
        Ok(contract) => assert!(contract.validate().is_err()),
    }
}

#[test]
fn contract_rejects_invalid_semantics() {
    for (pointer, replacement) in [
        ("/revision", json!(0)),
        ("/revision", json!(u64::MAX)),
        ("/task_id", json!("")),
        ("/title", json!("   ")),
        ("/objective", json!("")),
        ("/profile_ref", json!("")),
        ("/criteria", json!([])),
        ("/outputs", json!([])),
        ("/criteria/0/id", json!("")),
        ("/criteria/0/columns", json!([])),
        ("/criteria/0/columns", json!(["supplier", "supplier"])),
        ("/criteria/1/artifact", json!("missing-output")),
        ("/criteria/1/expected", json!(u64::MAX)),
        ("/criteria/2/required_fraction", json!(0.0)),
        ("/criteria/2/required_fraction", json!(1.01)),
        ("/outputs/0/name", json!("../outside.csv")),
        ("/outputs/0/required", json!(false)),
        ("/allowed_effects", json!(["submit"])),
        ("/allowed_effects", json!(["navigate", "navigate"])),
        ("/forbidden_effects", json!(["purchase", "purchase"])),
        ("/processor_grants", json!(["remote-processor-grant"])),
        ("/screenshot_transfer_allowed", json!(true)),
        ("/dependencies", json!(["supplier-research-fixture"])),
        ("/dependencies", json!(["task-other", "task-other"])),
        ("/account_refs", json!(["", "account-2"])),
        ("/stop_conditions", json!([])),
        (
            "/stop_conditions",
            json!(["budget_exhausted", "budget_exhausted"]),
        ),
        ("/unresolved_questions", json!([{"id":"q","prompt":""}])),
        ("/retention/snapshot_days", json!(366)),
        ("/retention/artifact_days", json!(366)),
    ] {
        let mut value = fixture();
        *value.pointer_mut(pointer).unwrap() = replacement;
        rejects(value);
    }
    let mut value = fixture();
    for criterion in value["criteria"].as_array_mut().unwrap() {
        criterion["required"] = json!(false);
    }
    rejects(value);
    let mut value = fixture();
    value["outputs"][0]["format"] = json!("markdown");
    rejects(value);
}

#[test]
fn contract_budgets_are_bounded_and_money_is_explicit() {
    for key in [
        "max_action_attempts",
        "max_active_ms",
        "max_input_tokens",
        "max_output_tokens",
        "max_purchase_minor_units",
        "max_inference_minor_units",
    ] {
        for invalid in [json!(-1), json!(1.5), json!("1"), json!(u64::MAX)] {
            let mut value = fixture();
            value["budgets"][key] = invalid;
            rejects(value);
        }
    }
    for key in ["max_action_attempts", "max_active_ms"] {
        let mut value = fixture();
        value["budgets"][key] = json!(0);
        rejects(value);
    }
    for currency in ["", "usd", "US", "USDX", "1SD", "💵"] {
        let mut value = fixture();
        value["budgets"]["budget_currency"] = json!(currency);
        rejects(value);
    }
    let mut value = fixture();
    value["budgets"]["max_input_tokens"] = json!(i64::MAX);
    value["budgets"]["max_output_tokens"] = json!(1);
    rejects(value);
    for optional_money in [json!(null), json!(0), json!(i64::MAX)] {
        let mut value = fixture();
        value["budgets"]["max_inference_minor_units"] = optional_money;
        value["budgets"]["max_input_tokens"] = json!(0);
        value["budgets"]["max_output_tokens"] = json!(0);
        assert!(parsed(value).validate().is_ok());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 0.0, 1.1] {
        assert!(CoverageFraction::new(invalid).is_err());
    }
    assert_eq!(CoverageFraction::new(1.0).unwrap().get(), 1.0);
}

#[test]
fn contract_scopes_are_explicit_and_consistent() {
    for origin in [
        "",
        "*",
        "file:///tmp",
        "javascript:alert(1)",
        "https://user@example.test",
        "https://example.test/path",
        "https://example.test/",
        "https://example.test?q=1",
        "https://example.test#part",
        "https://*.test",
        "https://example.test:65536",
        "https://127.1",
        "HTTPS://example.test",
        "https://EXAMPLE.test",
        "https://example.test.",
        "https://example.test:443",
        "https://example.test:0444",
        "https://example.test:0",
    ] {
        let mut value = fixture();
        value["allowed_origins"] = json!([origin]);
        value["allowed_routes"] = json!([]);
        rejects(value);
    }
    let mut value = fixture();
    value["allowed_origins"] = json!(["https://supplier-a.test", "https://supplier-a.test"]);
    rejects(value);
    for origin in [
        "http://localhost:8000",
        "https://127.0.0.1",
        "http://[::1]:8000",
    ] {
        let mut value = fixture();
        value["allowed_origins"] = json!([origin]);
        value["allowed_routes"] = json!([]);
        assert!(parsed(value).validate().is_ok()); // Syntax, not SSRF/network authorization; no routes granted.
    }
    let mut value = fixture();
    value["allowed_routes"] =
        json!([{"origin":"https://supplier-a.test", "path_prefix":"/catalog"}]);
    value["source_classifications"] =
        json!([{"origin":"https://supplier-a.test", "classification":"public"}]);
    assert!(parsed(value.clone()).validate().is_ok());
    value["allowed_routes"][0]["origin"] = json!("https://elsewhere.test");
    rejects(value);
    for path in [
        "",
        "relative",
        "//host/path",
        "/a/../b",
        "/a/./b",
        "/%2fsecret",
        "/a?b",
        "/a#b",
        "/a\\b",
    ] {
        let mut value = fixture();
        value["allowed_routes"] = json!([{"origin":"https://supplier-a.test", "path_prefix":path}]);
        rejects(value);
    }
    let mut value = fixture();
    value["source_classifications"] =
        json!([{"origin":"https://elsewhere.test", "classification":"public"}]);
    rejects(value);
}

#[test]
fn contract_start_checks_do_not_invent_provider_or_background_support() {
    let available = StartPrerequisites {
        local_processor_available: true,
        background_service_available: false,
    };
    let unavailable = StartPrerequisites {
        local_processor_available: false,
        background_service_available: false,
    };
    let mut contract = parsed(fixture());
    assert!(contract.validate().is_ok());
    assert!(contract.validate_for_start(&unavailable).is_err());
    assert!(contract.validate_for_start(&available).is_ok());
    contract.close_behavior = aero_core::task::CloseBehavior::Background;
    assert!(contract.validate_for_start(&available).is_err());
    assert!(
        contract
            .validate_for_start(&StartPrerequisites {
                background_service_available: true,
                ..available
            })
            .is_ok()
    );
    let mut value = fixture();
    value["processing_mode"] = json!("approved_cloud");
    rejects(value.clone());
    value["processor_grants"] = json!(["processor-grant-1"]);
    assert!(parsed(value).validate_for_start(&unavailable).is_ok()); // Reference shape only, not grant issuance/validity.
    let mut value = fixture();
    value["unresolved_questions"] = json!([{"id":"question-1","prompt":"Which account?"}]);
    assert!(parsed(value.clone()).validate().is_ok());
    assert!(parsed(value).validate_for_start(&available).is_err());
}

#[test]
fn contract_nested_objects_require_fields_and_reject_arrays_and_duplicates() {
    let mut value = fixture();
    value["allowed_routes"] =
        json!([{"origin":"https://supplier-a.test","path_prefix":"/catalog"}]);
    value["source_classifications"] =
        json!([{"origin":"https://supplier-a.test","classification":"private"}]);
    value["unresolved_questions"] = json!([{"id":"q","prompt":"Which source?"}]);
    for pointer in [
        "",
        "/budgets",
        "/retention",
        "/criteria/0",
        "/criteria/1",
        "/criteria/2",
        "/outputs/0",
        "/allowed_routes/0",
        "/source_classifications/0",
        "/unresolved_questions/0",
    ] {
        let object = value.pointer(pointer).unwrap().as_object().unwrap();
        for key in object.keys() {
            let mut missing = value.clone();
            missing
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            assert!(
                serde_json::from_value::<TaskContract>(missing).is_err(),
                "missing {pointer}/{key}"
            );
        }
        for bad in [json!([]), json!(null), json!(1)] {
            let mut nested = value.clone();
            *nested.pointer_mut(pointer).unwrap() = bad;
            assert!(serde_json::from_value::<TaskContract>(nested).is_err());
        }
        let mut extra = value.clone();
        extra.pointer_mut(pointer).unwrap()["grant_authority"] = json!(true);
        assert!(serde_json::from_value::<TaskContract>(extra).is_err());
    }
    for version in [json!(0), json!(2), json!("1"), json!(1.0), json!(null)] {
        let mut bad = value.clone();
        bad["schema_version"] = version;
        assert!(serde_json::from_value::<TaskContract>(bad).is_err());
    }
    let mut contract = parsed(value.clone());
    contract.schema_version = 2;
    assert!(contract.validate().is_err());
    let raw = serde_json::to_string(&value).unwrap();
    for raw in [
        format!("{{\"schema_version\":1,{}", &raw[1..]),
        raw.replace(
            "\"max_inference_minor_units\":null",
            "\"max_inference_minor_units\":null,\"max_inference_minor_units\":1",
        ),
        raw.replace("\"required\":true", "\"required\":true,\"required\":false"),
        format!("{raw} {{}}"),
    ] {
        assert!(serde_json::from_str::<TaskContract>(&raw).is_err());
    }
    for kind in [
        json!([
            "artifact_schema",
            "id",
            true,
            "supplier-comparison",
            ["supplier"]
        ]),
        json!(["row_count", "id", true, "supplier-comparison", 2]),
        json!(["evidence_coverage", "id", true, "supplier-report", 1.0]),
    ] {
        let mut nested = value.clone();
        nested["criteria"][0] = kind;
        assert!(serde_json::from_value::<TaskContract>(nested).is_err());
    }
}

#[test]
fn contract_errors_do_not_echo_private_values() {
    let mut value = fixture();
    value["outputs"][0]["name"] = json!("/SYNTHETIC_PRIVATE_PATH");
    let error = parsed(value).validate().unwrap_err();
    assert!(!format!("{error} {error:?}").contains("SYNTHETIC"));
}

#[test]
fn contract_accepts_exact_bounds_and_rejects_overflow() {
    use aero_core::task::{MAX_ITEMS, MAX_REFERENCE_BYTES, MAX_TEXT_BYTES};
    for (pointer, limit) in [
        ("/title", MAX_REFERENCE_BYTES),
        ("/objective", MAX_TEXT_BYTES),
        ("/task_id", MAX_REFERENCE_BYTES),
        ("/profile_ref", MAX_REFERENCE_BYTES),
        ("/criteria/0/columns/0", MAX_REFERENCE_BYTES),
    ] {
        let mut value = fixture();
        *value.pointer_mut(pointer).unwrap() = json!("x".repeat(limit));
        assert!(parsed(value.clone()).validate().is_ok(), "exact {pointer}");
        *value.pointer_mut(pointer).unwrap() = json!("x".repeat(limit + 1));
        rejects(value);
    }
    let mut value = fixture();
    value["objective"] = json!("雪".repeat(MAX_TEXT_BYTES / 3));
    assert!(parsed(value.clone()).validate().is_ok());
    value["objective"] = json!("雪".repeat(MAX_TEXT_BYTES / 3 + 1));
    rejects(value);
    for key in ["account_refs", "dependencies"] {
        let mut value = fixture();
        value[key] = json!(
            (0..MAX_ITEMS)
                .map(|i| format!("ref-{i}"))
                .collect::<Vec<_>>()
        );
        assert!(parsed(value.clone()).validate().is_ok());
        value[key].as_array_mut().unwrap().push(json!("overflow"));
        rejects(value);
    }
    let mut value = fixture();
    value["criteria"] = json!((0..MAX_ITEMS).map(|i| json!({"kind":"evidence_coverage","id":format!("criterion-{i}"),"required":true,"artifact":format!("output-{i}"),"required_fraction":1.0})).collect::<Vec<_>>());
    value["outputs"] = json!(
        (0..MAX_ITEMS)
            .map(|i| json!({"name":format!("output-{i}"),"format":"markdown","required":true}))
            .collect::<Vec<_>>()
    );
    assert!(parsed(value.clone()).validate().is_ok());
    value["criteria"].as_array_mut().unwrap().push(json!({"kind":"row_count","id":"overflow","required":false,"artifact":"output-0","expected":1}));
    rejects(value);
    let mut value = fixture();
    value["budgets"]["max_action_attempts"] = json!(i64::MAX);
    value["budgets"]["max_active_ms"] = json!(i64::MAX);
    value["budgets"]["max_input_tokens"] = json!(i64::MAX);
    value["budgets"]["max_output_tokens"] = json!(0);
    value["revision"] = json!(i64::MAX);
    value["retention"]["snapshot_days"] = json!(0);
    value["retention"]["artifact_days"] = json!(365);
    assert!(parsed(value).validate().is_ok());
}

#[test]
fn contract_unknown_vocabularies_and_executable_checkers_are_rejected() {
    for (pointer, value) in [
        ("/allowed_effects/0", json!("execute_javascript")),
        ("/processing_mode", json!("fallback_to_cloud")),
        ("/criteria/0/kind", json!("script")),
        ("/outputs/0/format", json!("shell")),
        ("/close_behavior", json!("ignore_close")),
        ("/stop_conditions/0", json!("model_says_success")),
    ] {
        let mut bad = fixture();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(serde_json::from_value::<TaskContract>(bad).is_err());
    }
    for key in ["checker_code", "passed", "role", "issuer"] {
        let mut bad = fixture();
        bad["criteria"][0][key] = json!("synthetic");
        assert!(serde_json::from_value::<TaskContract>(bad).is_err());
    }
    let contract = parsed(fixture());
    assert_eq!(
        contract.criteria[0].verifier(),
        aero_core::task::VerifierKind::ArtifactSchema
    );
    assert_eq!(
        contract.criteria[1].verifier(),
        aero_core::task::VerifierKind::RowCount
    );
    assert_eq!(
        contract.criteria[2].verifier(),
        aero_core::task::VerifierKind::EvidenceCoverage
    );
}

#[test]
fn contract_cannot_omit_mandatory_stop_conditions() {
    for index in 0..3 {
        let mut value = fixture();
        value["stop_conditions"]
            .as_array_mut()
            .unwrap()
            .remove(index);
        assert!(parsed(value).validate().is_err());
    }
}

#[test]
fn local_export_intent_is_not_confused_with_remote_processing() {
    let mut value = fixture();
    value["export_grants"] = json!(["local-export-reference"]);
    value["allowed_effects"]
        .as_array_mut()
        .unwrap()
        .push(json!("export"));
    // Only a requested reference: the future gateway must verify a local recipient,
    // exact data/purpose/task/revision and trusted grant before any actual export.
    assert!(parsed(value).validate().is_ok());
}

#[test]
fn contract_processing_references_are_not_readiness_or_implicit_authority() {
    let unavailable = StartPrerequisites {
        local_processor_available: false,
        background_service_available: false,
    };
    let mut value = fixture();
    value["processing_mode"] = json!("sensitive");
    value["retention"]["memory_opt_in"] = json!(true);
    rejects(value.clone());
    value["processor_grants"] = json!(["sensitive-memory-exception"]);
    assert!(parsed(value.clone()).validate().is_ok());
    assert!(
        parsed(value.clone())
            .validate_for_start(&unavailable)
            .is_err()
    );
    value["screenshot_transfer_allowed"] = json!(true);
    assert!(parsed(value.clone()).validate().is_ok());
    value["processor_grants"] = json!([]);
    rejects(value);
    let mut local = fixture();
    local["allowed_routes"] = json!([]);
    assert!(parsed(local).validate().is_ok()); // No routes, not implicit whole-origin permission.
}

#[test]
fn documented_task_fixture_matches_the_compiled_contract() {
    let plan = include_str!("../../../docs/implementation-plan.md");
    let section = plan
        .split("### 7.10 TaskContract, criteria, data-flow and outcome contracts")
        .nth(1)
        .unwrap();
    let raw = section
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let value: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(value, fixture());
    assert!(parsed(value).validate().is_ok());
}

#[test]
fn contract_every_object_and_criterion_rejects_raw_duplicate_and_positional_fields() {
    use aero_core::task::{
        Budget, Criterion, OutputRequirement, Question, Retention, RouteScope, SourceClassification,
    };
    use serde::de::{DeserializeOwned, MapAccess, Visitor};
    use serde::{Deserialize, Serialize};

    struct Fields;
    impl<'de> Visitor<'de> for Fields {
        type Value = Vec<(String, Value)>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut fields = Vec::new();
            while let Some(entry) = map.next_entry()? {
                fields.push(entry);
            }
            Ok(fields)
        }
    }
    fn check<T: DeserializeOwned + Serialize>(value: Value) {
        let model: T = serde_json::from_value(value).unwrap();
        let raw = serde_json::to_string(&model).unwrap();
        let fields = serde::Deserializer::deserialize_map(
            &mut serde_json::Deserializer::from_str(&raw),
            Fields,
        )
        .unwrap();
        let positional = Value::Array(fields.iter().map(|(_, value)| value.clone()).collect());
        assert!(serde_json::from_str::<T>(&positional.to_string()).is_err());
        for (key, value) in fields {
            let key_json = serde_json::to_string(&key).unwrap();
            for first in [value.clone(), Value::Null] {
                let duplicate = format!("{{{key_json}:{first},{}", &raw[1..]);
                assert!(
                    serde_json::from_str::<T>(&duplicate).is_err(),
                    "duplicate {key}"
                );
            }
            let mut missing: Value = serde_json::from_str(&raw).unwrap();
            missing.as_object_mut().unwrap().remove(&key);
            assert!(serde_json::from_value::<T>(missing).is_err());
        }
    }
    let value = fixture();
    check::<TaskContract>(value.clone());
    check::<Budget>(value["budgets"].clone());
    check::<Retention>(value["retention"].clone());
    for criterion in value["criteria"].as_array().unwrap() {
        check::<Criterion>(criterion.clone());
    }
    check::<OutputRequirement>(value["outputs"][0].clone());
    check::<RouteScope>(value["allowed_routes"][0].clone());
    check::<SourceClassification>(
        json!({"origin":"https://supplier-a.test","classification":"private"}),
    );
    check::<Question>(json!({"id":"question-1","prompt":"Which source?"}));
    assert!(CoverageFraction::deserialize(json!("1.0")).is_err());
}

#[test]
fn contract_rejects_mapped_ipv6_origins_in_both_spellings() {
    for origin in ["https://[::ffff:192.0.2.1]", "https://[::ffff:c000:201]"] {
        let mut value = fixture();
        value["allowed_origins"] = json!([origin]);
        value["allowed_routes"] = json!([]);
        assert!(
            parsed(value).validate().is_err(),
            "mapped IPv6 is unsupported: {origin}"
        );
    }
    let mut value = fixture();
    value["allowed_origins"] = json!(["https://[2001:db8::1]"]);
    value["allowed_routes"] = json!([]);
    assert!(parsed(value).validate().is_ok());
}

#[test]
fn contract_required_row_counts_must_agree_per_output() {
    for (required, expected, valid) in [(true, 3, false), (true, 2, true), (false, 3, true)] {
        let mut value = fixture();
        value["criteria"].as_array_mut().unwrap().push(json!({
            "kind":"row_count", "id":"another-count", "artifact":"supplier-comparison",
            "required":required, "expected":expected
        }));
        assert_eq!(parsed(value).validate().is_ok(), valid);
    }
    let mut value = fixture();
    value["outputs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"other-table","format":"csv","required":true}));
    value["criteria"].as_array_mut().unwrap().push(json!({
        "kind":"row_count", "id":"other-count", "artifact":"other-table", "required":true, "expected":3
    }));
    assert!(parsed(value).validate().is_ok());
}
