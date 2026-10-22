# AeroBrowser — implementation handoff

Document-set revision: 2.3, 2026-10-22. Product specification remains revision 2.0. Status: B01 pure primitives committed as 54e0a9573d8eb90746243f0f24056b05a3022988. This approval/status update follows the feature commit; consult Git for its final metadata-commit SHA and verify remote main separately. Browser/runtime/native feature qualification remains unverified.

## Current B01 checkpoint

This is the current transfer record; the BATCH-00 and documentation-only sections below are historical evidence, not instructions to repeat setup. The user approved B01 after a proposal to implement typed IDs and safe geometry, then stop for review before any commit. After the review report and passing checks, the user explicitly said “commit and push.” That authorizes publication of this B01 batch and its approval/status record, not implementation or publication of later packages.

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

Review/state: implementation self-review found no remaining B01 primitive defect; integrating checks above pass. The full workspace/build/fmt/Clippy and documentation/scope gates were rerun successfully immediately before committing the approved exact 11-file batch as 54e0a9573d8eb90746243f0f24056b05a3022988, `feat(core): define scoped IDs and CSS geometry`. The tree was clean after that commit. Ledger state is committed; this follow-up changes documentation only to record the actual SHA/approval. Push is authorized, but remote delivery must be read back from main before reporting success. No browser, provider, account or host-security operation was needed, and no long-running process was started by this batch. ENV-002/ENV-004 remain unverified native/runtime gates, not pure-core blockers.

Next safe action after verifying the authorized push: propose B02 strict action/decision contracts, then B03 snapshots/events and N01 TaskContract before B04–B06 storage. Do not begin those packages as part of finishing B01. They require separate approval.

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
