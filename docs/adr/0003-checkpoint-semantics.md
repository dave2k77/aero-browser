# ADR 0003: durable intent, conservative recovery and partial browser restore

## Status

Accepted recovery semantics; implementation and crash/browser/keyring proof remain `not_started` in the [traceability matrix](../requirements.md). This ADR documents invariants, not executed recovery or future test results.

## Context

[Specification §11](../specification.md#11-persistence-recovery-and-truthful-reversibility), [SPEC-04–SPEC-13](../implementation-plan.md#3-required-corrections-to-the-supplied-specification) and N01–N03/N06/N19 correct snapshot-only recovery, competing checkpoint boundaries and false lossless/exactly-once claims. Durable Task intent is distinct from a run and transient Chromium state. SQLite transactions cannot atomically commit a remote website effect.

## Decision: application-owned durable state

- One SQLite database and one bounded blocking writer own tasks, immutable contract revisions, cumulative counters/reservations, run history, intents/outcomes, grants/approvals, criteria, evidence/artifact metadata and checkpoints. Use the [canonical store/step contract](../implementation-plan.md#75-initial-schema--cratesaero-agentsrcstoremigrations001_initialsql), not duplicated SQL in this ADR.
- `sessions` represent runs; Task is the durable owner. Resume, a new run, provider change or UI reconnect cannot reset action attempts, tokens/reservations, active execution time, recovery count, unresolved effects or history. Human-wait time is separate from active-time consumption. Unknown inference usage remains conservatively reserved until reconciliation; unknown price is not zero cost.
- Contract changes create trusted-user-only immutable revisions. Grant payload/issuer/task/revision are immutable; expiry, revocation and quota consumption remain durable authority checks. Revalidate them on recovery; do not recreate grants from conversation history or blindly restore an approval from an old snapshot. Materially affected approvals/grants require revalidation or renewed human decision.
- Reserve budget and a unique run-step slot in the writer transaction before external work. Persist prepared intent and increment the cursor/attempts once. Outcome/checkpoint/owned evidence/artifact metadata and status commit together afterward without a second cursor increment. Never infer the next step or counters solely from an older checkpoint.
- Schema mismatch, corruption, disk failure, ownership mismatch and missing encryption key are typed failures, not deserialization-to-empty successful state. Greenfield initial schema design does not require a fictitious upgrade migration; real later versions migrate transactionally.

## Decision: prepared-intent uncertainty

A persisted prepared intent can exist before dispatch, during connection loss, after a server effect or during an interrupted outcome transaction. Without durable proof distinguishing these points, recovery must mark it uncertain and the task/run `needs_review`. **No automatic repeat**, including the apparent intent-before-dispatch case: the durable record alone cannot prove the remote effect never happened.

Only a supported independent, policy-bound read-only reconciliation that identifies the actual resulting record may resolve a specific uncertain effect. Otherwise ask the user. Reconciliation is not a speculative retry or a mutating compensation. Required fixture crash proof keeps the independent server counter exactly 1 after an effect-before-commit crash. Unresolved effects block task completion regardless of planner confidence or a restored browser screenshot.

After a fully committed outcome, resume at the already-reserved next cursor, not the previous action. Atomic DB rollback must produce all-old or all-new metadata, never a mixed outcome/checkpoint/cursor. This provides durable local bookkeeping, not exactly-once remote transactions.

## Decision: safe runtime reconstruction and bounded lifecycle

The supervisor races every phase against monotonic deadlines, cancellation, disconnect and browser exit; recovery cannot depend on the hung future returning. Planning/read-only waits are promptly interruptible; pause blocks new dispatch and acknowledges only after bounded input has finished or is explicitly uncertain. Paused/human-wait states are not restart triggers.

On restart, cancel and join the entire old `BrowserRuntime` generation, including connection listeners, target/frame maps, storage/network/download managers and worker supervision. Rebuild all managers from retained configuration and new connections; replacing only a CDP pointer is forbidden. Invalidate runtime/document/observation epochs and stale element/context/target handles. Reobserve before automation; never send new requests through old managers.

Recovery attempts/backoff are task-cumulative and capped by the plan, including across resume/new runs. Default close pauses/flushes/reconciles and reaps owned processes; an in-process host crash can end execution. Optional background survival requires the separately proven service backend and explicit task grant, not a checkpoint flag.

## Decision: best-effort browser hydration

Application-owned contract/history/counters and browser state have different restore guarantees:

1. Load validated original Task/run/revision, latest durable journal/counters, eligible checkpoint and authorized tab/origin inventory. A dedicated resume route does not require a new goal, insert a duplicate task or navigate to an unrelated default URL.
2. Require authenticated decryption of sensitive snapshots with an available key outside Chromium. Bind encrypted payload to its version/session/step; fresh nonces and tamper failures are mandatory. No plaintext fallback. Browser-managed live profile files may still contain sensitive data; snapshot encryption does not encrypt the entire browser profile.
3. Restore context-scoped cookies and authorized origin/tab storage **before** application boot scripts, using bounded lifecycle/readiness and origin-guarded preload scripts. Remove restore scripts after confirmed use. Reject redirect/origin mismatch and do not leak one tab/origin's session storage into another.
4. Hydrate eligible forms through the shared exact frame/open-shadow locator strategy, verifying origin/route/tag/type/name/group identity, native setters/events and actual readback/application state. Preserve empty/false/radio-none and selections by value. Missing/ambiguous/changed fields are reported, never guessed. Bounded readiness replaces fixed sleeps.
5. Exclude secrets, hidden/file/disabled/readonly controls under the capture policy. File inputs require explicit reselection/reapproval. Never submit during hydration; an input handler that can cause effects remains a disclosed restore-permission risk rather than a promise of harmless replay.

No checkpoint restores arbitrary JS heaps, IndexedDB, passkeys, expired authentication, files or remote wizard/server state. Custom/closed-shadow/transformed cases are supported only where the plan supplies a tested adapter; otherwise report unsupported. Do not claim lossless web-session recovery.

## Decision: report partial restoration and truthful reversibility

Every recovery returns one typed `RecoveryReport` to CLI/workspace: restored task history; restored/skipped storage; restored/missing/ambiguous/rejected fields; authentication/key status; unsupported state; unresolved effects; safe next step and required user decisions. Partial browser hydration cannot be rendered as full task success. Browser reauthentication and unresolved commit review may require human control before fresh automation.

Classify reversible local, compensatable external, irreversible external and unknown effects. Offer local undo only for explicitly implemented support/postconditions. External compensation is a **new** authorized journaled effect, never history erasure or silent retry. Do not display universal Undo. Offline audit replay is side-effect free; opt-in live re-execution obtains fresh locators/current authorization and stops on divergence.

## Required future evidence

The [crash matrix](../implementation-plan.md#required-crash-matrix) owns all injection boundaries; the following exact planned proofs are entry points, not a claim that the matrix ran:

| Invariant | Planned proof |
|---|---|
| Immutable intent/run ownership and atomic journal | `crates/aero-agent/tests/tasks.rs :: contract_revision_requires_trusted_owner`, `new_run_preserves_task_identity_and_history`; `crates/aero-agent/tests/journal.rs :: schema_and_foreign_keys`, `step_and_checkpoint_are_atomic` |
| Cumulative reservations/counters | `crates/aero-agent/tests/budget.rs :: suite` (N03 individual names unspecified); `crates/aero-agent/tests/scenarios.rs :: task_budget_survives_new_run`, `resume_keeps_task_attempt_and_token_budget`, `recovery_budget_survives_resume` |
| Revalidated durable grants | `crates/aero-agent/tests/capabilities.rs :: expired_or_revised_grant_is_rejected`, `preview_change_invalidates_commit`, `approval_is_single_use` |
| No duplicate uncertain effect | `crates/aero-agent/tests/recovery.rs :: uncertain_effect_is_not_repeated`; browser variant `crates/aero-agent/tests/scenarios.rs :: kill_after_server_effect_before_commit` |
| Independent interruption and complete runtime rebuild | `crates/aero-agent/tests/recovery.rs :: hung_operation_is_interruptible`; `crates/aero-agent/tests/scenarios.rs :: browser_kill_restores_committed_state`, `all_managers_use_new_runtime` |
| JSON-safe exact forms and scope | `crates/aero-core/src/form.rs :: form_json_preserves_empty_and_false`; `crates/aero-agent/tests/scenarios.rs :: form_hydration_restores_exact_values`, `reordered_multiselect_uses_values`, `form_restore_refuses_wrong_origin`, `react_controlled_form_recovers`, `shadow_form_recovers`, `iframe_form_recovery_respects_origin` |
| Preboot storage and script cleanup | `crates/aero-agent/tests/scenarios.rs :: storage_exists_before_application_boot`, `restore_redirect_does_not_leak_state`, `restore_scripts_are_removed`, `session_storage_is_tab_scoped` |
| Encrypted snapshot failure modes | `crates/aero-agent/tests/vault.rs :: snapshot_ciphertext_hides_sentinels`, `tampered_or_wrong_session_snapshot_fails`, `missing_key_never_writes_plaintext`, `nonce_is_unique`; isolated native Secret Service provisioning gate |
| Resume/report/reversibility | `crates/aero-cli/tests/cli.rs :: resume_requires_no_goal`; `crates/aero-agent/tests/recovery_report.rs :: partial_restore_is_visible`, `irreversible_effect_has_no_undo`; `crates/aero-agent/tests/scenarios.rs :: partial_restore_requests_user` |
| No false completion and side-effect-free audit | `crates/aero-agent/tests/verification.rs :: model_complete_cannot_bypass_criteria`; `crates/aero-agent/tests/journal.rs :: offline_replay_has_no_side_effects` |

F10's journal-only guard precedes enabling E05 dispatch; its actual server/browser crash proof follows provisioned runtime support. F08's cancellation guard is likewise early even though full rebuild is G2. Native browser/keyring availability and CLI/workspace rendering/manual checks remain separate unexecuted gates. No recovery fidelity percentage, crash-test result or exactly-once guarantee is implied.
