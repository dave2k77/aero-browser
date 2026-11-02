# AeroBrowser — implementation handoff

Document-set revision: 2.20. Current checkpoint: chain batch CH-5 below (uncommitted, ready for review). Everything through fa09ba9 (CH-4) is committed and pushed. Prior checkpoints below are historical where their next actions or test counts differ.

## Current chain batch CH-5 checkpoint

CH-4 was approved, committed as fa09ba9 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-5.

CH-5 — coordinated task deletion (N16), uncommitted, per DEC-010 (in-place schema) and DEC-011 (tombstone):

- Schema: `retention_holds(task_id, id, created_at)` cascading with the task; `task_tombstones(task_key, state, created_at, updated_at)` with no task ID or content, permanent (no delete), immutable key/created time, state only `deleting` → `complete`, no replacement. Schema identity test now expects 14 tables.
- `crates/aero-agent/src/store/worker.rs`: `JournalClient::delete_task` is now the journal half — one transaction refuses Held (any hold) or NeedsReview (unresolved work), writes the `deleting` tombstone and deletes the task's records; repeating while deleting is Ok, after completion Deleted. `create_task` and `place_hold` refuse tombstoned IDs with the new `StoreError::Deleted`. Crate-private `complete_deletion`, `pending_deletions` (bounded 1024), `place_hold`, `release_hold`. New `StoreError::Held`.
- `crates/aero-agent/src/retention.rs`: crate-private `purge_task_files(&TaskLease)` removes everything except `controller.lock`, descriptor-relative, unlinking links without following them, refusing other-device directories, bounded to 100,000 entries and depth 32, then syncs the task directory. `config.rs`: `acquire_task_key` (by hashed key) and crate-private `TaskLease::shell`.
- `crates/aero-agent/src/tasks.rs`: `TaskAuthority::delete_task(&state, &task)` = retire gateways → nonblocking task lock (Busy, nothing changed, while any lease/store/runtime is live) → journal half → purge → complete; `resume_deletions(&state)` finishes `deleting` tombstones; owner-only `TaskClient::place_hold` / `release_hold`.
- Tests in new binary `crates/aero-agent/tests/deletion.rs`: purge of artifacts, evidence, runtime profile/staging and an outward symlink (target preserved) with the other task intact and the tombstone holding only key/state/timestamps; holds and a prepared action block deletion before it starts, and holds cannot be placed afterwards; journal-half-only deletion resumed after a journal restart, idempotently. All three observed red against stubs, then green. The old SQL-only test comment now describes the journal half.
- Gates: workspace 246 passed (233 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass; 15 repeated runs of the deletion, retention and task suites passed. Not independently reviewed.

Limits: deletion is blocking (host thread). Gateways are retired even if deletion is then refused. Filesystem leases do not consult tombstones: a later `acquire_task` for a deleted ID can create files, but the journal refuses all its writes and `delete_task` returns Deleted; such files are not purged automatically. Unlinking is not secure erasure, and exported/remote copies are not recalled. Tasks with unresolved effects cannot be deleted until reviewed (there is no reconciliation API yet). The hashed key is pseudonymous.

Next: CH-6 N05 atomic completion gate.

## Previous chain batch CH-4 checkpoint (committed fa09ba9)

CH-3 was approved, committed as 7f88df2 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-4.

CH-4 — gateway retirement wired to the run lifecycle (N13 slice), uncommitted:

- `crates/aero-agent/src/data_flow.rs`: retirement logic extracted to `retire_state` (unchanged behaviour: closes admission, waits for an admitted send, detaches state even after poison, drops transports after unlocking); new crate-private weak `GatewayLink` from `GatewayHost::link`.
- `crates/aero-agent/src/tasks.rs`: `TaskAuthority::register_gateway(&host)`; an owner-only registry (`Access::Owner` now carries it, model clients do not) with at most `MAX_GATEWAYS_PER_TASK` = 64 live links per task, dropped gateways pruned. The owner's `start_run` and `revise_contract` retire all registered gateways for the task, outside the registry lock, before submitting to the journal.
- Tests in new binary `crates/aero-agent/tests/gateway_lifecycle.rs` (kept apart from the data_flow suite, which spawns child processes): new-run retirement observed before the commit reply is awaited, other tasks unaffected and a new gateway for the new run works; contract revision retires; model start/revise are denied without retiring; registration bound and pruning. All three observed red against a no-op registration (after fixing a fixture that reused a journal-wide run ID), then green.
- Gates: workspace 243 passed (230 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: retirement happens before submission and is not undone if the journal rejects the request (fail closed: create a new gateway). It cannot recall a send already in flight and blocks on a stuck transport. Only gateways registered with the same `TaskAuthority` are covered; future N09 close and N16 deletion must use the same retirement. Still no real transport (DEC-004) or durable grants (N06).

Next: CH-5 N16 coordinated deletion with the DEC-011 tombstone.

### Earlier: chain batch CH-3 checkpoint (committed 7f88df2)

CH-2 was approved, committed as 3a9c3ce and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-3.

CH-3 — durable evidence capture (N17 slice), uncommitted:

- `crates/aero-agent/src/artifacts.rs`: the store now has a name namespace and subdirectory (`open_namespace`), `stage_with` for content built from the generated name, `check_evidence_owner`; artifact behaviour is unchanged. `crates/aero-agent/src/config.rs`: `artifact_directory` generalised to `task_subdirectory(name)`.
- `crates/aero-agent/src/evidence.rs`: `FactCapture`, `EvidenceStore::{open, capture, load}`, `StagedEvidence::record`. Capture validates the same bounds the comparison uses (scoped URL, nonempty excerpt, bounded text, nonzero time), refuses Unknown/Sensitive (no encryption yet), and writes strict versioned JSON embedding the generated `e1-` ID and run context. Load reads back against the journal record, requires an object, exact version/ID, and the record's run context (else OwnershipMismatch).
- `crates/aero-agent/src/store/worker.rs`: `EvidenceRecord` gains `bytes`; commit attachments and checkpoint loads require task-owned `e1-` IDs; new `JournalClient::load_evidence(task, id)`. Evidence is inserted inside the existing outcome transaction, so it commits atomically with the step.
- Schema (DEC-010, in place): evidence rows refuse update, replacement and individual deletion.
- Tests in `crates/aero-agent/tests/evidence.rs`: `journal_evidence_records_must_name_task_captures` observed red (fake ID accepted), then red again at the raw UPDATE, then green after the triggers; `captured_facts_round_trip_through_the_journal_and_store` was written together with the implementation, so it is not red/green evidence. Journal/worker fixtures now use well-formed `e1-` IDs with a byte length; the payload-corruption unit test drops the matching trigger to simulate corruption below SQL.
- Gates: workspace 240 passed (227 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: nothing yet produces captures from a real authorized observation (needs D01); Public/Redacted are trusted host assertions; the existing caller-constructible `CapturedFact` comparison remains for synthetic use and must not be fed caller facts by N05, which must load facts through `load_evidence` + `EvidenceStore::load`. No evidence reconciliation (orphaned captures) yet.

Next: CH-4 N13 gateway retirement wired to journal task cancellation.

### Earlier: chain batch CH-2 checkpoint (committed 3a9c3ce)

CH-1 was approved, committed as c2aa4a2 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-2.

CH-2 — artifact reconciliation (N18 slice), uncommitted:

- `crates/aero-agent/src/store/worker.rs`: `JournalClient::load_artifacts(task)` returns the task's committed artifact records ordered by ID; more than `MAX_ARTIFACT_INVENTORY` (1024) is BudgetExceeded, never a truncated list. Rows are read with a 4 KiB per-row bound and validated by the same `checked_artifact` used by `load_artifact`.
- `crates/aero-agent/src/artifacts.rs`: `ArtifactStore::reconcile(&records)` returns `ArtifactReconciliation { verified, missing, mismatched, unrecorded, unexpected_entries }` with `is_consistent()`. Every recorded file is read back against its record; unrecorded means a regular file with this task's generated-name shape but no record; anything else (foreign/malformed names, directories, links) is unexpected. Records must belong to this task and be unique; more than 4096 directory entries is BudgetExceeded. Listing uses the live owned directory descriptor via /proc/self/fd, as the store opener already does. Nothing is deleted, repaired or re-recorded.
- Tests in `crates/aero-agent/tests/artifacts.rs`: `reconciliation_reports_unrecorded_missing_and_mismatched_files` observed red (empty inventory against the scaffold) then green; `inventory_limits_fail_closed_instead_of_truncating` was added after the implementation and passed on its first run, so it is coverage rather than red/green evidence.
- Gates: workspace 238 passed (225 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: the report is a snapshot; run it under the task lease while nothing is staging and no step is prepared, otherwise an in-flight stage appears as unrecorded. Hashing reads every recorded file (up to 16 MiB each). Cleanup of unrecorded or unexpected entries is N16's job. N18 still lacks the bundle manifest and the export grant.

Next: CH-3 N17 sealed capture interface and evidence rows bound into the outcome transaction.

### Earlier: chain batch CH-1 checkpoint (committed c2aa4a2)

Authorization (2026-11-02): the owner asked to resolve the blocked prerequisite chain (N05 ← N17/N18 + DEC-009; N17/N18 ← capture, N13, N16, schema). Decisions taken: DEC-009 accepted as no attestation; DEC-010 edit the unshipped v1 schema in place (no AeroBrowser state directory existed on the host, so no data is affected); DEC-011 minimal permanent tombstone, holds cannot stop a started deletion. Scope order: resolve all non-browser prerequisites in verified batches, then start C01–C04/D01 for real capture. The owner approved and the reviewed backlog was committed as seven per-package commits (each verified to build, test, lint and format on its own) and pushed: 0e7165b..5af0e9d.

CH-1 — journal-backed artifact records (N18 slice), uncommitted:

- `crates/aero-agent/src/artifacts.rs`: `StagedArtifact::record(context)`; `ArtifactStore::verify(&ArtifactRecord)` reads the file back against the journal record's task, length and SHA-256; shared `check_artifact_owner` for the `a1-<task key>-<nonce>` ID shape.
- `crates/aero-agent/src/store/worker.rs`: commit attachments and checkpoint loads require store-generated IDs owned by the run's task (malformed → InvalidInput, other task → OwnershipMismatch, persisted mismatch → CorruptDatabase); new `JournalClient::load_artifact(task, id)`.
- `crates/aero-agent/src/store/migrations/001_initial.sql` (in place per DEC-010): artifact rows cannot be updated, replaced or individually deleted; task cascade still removes them.
- Tests: three new integration tests in `crates/aero-agent/tests/artifacts.rs`, each observed red (journal accepted `artifact`; lookup unimplemented; raw UPDATE succeeded) then green. Journal/worker fixtures now use well-formed owned IDs; the payload-corruption unit test drops the new trigger to simulate corruption below SQL.
- Gates: `cargo test --workspace --locked --offline` 236 passed (223 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: readback proves byte identity with the record, not correctness, relevance, classification or export permission. Orphaned staged files and records whose file is missing are detected only per record; no inventory reconciliation yet. Existing development journals with the previous schema are refused by the exact-schema check (expected under DEC-010).

Next batches (proposed, in order): CH-2 artifact reconciliation; CH-3 N17 sealed capture interface and evidence rows bound into the outcome transaction; CH-4 N13 retirement wired to journal cancellation; CH-5 N16 coordinated deletion with tombstone; CH-6 N05 atomic completion gate; then C01–C04/D01.

## Previous checkpoint (2026-10-28 stop)

Historical (revision 2.15): Session stopped 2026-10-28 16:18 +08 at the owner's request. N05, N13, N16, N17 and N18 remain blocked as complete packages; each has partial foundations described below. The recommended next batch is a preserving schema-evolution design and implementation tested only on disposable databases, subject to the owner's go-ahead. Do not migrate, reset or delete real user data; do not stage, commit or push without separate explicit approval. Browser/runtime/native qualification remains unverified. Prior checkpoints below are historical where their next actions or test counts differ from this current checkpoint.

## Next session: shared storage prerequisite

The most useful shared infrastructure dependency is safe journal schema evolution. `crates/aero-agent/src/store/mod.rs` currently accepts versions 0 and 1 and validates the exact supported schema; prior edits to the unshipped v1 schema can make older development databases fail to open. Evidence provenance, artifact reconciliation and durable deletion state need a way to evolve storage without losing existing records. The recommendation is to design and implement a preserving v1-to-v2 path using disposable fixture databases only, with explicit schema identity/version checks, preservation of representative task/run/journal records, transactional rollback on injected failure, unsupported-schema refusal, foreign-key/integrity checks and tests for interrupted or repeated attempts. First inventory the distinct v1 schemas actually supported: do not call every historical development schema valid or promise a migration from unknown versions without verifying their identities. Define backup/recovery and the opt-in boundary before any real migration. No user database operation was authorized by this recommendation.

This is a proposed next batch, not a completed migration or authorization to begin it. Obtain the owner's go-ahead before implementation; applying any migration to existing user databases is a separate decision. Schema evolution alone will not complete N05: N17 still needs a trusted real observation/capture owner, N18 needs verified artifact repository/readback and reconciliation, and any subjective attestation needs explicit permission and attribution. Finalization must atomically recheck accepted revision, run, unresolved work and trusted proof identities. N13 still needs real adapter/source/grant/lifecycle integration; N16 still needs trusted holds, coordinated admission, safe cleanup/recovery and privacy/cutover decisions. The retention design is in the N16 section of [the implementation plan](implementation-plan.md).

At stop, local branch `main` is at `af88105`; the workspace has substantial prior modified/untracked work and nothing staged. Preserve it. Most recent full workspace execution: `cargo test --workspace --locked --offline` passed 220 behavior tests and 13 compile-fail doctests, zero failures/ignored; format, warning-denied Clippy, offline build and rustdoc passed. The contributor documentation checker passed earlier in the session; a separate local link/anchor check was blocked by a consent timeout and was not retried. `git diff --check` passed after the checkpoint. No tests were rerun for this handoff-only edit. N13's latest poison/cycle repair passed its regression but has not been independently re-reviewed.

## Current N13/N16 work in progress

The owner said “Proceed with N13 and N16 as required.” N13 now has an in-memory host-issued gateway and fake-recipient tests; it is not connected to real adapters, a durable grant owner or trusted source capture. The parent reproduced two authorization/lifecycle gaps (retained handle after host drop and task-content telemetry), added red/green regressions and closed both in this slice; payload bytes and identity counts are also bounded. Independent review identified unscoped purposes and missing retirement: the parent added purpose-scoped handles and explicit irreversible retirement with red/green tests after review. A follow-up review found shutdown resource-retention and reentrant destructor deadlock defects; the parent reproduced both with bounded tests and fixed cleanup to detach values under lock and drop them after unlock, even after poison. A further review reproduced retention after poisoned retirement and a transport/host strong-reference cycle that prevents automatic host drop. The parent added a red/green regression: retirement now detaches resources even when poisoned while still returning `Unavailable`; the host must explicitly retire and transports must not retain strong host references. This last fix has not been independently re-reviewed. Size limits bound encoded lengths rather than caller-owned allocation capacity. Retirement is not wired to journal cancellation and cannot interrupt an in-flight send. N16 has a filesystem-only preflight that retains an existing exclusive task lock but always reports unknown hold/journal status and cannot delete or authorize deletion. Independent review found no false deletion eligibility in that scope; later cross-process tests cover TaskLease/preflight exclusion in both directions. A synthetic test confirms the existing `JournalClient::delete_task` removes SQL only while an artifact writer remains active; it must not be exposed as complete deletion. The test and API docs describe this limitation, not a repair. A proposed crash-safe coordinated deletion design and its unresolved privacy/hold/cutover decisions are recorded in the N16 plan. N13/N16 are blocked as complete packages. The current workspace passes 220 behavior tests and 13 compile-fail doctests; fmt, warning-denied Clippy, build, rustdoc and tracked diff whitespace checks also pass. Parallel fixture process spawning caused intermittent Busy on reopened locks; N16 retention and existing budget test binaries now serialize independent fixture tests while retaining within-test and cross-process assertions. Stress runs passed 40 retention suites and 20 budget suites before the last added retention test. Preserve all existing databases and user data; no migration/reset, real task-data deletion, Git staging, commit or push is authorized. N17/N18 and N05 remain blocked.

## Current N17/N18 blocked checkpoint

The owner said “Proceed with N17 and N18.” Independent design audits found no live observation owner, real evidence source or artifact repository, and existing journal evidence rows are digest-only; exact-schema checks reject in-place DDL changes to old databases. N17 has an explicitly non-authoritative, in-memory exact structured-value comparison in `crates/aero-agent/src/evidence.rs`: a scratch harness observed its planned named test red against an Inconclusive scaffold and green after implementation. It is not a repository, authorized capture, durable provenance, secret redaction, actual artifact readback or completion proof; `CapturedFact` is caller-constructible and must not be accepted as authority. N18 has a task-lease-bound, create-only private byte store with bounded readback and digest checks in `crates/aero-agent/src/artifacts.rs`. `Public`/`Redacted` are trusted-host assertions, not classification or redaction; `Unknown`/`Sensitive` fail before writing. The store is not an atomic bundle, journal-backed, crash-recoverable, encrypted, retention-managed, exported, or evidence-verified artifact repository. An independent adversarial review found no remaining confirmed defect in its reviewed snapshot; the reviewer reproduced the plaintext-write issue before the parent added the Unknown/Sensitive refusal. The review also identified absent-excerpt and conflicting-capture cases; the parent added tests and made those inputs inconclusive afterward, without a second independent review. Incomplete writes may leave private orphan files and aggregate storage is not bounded. N13 policy, N16 retention, real observation capture and a preserving schema/migration design remain necessary. No database reset, migration, N05 completion, attestation, publication or export is authorized.

Verification: final workspace `cargo test --workspace --locked --offline` passed 191 behavior and 12 compile-fail tests, with zero failed or ignored. Formatting, warning-denied Clippy, offline build, warning-denied rustdoc, documentation checker and tracked-diff whitespace checks passed. No changes staged or committed.

Historical next-step note: N13 and N16 were subsequently authorized and partially implemented; see the current checkpoint above. A preserving schema strategy and real observation ownership remain necessary. Do not migrate a user's database or claim N17/N18 complete merely from these foundations.

## Current N05 checkpoint

The owner authorized N05. The pure `completion_gate` predicate was first tested against an always-false implementation (the all-passed case failed), then implemented and the exact focused test passed. A separate regression records a model Complete proposal through the real journal and verifies the task remains running with a prepared unresolved intent, no artifact and no criterion records. This is not an orchestrator test, an inconclusive-criterion test, or proof of an implemented completion gate. An earlier assertion incorrectly expected paused; actual prepare changes the task to running, and the assertion was corrected. The final workspace gate passes 181 behavior and 12 compile-fail tests; format, warning-denied Clippy, build, rustdoc and the documentation checker pass.

Independent design review confirmed that the plan schedules N17 evidence and N18 artifact repositories before the full N05 gate. Existing evidence/artifact journal records only contain IDs, context, hashes and byte counts; they cannot establish output-name binding, source relevance, freshness or actual artifact readback. TaskContract has no subjective criterion/attestation permission, attributed user decision, or decision-evidence record. N04 reports are forgeable data, not verifier capabilities. The current resume view is a snapshot, so a separate check-then-complete write would race new work or revision changes. No production completion transition, attestation path or synthetic production success has been added.

Before enabling completion, implement the missing N17/N18 infrastructure and obtain authorization for a trusted attestation contract. N05 must bind trusted results to the persisted accepted task/revision and issued artifact identity; revalidate proof freshness, current run, task-wide unresolved actions/reservations and budget consistency atomically with task/run completion. External readbacks themselves cannot be made atomic by SQLite; bind immutable/versioned captures and recheck their identities. Ensure later admission cannot reopen or overwrite a completed run. Preserve existing databases; N17/N18 were subsequently authorized and partially implemented, but no schema reset, database migration, staging, commit or push has been approved. The pure predicate is a boolean helper, not an authorization boundary.

## Current N04 checkpoint

Owner authorized N04 independent verification. The fixed registry compares host-installed readbacks for navigation, nonsecret field state, boolean secret state, ordered artifact schema, row count and evidence-reference presence; unsupported checks and unavailable, malformed or wrong-scope readbacks are inconclusive. Readback requests carry identities but not expected values. Reports are versioned, bounded after decoding and carry status, method, references, time and static reason. Test-local dispatcher and readback show acknowledged wrong state fails and matching state passes. A compile-fail example checks the restricted TaskClient has no criterion-result writer. Independent source review found the original dispatch test did not call a dispatcher; the parent added and ran the test-local dispatch path. Latest workspace: 179 behavior and 12 compile-fail tests pass, including seven N04 integration tests; fmt, Clippy, build, rustdoc and documentation checks pass.

Critical limits: the registry is in-process host-owned infrastructure, not a sandbox; adapters must establish ownership/freshness and reject raw secret reads. There is no real readback adapter, accepted-output-name-to-artifact binding, evidence relevance/coverage, persisted criterion provenance, N05 completion gate, or untrusted postcondition ingress. Postconditions have no version envelope, and limits apply after deserialization; any future ingress must enforce a byte cap and version first. An independently reproduced forged Passed/Matched report validates structurally; decoded DTOs and raw host JournalClient StepOutcome reports must never be treated as verifier provenance. The N05 controller must bind the accepted task/revision/criterion, trusted verifier and artifact identity and independently check freshness, unresolved effects and required outputs before completion. An evidence-reference presence pass is not evidence coverage or truth. URLs and IDs may themselves be private; outputs require privacy policy, not just structural validation. N03's schema/performance caveats remain below. No N05, staging, commit or push is authorized.

## Current N03 checkpoint

The 2026-10-26 N03 session was interrupted after implementation began. The owner subsequently approved finishing the work and its review. N03 adds task-wide work/action reservations, pricing and separate purchase accounting, an injected active timer, and storage/recovery tests. Independent review found action overrun reports could be overwritten by smaller settlements and spent counters were not reconciled to durable history. The parent reproduced both; action overruns now retain uncertain holds and action settlement records reconcile spent counters. Focused regressions pass, including restart and injected rollback for overrun, and counter/receipt mutation tests. This is local storage/accounting evidence, not application acceptance.

Independent review also found attachment insertion could roll back overrun quarantine, settled work receipts could be read despite inconsistent totals, and overrun amounts were discarded. The attachment-conflict regression was observed red then green; work receipts now reject inconsistent totals and settled work records have an immutability trigger. Overrun amounts are retained separately from accepted settlement and survive restart in focused tests. Further review found malformed attachments blocked quarantine, unexpected tokens could still report a zero inference upper bound, and overrun reports/work records could be individually erased. Parent observed red/green regressions and added attachment-independent quarantine, conservative unknown inference, immutable reports and task-owned delete guards. Attachment validation errors for non-overruns are now delivered through the writer reply; malformed attachments are discarded on overrun. The final independent review verified those repairs but identified invalid outcome metadata blocking quarantine and a potential SQLite replacement bypass. The parent reproduced both as failing tests, fixed them, and reran the complete workspace: 172 behavior tests and 11 compile-fail doctests pass; format, warning-denied Clippy, build and rustdoc pass. Invalid outcomes on overrun are omitted rather than fabricated; invalid attachments are discarded, and non-overrun metadata errors are returned by the writer. Replacement is refused even when recursive triggers are off. The integrating parent reviewed and tested these last repairs; they were not independently re-reviewed after the changes.

New indexes use task/state searches, but full settled-history reconciliation still scales with lifetime history and has no hard deadline; performance and bounded retention remain open. ID-ordered history pages are not a concurrent snapshot and can miss inserts before the cursor. An actual SQL/I/O failure can roll back quarantine: the host must stop dispatch and reconcile rather than retry a smaller report.

The unshipped v1 schema was changed again; older development databases fail exact-schema identity checks. There is no automatic migration or approved deletion/reset. Preserve existing data. Accounting records are trusted host inputs, not grants or proof of provider billing; real provider/browser dispatch, active deadline enforcement and native qualification remain unimplemented. At the N03 checkpoint, N04, staging, commit and push had not yet been authorized; N04 was subsequently authorized as recorded above.

## Current N02 checkpoint

Authorization: the user said, “Awesome, let's continue with N02.” This covers the host-issued task capability boundary, immutable revision/run ownership and its tests, not N03 budgeting, N06 effect-grant issuance, N08 adapter authentication/controllers, browser/provider execution or publication.

Implemented paths: new `crates/aero-agent/src/tasks.rs` and `crates/aero-agent/tests/tasks.rs`; module export in agent `src/lib.rs`; scoped worker visibility/start/status/read-view changes and static errors in `src/store/worker.rs` and `src/store/mod.rs`. Existing journal integration fixtures explicitly route task mutations through a real TaskAuthority owner client; private worker fixtures supply synthetic StartPrerequisites and test denial against a full queue. Config, initial SQL schema, core/CLI/audit code, manifests/lock, pin and specification remain byte-for-byte preserved. No dependency or schema change.

Behavior: TaskAuthority is privileged host bootstrap bound to one Journal. It issues immutable, non-wire TaskClient owner or model-read-only capabilities; model clients cannot create/revise/start tasks and can read only the host-selected task. Persisted run/task/revision checks reject forged contexts. Raw JournalClient task creation/revision/start methods are crate-private. Owner writes reuse B05 exact-next immutable revision transactions, grant/approval invalidation, history and cumulative budget preservation. Start validates the stored contract and host prerequisites in the same transaction before retiring/inserting runs. Accepted intent stays draft; a new run record starts paused, not browser-ready. ResumeState now reports current persisted run status separately from historical checkpoint status.

Evidence:

- Required `cargo test -p aero-agent --test tasks contract_revision_requires_trusted_owner -- --exact` executed and failed on model revision being accepted against the permissive guard scaffold, then passed after owner checks. `start_checks_persisted_contract_prerequisites_without_writes` likewise failed Ok versus StartBlocked before persisted transaction-local checks. `intent_and_run_records_do_not_claim_execution_readiness` failed ready versus draft before correcting draft/paused bookkeeping. Scratch red logs use `aero-n02-*-red.log` and `aero-n02-readiness-red.log`.
- Nine task integration tests pass: owner/model denial, immutable history and grant invalidation, cross-task/forged run/revision reads, concurrent revision sequencing, cumulative history across runs/reconnect, persisted start gates and rollback, journal-bound scope/close behavior, invalid-model payload denial before closed-queue admission, and lost start acknowledgements. Compile-fail tests reject deserialized/externally constructed clients and direct raw task writes. The earlier bounded-worker test also verifies model denial takes precedence over QueueFull.
- Final workspace: 143 behavior tests and eleven compile-fail doctests pass; no failed/ignored/filtered tests. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc pass. Agent tests include 13 unit, 34 journal, nine task integration and three compile-fail tests. Final logs are `aero-n02-final-gate-0.log` through `-4.log`. Documentation/link/traceability, source-preservation and whitespace/index checks pass.
- Design review confirmed the boundary and identified readiness/status-view clarifications; draft/paused state, current_status and explicit privilege/lifetime documentation cover them. Independent source review reported no actionable N02 defects and independently ran the then-current seven task tests plus existing agent suites/Clippy. Parent subsequently ran all nine task tests and final workspace gates. No review finding remains outstanding.

Limits: factory possession is host-root composition, not proof of human/peer authentication. Never give a model adapter TaskAuthority, Journal, raw JournalClient, owner clients or an authority-minting callback. Scope is journal plus stable TaskId, not user/revision/run or revocable multi-user login; clones intentionally retain reads across revisions/new runs. Delete/recreate of a delegated ID within the same journal lifetime would reuse scope, so the host must not reuse IDs for unrelated work. Closing the Journal ends admission; accepted requests can survive lost replies and require reconciliation. Start prerequisites are trusted observations, not actual provider/background readiness, dependency completion, grants or launch. Scoped history is private data, not authorized provider egress. B06 leases remain independent filesystem ownership; runtime controllers, effect permissions, monetary budgets, encryption and N13 transfers remain later owners. ENV-002/ENV-004 unchanged.

Stop boundary: N02 is ready_for_review, uncommitted. No staging/commit/push, N03, browser/provider/account operation or new service process. Next separately approved batch is N03 cumulative reserved budgets (`crates/aero-agent/src/budget.rs`, `crates/aero-agent/tests/budget.rs`, writer integration): concurrent reservations, active-versus-paused time, known/unknown pricing and separate purchase limits, preserving B05 counters. Historical checkpoint sections below are evidence, not current next-action instructions.

## Current B06 checkpoint

Historical checkpoint: N02 above supersedes this section's next-action/status text; B06 evidence is retained.

Authorization: the user said, “Fantastic, proceed to B06.” Scope is private state/task/run/runtime layout and cooperative lifetime controller locks, not N02 roles, actual browser/input ownership, encryption, cleanup or publication.

Implemented paths: new `crates/aero-agent/src/config.rs`; scoped changes to agent `src/lib.rs`, `Cargo.toml`, `src/store/mod.rs`, `tests/journal.rs`, and `Cargo.lock`. The root manifest, worker implementation/tests, initial SQL schema, core/CLI/audit sources, browser pin and specification are preserved against the B06 baseline. Agent adds an edge to existing locked sha2 0.10.9; no package/version change is needed. Shared filesystem helpers retain B04's final-component-only open contract while supporting safe recursive configuration creation and exclusive runtime allocation.

Behavior: pure OS-native XDG_STATE_HOME or HOME/.local/state resolution, private `aerobrowser` root, separate journal/tasks directories, exact-byte domain-separated task/run/epoch path keys and a persistent per-task controller.lock. Owned 0700 directories and 0600 single-link regular locks are required; existing permissions are refused, not repaired. Runtime epochs allocate exclusively and never reuse earlier or partially created profiles/staging. RuntimeDirs retains its owner so dropping TaskLease cannot unlock while derived runtime handles remain. Drop never deletes files or resets task counters.

Verification and review:

- Required `cargo test -p aero-agent --test journal private_state_and_task_lock -- --exact` ran against the compilable no-lock scaffold and failed on duplicate same-task acquisition instead of Busy. The same test passes after lifetime flock enforcement; scratch evidence is `aero-b06-required-red.log`.
- Eight B06 integration tests pass: required permissions/ownership, XDG default/precedence/error rules, real cross-process exclusion/release and independent tasks, opaque traversal-looking/Unicode/byte-boundary IDs, symlink/hardlink/permission refusal, preserved B05 journal history across runtime generations, concurrent/partial epoch refusal with sentinel preservation, and non-UTF-8 state paths. The cross-process test also passed three repeated runs.
- Final agent suites: 13 module and 34 journal integration tests. Final workspace: 134 behavior tests and eight compile-fail doctests, none failed/ignored/filtered. Build, formatting, warning-denied Clippy and warning-denied rustdoc pass. Logs: `aero-b06-final-gate-0.log` through `-4.log`. Documentation/link/traceability, preservation, dependency, whitespace and empty-index checks pass.
- Read-only design review clarified exclusive epochs, every application-owned directory's permissions, lease/path distinctions, journal-versus-task authority, and persistent-state/environment assumptions. Those contracts are implemented or explicitly deferred to their runtime owners. Independent final source review found no actionable in-scope defect and reported rerunning the then-current 32 integration tests; the parent subsequently ran all 34 including the two additional boundary tests. Review self-report is not the final gate evidence.

Integration limits: task identity is scoped to the configured state root; hashes are not encryption or protection against guessing IDs. Local Linux filesystem semantics are a deployment requirement, not a locality detector. Acquire Journal first, then task lease, then runtime directories; on failure drop acquired guards without unlinking locks. Stop/join all browser/runtime consumers before releasing RuntimeDirs—copied path strings are not leases. This does not establish journal run authority, actual single-input dispatch/takeover, browser sandboxing or hostile same-UID/root isolation. Persistent profiles/staging can be sensitive; N16 owns future validated cleanup. No global /tmp or XDG_RUNTIME_DIR fallback, implicit deletion, browser/provider/account operation, or N02 implementation occurred. ENV-002/ENV-004 remain unchanged.

Stop boundary: B06 is ready_for_review, not committed. Next separately approved batch is N02 immutable revisions/run ownership, starting with `crates/aero-agent/tests/tasks.rs :: contract_revision_requires_trusted_owner`; then `new_run_preserves_task_identity_and_history`. Reuse B05 transactions and B06 lifetime primitives behind host-constructed caller capabilities rather than exposing them as model authority. No automatic commit/push or N02 work. Historical checkpoint sections below preserve evidence only.

## Current B05 checkpoint

Historical checkpoint: the B06 record above supersedes this section's next-action/status text; B05 evidence is retained.

Authorization: the user said, “Ok, let's go with B05.” Scope is the bounded writer and transactional task/revision/run/prepare/commit/load/delete storage boundary, not B06 configuration/controller locks, N02 role enforcement, real dispatch, encryption, providers or publication. The prior core/CLI/audit sources and specification are preserved byte-for-byte against the B05 baseline.

Implemented paths: new `crates/aero-agent/src/store/worker.rs` and private `crates/aero-agent/src/store/worker_tests.rs`; scoped changes to `crates/aero-agent/src/store/mod.rs`, `crates/aero-agent/src/store/migrations/001_initial.sql`, `crates/aero-agent/tests/journal.rs`, root/agent manifests and lockfile. Documentation is synchronized across the plan, requirements, README, contributor guide, tracker, development guide, ADR0003 and this handoff. Tokio 1.54.1 enables only sync for oneshot replies; pin-project-lite 0.2.17 is its new dependency. Existing serde/serde_json are direct agent edges. No Tokio executor, network or macro feature is enabled.

Implemented behavior: one bounded worker, immediate QueueFull on rejected admission, async/synchronous replies, accepted-work drain and synchronous join on close/drop. Task/revision/run writes are transactional; revisions preserve spent resources and invalidate old grants/approvals; new runs retire old active/paused runs. Prepare atomically reserves one attempt/step and directional tokens/active time. Commit atomically writes outcome, usage/reservation settlement, typed evidence/artifact descriptors, metadata-only checkpoint and statuses without advancing the cursor twice. Unknown consumption/effects remain reserved/needs_review. Startup classifies prepared orphans before exposing clients; load_resume refuses live prepared work instead of racing it. Audit is bounded/read-only; task deletion affects SQL-owned records only.

The initial schema is still unshipped. Directional token and active-ms reservation columns refine version 1 directly; old B04-shaped development databases fail exact-schema validation rather than being silently reset or reinterpreted. The full B03 form-containing Checkpoint is never serialized to plaintext metadata. Sensitive contracts and explicit host-classified RequiresEncryption decisions are refused; the host must supply already-redacted permitted contract/Decision data and opaque identifiers. No automatic redaction, encryption, provenance or authorization is established by these types.

Observed red/green and review:

- `writer_persists_validated_task_contract` first failed on the compilable stub, then passed. The required `cargo test -p aero-agent --test journal step_and_checkpoint_are_atomic -- --exact` subsequently demonstrated the actual partial-write defect: a duplicate artifact failure left the action Committed rather than Prepared without a surrounding transaction. Wrapping the full commit fixed that exact regression. A subsequent private matrix injects failures after four prepare and eight commit write points; all rollback assertions pass.
- Design review led to parent-observed red/green tests for stale run reuse after a newer run settles, and load_resume racing live prepared work. New runs now retire old runs; orphan classification occurs only under exclusive startup. Repeated resume retains reservation/cursor and cannot reconcile uncertainty.
- Parent hardening tests reproduced and fixed mismatched persisted reservation SQL ownership versus payload, and checkpoint usage failing to cover its saved cursor.
- Final read-only source review found two P2 persisted-corruption gaps, not remote authorization bypasses: contradictory action-state/outcome pairs and unchecked referenced evidence/artifact payload identity. Both were reproduced in repository tests before fixes. Audit now checks valid state/outcome combinations; checkpoint references bounded-decode and validate embedded ID/context/digest/byte count for both metadata tables. No review finding remains outstanding in this batch.
- Final agent suite: 13 module tests plus 26 journal integration tests, all passing. Workspace: 126 behavior tests and eight compile-fail doctests, no failures/ignored/filtered tests. Build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Five additional repeated worker-suite runs pass. Contributor documentation checks, local links/anchors, traceability, preservation and whitespace/index checks pass. Scratch logs are `aero-b05-*-red.log`, `aero-b05-atomic-behavior-red.log`, and `aero-b05-final-gate-0.log` through `-5.log`.

Limits and next integration obligations: JournalClient is trusted host storage plumbing, not model-callable N02 service authority. Digests and verification statuses are caller reports, not actual file/source checks; no task-completed setter exists. Monetary pricing/purchases and provider-specific charging remain N03; independent criteria/completion remain N04/N05. No reconciliation API, browser restoration or real crash/server proof is implemented. Synchronous start/close/drop must run on a blocking host thread; the queue is bounded but kernel filesystem waits are not cancelable deadlines. Lost replies require reconciliation, never assumed cancellation or repeated external dispatch. SQL deletion is neither secure erasure nor N16 artifact/profile cleanup. ENV-002/ENV-004 remain unchanged.

Stop boundary: B05 is ready_for_review, not committed. No staging/commit/push, browser/server/provider operation or B06 implementation occurred. Next separately approved batch is B06 private XDG state/task-controller isolation, beginning with `crates/aero-agent/tests/journal.rs :: private_state_and_task_lock`; N02–N03 follow B06. F10's later journal/executor guard and real crash matrix remain required before dispatch acceptance. Historical records below preserve previous evidence but no longer define current next actions.

## Current B04 checkpoint

Historical checkpoint: the B05 record above supersedes this section's next-action/status text; B04 evidence is retained.

Authorization: the user said, “Ok, let's go with B04”. This authorizes the initial task-owned schema/private-open batch, its bundled SQLite dependency and synchronized documentation, not B05 transactions, B06 controller/configuration, N02 authority, browser/provider/account operations or publication. Prior core/audit source and specification are preserved byte-for-byte against the B04 baseline.

Implemented paths: `crates/aero-agent/src/store/mod.rs`, `src/store/migrations/001_initial.sql`, `src/lib.rs`, `tests/journal.rs`, root and agent `Cargo.toml`, and `Cargo.lock`. Synchronized docs: README, contributor guide, plan, requirements, development, tracker, ADR0003 and this handoff. Bundled rusqlite 0.41.0 (default features off) resolves ten new package versions; no existing external package versions are removed or changed. Agent reuses existing libc and dev tempfile dependencies. The earlier CLI libc edge remains prior audit work.

Implemented boundary: eleven STRICT tables; task/run/revision composite ownership; immutable revision and grant-payload update/replacement guards; bounded counters/statuses/JSON-object syntax; transactional schema/application/version initialization; integrity/FK/schema identity checks and typed refusal of foreign/future/partial/corrupt stores. Dedicated Linux private paths, no-follow component traversal, owned 0700/0600 storage, safe auxiliary-file metadata checks and a cooperative lifetime flock protect the synchronous connection. SQLite pragmas are verified through Store settings. Store exports no raw connection or domain-write API.

Observed evidence:

- `cargo test -p aero-agent --test journal schema_and_foreign_keys -- --exact` first executed and failed exit 101 with schema version 0 versus expected 1 against the compilable scaffold; the same test now passes. Logs remain under the session scratch directory as `aero-b04-required-red.log`.
- Two hardening regressions were independently observed red then green: schema identity must not hide `sqliteXforeign` through LIKE's underscore wildcard; nonsticky writable ancestors must be rejected before creating the leaf. The implementation uses literal-prefix GLOB and trusted ancestry checks. Test parents are explicitly private, not a reason to weaken production checks.
- Final `cargo test -p aero-agent --locked`: two module tests and fifteen integration tests pass. Coverage includes migration rollback, actual SQLite-created WAL/SHM permissions, same-/cross-process owner exclusion and release, reopen persistence, cross-owner/revision rejection, immutable revisions, task cascades, invalid counters/JSON/ciphertext triples, FK corruption despite integrity_check=ok, schema/version/corruption refusal, failed-open lock release and auxiliary-file refusal without changed database bytes.
- Final `cargo test --workspace --locked`: 104 behavior tests and eight compile-fail doctests pass, no failures/ignored/filtered tests. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Logs: `aero-b04-final-gate-0.log` through `-5.log` in session scratch. Contributor documentation checks and final source/scope/whitespace validation pass.
- Read-only design review was source-only (its probes did not run); its ownership, FK-check, replacement, expiry and sidecar points are integrated or explicitly assigned to future trusted lifecycle enforcement. Independent final source review reported no actionable in-scope defect. Its failed-open/sidecar byte-preservation probe suggestions are now assertions in the parent-run repository suite; review self-report does not substitute for these executed gates.

Limits: structural SQL acceptance is not trusted TaskContract semantics, grant issuance, revocation/quota monotonicity, criterion existence, payload-size enforcement or encryption. B05's private writer must enforce authorized lifecycle/DTO/payload ownership; N02/N03/N06 own authority and budgets. Never serialize the entire form-containing Checkpoint into plaintext metadata. Linux procfs/local Unix filesystem/WAL/advisory-lock assumptions are explicit; hostile same-UID/root access and production power-loss/browser recovery are not proven. No XDG/task-controller boundary, writer queue, prepared intent, commit_step, task execution, keyring, providers or browser launch has been implemented. ENV-002/ENV-004 remain unchanged.

Stop boundary: B04 is ready_for_review, not committed. Main remains at the earlier local-only documentation commit af88105; the index is empty. No browser/server/database service was started by B04; tests use disposable synthetic files. Next separately approved batch is B05, starting with `crates/aero-agent/tests/journal.rs :: step_and_checkpoint_are_atomic`, then bounded writer/domain transactions and failure injection. Do not mark SPEC-09 fully implemented until that protocol is proven. B06 then N02–N03 remain later. Historical checkpoint sections below are evidence, not current next-action instructions.

## Current N01 checkpoint

Historical checkpoint: B04 above supersedes the next-action/status text below; the original N01 evidence is retained.

Authorization: after the verified audit and next-batch boundary were explained, the user said, “Alright, let's move on.” This authorizes N01's pure TaskContract batch, not a commit/push, B04 storage, N02 runtime ownership, providers, real accounts, browser execution or dependency installation. Existing audit files remain uncommitted and preserved.

Implemented files: `crates/aero-core/src/task.rs`, one `pub mod task` export in `src/lib.rs`, and `crates/aero-core/tests/task_contract.rs`. Documentation is synchronized in the plan, requirements, README, contributor guide, tracker and this handoff. All prior source (including actions/wire/CLI), manifests, lock, pin, scripts and specification are byte-for-byte unchanged from N01 start, except the intended lib.rs export. No dependency was added.

Contract: mandatory version 1, nominal TaskId and positive revision, title/objective/profile/account metadata, explicit origin/route/effect scope, processing mode with opaque processor/export references, source classifications, required typed criteria and named outputs, cumulative budget limits, dependencies, mandatory safety stops, questions, close behavior and bounded retention. Strict map-only structs and tagged criteria reject missing/unknown/duplicate/positional fields; nullable inference money is present explicitly. Static semantic errors and no raw aggregate Debug protect against accidental payload diagnostics, not all serialization/logging.

Design review resolved omissions in the earlier illustrative fixture against specification §4: add its title/routes/accounts/classifications/exports/dependencies/stops/questions explicitly; use budget_currency for both monetary caps. This is an unshipped initial format, so no migration or hidden legacy default. The documentation fixture now compiles through a test that checks exact JSON equality. Routes are deny-by-empty with explicit "/" for whole-origin scope; future policy matching is segment bounded and applies to parsed effective URLs. Mandatory stops cannot be omitted. Local-only export intent may reference a local grant without being mistaken for remote processing; actual recipient/purpose/data/task/revision/grant validity is still enforced by future trusted gateways. No automatic authority comes from references, source labels, selected effects or successful validation.

Observed TDD:

- The exact required command, `cargo test -p aero-core --test task_contract contract_is_strict_and_roundtrips -- --exact`, executed once and failed with exit 101 on duplicate criteria against the initial compilable validator scaffold. Full initial integration suite executed eight tests: seven failed, one nominal-ID proof passed. The same required test now executes and passes.
- Schema-review tests `contract_cannot_omit_mandatory_stop_conditions` and `local_export_intent_is_not_confused_with_remote_processing` each executed once and failed exit 101 before their corresponding validation fixes; both now pass. This records a pre-review design correction, not a shipped defect.
- Final source-review tests `contract_rejects_mapped_ipv6_origins_in_both_spellings` and `contract_required_row_counts_must_agree_per_output` each executed once and failed exit 101 before their fixes. Mapped IPv6 is now rejected in both dotted/hex spellings (ordinary canonical IPv6 remains valid); distinct required counts for the same output are rejected, while equal counts, different outputs and optional criteria remain valid. Parent reproduced/integrated both findings; no B02 parser change or dependency addition.
- Additional exact-bound/UTF-8, counter/money/fraction, canonical-origin/route/reference, processing/start-prerequisite, unknown-checker/authority, nested raw-duplicate/positional, privacy and documentation-fixture regressions pass. Positive zero/null/empty distinctions are explicit. CoverageFraction excludes NaN/infinity/out-of-range values through both decoding and private construction; no duplicate Task/Run aliases introduced.

Current executed gates: `cargo test -p aero-core --locked` passes 51 unit tests, one import, 17 TaskContract integration tests and eight compile-fail doctests. Workspace passes 87 behavior tests and eight compile-fail doctests, no failed/ignored/filtered tests in full runs. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Source-preservation hashes pass. Contributor documentation command executed verbatim passes 12 documents/26 requirements/31 corrections/100 ledger packages; extended final checks pass 151 local links/anchors, 57 matrix rows, all 100 definitions, tracker references, JSON examples and 23 cumulative changed paths (prior work plus N01's two new source/test files). `git diff --check` passes and index is empty. Read-only design/source reviews are integrated, with both final source findings observed red/green and all gates rerun afterward. No remaining blocker was identified in the N01 pure-contract scope; no feature-level native/browser/provider/store proof is claimed.

Scope/limits: N01 validates representation/internal consistency and separately supplied host prerequisite facts, not actual provider availability or launch permission. Sensitive/local-only require local capability; background-service availability never substitutes for N09's explicit task grant/isolation. Questions block the start-prerequisite check, but dependency completion/cross-task cycles and trusted resolution remain runtime owners. Origins are a conservative canonical ASCII subset, not DNS/SSRF/redirect/egress policy. Criterion output references are names, not issued artifact records. No verifier/file writer, encrypted store, trusted immutable revision service, budget reservation or completion setter is implemented. N02–N05/N06/N09/N13–N18 remain responsible for those gates; ENV-002/ENV-004 are unchanged. No Hivemind use or external network/account operation.

Stop boundary: N01 is ready_for_review; stop for owner review/explicit scoped commit approval, not automatic publication or another implementation batch. Next separately authorized implementation is B04's task-owned initial SQLite journal schema, not N02 yet: dependency order is N01 → B04–B06 → N02–N03. B04's first named proof is `crates/aero-agent/tests/journal.rs :: schema_and_foreign_keys`. No staging, commit, push, B04 or N02 begun. Historical audit/B03/B02 evidence below is retained but no longer defines the current batch.

## Current pre-N01 audit checkpoint

Historical audit checkpoint: the current N01 record above supersedes its next-action/status text; the audit evidence remains valid.

Authorization: the user said, “Ok, let's resolve this bug, and let's find any other bugs or issues that needs to be fixed before moving on.” DEC-008 is accepted for BUG-001 and an audit/fix pass over implemented code, not N01/new features, installation, provider/account/browser operations or staging/commit/push. The bounded lifecycle repair reuses the already-locked libc package explicitly as documented below rather than introducing a new package/version.

BUG-001: two repository regressions were added before the change and each executed/faulted with exit 101 on positional Action arrays. Action now uses the existing map-only tagged helper, preserving valid object wire data, Debug/PartialEq and optional/null scroll target behavior. Both regressions pass across all 18 variants, direct and nested; existing core tests, workspace tests, build, formatting, warning-denied Clippy and core rustdoc pass. At that BUG-001-only step, manifests/lock/specification were unchanged; actions.rs was deliberately changed under the new authorization rather than covered by B03's historical preservation promise. The subsequent signal-ordering repair adjusts the root/CLI manifests and one existing lock edge, detailed below; core manifest, pin and specification remain unchanged.

Audit findings: three read-only audit streams covered B03 schemas/validation, B01 geometry/IDs/B02 parameter validation, and implemented CLI diagnostics/scripts. No additional concrete core defect was reported beyond BUG-001. The CLI audit identified three issues; the parent reproduced each against repository tests before fixing it. Review-reported stress/probe counts are supplemental, not a browser/model benchmark or a proof that no bugs exist. Parent owns all repository writes.

| Issue | Observed repository red | Local repair / green |
|---|---|---|
| BUG-001: Action accepts positional arrays | Two new core tests each ran once, failed exit 101 before the fix. | Shared map-only decoder; both pass across all 18 variants direct/nested, with valid objects/optional targets unchanged. |
| BUG-002: diagnostic descendants leak | Two descendant tests failed exit 101 after cleaning their fixtures. Review then identified signal-after-reap: parent ordering regression failed exit 101 (getpgid=-1); no rare PID-reuse collision was forced. | Dedicated groups; Linux waitid/WNOWAIT retains the leader until group signalling completes, then reaps it with no second group signal. Nondefault/auto-reaping SIGCHLD is refused before spawn; ECHILD drops signalling authority. Cleanup/order/policy tests pass. Not C04 runtime or hostile-executable containment. |
| BUG-003: helper requires absent python alias | Three new shell-adapter tests all failed exit 101 under a controlled Python-3-only/missing-Python PATH; helper returned 127. | Use/check python3 before Cargo; three tests pass, including mismatch rejection and Cargo-error propagation. Contributor check and prerequisite docs also use/name Python 3. Reports are synthetic adapter fixtures, not actual pin matches. |
| BUG-004: unbounded integrity read | Sparse 1 GiB+1 file failed prompt-rejection assertion, one test, exit 101. | Regular-file/1 GiB actual-byte bounds and a two-second integrity caller deadline. Size, preserved digest and blocked-job deadline tests pass. Blocked OS I/O cannot be canceled; timeout does not join the worker, which ends with this one-shot CLI process. |

Additional regression hardening: `wire::tests::recovery_and_event_objects_reject_field_corruption_systematically` now covers the recovery/event objects and every tagged variant, including each required field, invalid null, raw duplicates/null-first duplicates, unknown fields, trailing JSON, scalars and full positional arrays in serialized declaration order. It passed immediately against the existing implementation and is not claimed as another reproduced defect. Core has 51 unit tests plus one import and seven compile-fail doctests. Workspace has 70 behavior tests (51 core unit, one import, five CLI unit, three shell-helper and ten doctor) plus seven compile-fail doctests; all passed locally, no ignored/filtered tests in the full gate. Final full workspace tests/build/fmt/warning-denied Clippy and workspace rustdoc (`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`) pass. `bash -n` passes for the pin helper; the contributor Python3 check executes verbatim and passes 12 documents/26 requirements/31 corrections/100 packages. Extended final validation passes 148 local links/anchors, 57 matrix rows, all 100 definition/ledger mappings, JSON examples, tracker indexes and 21 exact reviewed changed paths, including previous B02/B03 work and the explicit manifest/lock edge. Index is empty, HEAD is still af88105, and the core manifest/pin/specification are unchanged. Source review's signal-ordering finding was parent-reproduced and fixed, and all Rust gates were rerun to the final 70+7 result; no remaining blocker was identified in the implemented pre-N01 scope. Both descendant tests also passed three repeated runs. The final lock readback proves only the existing libc edge changed, with package/version/checksum identities preserved.

Scope/preservation: production changes are Action's decoder, private wire helper reuse, CLI diagnostics and the pin helper; tests cover these and B03 negative shapes. Core manifest, specification and browser pin are unchanged. The root/CLI manifests now declare existing locked libc 0.2.191 explicitly for target-correct waitid/killpg/sigaction types/constants instead of hand-coded ABI layouts. Offline Cargo changed only aero-cli's libc edge: package/version/checksum identities are unchanged, no package download/install/update. Linux default SIGCHLD policy is queried, never modified. No provider/account/browser session/host-security setting or database touched. Diagnostic group cleanup is not renderer/runtime qualification; ENV-002/ENV-004 stay unverified.

Stop boundary: BUG-001–004 and the completed audit remain ready_for_review, not closed/committed; no remaining pre-N01 code blocker was identified after review and parent verification. No staging, commit, push or N01. After the fixes are reviewed/approved, N01 remains the next separate batch (`crates/aero-core/tests/task_contract.rs :: contract_is_strict_and_roundtrips`), before B04–B06 storage. Historical B03/B02 records below retain their original evidence but no longer describe current BUG-001 authorization/state or final audit totals.

## Current B03 checkpoint

Historical B03 checkpoint: superseded for BUG-001/DEC-008 and next action by the current audit record above; the B03 feature evidence remains valid.

Approval: after the B03 form/checkpoint/public-event scope and outstanding schema decisions were explained, the user said, “Great, let's proceed then.” This authorizes the pure B03 contracts and synchronized documentation, not commits/pushes (including B02), N01 TaskContract, persistence/encryption, browser capture/hydration, provider/account operations or UI work.

Implemented scope:

- `crates/aero-core/src/form.rs`: JSON array entries with strict locator/typed value objects, preserving empty text, Unicode, false checks, radio-none versus selected empty value and empty/multiple selections. Validators check locator shape/count/duplicate identity and control/value compatibility; no raw Debug on private aggregates.
- `crates/aero-core/src/ids.rs`: nominal TaskId/RunId/EvidenceId/ArtifactId using the existing opaque-string convention. IDs remain unnormalized and do not establish issuance, ownership or authority. N01 will reuse them, not introduce duplicate Task/Run types.
- `crates/aero-core/src/checkpoint.rs`: version-1 checkpoint, expected RunContext, cumulative task counters, SQL run statuses, explicit complete/partial/nonresumable capture report, runtime/document/observation generations and tab/frame/form inventory with evidence/artifact references. Scope/counter/cursor/uniqueness/aggregate limits are validated. Original encoded bytes have a separate ≤2 MiB size guard; inventory caps are 8 tabs, 20 origins and 2,000 total entries, never silently truncated.
- `crates/aero-core/src/events.rs`: version-1, sequenced public status/usage/step-outcome metadata. Dispatch and verification are separate; uncertain+passed is rejected. There is no raw form/action/diagnostic/authority payload field or executable completion setter. Opaque ID strings are still arbitrary data: trusted producers must issue/resolve them and apply privacy/provenance policy.
- `crates/aero-core/src/wire.rs` and `src/lib.rs`: private schema-once, map-only deserialization helper shared by B03 objects, with required explicit nullable fields, unknown/missing/duplicate/array rejection, supported-version checks and payload-free validation errors re-exported at the core root. B02's action parser and its public error type are unchanged.
- Final source-review repair: `strict_tagged_object!` gates FieldValue/EventKind through map visitation before a generated serde remote decoder, preserving one variant declaration and all duplicate/unknown/missing-field checks. Derived internally tagged enums alone accepted positional arrays even inside strict surrounding structs. This B03 fix leaves B02's analogous Action parser untouched; its separate reproduced defect is BUG-001, not a completed fix.
- Documentation scope: README, CONTRIBUTING, implementation plan, traceability matrix, tracker and this handoff. The plan links to compiled models and replaces its inconsistent checkbox/Text illustration. Future N01/N04 reuse existing IDs/verification status. Manifests, lockfile, specification and B02 actions.rs are byte-for-byte unchanged against the B03 start baseline.

Observed TDD evidence on existing pinned Rust/Cargo 1.99.1, default features, WSL development host:

| Subtask | Actual red | Actual green |
|---|---|---|
| B03.1 form representation | Tests first against a minimal compilable scaffold; `cargo test -p aero-core form_ -- --nocapture` ran three tests, all failed, including `form_json_preserves_empty_and_false`'s array assertion and invalid-input rejection. | The same tests pass after complete serialization, map-only parsing and structural validation; no permissive serializer/validator remains. |
| B03.2 durable references | Temporary aliases made `ids_are_distinct` fail (six identities instead of ten); the new Task/Run compile-fail example incorrectly compiled. Both commands exited 101. | Nominal wrappers pass the ID suite, exact Unicode/whitespace serialization, nonstring rejection/map ownership and the Task/Run compile-fail check. |
| B03.3 checkpoint contracts | After fixing a test-only borrow error, `cargo test -p aero-core checkpoint_ -- --nocapture` ran six tests: five failed, exposing unsupported-version/context/counter/inventory/size acceptance; structural helper tests already passed. | Required version and validators made the same six pass, including `checkpoint_version`, zero/max/gapped cursor cases and aggregate entry/byte limits. |
| B03.4 public events | `cargo test -p aero-core public_event_ -- --nocapture` ran five tests: four failed, exposing unsupported versions, wrong context/counters, private payload extras and uncertain+passed acceptance. | All five pass with strict version/kind fields, shared validation and distinct dispatch/verification. |
| B03.5 schema-review refinements | `form_value_requires_compatible_control_metadata` and `checkpoint_capture_completeness_is_explicit` each ran once and failed assertions, exit 101, before their changes. | Both pass after explicit capture-status serialization and control/value compatibility. Fixtures use distinct matching locators; no raw Debug was introduced. |
| B03.6 tagged-object enum repair | Final reviewer flagged derived tagged-enum sequences. Parent added direct/nested variant-array regressions; `form_objects_are_strict_at_every_level` and `public_event_objects_are_strict_and_required` each ran once and failed, exit 101, accepting Text and StatusChanged arrays respectively. | Shared map-only tagged decoding makes both pass for all four FieldValue/three EventKind variants, direct and nested; existing positive object roundtrips, nullable presence, duplicates and private-field rejections remain green. A now-unused serde import was removed after Clippy detected it. |

Current observed gates after those refinements:

- `cargo test -p aero-core --locked`: 48 unit tests, one import test and seven compile-fail doctests passed, no failed/ignored/filtered tests. B03 adds 17 unit tests and two compile-fail examples over B02.
- `cargo test --workspace --locked`: 57 behavior tests (48 core unit, one import, eight CLI) and seven compile-fail doctests passed. Empty runtime scaffold suites are not feature coverage.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: exited 0. Normal core dependency graph remains serde/derive only; serde_json stays test-only.
- All core/workspace/build/fmt/Clippy gates passed again after the final tagged-enum repair; `RUSTDOCFLAGS="-D warnings" cargo doc -p aero-core --no-deps --locked` also exited 0. The existing suite has no failed/ignored/filtered tests, but the separate known B02 scratch probes below still fail and block B02.
- Updated plan §7.10 completion-predicate example compiled against the actual existing verification-status enum in a scratch harness and passed its one test. This is documentation example evidence, not an implemented N04/N05 completion gate or an extra production test.
- Contributor-guide documentation command executed verbatim: PASS for 12 documents, 26 requirements, 31 corrections and 100 ledger packages. Extended final validation also passed 148 local links/anchors, 57 matrix rows, all 100 package definitions, two plan JSON examples, implemented matrix test names, tracker index/record equality, explicit B02 blocked/B03 ready_for_review/N01 not_started lifecycle checks and direct untracked-source whitespace checks; `git diff --check` passed. An initial extended-check regex missed bold/heading-style package definitions; correcting the checker, without changing valid package records, made full ledger/definition set comparison pass.

Review/proof boundaries: schema review confirmed the task/run/cursor design and led to capture completeness, control/value compatibility and explicit persistence/provenance boundaries. The first read-only source review found no defect outside those then-in-flight refinements; its three nonblocking suggestions are incorporated: raw duplicate nullable locator fields in both null orders, every NotDispatched verification report, and negative gapped/positive task-cumulative cursor cases. These test additions were already green against the existing implementation; they are not claimed as newly reproduced defects. Core/workspace/build/fmt/Clippy and preservation gates passed again afterward, with counts unchanged. Final whole-slice review found the tagged-enum array gap; the parent reproduced and repaired it in B03, checked sibling paths, and reran the gates. No remaining B03 source defect was identified; B02's reproduced sibling issue remains open, not covered by that statement. B03 reports capture status but does not run capture or prove that producer reports are truthful. Raw form/URL data is pre-encryption: the entire Checkpoint must never be written wholesale into plaintext SQL metadata_json or public events. B04/B05/F01/F05 own the safe metadata/authenticated-encrypted-payload split and missing-key failure; none is implemented here. Selectors, origins and captured IDs need real DOM/permission/freshness checks later. Context comparisons require a separately trusted expected value; reference existence, history/usage monotonicity, actual grants, event ordering/reconnect, independent verification and task completion remain future owners. F01/F02/F03 still own comprehensive capture eligibility, classification/redaction and hydration. Capture/size-limit failures must be reported as partial/nonresumable by future capture/recovery controllers, not silently dropped. ENV-002/ENV-004 are unchanged and unverified; Hivemind was not used.

Outstanding B02 sibling defect: a separate scratch harness compiled against the actual built library accepted Action `["click","target-1"]`, directly and in a valid version-1 Decision. `action_array_is_rejected` and `nested_action_array_is_rejected` both failed (exit 101). These two failing probes are outside the green workspace total and must not be hidden by it. Source/binary/log are `/home/davia/.hermes/cache/scratch/aero-b03-sibling-array-probe{.rs,,.log}` (local/ephemeral). BUG-001 blocks B02 readiness/commit; DEC-008 asks permission to change the explicitly preserved B02 parser/tests. The approval question timed out without response; no approval or rejection was inferred. B02 actions.rs, manifests, lockfile and specification remain unchanged.

Git/stop boundary: main remains af88105 with the earlier local-only README/handoff commit; B02 work was already uncommitted when B03 began. B03 adds uncommitted work without staging, committing or pushing either batch. No browser/server/provider/database was started. Read-only schema/source reviews have no write ownership. Next action is owner review and DEC-008's narrow B02-fix decision; fix/verify BUG-001 before B02 approval/commit and moving to separately approved N01, not automatic scope expansion or publication. After its separate approval, N01 begins at `crates/aero-core/tests/task_contract.rs :: contract_is_strict_and_roundtrips`; `cargo test -p aero-core --test task_contract contract_is_strict_and_roundtrips -- --exact` must eventually execute the actual test. N01 still precedes B04–B06 storage.

## Current B02 checkpoint

Retained B02 evidence: the current B03 record above supersedes its next-batch instructions and readiness claim. B02's action source remains unchanged and uncommitted, with newly reproduced BUG-001 blocking its object-only parser guarantee. Historical passing tests below did not cover tagged action arrays.

Approval: after the B02-only scope/limits/testing/stop boundary were explained, including the missing explicit wire version, the user said, “Ok, fantastic! Let's get started then.” This authorizes B02 implementation and its synchronized documentation, not a commit/push, B03, a new dependency, provider/account operation or browser/UI expansion.

Implemented scope:

- `crates/aero-core/src/actions.rs` and its `src/lib.rs` export: the 18 preplanned action variants, required `schema_version=1`, existing typed `ObservationId`, public rationale, strict object-only decision deserialization, strict action tags/fields, and pure `Decision::validate()` / `Action::validate()`.
- Validation: mandatory version, nonempty opaque references, named-key allowlist, conservative HTTP(S) syntax subset (ASCII DNS/canonical IPv4/bracketed IPv6 hosts, decimal u16 ports, well-formed percent escapes; no userinfo or browser-repair forms) and exact navigation-only `about:blank` exception, finite/bounded scroll, UTF-8 byte limit for typing, Unicode-scalar limits for rationale/summary, wait/extraction/batch bounds and csv/parquet format names. Validation errors contain static field/reason strings, never rejected payloads.
- Documentation: `README.md`, `CONTRIBUTING.md`, `docs/implementation-plan.md`, `docs/requirements.md`, `docs/bug-fix-tracking.md`, `docs/handoff.md`. The plan links to compiled Rust instead of retaining a competing uncompiled action definition. Product requirements and original source artifacts unchanged. No manifest, lockfile or dependency change.

Observed TDD evidence on Rust/Cargo 1.99.1 in the existing WSL workspace, default features:

| Subtask | Red observation | Green evidence |
|---|---|---|
| B02.1 strict parsing/version/validation | Tests were written first; initial missing-symbol compilation exited 101, then a deliberately permissive scaffold allowed real assertions to run. `cargo test -p aero-core decision_ -- --nocapture` executed 12 tests: 10 failed, including all three required names, with ignored fields/unknown actions/unsupported versions/unvalidated parameters exposed. | The same command passed all initial 12 tests after strict parsing and validators; deliberate unknown-action fallback and always-OK validator removed. |
| B02.2 object envelope | Review found serde's derived struct parser also accepted a positional JSON array. `cargo test -p aero-core decision_requires_an_object_envelope -- --nocapture` ran once and failed its rejection assertion, exit 101. | A map-only deserializer with required/duplicate/unknown-field checks made the identical test pass. Optional viewport scroll target also tested; the then-focused suite passed 14 tests. |
| B02.3 conservative URL syntax | Read-only contract review recommended rejecting malformed authorities rather than accepting any HTTP-shaped string. `cargo test -p aero-core decision_rejects_malformed_url_syntax -- --nocapture` ran once and failed, exit 101, exposing userinfo/bad-host/bad-port/bad-percent acceptance across navigation, new-tab and metadata variants. | Shared syntax helpers made the identical test pass without a parser dependency; Unicode paths, trailing-dot DNS, canonical IP hosts, port boundaries and valid escapes remain accepted. Unsupported raw Unicode hosts/IPv4 repair forms are explicit. Final focused decision suite: 15 passed, no ignored tests. |

Actual shared gates after both review repairs:

- `cargo test -p aero-core --locked`: 31 unit tests, one import test and five compile-fail doctests passed, no failed/ignored/filtered tests.
- `cargo test --workspace --locked`: 40 behavior tests (31 core unit tests, one core import test, eight CLI tests) and five compile-fail doctests passed; scaffold suites still contain no feature tests.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: all exited 0.
- `cargo tree -p aero-core --edges normal` still contains only serde and its derive machinery; no runtime/browser/storage/provider dependency was introduced.
- Contributor documentation command executed verbatim: PASS for 12 documents, links/fences/whitespace, 26 requirements, 31 corrections and 100 ledger packages. Extended checks passed for 12 tracked Markdown documents (including README), 140 local links/anchors, 57 unique matrix rows, two parsed JSON blocks, 18 action variants, 15 decision tests and the exact eight-file scope; only B02's ledger state changed and the Git index is empty. Direct new-source/whitespace inspection and `git diff --check` passed. These are documentation/scope checks, not extra application tests.

Proof boundaries: the DTOs do not execute actions, establish grants/freshness, fill credentials, export Parquet or mark a task successful. Callers must parse original response bytes directly then validate, and later trusted policy still owns authorization. The conservative URL subset is not full browser/WHATWG/IDNA parsing, origin/redirect/SSRF policy or network confinement; future URL consumers must parse/resolve and policy-check effective destinations. Raw Unicode hosts need already encoded ASCII IDNA; IPv6 zones and browser-repair forms are unsupported. B02 does not supply a global wire-payload cap or arbitrary-text redaction. Raw serde diagnostics and proposal Debug output can contain input and are not sanitized audit records. SPEC-26/SPEC-27 advance only partially; cross-surface privacy/history/replay acceptance remains future work. ENV-002/ENV-004 remain unverified and unchanged.

Review outcome: integrating self-review and the read-only source review found no remaining B02 blocker. The source reviewer excluded URL tests while those helpers were changing; the integrating implementer reviewed the conservative syntax repair and independently ran its focused regression and the complete workspace gates. Three suggested raw-JSON coverage cases were added for duplicate envelope rationale/action and duplicate scroll target after null; the existing implementation rejected all three without further production changes. The full focused/workspace/build/fmt/Clippy gates were rerun successfully after those fixtures. This is ready for owner review, not commit approval or product acceptance.

Git/publication: resumption checks found clean main at af88105, one commit ahead of origin/main. Independent `git ls-remote origin refs/heads/main` read back 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd. af88105 remains the previous local README/handoff commit, unpushed; B02 changes are additionally uncommitted. No staging, commit or remote write performed. No browser/server was started; read-only contract/source-review children were used, with no write ownership, and both have completed.

Next safe action: owner review of B02 and separate explicit commit/push approval if desired. Do not implement B03 automatically. After its own approval, B03 starts with `form_json_preserves_empty_and_false` in `crates/aero-core/src/form.rs`, then `checkpoint_version` checks per the plan; N01 TaskContract still precedes B04–B06 storage. No Rust/browser provisioning or B01 repeat is needed. Hivemind remains unavailable and was not used.

## End-of-session handoff

Historical 2026-10-22 stop record: the current B02 checkpoint above supersedes these restart instructions; preserve this section as B01 publication/approval evidence.

Stop instruction: the user said, “ok update the hand-off and we stop here tonight,” then requested, “update the main README.md and then do a final commit for the night.” This authorizes one final documentation commit containing README.md and this handoff, not B02 implementation or another push.

- At 2026-10-22 21:55 +08:00, local main and remote main were independently checked at 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd; the working tree was clean before this handoff edit. The preceding publication check also confirmed the same SHA through the GitHub API.
- Both B01 commits are pushed. The final documentation scope is `README.md` plus `docs/handoff.md`, committed locally under the user's final-night request. Use Git for that documentation commit's actual SHA; it is not self-embedded here. The last verified published checkpoint is 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd; this final documentation commit is not pushed. Recheck local/remote state next session rather than assuming they still match.
- Last application gates: 25 behavior tests and five compile-fail doctests passed, with workspace build, formatting and warning-denied Clippy. These were rerun before the approved B01 commit; they are not claimed rerun for this documentation-only stop note. Documentation validation is used for this edit.
- No background job, server, browser session or automatic continuation was started by this batch. No cleanup or native qualification is implied for unrelated machine processes.
- Remaining gates: ENV-002 browser/sandbox/provenance qualification and ENV-004 native display/keyring/platform evidence remain unverified. AeroBrowser is not yet a usable browser; handle-map freshness, task execution, permissions, storage/recovery, providers and desktop remain future work.

Next session:

1. Read this handoff and recheck Git status/log and local main versus origin/main. The final README/handoff commit is local-only; obtain a push request before publishing it. Do not repeat Rust provisioning, bootstrap, README or B01 work.
2. Obtain approval for B02 only: strict versioned action/decision contracts in `crates/aero-core/src/actions.rs` and its `src/lib.rs` export, following the canonical plan §7.1 and B02 assertions. B02 is not authorized by tonight's stop instruction.
3. Once approved, add the named decision tests first (`decision_is_strict_and_round_trips`, `decision_rejects_limits`, `decision_rejects_unknown_action`); run `cargo test -p aero-core decision_ -- --nocapture` and obtain meaningful red before minimal implementation. A zero-test run is not proof.
4. Run core/workspace/build/fmt/Clippy and documentation gates, update traceability, then stop for review before committing. Later sequence remains B03, N01, then B04–B06; no browser/provider/UI expansion is part of B02.

Environment reminder: Cargo may be absent from a fresh shell's PATH even though the pinned toolchain is installed. Use `export PATH="$HOME/.cargo/bin:$PATH"` before Rust checks; do not reinstall or change the pin to work around PATH.

## Current B01 checkpoint

This preserves the approved B01 checkpoint; the current B02 record above supersedes its next-session instructions. The BATCH-00 and documentation-only sections below are historical evidence, not instructions to repeat setup. The user approved B01 after a proposal to implement typed IDs and safe geometry, then stop for review before any commit. After the review report and passing checks, the user explicitly said “commit and push.” That authorizes publication of this B01 batch and its approval/status record, not implementation or publication of later packages.

Implemented scope:

- `crates/aero-core/src/ids.rs`: six distinct string-backed IDs, private inner strings, exact serde string round trips, owned map-key behavior, representative compile-fail cross-ID checks. These types do not generate or validate external identities, establish authority, or invalidate handles by themselves.
- `crates/aero-core/src/types.rs`: `CssRect::clipped_center`, finite/positive/representable bounds, visible intersection, overflow-resistant midpoint and strict interior checks; validated `FrameLocalRect`, `RootViewportPoint` and `ScreenshotPixelPoint` without implicit conversions. Clipping stays in its input space; there is no frame projection, DPR conversion or current-browser hit testing.
- Core `Cargo.toml`, `src/lib.rs`, root `Cargo.lock`: module exports, existing workspace serde dependency and test-only serde_json. No newly resolved package versions; normal core dependency tree remains serde and its derive machinery, with no browser/storage/provider runtime.
- Status/traceability: `README.md`, `CONTRIBUTING.md`, `docs/implementation-plan.md`, `docs/requirements.md`, `docs/handoff.md`, `docs/bug-fix-tracking.md`. Requirements unchanged. The plan now explicitly assigns the actual epoch-bound element handle map to its D01/D02 observation/input owners; B01 only supplies the types. Its mandatory freshness behavior is not waived or implemented here.

Observed TDD evidence (all deliberate red commands exited 101, not missing-tool failures):

| Subtask | Red observation | Green evidence |
|---|---|---|
| B01.1 identifiers | `cargo test -p aero-core ids_are_distinct -- --nocapture` failed against string aliases; `cargo test -p aero-core --doc` failed because cross-ID examples incorrectly compiled. | Four ID unit tests and three ID compile-fail doctests pass with nominal wrappers. |
| B01.2 clipping | `cargo test -p aero-core center_uses_visible_intersection -- --nocapture` failed against a method returning None. | Named test passes; nonfinite inputs, nonpositive dimensions, offscreen/empty area and overflowing bounds are covered. |
| B01.3 rounded boundaries | `cargo test -p aero-core center_rejects_unrepresentable_interior -- --nocapture` failed when the calculated midpoint rounded onto an edge. | Strict interior predicate rejects that case; `center_keeps_small_representable_intersections` still accepts representable subnormal interiors. Seven geometry tests pass. |
| B01.4 coordinate spaces | `cargo test -p aero-core coordinate_ -- --nocapture` exposed the screenshot/root alias and unvalidated points; `cargo test -p aero-core frame_local_rect_rejects_invalid_or_unrepresentable_bounds -- --nocapture` exposed the unchecked constructor; doctests exposed accepted cross-unit calls. | Five coordinate-space tests and two coordinate compile-fail doctests pass. |

Actual shared gates on Rust/Cargo 1.99.1 in the existing WSL workspace, default feature selection:

- `cargo test -p aero-core --locked`: 16 unit tests, one pre-existing import test, five compile-fail doctests passed.
- `cargo test --workspace --locked`: 25 behavior tests (including all eight pre-existing CLI tests) and five compile-fail doctests passed; no failed, ignored or filtered tests. Empty scaffold suites are not feature coverage.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: all exited 0.
- Contributor documentation checker: PASS for 12 documents, 26 requirements, 31 corrections and 100 ledger packages. Extended validation: 12 tracked documents, 135 local links/anchors, 57 unique matrix rows; only SPEC-17/SPEC-19 advanced partially. Updated §7.3 reference code separately compiled and passed its three tests; this is additional documentation evidence, not extra production tests. Final diff/whitespace and exact 11-file scope checks passed, including direct inspection of the two new source files; index empty and lock package versions/provenance unchanged. Application acceptance is not inferred from these checks.

Review/state: implementation self-review found no remaining B01 primitive defect; integrating checks above pass. The full workspace/build/fmt/Clippy and documentation/scope gates were rerun successfully immediately before committing the approved exact 11-file batch as 54e0a9573d8eb90746243f0f24056b05a3022988, `feat(core): define scoped IDs and CSS geometry`. Approval/status commit 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd followed, and both were pushed to main. Local HEAD, origin/main, remote main and the GitHub API were read back at the same final SHA; the working tree was clean after publication. Ledger state is committed. The end-of-session section above records the subsequent local README/handoff documentation commit. No browser, provider, account or host-security operation was needed, and no long-running process was started by this batch. ENV-002/ENV-004 remain unverified native/runtime gates, not pure-core blockers.

Work is paused at the user's request. Next session, propose B02 strict action/decision contracts, then B03 snapshots/events and N01 TaskContract before B04–B06 storage. Do not begin those packages tonight or as part of finishing B01. They require separate approval.

## 0. Previous BATCH-00 checkpoint

This section supersedes the historical documentation-only/environment snapshot below. The user approved first-phase implementation and subsequently said, “ok, let's commit and push.” That authorizes the reviewed bootstrap baseline and checkpoint update to private main, not later features, real accounts/providers, license/redistribution choices, public release or host-security weakening.

- A01: parent read all five requirements/architecture/ADR documents and independently validated exactly 26 requirement and 31 correction rows, canonical package/test-name mappings, all local file links and anchors. Existing suite/manual gates and missing dedicated SPEC-23/SPEC-26 behavioral proof names are disclosed, not invented or considered application coverage. Requirements remain not_started until actual acceptance. Documentation is ready for user review.
- A02: official user-local Rust/Cargo 1.99.1 with rustfmt/clippy provisioned; compiler pinned in rust-toolchain.toml. GCC and Bubblewrap already present. User-installed pkg-config 2.5.1 verified at /usr/bin/pkg-config (canonical /usr/bin/pkgconf), resolving ENV-003 for bootstrap availability. Native platform/keyring/sandbox and future native-library gates remain unproven.
- A03: exact six-crate workspace implemented and metadata checked; minimal core import/version test passed after observed behavioral red. Manifests, lockfile and libraries are scaffolding, not implemented task/CDP subsystems.
- A04 baseline: real CLI --help/--version/argument handling and bounded prerequisite/version/hash diagnostics implemented. Eight CLI tests passed after observed red; explicit empty/missing path never falls back, hung versions are killed/waited, oversized output rejected, failed stderr hidden, child environment cleared. No browsing session is launched.
- Development browser: Chrome for Testing 156.0.8078.47 Linux64 was provisioned in the user's private cache. Real --version and Rust local executable-pin check passed. Vendor archive MD5 was compared against separately fetched Storage response headers; SHA-256 values are local byte pins, not independent vendor signatures. Redistribution is not approved. Details: [development.md](development.md).
- Intentional unready report: follow-up actual doctor JSON reports available browser, Bubblewrap and pkg-config 2.5.1, matched_local_pin, sandbox/user namespaces/keyring not_checked, display not_configured, runtime_ready=false; exit 2 is correct. This does not satisfy C04/G0/G1.
- Git: main, origin git@github.com:dave2k77/aero-browser.git; root commit 5a4fac91e5638735aa1cd762c183f63c9efa39de contains exactly the reviewed 32 text files. PDF, browser binaries, target/runtime data/secrets and .hermes artifacts are excluded. Cargo.lock is tracked. Check Git for the checkpoint-update commit and current push verification; do not infer remote delivery from this static document.

Actual red/green and verification evidence:

| Subtask | Observed result |
|---|---|
| A03.1 initial import introduction | cargo test -p aero-core --test import first exited 101 with missing manifest/module, then the named test ran and failed its empty VERSION versus 0.1.0 assertion. Replacing VERSION with env!(CARGO_PKG_VERSION) made the same test pass once. |
| A03.2 workspace boundary | cargo metadata --format-version 1 --no-deps parsed and checked exactly aero-core, aero-cdp, aero-perception, aero-executor, aero-agent and aero-cli. |
| A04.1 diagnostic behavior | cargo test -p aero-cli --test doctor ran eight named tests to behavioral red before implementation. An intermediate run passed seven and exposed Clap's rejection of an explicit empty PathBuf; OsString parsing fixed it without fallback. Same suite then passed all eight. |
| A04.2 integrity helper | bash -n scripts/lock-browser.sh passed; actual candidate check exited 0, nonexistent browser check exited 1. Neither result claims a working sandbox. |
| Shared quality gate | cargo test --workspace --locked passed the core import test and eight CLI tests; scaffold libraries have no behavior tests yet. cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked -- -D warnings, cargo build --workspace --locked all exited 0. |
| Documentation integration | Contributor checker executed: PASS for 12 documents, links/fences/whitespace, 26 requirements, 31 corrections and 100 ledger packages. Extended validation passed 123 local links including anchors, 57 unique matrix rows and canonical named proofs/explicit suite/manual gates. The parent's real checks supersede the subagent's blocked validator command; no child self-report was accepted as proof. |
| Working-tree boundaries | git diff --check passed (tracked diff is empty before first commit); independent whitespace/scope inspection checked all 32 project source/config/doc files. Historical PDF SHA-256 unchanged; target/PDF/local Hermes plan ignored; Git index empty and HEAD absent. GitHub read-back confirmed PRIVATE and isEmpty=true. |

Historical next action was B01 preparation; the current B01 checkpoint above supersedes it. Bootstrap review/commit approval did not authorize later commits. Native/runtime qualification remains incomplete.

## 1. Start here

Workspace: `/home/davia/projects/aero-browser`.

Read in this order:

1. [Specification](specification.md): authoritative requirements, product scope, and invariants.
2. [Implementation plan](implementation-plan.md): §0 priorities/live ledger, §3 source corrections, §6 TDD protocol, §7 reference contracts/code, §8 dependency-ordered packages, §9–10 acceptance.
3. [CONTRIBUTING.md](../CONTRIBUTING.md): ownership, security, verification, review, and commit rules.
4. [Bug/fix tracker](bug-fix-tracking.md): current environment blockers and pending owner decisions.

The old `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md` path redirects to the live plan. Do not rebuild a second plan or use the historical PDF as an implementation status report.

Document ownership:

- Specification: requirements and stable REQ-* IDs.
- Plan: task dependencies, exact implementation/test paths, contracts, live package states and owners.
- Tracker: actual bugs, environment blockers, decisions, regression/closure evidence.
- Handoff: current verified checkpoint, approvals, changed files, next safe action, and transfer notes.
- Contributor guide: recurring working rules, not another task/status backlog.

The project owner approves requirements/scope, setup, commits, and external actions. Hermes is the integrating implementer. Bootstrap, README and B01 were separately approved and committed; later batch approvals remain separate. A01's child owned only its five documentation paths and has completed that work.

## 2. Historical documentation baseline — superseded by §0

### What exists

| Artifact | State / use |
|---|---|
| `aero-browser-specification-chat.pdf` | Original 96-page historical input, preserved unchanged. The prior review recorded complete page coverage. Do not transcribe its Rust samples as tested source. |
| `docs/specification.md` | Authoritative revision 2.0; 26 stable REQ-* requirements and nine approved recommendation groups. Authority link now points to the live plan. |
| `docs/implementation-plan.md` | Published detailed plan: 31 SPEC-* corrections, 70 A–I packages, 24 N packages, six separately gated J tracks, complete initial 100-package ledger. All application work is unimplemented. |
| `docs/handoff.md` | This current checkpoint and transfer record. |
| `CONTRIBUTING.md` | TDD/review/approval/security/parallel ownership rules and executable documentation check. |
| `docs/bug-fix-tracking.md` | Local authoritative issue register. Four environment records and seven pending owner-decision records; no reproduced application bugs. |
| `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md` | Compatibility redirect, not another live plan. |

No source tree, Cargo manifest/lockfile, frontend manifest, build script, CI workflow, application test, benchmark report, package, Git repository, branch, or commit exists. All application paths in the plan are future paths. `git status --short` and `git branch --show-current` returned `fatal: not a git repository`; do not report the directory as a clean Git checkout.

### Observed environment — read-only inspection on 2026-10-22

| Item | Observed result |
|---|---|
| OS | `/etc/os-release`: Ubuntu 26.04.1 LTS, version ID 26.04. |
| Kernel/platform | Linux 6.18.33.1-microsoft-standard-WSL2, x86_64. |
| Release baseline | Native Ubuntu 24.04 x86_64 is the approved target, not the host just inspected. |
| Available tools | Bubblewrap 0.11.1; Node v24.18.0; npm 12.2.0. |
| Not found on PATH | cargo, rustc, rustup, chromium, chromium-browser, google-chrome, pkg-config, flatpak-builder, promtool. This is a PATH observation, not a full-filesystem inventory. |
| Native display/keyring/sandbox | Not exercised. WSL is not proof of native X11/Wayland, Secret Service, renderer sandbox, accessibility, or IME behavior. |
| Local instructions | No AGENTS.md found at `/home`, `/home/davia`, `/home/davia/projects`, or this root; no root CLAUDE.md or .cursorrules found at discovery. Recheck if later supplied. |
| Organization memory | Hivemind capture/recall disabled because organization credits are exhausted; not used as an implementation dependency. |

No dependency was installed, browser/provider/account operation performed, source code generated, Git initialized, commit made, or external system changed for this document-set delivery. No long-running process was started by this work; ambient machine processes were not exhaustively inventoried.

## 3. Decisions and invariants to preserve

Product: a Linux-native browser for delegated work with verified outcomes, explicit scoped permissions, evidence-linked artifacts, and reliable manual control. First useful workflow is deterministic local supplier research producing ordered CSV, sourced report, unknown/exclusion labels, manifest, and inspectable evidence. No account creation, submission, upload, or purchase is needed for that proof.

Architecture: six initial Rust crates; pure core models, owned CDP/runtime, separate perception/input libraries, one role-scoped application interface, and SQLite-first durability. Initial service is in-process; desktop/CLI/MCP are adapters. Tauri's trusted WebKit UI is not the controlled Chromium page. Run the actual-viewport feasibility gate before committing to substantial desktop work.

Non-negotiable invariants:

- Durable TaskContract/revisions and cumulative budgets survive runs, provider changes, reconnects, and recovery. Browser IDs/handles remain transient.
- Planner proposes; trusted policy authorizes; executor acts; independent verifier checks; journal records. Model completion/dispatch/citation presence alone never proves success.
- Every real effect path, including Enter/replay/MCP/workers, uses the same scope/preview/expiry/quota gate. Model/page/tool/procedure can request approval, never issue it.
- Intent and resource reservations are durable before dispatch. Unresolved external effects become `needs_review`, never silently replayed. Local DB transactions cannot promise exactly-once remote effects.
- Processor/tool/export transfers use the data-flow gateway. Local-only processing cannot fall back remotely and does not claim offline website browsing/full egress confinement.
- Secret sentinels are tested across planner/log/audit/screenshot/artifact/export surfaces; no raw credentials or private pages in docs, fixtures, bug reports, or traces.
- Pause/close blocks later dispatch, interrupts read-only/planner waits, reconciles bounded input, flushes state, and releases owned processes. Takeover requires acknowledged exclusive ownership; resume observes fresh state.
- Browser recovery is best-effort and visibly partial. No universal Undo, lossless JS/IndexedDB restoration, stealth, native hardware-input, or zero-exposure claim.
- Keep Chromium's own sandbox. No insecure fallback flag, permissive mount, plaintext snapshot fallback, or strict-mode downgrade to get tests green.

Lean defaults: CSV, sequential work, bounded history/SQLite, minimal default graph, native distribution first, pause-on-close. Heavy retrieval/vision/Parquet/workers/metrics/MCP, daemon extraction, Flatpak, remote workers, and training need separate selected scope and evidence; they are not first-workflow prerequisites.

### Ordering repairs incorporated into revision 2.1

The plan's new operational entry clarifies early baseline N09 close handling, early F08 cancellation/timeouts, a B05/F10 prepared-intent safety regression before dispatch, browser permission deny defaults before N15 expansion, bounded history before optional retrieval, and task-owned cleanup when artifacts first persist. Pure Rust bootstrap can proceed after its approved prerequisites without waiting for every optional native/browser tool. The geometry reference now rejects overflowed rectangle bounds and avoids overflow in finite midpoint calculation, with a named future regression test; it remains uncompiled reference code. These clarify existing requirements rather than add a second architecture.

## 4. First implementation batch

This is the original BATCH-00 checklist; actual implemented progress and remaining blockers are in §0. BATCH-00 was not permission to execute during the earlier documentation-only delivery; the later user approval and current scope are recorded above. Do not execute A–I–N alphabetically. Detailed dependencies and contracts remain in implementation-plan §8.

### 00.1 — owner review and authorization

Read the four living documents. Resolve applicable DEC-001 setup scope and DEC-002 local Git authorization. Record the owner's actual response and excluded actions in the tracker. Installing tools, initializing Git, committing, provider use, account operations, and publishing are separate actions; approval of this documentation is not all of those permissions.

Read-only preflight from the active workspace:

```bash
pwd
git status --short
git branch --show-current
python -c 'import shutil; print({x: shutil.which(x) for x in ("cargo", "rustc", "rustup", "bwrap", "pkg-config", "chromium", "chromium-browser", "google-chrome")})'
```

Expected at this baseline: the recorded workspace, Git's not-a-repository failures, missing tool paths as above, and an available bwrap path. These are observations to recheck, not failing application tests. If state differs, update this handoff before proceeding.

### 00.2 — A01 documentation traceability, in small edits

Create only the missing A01 paths, leaving accepted requirements intact:

1. `docs/requirements.md`: one row per REQ-* and SPEC-* ID, linked source, plan package(s), exact planned test path/name, required milestone/feature, and `not_started` until real proof exists. No completed checkbox based solely on document text.
2. `docs/architecture.md`: dependency direction and explicit Task/Run/BrowserSession, trusted service/role, gateway, journal, runtime, verifier, and artifact ownership. Reference rather than duplicate wire types/SQL.
3. `docs/adr/0001-ui-engine-boundary.md`: current native-companion/stream candidates and N10 acceptance matrix; decision remains pending actual feasibility, not a fabricated accepted result.
4. `docs/adr/0002-planner-and-mcp.md`: configured inference adapter versus restricted tool protocol; no agent approval issuer and no assumed MCP sampling inference server.
5. `docs/adr/0003-checkpoint-semantics.md`: prepared intent, cumulative reservations, atomic outcome/checkpoint/cursor, uncertain remote effects, bounded runtime rebuild, and explicit partial restoration.

Run the contributor guide's documentation checker and inspect the new matrix for 26 requirement and 31 correction IDs with no missing/duplicate ID. Extend the checker to include the new documents and their actual links when adding them. Expected: real passing document checks, not Rust/browser results. Submit this small documentation batch for review before treating A01 as verified or committing it.

### 00.3 — A02 setup in the approved environment

Perform only the owner-approved setup/download scope. Use that environment's actual OS/package manager, not a blind paste of Ubuntu 24.04 instructions onto Ubuntu 26.04 WSL. Capture `rustc --version`, `cargo --version`, `bwrap --version`, and `pkg-config --version` in `docs/development.md` when those tools exist. Browser source/version/provenance is a separately validated A04 dependency; do not fabricate upstream checksums or change sysctls/AppArmor to bypass a launch failure.

Update ENV-001/ENV-003 only with observed results. ENV-002 remains blocked until a real Linux browser and its required sandbox tests pass. Native platform evidence remains unverified until actual execution.

### 00.4 — A03 minimal scaffold, then a real core import test

Exact initial files: root `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, and `Cargo.toml` plus `src/lib.rs` under each of `crates/aero-core`, `crates/aero-cdp`, `crates/aero-perception`, `crates/aero-executor`, and `crates/aero-agent`; `crates/aero-cli/Cargo.toml` and `crates/aero-cli/src/main.rs`; resolve `Cargo.lock`. Add a focused import test at `crates/aero-core/tests/import.rs`. No desktop crate, provider integration, runtime database, downloaded browser tree, or optional feature stack belongs in this scaffold.

Use the chosen pinned compiler, edition 2024/resolver 3, exact six members, shared dependency declarations, and explicit `aero` binary. Introduce the import test first and observe its initial missing-workspace/module failure honestly; scaffold minimally so it runs, then introduce meaningful behavior tests before feature implementation.

```bash
cargo metadata --format-version 1 --no-deps
cargo test -p aero-core --test import -- --nocapture
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected after implementation: metadata reports exactly the six initial members, the named import test executes and passes, and all commands exit 0. Missing tool/dependency setup is a blocker, not a meaningful behavioral red. No result above has been produced at this documentation baseline.

### 00.5 — A04 truthful doctor and browser lock

Files: `packaging/browser-lock.json`, `scripts/lock-browser.sh`, `crates/aero-cli/src/doctor.rs`, `crates/aero-cli/tests/doctor.rs`, and the necessary exports/CLI routing in `crates/aero-cli/src/main.rs`/manifest. Implement one doctor check at a time: missing browser returns code 2 and `browser.available=false`, actual path/version/provenance readback, unsupported sandbox/display/keyring explicitly distinguished from passed support. Doctor never downloads a browser or changes security settings.

```bash
cargo test -p aero-cli --test doctor doctor_missing_browser_is_nonzero -- --exact --nocapture
cargo test -p aero-cli --test doctor
cargo run -p aero-cli -- doctor --json
```

Expected red/green: named test fails before the behavior exists, then runs once and passes; doctor reports actual configured prerequisites and returns nonzero when required ones are unavailable. It must not claim full launch/sandbox validation before C04 supplies the corresponding real checks. Document partial doctor readiness instead of calling ENV-002 closed early.

### Batch stop and first feature test

Stop at a reviewed bootstrap/doctor checkpoint, record exact changed files and real evidence, and ask for scoped commit approval. If any prerequisite remains blocked, report the verified subset without declaring the whole batch complete. After review, the next pure feature is B01 `center_uses_visible_intersection` in `crates/aero-core/src/types.rs`, with complete code/test in implementation-plan §7.3 and the same focused command in CONTRIBUTING.md. Integrate N01 before a session-only persistence design can become established.

## 5. Current blockers and approvals

The tracker owns statuses; this is a navigation summary, not a duplicate issue board:

- ENV-001: user-local pinned compiler verified. ENV-003: user-installed pkg-config 2.5.1 and doctor availability verified; bootstrap blocker resolved. DEC-001 accepted the first-phase scope. Future native-library/backend validation is not implied.
- ENV-002: development browser version/byte pin matches; native sandbox integration and production provenance/update/redistribution review remain required before dependent claims.
- ENV-004/DEC-003: native platform/viewport/keyring evidence needed before dependent support claims; does not stop independent pure Rust work.
- DEC-002: GitHub creation/local initialization accepted and completed; source/PDF licensing remains under DEC-005 and commits still need explicit batch approval.
- DEC-004: real inference/provider/data/budget configuration, only before actual-model work.
- DEC-005: application ID/rights/update/signing/release ownership, only before its dependent release actions.
- DEC-006: held-out source and threshold governance; current status is `holdout_not_available`, with no invented baseline.
- DEC-007: optional backend/feature scope; lean defaults remain off until justified.

GitHub creation and the explained first-phase setup/implementation were explicitly authorized by later user messages. The tracker records their scope. No commit/push, provider/account use, signing/publication, browser redistribution or optional-backend approval has been recorded; setup approval is not blanket authority.

## 6. Verification record for this documentation delivery

Actual documentation checks passed on 2026-10-22, using Python standard-library validation and Node syntax checking without creating probe files:

- The complete read-only documentation command embedded in CONTRIBUTING.md was extracted and executed from the workspace root. Observed output: `PASS: 6 documents, links/fences/whitespace, 26 requirements, 31 corrections, 100 ledger packages`; exit 0.
- Extended validation checked 29 local document links including fragment anchors; 100 unique work-package definitions exactly matched the 100 initial `not_started` ledger entries; all four ENV and seven DEC records matched their indexes and references.
- One JSON fenced example and two Rust raw JSON fixtures parsed. One JavaScript block passed `node --check -` through stdin. Parsing JSON inside Rust is not compiling Rust.
- The SQL schema was exercised in memory using synthetic records: schema version, foreign keys, immutable contract/grant payload, nonnegative budget, transactional rollback, deletion cascades, and final foreign-key consistency passed. No application database was created or tested.
- The original PDF's SHA-256 remained `31d4abdce5b505c8c1e31886077d301df47f516e2a8fe64d047824a4cbf39d17`. Workspace inventory contained only the six living/redirect Markdown documents plus the original PDF; no app source, Cargo manifest, or Git directory was created.

Application build/tests, Rust snippet compilation (including the overflow-safe geometry reference), browser integration, actual-model/held-out evaluation, native GUI/keyring, packaging, and remote CI have not been run. Expected commands elsewhere are not evidence of execution. No `git diff --check` result is claimed because this is not a Git repository.

## 7. Parallel work and transfer discipline

After setup/contracts stabilize, independent core-model tests and CDP transport tests may be delegated with explicit non-overlapping paths. Serialize shared manifests/module exports and all store/controller/runtime/recovery ownership changes through the integrating implementer. N10 native validation can proceed alongside the CLI slice once its actual input/runtime prerequisites exist. No child can approve grants/commits or close tracked work solely from its own summary.

Before handing off, record:

```text
Checkpoint / batch and exact package.subtask:
Current workspace, branch, HEAD or no repository:
Owner / reviewer and writable file scope:
Approved action scope and pending decision IDs:
Actual changed files, including uncommitted/untracked work:
Red test command/name/result:
Green and affected-suite commands/results:
Artifact/effect/readback evidence:
Not run, unsupported environments, remaining uncertainty:
Plan-ledger and tracker updates:
Processes started by this work and exact cleanup/ownership:
Next test file, test name, exact command, expected behavior:
Commit approval and actual SHA, or no commit:
```

Do not erase the prior checkpoint's evidence or describe a task as complete because the plan was completed. Retain concise batch history below and link larger reviewed evidence records from the plan ledger. Never include credentials, private pages, runtime profiles, or hidden model reasoning.

## 8. Batch history

### BATCH-00 commit approval

User request: “ok, let's commit and push.” Primary implementation commit: 5a4fac91e5638735aa1cd762c183f63c9efa39de, feat: bootstrap AeroBrowser workspace and readiness diagnostics. All bootstrap tests, build, formatting, warning-denied clippy and documentation/staged-scope checks passed. This accompanying documentation checkpoint records the real primary SHA rather than inventing a self-referential commit ID. Delivery verification compares final local HEAD with remote main; excluded artifacts remain local.

### BATCH-00 — initial implementation baseline, 2026-10-22

Follow-up: user reported pkg-config installed; independent version/path and doctor checks confirmed 2.5.1. Bootstrap tests/fmt/clippy passed. All A01–A04 bootstrap scopes are now ready_for_review; native runtime/feature qualification remains unverified. A new terminal initially lacked the user-local Cargo PATH; restoring the already documented PATH was sufficient, with no new installation or shell configuration change.

- Implemented and exercised: pinned Rust scaffold, core import/version contract, CLI doctor, development browser pin check and source/requirement/architecture documentation. Actual red/green and quality evidence is recorded in §0.
- Source scope: root Cargo.toml/Cargo.lock/rust-toolchain.toml/.gitignore; manifests and lib.rs for core/CDP/perception/executor/agent; core import.rs; CLI manifest/main.rs/doctor.rs/tests/doctor.rs; packaging/browser-lock.json; scripts/lock-browser.sh. No task/runtime/provider/GUI implementation added.
- Documentation scope: docs/requirements.md, docs/architecture.md, ADRs 0001–0003, docs/development.md, plus current plan/handoff/tracker/contributor updates. Specification product requirements and original PDF unchanged.
- Installed outside repository: user-local Rust and testing-browser cache only. No system package install, security-policy change, commit, staging, push or file upload. Native prerequisite and integration limitations remain explicit.
- Status: A01/A03/A04 baseline ready_for_review; A02 blocked on native pkg-config setup; all later packages not_started. Future dedicated SPEC-23/SPEC-26 proof names remain a documented plan refinement before their dependent feature acceptance, not hidden passing coverage.

### DOC-001 — contributor-facing document baseline, 2026-10-22

- Scope: publish the existing detailed plan with priorities/100-package ledger and ordering clarifications; add handoff, contributor guide, and local bug/fix/blocker/decision register; update specification authority link and previous plan redirect.
- Files: `docs/implementation-plan.md`, `docs/handoff.md`, `CONTRIBUTING.md`, `docs/bug-fix-tracking.md`, `docs/specification.md`, `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md`.
- Application work: none. Installs, Git initialization, commits, accounts, provider calls, release, and remote writes: none.
- Verification: documentation links/anchors, mappings, ledger/tracker coverage, JSON/JavaScript/SQL checks and PDF/scope checks passed; actual evidence and unrun application/native gates are recorded in §6.
- Next action: owner document review and bounded setup/Git decisions, then BATCH-00; no source implementation begins as a side effect of this delivery.
