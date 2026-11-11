# AeroBrowser — bug, fix, blocker, and decision tracker

Updated 2026-11-10 for the host-migration checkpoint and D05; see the [host-migration record](handoff.md#previous-checkpoint--host-migration-and-native-baseline-2026-11-10) and the [D05 record](handoff.md#current-checkpoint--d05-frame-aware-grounded-clicks-2026-11-10). This remains the authoritative local issue register. Everything through D04 is committed and pushed (main at ddc1c40); fixes for BUG-001–BUG-005 are in those commits and await owner closure. The remote was created private (DEC-002) but was anonymously readable on 2026-11-10; the owner should confirm the intended visibility. Hosted issue migration, public release and private-evidence publication are not approved. Verify actual remote delivery separately.

Read [the specification](specification.md), [implementation plan](implementation-plan.md), [handoff](handoff.md), and [contribution rules](../CONTRIBUTING.md). Bootstrap code and prerequisite diagnostics now exist. Deliberate TDD red states, missing setup and untested platform assumptions are not disguised as production defects or completed fixes.

## 1. What belongs here

| Prefix | Meaning | Authority / relationship |
|---|---|---|
| BUG-NNN | Reproduced discrepancy between implemented behavior and an accepted requirement | This tracker owns reproduction, severity, fix, regression evidence, and closure. |
| ENV-NNN | Missing or unverified environment/tool/platform prerequisite | This tracker owns observed evidence and unblock/validation conditions. |
| DEC-NNN | Owner decision/approval needed before dependent work | This tracker owns pending/accepted/rejected/superseded decision records; an accepted decision is not evidence its implementation passed. |
| SPEC-01–SPEC-31 | Existing source-design corrections, not reproduced app bugs | Preserve the full register in implementation-plan §3; do not duplicate 31 fake bug reports here. |
| REQ-* | Accepted product obligations | Specification §13 is authoritative; link exact plan package and test when filing a bug. |
| A01–N24 package IDs | Work to implement the product | Plan §0 owns execution status; do not maintain a competing backlog here. J packages are feasibility tracks. |

Allocate the next unused integer separately for each BUG/ENV/DEC prefix. Never reuse or renumber IDs. Keep full closed records; link duplicates to a canonical record. Examples below use NNN intentionally and are not live issues. Future hosted issues must preserve a backlink/ID mapping; migration requires owner approval and reviewed data classification.

## 2. Severity, priority, and release blocking

Severity describes impact; priority describes scheduling. A missing compiler can be P0 setup work without being an S0 security vulnerability.

| Severity | Definition / handling |
|---|---|
| S0 — critical | Confirmed secret/private-data exposure, authorization/sandbox bypass, unintended irreversible effect, duplicate high-impact effect, or destructive data loss. Stop the affected path, privately notify the owner through an approved channel, preserve sanitized evidence, and block affected releases. |
| S1 — high | False verified success, broken required workflow, loss of durable task/budget/uncertainty state, stuck cancel/takeover, or material integrity failure. Block the affected required milestone/release. |
| S2 — moderate | Recoverable defect with a safe supported workaround outside critical invariants; nonessential feature or limited compatibility failure. Scope impact explicitly; cannot waive a required gate merely by labelling it S2. |
| S3 — low | Cosmetic/documentation/usability issue that does not misrepresent authority, outcome, supported behavior, or safety. |
| N/A | Environment prerequisite or owner decision, with priority and affected gates recorded instead of a fictitious application severity. |

Priorities: P0 = current batch/safety blocker; P1 = before the affected milestone ships; P2 = measured enhancement or later supported scope. Any failing mandatory case blocks its declared gate regardless of issue priority or aggregate score. Owner acceptance can defer optional scope but cannot convert an unauthorized effect or a missing mandatory test into a pass.

## 3. Workflow and fix acceptance

Bug workflow:

`reported → triaged → reproducing → fixing → ready_for_review → verified → closed`

`blocked` links a prerequisite and preserves the prior state. `duplicate`, `not_a_bug`, and `deferred` require a reason, canonical record/scope decision, and reviewer. Reopening preserves previous fix evidence and adds new reproduction data rather than overwriting history.

Environment records use `blocked`, `needs_validation`, `ready_for_review`, `verified`, and `closed`. Decision records use `pending`, `accepted`, `rejected`, and `superseded`, with actual decision text and approval reference. A DEC acceptance does not close its related ENV record until the actual verification condition is satisfied.

For each code fix:

1. Reproduce against the actual code/configuration and record expected versus observed behavior, affected requirement and runtime boundary. If implementation does not exist, use a plan correction or environment record instead.
2. Add the smallest deterministic regression test using synthetic fixtures. Run it to obtain a meaningful behavioral failure; a missing tool is not red.
3. Identify the root cause and sibling paths (Enter/submit, replay, workers, MCP, resume, provider fallback, etc.). State why the fix covers the class without widening unrelated scope.
4. Implement minimally, run the exact regression to green, then affected suites/feature/native gates. Read back actual effects/artifacts; dispatch acknowledgement alone is not proof.
5. Submit a sanitized diff/evidence record. The integrating reviewer independently verifies the reported result and updates status; subagent self-reports do not close records.
6. Obtain explicit user approval before a scoped commit. Record the actual SHA if committed; never make up a commit or call uncommitted work committed.
7. Close only after reproduction is resolved, regression and affected gates pass, evidence/limitations and review are recorded, and required documentation/ledger/handoff are updated. A verified uncommitted local fix may close only with explicit reviewer acceptance of that state; commit approval remains separate.

For an environment issue, close only after actual tools/backend checks pass in the intended environment and the handoff records the new facts. For a decision, record the owner's exact accepted scope and what remains unapproved. Decision entries are not authorization until that response exists.

## 4. Live register

### Application defects

N02's current workspace passes 143 behavior tests/eleven compile-fail doctests. Required model-revision denial, persisted start prerequisites and truthful draft/paused state were observed red/green. Nine task integration tests, three capability compile-fail proofs, design/source review and final gates pass; no actionable N02 finding remains. Capability scope/lifetime and authentication/egress limits are explicit in the handoff. Existing BUG/ENV records are unchanged; older checkpoint counts below are historical, not current totals.

B06's current workspace passes 134 behavior tests/eight compile-fail doctests. Its required duplicate-owner assertion was observed red/green; eight private-state/lease integration tests and design/source review are complete with no remaining in-scope defect identified. Existing BUG/ENV states are unchanged. Cooperative filesystem ownership is not actual dispatch/takeover authority or native qualification. Counts and next-action text in the older paragraphs below are historical.

B05's current workspace passes 126 behavior tests/eight compile-fail doctests. Atomicity, stale-run/live-recovery, reservation-owner/checkpoint-cursor and two final-review corruption defects were reproduced and fixed within the unshipped batch; exact red/green evidence is in the current handoff. The final review's action-state/outcome and evidence/artifact payload findings are covered by parent-run repository regressions. No outstanding B05 defect was identified after integration; this is not N02 authority, money-budget, encryption or real crash qualification. Earlier checkpoint counts below are historical; existing BUG/ENV records are not closed by this new batch.

B04 now adds seventeen schema/private-open tests; the final workspace has 104 behavior tests and eight compile-fail doctests. Required schema-version and schema-prefix/ancestor hardening failures were observed red/green during this unshipped batch, not filed as fabricated shipped defects. Source review identified no remaining in-scope blocker; parent-run tests include failed-open lock release and auxiliary-file byte preservation. B04 establishes initial SQL/storage structure only: B05 transactions/queue, trusted authority, encryption and recovery remain future work. Existing BUG-001–004 and ENV statuses below are unchanged; N01 counts in the following historical paragraph describe its own checkpoint, not the current total.

The pre-N01 audit reproduced/fixed BUG-001 and three additional CLI issues below; its 70 behavior tests and seven compile-fail doctests remain historical evidence. N01 adds 17 TaskContract integration tests and one compile-fail example for a current 87 behavior tests/eight compile-fail doctests. N01's deliberate red scaffolds and pre-review mandatory-stop/local-export/mapped-IPv6/required-row-count refinements are recorded with parent-observed red/green in the handoff rather than fabricated shipped BUG records. Both final source-review findings are fixed and gates rerun; no remaining N01 pure-contract blocker was identified. Existing audit fixes remain ready for owner review. None of these uncommitted fixes is closed merely on a subagent report. Browser/task execution, persistence/encryption, policy and recovery remain unimplemented. ENV-002/ENV-004 are unchanged unverified runtime/native gates, not closed by pure-core tests.

| ID | Severity / priority | State | Owner | Defect / blocked gate |
|---|---|---|---|---|
| BUG-001 | S2 / P0 | ready_for_review | Hermes | Authorized map-only Action fix and two all-variant repository regressions are green; final audit/gates pass. Committed in fc228c7; not closed. |
| BUG-002 | S2 / P1 | ready_for_review | Hermes | Diagnostic probe descendants outlive their root/timeout; dedicated group cleanup and regression tests pass. Committed in 8f8b235; not closed. |
| BUG-003 | S2 / P1 | ready_for_review | Hermes | Pin helper requires absent python alias on Python-3-only systems; explicit python3/preflight and adapter regressions pass. Committed in 8f8b235; not closed. |
| BUG-004 | S2 / P2 | ready_for_review | Hermes | Unbounded executable hashing can stall doctor; regular-file/1 GiB/read/deadline limits and regression tests pass. Committed in 8f8b235; not closed. |
| BUG-005 | S2 / P1 | ready_for_review | Hermes | Configured root viewport only sized the headless window (pages got 1265 × 657); applied per attached page with `Emulation.setDeviceMetricsOverride`; regression assertion in `click_has_one_event_sequence`. Committed in 5c272d5; not closed. |
| BUG-006 | S2 / P2 | ready_for_review | Hermes | Debug-profile doctor missed its two-second hashing limit on the 300 MB pinned browser, so `scripts/lock-browser.sh` reported FAIL for matching bytes on the native host (fails closed). `sha2` is now built optimized in the dev profile; regression test red then green. Committed 2026-11-10; not closed. |
| BUG-007 | S3 / P3 | triaged | Unassigned | `Emulation.setDeviceMetricsOverride` on the page session does not reach cross-site out-of-process frames: with device scale factor 2 they keep `devicePixelRatio` 1. Latent: the configured factor is 1 and clicks use CSS pixels; affects any future non-1 factor and frame screenshots. Not fixed. |

#### BUG-001 — B02 action enum accepts positional arrays

- Requirement/package: B02 / §7.1's mandatory object-only action wire shape; complements SPEC-26/SPEC-27's partial typed-wire foundations.
- Environment/revision: main af88105 plus local uncommitted B02/B03, pinned Rust/Cargo 1.99.1, serde 1.0.229, default features; no browser or provider.
- Actual reproduction: a scratch Rust harness compiled against the built core and existing test-only serde_json. `action_array_is_rejected` feeds `["click","target-1"]`; `nested_action_array_is_rejected` feeds the same array as `action` in a valid version-1 Decision. Both expect decoding failure but receive Ok; running `/home/davia/.hermes/cache/scratch/aero-b03-sibling-array-probe --nocapture` exits 101, two failed tests. Source/evidence: sibling-array-probe `.rs`/`.log` at that same scratch prefix; artifacts are local and ephemeral, not committed regression coverage.
- Root cause/siblings: derived internally tagged enum decoding accepts sequences. B03's FieldValue/EventKind had the same issue and now use a shared map-only tagged decoder with observed direct/nested red-to-green. Decision's map-only outer envelope does not protect its inner Action enum.
- Impact/containment: S2 pure-parser contract discrepancy with no runtime consumer or demonstrated authority bypass. P0 before B02 review/commit: do not treat the green existing suite or successful parameter validation as proof of required shape/authorization. Do not enable a planner/executor on this parser while this mandatory case fails.
- Actual minimum fix under accepted DEC-008: Action now uses the existing schema-once map-only tagged helper; valid object serialization, optional/null scroll-target semantics, Debug/PartialEq behavior and existing validation remain unchanged. No dependency or runtime behavior added. The earlier B03 preservation baseline is historical; the new user approval explicitly permits this actions.rs correction.
- Repository regression evidence: `cargo test -p aero-core action_requires_an_object_for_every_variant -- --nocapture` and `cargo test -p aero-core decision_rejects_positional_action_arrays -- --nocapture` each executed one test and failed with exit 101 before the parser change, accepting Click arrays. Both now pass across all 18 declared variants, direct and nested. Existing valid-object roundtrips, optional target, duplicate/null-first fields and bounds remain green; at that fix step full core passed 50 unit tests, one import and seven compile-fail doctests; systematic rejection hardening subsequently raises core unit coverage to 51. Workspace/build/fmt/warning-denied Clippy and core rustdoc gates pass as well.
- Approval/review: DEC-008 accepted by the user's explicit follow-up; prior unanswered form is historical. Integrating reviewer: Hermes. Existing-code audit/source-review integration is complete and the fix remains ready_for_review, not closed or committed. No staging/push performed; owner review and explicit commit approval remain separate.
- Commit (recorded 2026-11-10 from Git): the fix is in fc228c7, pushed. Statements above that it is uncommitted predate that commit. Still ready_for_review: closure needs the owner or integrating reviewer.

#### BUG-002 — Doctor leaves diagnostic probe descendants running

- Scope/severity: A04 diagnostic lifecycle, S2/P1; reproducible process leakage, not a demonstrated browser sandbox escape. User's DEC-008 existing-code audit authorization covers the repair; no C04 runtime implemented.
- Parent reproduction: `cargo test -p aero-cli --test doctor doctor_probe_cleans_descendants -- --nocapture` executed two tests; both failed (exit 101) after observing live synthetic `sleep` descendants. Fixtures explicitly killed leaked processes before failing, so red-state tests did not leave running fixtures.
- Root cause: only immediate-child timeout killing, with no owned process group and no cleanup on normal root exit/stdout timeout.
- Fix: `doctor.rs` starts each trusted diagnostic in its own process group and uses Linux waitid/WNOWAIT to observe exit without reaping the leader. Cleanup signals that still-owned group before the final direct-child wait/reap; consuming the group ID prevents another signal on Drop. Normal/nonzero root exits clean the group before stdout EOF; timeout/error paths use the same guard. Nondefault/auto-reaping SIGCHLD policy is refused before spawn, and an unexpected ECHILD discards numeric signalling authority. This is not hostile process containment or a guarantee against deliberately detached descendants.
- Review refinement/proof: the first group-cleanup repair still used reaping try_wait, which could release its PID before signalling. Parent's `probe_keeps_leader_unreaped_until_group_cleanup` regression failed exit 101 before WNOWAIT (getpgid returned -1), then passed after the repair. This proves the ownership-order violation, not a forced rare PID-reuse collision. The group-cleanup, no-double-signal and wait-policy tests plus all original doctor tests pass; source finding is resolved by the integrating parent, not closed on a child claim.
- Dependency adjustment: root/CLI manifests explicitly reuse already-locked libc 0.2.191 for correct platform layouts/constants and waitid/killpg/sigaction bindings rather than hand-code siginfo_t. Offline Cargo changed only the aero-cli→libc edge in Cargo.lock; the package/version/checksum set is unchanged. No package installation, update, network, browser, host-setting, staging/commit/push or account use. The initial no-manifest-change expectation is superseded by this bounded correctness repair.
- Commit (recorded 2026-11-10 from Git): the fix is in 8f8b235, pushed. Statements above that it is uncommitted predate that commit. Still ready_for_review: closure needs the owner or integrating reviewer.

#### BUG-003 — Browser pin helper requires an undocumented python alias

- Scope/severity: A04 shell adapter, S2/P1; a Python-3-only environment could not use the documented helper.
- Parent reproduction: new `cargo test -p aero-cli --test browser_pin_helper -- --nocapture` ran three tests, all failed (exit 101). A controlled PATH exposed Python 3 but no `python`; the helper returned 127 instead of parsing a synthetic valid report. Missing-Python preflight and mismatch/failed-Cargo cases also failed at the absent alias.
- Fix: `scripts/lock-browser.sh` explicitly checks `python3` before running Cargo, exits 2 with an actionable message if absent, and invokes python3 for JSON parsing. Contributor checks use python3 as well; the development guide names the prerequisite. No Python package/install or download added.
- Proof: all three adapter tests pass for Python-3-only success, fail-before-Cargo without Python 3, mismatching pin rejection and original Cargo-error propagation. Cargo reports are deliberately synthetic shell-adapter fixtures, not claimed browser qualification. No pin change; uncommitted, ready for owner review.
- Commit (recorded 2026-11-10 from Git): the fix is in 8f8b235, pushed. Statements above that it is uncommitted predate that commit. Still ready_for_review: closure needs the owner or integrating reviewer.

#### BUG-004 — Executable integrity hashing is unbounded

- Scope/severity: A04 diagnostic robustness, S2/P2; an oversized/self-growing or stalled executable read could hang a diagnostic despite its version deadline. Ordinary-browser failure or integrity bypass was not demonstrated.
- Parent reproduction: `cargo test -p aero-cli --bin aero integrity_rejects_oversized_files_before_hashing -- --nocapture` executed once and failed (exit 101); a sparse 1 GiB+1 fixture was not promptly rejected. The failed filtered test process ended its hash worker; the fixture held no real browser bytes.
- Fix: regular-file and 1 GiB metadata cap, an actual-byte read cap (including post-metadata growth), deadline checks between chunks and a separate two-second caller wait for the integrity job. Size/type/timeout failure reports browser_lock unavailable, never matched or a sandbox success.
- Proof: oversized-file rejection, unchanged small-file digest and a deliberately blocked-job timeout test pass. Filesystem work is not joined on timeout: already blocked kernel I/O cannot be canceled by Rust threads, and the one-shot doctor's process exit ends outstanding threads. This bound is not browser-runtime recovery, file authenticity or a hard real-time scheduling guarantee. The hashing repair adds no package/pin change or real browser operation; uncommitted, ready for owner review.
- Commit (recorded 2026-11-10 from Git): the fix is in 8f8b235, pushed. Statements above that it is uncommitted predate that commit. Still ready_for_review: closure needs the owner or integrating reviewer.

#### BUG-005 — Configured root viewport is not applied to pages

- Scope/severity: C03/C05 runtime contract, S2/P1; the plan's default root viewport (§4: 1280 × 800 CSS px, device scale factor 1) and the launcher's `Viewport` are passed only as `--window-size`, which sizes the headless window, not the page. Grounded input (D02) reads live layout metrics, so clicks were not misplaced; observations, screenshots and any fixed-coordinate assumption were taken at an unplanned size.
- Reproduction: the D02 spike measured a 1265 × 657 CSS px layout viewport with `--window-size=1280,800`; `click_has_one_event_sequence` then failed its `[innerWidth, innerHeight, devicePixelRatio] == [1280, 800, 1]` assertion before the fix.
- Fix (CH-15): `SandboxSpec` keeps its requested `Viewport`; `BrowserRuntime::attach` applies it with `Emulation.setDeviceMetricsOverride` (device scale factor 1, not mobile) before returning the page, and an attach whose override fails is an error.
- Proof: the same assertion passes against the real browser; a mutation replacing the override is caught. Uncommitted, ready for owner review with D02.
- Commit (recorded 2026-11-10 from Git): the fix is in 5c272d5, pushed. Statements above that it is uncommitted predate that commit. Still ready_for_review: closure needs the owner or integrating reviewer.

#### BUG-006 — Debug-profile doctor cannot hash the pinned browser within its deadline

- Scope/severity: A04 diagnostic and `scripts/lock-browser.sh`, S2/P2. It fails closed: a matching browser is reported unavailable; a mismatching one is never reported as matched. The documented pin check cannot pass in Cargo's default profile on the affected host.
- Environment: main ddc1c40, native Ubuntu 26.10 host (AMD Ryzen AI 7 350), Rust 1.99.1, pinned Chrome for Testing executable of 300,842,312 bytes whose `sha256sum` equals `executable_sha256` in `packaging/browser-lock.json`.
- Reproduction (2026-11-10): `AERO_CHROMIUM=<pinned chrome> cargo run -q -p aero-cli --locked -- doctor --json` reported `browser_lock.status` `unavailable` with “Executable integrity check exceeded its two-second deadline.” in 3 of 3 runs, so `bash scripts/lock-browser.sh "$AERO_CHROMIUM"` printed FAIL and exited 1. `target/release/aero doctor --json`, built from the same source, reported `matched_local_pin` in 0.17 s of wall time; `sha256sum` on the file took 0.14 s.
- Expected: the helper passes for the pinned executable, as [development.md](development.md) documents.
- Cause: the two runs differ only in Cargo profile, so the unoptimised hashing loop is too slow for a file this size within BUG-004's two-second limit. Why the same check passed on the WSL2 host is not established.
- Regression test: `integrity_hashes_a_browser_sized_file_within_the_deadline` in `crates/aero-cli/src/doctor.rs` hashes a sparse 320 MiB file through the same `sha256` path and compares the known digest. `cargo test -p aero-cli --locked --bin aero integrity_hashes_a_browser_sized -- --nocapture` executed one test and failed (exit 101) with `TimedOut` after 2.00 s.
- Fix: root `Cargo.toml` adds `[profile.dev.package.sha2] opt-level = 3`. The same command then passed in three consecutive runs (0.16–0.17 s); `bash scripts/lock-browser.sh "$AERO_CHROMIUM"` prints PASS and the dev-profile doctor reports `matched_local_pin`. BUG-004's two-second limit and 1 GiB bound are unchanged; `Cargo.lock` is unchanged; release builds were never affected. Workspace gates pass (302 tests, 13 doctests).
- Limits: this is a timing test. It has a twelvefold margin on this host; on a machine too slow to hash 320 MiB in two seconds it fails, as the real check would. Considered and not done: running the helper in the release profile, or relating the limit to file size.
- Review/commit: not independently reviewed; committed and pushed on 2026-11-10 at the owner's “yes” (SHA in Git); closure needs the owner or integrating reviewer.

#### BUG-007 — Device scale override does not reach cross-site frames

- Scope/severity: C05 runtime contract (BUG-005's per-page override), S3/P3, latent. `BrowserRuntime::attach` applies the configured viewport with `Emulation.setDeviceMetricsOverride` on the page session only. Out-of-process frames render in their own processes and do not inherit it.
- Reproduction (D05 spike and `nested_frame_coordinate_calibration`, native host, 2026-11-10): after a page-session override with device scale factor 2, the main frame, same-process frames and a same-site nested leaf report `devicePixelRatio` 2; the cross-site frames report 1.
- Impact: none on current code. The default factor is 1, and grounded clicks work in CSS pixels; the calibration scenario's clicks land at factor 2. A future non-1 factor would render cross-site frames at the wrong density in screenshots and in anything reading `devicePixelRatio`.
- Not fixed or regression-tested. A candidate fix applies the same override in each attached out-of-process frame session as the frame manager configures it; whether the browser honours it there is unverified.

### Environment blockers

| ID | Priority | State | Owner | Observed condition | Blocks |
|---|---|---|---|---|---|
| ENV-001 | P0 | verified | Hermes | User-local Rust/Cargo 1.99.1 with rustfmt/clippy; core import and workspace build/test/lint pass | No longer blocks pure Rust work |
| ENV-002 | P0 | needs_validation | Hermes | Pinned test browser provisioned on both development hosts with matching local byte pins; sandbox tests pass on WSL2 only and fail on the native host (ENV-005); no independent publisher SHA-256 | C04/C07 browser gates, production provenance review, G0/G1/GV |
| ENV-003 | P0 | verified | User installed; Hermes verified | pkg-config 2.5.1 available at /usr/bin/pkg-config (canonical /usr/bin/pkgconf); doctor detects it; bootstrap tests/fmt/clippy pass | Bootstrap tool-availability blocker resolved; future native library/backend qualification remains separate |
| ENV-004 | P1 | needs_validation | Unassigned | First native run (Ubuntu 26.10, 2026-11-10): non-browser gates pass, the browser sandbox is blocked by ENV-005; the Ubuntu 24.04 baseline, X11/Wayland interaction and Secret Service are untested | GV, native vault integration, G4/G5 support claims |
| ENV-005 | P0 | verified | Project owner (DEC-014); Hermes verified | On native Ubuntu 26.10 AppArmor runs everything Bubblewrap starts under a capability-denying profile, so Chromium inside the sandbox exits with “No usable sandbox!” | Every browser-backed test on this host: C04–C07 and D02–D04 re-verification, D05–D07 and later browser packages; native ENV-002/ENV-004 evidence |

#### ENV-001 — Rust toolchain unavailable

- Type / severity: environment / N/A. Observed 2026-10-22, active workspace `/home/davia/projects/aero-browser`.
- Evidence: read-only `shutil.which` checks returned null for `cargo`, `rustc`, `rustup`; no Cargo manifest or source tree exists.
- Dependency: DEC-001 now records the user's approval to proceed with the explained first phase.
- Next action: approve and perform A02's compiler/tool setup, select/pin the exact supported compiler during A03, and document actual versions. Tool installation and lockfile resolution are not the same operation.
- Unblock proof: `rustc --version` and `cargo --version` exit 0; the approved compiler matches `rust-toolchain.toml`; A03 `cargo metadata --format-version 1 --no-deps` and import test execute successfully. Record outputs, not anticipated versions.
- Actual unblock evidence: official rustup minimal install followed by pinned 1.99.1 plus rustfmt/clippy; actual versions, six-member metadata, core import red/green, workspace build/test, formatting and warning-denied clippy passed. Parent reviewed real output. Initial evidence above is historical; see docs/development.md for current setup. No commit.
- Second host (2026-11-10, native Ubuntu 26.10): the snap-packaged rustup 1.29.2 had no toolchain; the pinned 1.99.1 (same `rustc`/`cargo` builds as above) was installed with the minimal profile plus rustfmt and clippy, with the owner's approval, and became rustup's default. Formatting, warning-denied Clippy, build and the workspace tests pass there (see the handoff for counts and the `TMPDIR` condition).

#### ENV-002 — Linux browser provisioning and provenance missing

- Type / severity: environment / N/A.
- Evidence: read-only PATH checks returned null for `chromium`, `chromium-browser`, and `google-chrome`. A separately supplied off-PATH executable has not been checked; do not claim no browser can exist anywhere on the machine.
- Dependencies: DEC-001 approved provisioning; A03 before doctor code; A04 browser provenance/update policy before required integration.
- Next action: choose a maintained real Linux Chromium executable, record channel/version/license and verifiable provenance in `packaging/browser-lock.json`, then test the supported sandbox combination. Do not treat a Windows browser, Snap wrapper, or HTTP-mocked process as that proof.
- Unblock proof: `"$AERO_CHROMIUM" --version`, truthful `aero doctor --json` after implementation, required `launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`, and failure-cleanup tests execute in the approved Linux environment. Record independently sourced checksums where the selected channel supplies them; absent publisher hashes require a documented reviewed channel policy, never an invented value.
- Provisioning update: vendor Stable metadata selected Chrome for Testing 156.0.8078.47 Linux64/revision 1706539; archive MD5 matched separately fetched Google Storage headers. Local SHA-256 pins are recorded in packaging/browser-lock.json. The real binary --version succeeds, and doctor/lock-browser.sh match the local pin. This is development-only, not redistribution approval or independently published SHA-256. Runtime sandbox/namespace/cleanup tests remain unrun. No browsing session or commit.
- C04 update (WSL2 only): with `AERO_REQUIRE_BROWSER=1 AERO_CHROMIUM=<pinned chrome>`, `launch_reaches_cdp`, `readiness_failure_reaps_child`, `sandbox_cannot_read_host_canary` and `renderer_sandbox_remains_enabled` pass inside Bubblewrap 0.11.1 on kernel 6.18.33.1-microsoft-standard-WSL2 (no AppArmor user-namespace restriction present). Chromium's own namespace + seccomp-BPF sandbox reports "You are adequately sandboxed" and every renderer is seccomp-filtered in a nested PID namespace. Only local `about:blank`, `file:` and `chrome://sandbox` pages were opened. Still open: native Ubuntu 24.04 (AppArmor `apparmor_restrict_unprivileged_userns`, ENV-004), doctor reporting these results (`runtime_ready` stays false), provenance/redistribution policy.
- Native host update (2026-11-10): the lock's archive was downloaded again; archive and executable SHA-256 both equal the lock, and `--version` matches. The C04 browser tests do not pass there: `launch_reaches_cdp`, `sandbox_cannot_read_host_canary` and `renderer_sandbox_remains_enabled` fail at browser start (ENV-005). The debug-profile pin helper also reports FAIL for these matching bytes (BUG-006); the release-profile doctor reports `matched_local_pin`.

#### ENV-003 — Native build prerequisites not prepared

- Type / severity: environment / N/A.
- Evidence: `pkg-config` is absent from PATH. `/etc/os-release` reports Ubuntu 26.04.1 LTS under WSL2, not the native Ubuntu 24.04 release baseline. Other required libraries/toolchains are unverified, not assumed absent.
- Dependency: DEC-001 approved proceeding with first-phase setup; privileged package authentication remains unavailable.
- Next action: follow A02 using the selected environment's actual OS/package manager. Do not blindly execute the plan's Ubuntu 24.04 example against the current host; inspect package availability and approve the actual change. Defer desktop-only packages until their accepted gate.
- Unblock proof: `pkg-config --version` exits 0; selected native crate dependencies compile; `docs/development.md` records actual environment/versions/commands. A generic package-install success is insufficient if the required library cannot build.
- Actual blocker: sudo -n true returned interactive authentication required. No system package installation performed. User may run the pkg-config commands in docs/development.md in their own authenticated terminal, then native dependency validation must follow. No password is accepted in chat; no host security setting is changed. Pure Rust/CLI tests already pass without pkg-config.
- Follow-up verification after the user reported installation: pkg-config --version returned 2.5.1; command -v resolved /usr/bin/pkg-config; doctor resolved /usr/bin/pkgconf and reported available=true/version=2.5.1. All bootstrap tests, formatting and warning-denied clippy passed after restoring the documented user-local Cargo PATH. ENV-003 is verified for current bootstrap tool availability, not future GUI/keyring/native-library compatibility. No code, shell startup file, commit or push changed.

#### ENV-004 — Native platform and keyring evidence unavailable

- Type / severity: environment validation / N/A, not a confirmed platform defect.
- Evidence: inspected host is WSL2. No native X11/Wayland, accessibility/IME/dialog, nested sandbox, or Secret Service integration scenario was executed. Presence/absence of a usable display/keyring was not established by this documentation pass.
- Dependencies: owner provides appropriate native test access; N10 precedes H01 acceptance; G01 uses a dedicated test D-Bus/keyring session, never real credentials.
- Next action: select native Ubuntu 24.04 test machines/VMs and record actual compositor/display/browser/kernel/keyring versions; execute N10/C04/G01/H06 checklists when the relevant code exists.
- Unblock proof: measured presentation matrix with supported/unsupported results; actual sandbox and test-keyring results; accepted DEC-003 arrangement; native claimed-backend smoke. A WSL screenshot or automated UI test alone cannot close this entry.
- Fix / review / commit: not performed. Pure Rust and CLI work may proceed once their own prerequisites pass.
- Native update (2026-11-10): the project now also runs on native Ubuntu 26.10 (kernel 7.3.0-10-generic, GNOME on Wayland), which is not the 24.04 baseline. Non-browser gates pass. No browser test passes because of ENV-005, so no display, input, accessibility, IME, dialog or keyring evidence exists yet. A D-Bus session and `gnome-keyring-daemon` are present; neither was exercised.

#### ENV-005 — AppArmor denies the nested Chromium sandbox on native Ubuntu

- Type / severity: environment / N/A; P0 because it blocks every browser-backed test on the current development host.
- Observed 2026-11-10: native Ubuntu 26.10, kernel 7.3.0-10-generic, Bubblewrap 0.12.0, pinned Chrome for Testing 156.0.8078.47, main ddc1c40.
- Test evidence: `cargo test -p aero-cdp --locked --test sandbox --features browser-tests` passed 8 and failed 3 (`launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`); `cargo test -p aero-agent --locked --features browser-tests --test scenarios` failed all 13. Each failure is `EarlyExit { code: Some(134) }` with Chromium's “No usable sandbox!” message. `readiness_failure_reaps_child` passed and no sandbox or browser process remained.
- Policy evidence (read-only): `kernel.apparmor_restrict_unprivileged_userns = 1`. `/etc/apparmor.d/bwrap-userns-restrict` attaches profile `bwrap` to `/usr/bin/bwrap` and moves every program it starts into `unpriv_bwrap`, which contains `audit deny capability`; a process inside reports the label `bwrap//&unpriv_bwrap (enforce)`. Inside `bwrap --unshare-user`, `unshare -U true` succeeds, `unshare -U -r true` fails writing `uid_map`, and `unshare -U -r` with `-m`, `-p` or `-n` fails with “Operation not permitted”. A nested user namespace can therefore be created but holds no capabilities, which Chromium's namespace sandbox needs. The installed Google Chrome 157 is exempt only through the profile Ubuntu's `apparmor` package ships for `/opt/google/chrome/chrome`.
- Not tested: the Ubuntu 24.04 release baseline. It enables the same restriction by default, so the §7.8 launch design should be expected to need an AppArmor answer there too; that is an expectation, not a result.
- Product gap: §7.8 requires doctor to fail with an actionable explanation in this situation; doctor reported `user_namespaces` and `sandbox` as `not_checked`. Addressed and committed 2026-11-10: `user_namespaces` now reports `restricted` with the install command, or `restricted_project_launcher` when `AERO_BWRAP` is the launcher; tests `userns_check_*` in `crates/aero-cli/src/doctor.rs` were red against a stub, then green. `sandbox` stays `not_checked`: doctor launches no browser.
- Dependency: DEC-014. Nothing was changed to work around the failure: no `--no-sandbox`, no AppArmor or sysctl edit, no unconfined run.
- Prepared remedy (2026-11-10, not applied): `packaging/apparmor/aerobrowser-bwrap` and `scripts/install-apparmor-profile.sh`, described under DEC-014. The profile compiles with `apparmor_parser --skip-kernel-load --skip-cache`; it had not been loaded at that point.
- Installed and verified 2026-11-10: the owner ran `sudo bash scripts/install-apparmor-profile.sh install`. Read back: `/usr/local/libexec/aerobrowser` is `root:davianc` mode 0750; the launcher is byte-identical to `/usr/bin/bwrap`; `/etc/apparmor.d/aerobrowser-bwrap` equals the repository file. A process started by the launcher is labelled `aerobrowser-bwrap//&aerobrowser-sandboxed (enforce)` and `unshare -U -r -m -p -f -n true` succeeds inside it; under `/usr/bin/bwrap` the label is still `bwrap//&unpriv_bwrap` and the same `unshare` still fails. With `AERO_BWRAP` set to the launcher, `cargo test -p aero-cdp --locked --test sandbox --features browser-tests` passed 11/11 (including `launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`, `readiness_failure_reaps_child`) and `cargo test -p aero-agent --locked --features browser-tests --test scenarios` passed 13/13, in six consecutive runs of each; no browser process remained. Remaining: the profile must be reinstalled after bubblewrap package updates; the Ubuntu 24.04 baseline and the packaged (I03) form are untested; doctor does not yet detect the condition (§7.8).
- Unblock proof: the three C04 tests and the 13 scenarios pass in the chosen environment with Chromium's renderer sandbox enabled, under a recorded owner-approved policy.

### Owner decisions

| ID | Priority | State | Approver | Decision needed | Required before |
|---|---|---|---|---|---|
| DEC-001 | P0 | accepted | Project owner | User approved first-phase setup/scaffold/doctor; user-level Rust and testing browser provisioned, sudo auth unavailable | No commits/pushes/accounts/license/redistribution or host-security weakening approved |
| DEC-002 | P0 | accepted | Project owner | Explicit user request created private GitHub repo and linked main/origin | Repository initialization complete; commits/PDF inclusion/license/publication remain separately gated |
| DEC-003 | P1 | pending | Project owner | Accept measured native-companion/stream presentation and limitations | H01 desktop commitment |
| DEC-004 | P1 | pending | Project owner | Configure actual local/cloud provider/model, recipient/data grants, budgets, and prices or explicit unknown pricing | Actual-model smoke/evaluation, not deterministic local fixtures |
| DEC-005 | P1 | pending | Project owner | Confirm application ID/domain, project/asset/browser rights, browser update/redistribution owner, signing identity and release channel | Packaging/publication claims and release upload |
| DEC-006 | P1 | pending | Project owner | Supply/review protected holdout and freeze required workload/performance thresholds after baseline | N24 optimization/release acceptance claims |
| DEC-007 | P2 | pending | Project owner | Decide whether background service, strong egress/intranet backend, Flatpak, or other measured optional features earn scope | Their separate backend/feasibility implementation gates |
| DEC-008 | P0 | accepted | Project owner | User authorizes BUG-001 repair plus audit/fixes of implemented code before moving on | No N01/new feature, install/account operation, staging/commit/push or publication implied |
| DEC-009 | P0 | accepted | Project owner | No attestation for now: only machine-verified criteria can complete a task; subjective criteria stay inconclusive/needs_review | Full N05 completion gate and transition |
| DEC-010 | P0 | accepted | Project owner | Unshipped v1 journal schema is edited in place; development databases are disposable and older ones are refused; preserving migrations start at the first release | N16/N17/N18 storage changes |
| DEC-011 | P0 | accepted | Project owner | Deletion tombstone keeps only the hashed task key, state and timestamps, permanently; holds cannot stop a deletion once it has started | N16 durable coordinated deletion |
| DEC-012 | P1 | pending | Project owner | Confirm or revise the report claims format implemented as the default in CH-7 (one `aero-claims` JSON block per Markdown report; null evidence = unknown) | Agent report generation (E-series) and the G1 workflow |
| DEC-013 | P1 | pending | Project owner | Confirm or revise the planner-facing observation text format implemented as the default in D01 (one escaped line per node, `@e<n>` handles, priority truncation with an explicit `[omitted …]` line) | Planner prompt construction (E03/E05) and the G1 workflow |
| DEC-014 | P0 | accepted | Project owner | Choose how browser-backed tests (and later the product) run where AppArmor denies capabilities inside Bubblewrap (ENV-005) | D05–D07 and every other browser-backed package on the native host; native ENV-002/ENV-004 evidence; I03 packaging |

#### DEC-001 — Development setup authorization

Accepted scope: after setup, minimal six-crate scaffold and doctor checks were explained, the user said, “Ok, great. Let's proceed.” Work proceeds on the inspected WSL host with official user-local rustup and a development-only Linux testing browser; source/version/integrity and boundaries are recorded in docs/development.md. Native Ubuntu 24.04 release gates remain unproven. Sudo needs interactive user authentication. No commit/push, project license, redistribution, real account/provider use, host-security changes or optional backend is approved by this scope.

#### DEC-002 — Repository and source inclusion

Accepted repository scope: the user explicitly requested the GitHub repository before implementation. Created https://github.com/dave2k77/aero-browser as private/empty, initialized local main, connected origin via SSH, and read back those values. No commit, push, identity change or upload occurred. The original PDF, browser/runtime data and .hermes artifacts are ignored. Commits require explicit batch approval; PDF rights/inclusion and project/browser licensing remain undecided under DEC-005.

Follow-up approval: the user said, “ok, let's commit and push.” The reviewed BATCH-00 baseline was committed as 5a4fac91e5638735aa1cd762c183f63c9efa39de after passing bootstrap tests/build/fmt/clippy, document checks, exact staged-scope review and targeted credential-pattern scanning. This approval includes the accompanying checkpoint-record update, not unrelated feature work. Push completion must be verified by comparing remote main with local HEAD. No PDF, browser binary, profile, secret, target or Hermes session artifact is included.

Observation 2026-11-10: an unauthenticated `git ls-remote` and fetch of https://github.com/dave2k77/aero-browser succeeded, so the repository was publicly readable then. No visibility change is recorded in this tracker, and the repository has no license file (DEC-005). The owner should confirm the intended visibility; nothing was changed.

#### DEC-003 — Desktop presentation acceptance

Recommended candidate: trusted Tauri console plus the actual controlled Chromium native window, with a stream where useful. N10 must first supply native interaction/a11y/IME/focus/selection/dialog/takeover results. If neither candidate meets the required experience, commission separate embedding feasibility instead of silently navigating WebKit to the same URL. No presentation has been tested or accepted.

#### DEC-004 — Inference and data configuration

No default real model is assumed. Configure the real provider/model and approved processor/data categories explicitly; local-only failure pauses, never falls back remotely. Keep provider credentials outside documentation and normal payloads. Unknown monetary pricing stays unknown with token/time caps, not an invented free/limit guarantee. Scripted fixture evaluation remains separately labelled and needs no real-provider decision.

#### DEC-005 — Release ownership and rights

`com.aerobrowser.App` is provisional. A private GitHub source repository now exists, but domain ownership, project/PDF inclusion rights, project/browser/font licensing, production browser provenance/update ownership, redistribution, signing and release channel remain undecided. The test-browser lock records redistribution_approved=false and publisher_sha256=null; local pins are not signatures. Resolve before dependent production artifacts/publication; do not hold pure core tests for a signing identity. Publication requires explicit action approval.

#### DEC-006 — Holdout and quality thresholds

Current status: `holdout_not_available`; no baseline model/browser performance or comparative measurement exists. Owner-curated held-out cases must be unavailable to optimization agents. Freeze release workloads and actual thresholds after baseline and before tuning; repo fixtures are development cases, not a hidden set. Required safety negatives and workflow checks cannot be waived by an aggregate success figure.

#### DEC-007 — Optional scope selection

Default: in-process service, pause-on-close, CSV, sequential execution, SQLite-first bounded history, native package first. Keep daemon/background, Parquet, semantic retrieval, SOM, MCP, workers, metrics, Flatpak, remote workers, and training off until individually justified/approved/tested. A strict egress or continue-on-close request must fail honestly if its required backend is unavailable; it cannot downgrade to a weaker mode. An undecided optional feature does not block the minimal CLI slice.

#### DEC-008 — B02 parser-only scope extension

Accepted: the user now says, “Ok, let's resolve this bug, and let's find any other bugs or issues that needs to be fixed before moving on.” The previous unanswered form remains historical, not an approval. This new response authorizes BUG-001's parser/test correction and a review/reproduction/fix pass over existing implemented contracts, diagnostics and scripts. Preserve valid object wire data and optional/null scroll-target semantics. The lifecycle repair explicitly reuses already-locked libc through a new direct CLI edge for correct platform bindings; no new package/version or download is introduced. This adjustment is documented for owner review rather than hidden behind the initial no-dependency-change expectation. No N01/new feature implementation, install/provider/account operation, staging/commit/push or publication is included. Fixes require concrete reproduction, regression tests, affected gates and updated evidence before owner review.

#### DEC-009 — N05 prerequisite scope

Accepted 2026-11-02: asked how criteria that cannot be machine-checked could ever pass, the owner chose “No attestation for now: such criteria stay inconclusive/needs_review and the task cannot auto-complete; only machine-verified criteria complete.” N05 therefore implements no attestation path, owner or model. A contract with a subjective criterion cannot reach completed; it stays needs_review. Revisiting attestation is a new decision, not an extension of this one.

Implementation note (CH-6): the completion gate has no attestation path. Criteria it cannot check by machine (currently evidence coverage and any non-CSV check) are recorded Inconclusive and block completion.

Historical record before acceptance — partially resolved: the owner authorized N17/N18, but those packages now have only partial foundations and are blocked on the missing prerequisites in the [current handoff](handoff.md#current-n17n18-blocked-checkpoint). An explicitly permitted, attributed trusted-user attestation contract remains a separate decision. The current N05 pure predicate and narrow journal test do not establish completion, artifact/evidence truth or attestation. Finalization must close the transaction/freshness race described in the [N05 handoff](handoff.md#current-n05-checkpoint). No reset, migration, staging, commit or push was implied by the prerequisite approval.

#### DEC-010 — Unshipped schema evolution policy

Accepted 2026-11-02: no AeroBrowser state directory existed on the development host when this was decided. The owner chose “Keep editing the unshipped v1 schema in place; dev databases are disposable and old ones are refused (current behaviour); real migrations start at the first release.” Storage changes for N16/N17/N18 edit `001_initial.sql` directly and keep `user_version = 1`; exact schema identity checks continue to refuse older development databases with SchemaMismatch rather than silently resetting, rewriting or deleting them. This supersedes the earlier handoff recommendation to build a preserving v1-to-v2 migration first. A shipped release must introduce versioned, preserving migrations before its schema changes again. No real user database is migrated, reset or deleted by this decision.

#### DEC-011 — N16 deletion tombstone and hold semantics

Accepted 2026-11-02: the owner chose “Tombstone keeps only the hashed task key, state and timestamps (no titles/content), kept forever to prevent ID reuse; once deletion starts, holds cannot stop it.” The tombstone stores the domain-separated task key, never the raw TaskId, title, contract or content. It survives the task's SQL cascade and permanently prevents reuse of that ID in the same state root. Holds are checked before the admission-closing `deleting` transition; afterwards deletion proceeds or retries to completion. A hashed key is pseudonymous, not anonymous: anyone who knows a candidate ID can confirm it was deleted. Local unlink is still not secure erasure or revocation of exported/remote copies.

#### DEC-012 — Report claims and evidence citation format

Pending owner confirmation. The specification requires report fields/claims to link to captured evidence and unsupported assertions to be marked unknown, but defines no format. CH-7 implements a conservative default so evidence coverage can be machine-checked: a Markdown report output carries exactly one fenced block with info string `aero-claims` containing strict JSON `{"schema_version":1,"claims":[{"entity","field","value","evidence_id"}]}`, where `evidence_id` is a committed capture of the same run or `null` for an explicit unknown. The completion gate loads each cited capture itself and requires exact entity/field/value agreement; any contradicted claim fails, otherwise the matched share must reach the contract's `required_fraction`. Prose outside the block is not verified. Revising this format is cheap while nothing generates reports yet; it must be settled before E-series report generation and the G1 workflow.

#### DEC-013 — Planner-facing observation text format

Pending owner confirmation. The plan fixes the observation's limits (16 KiB UTF-8, names ≤256 Unicode scalar values, focus/dialog/actionable/nearby priority, explicit truncation) and that handles are opaque observation-scoped strings, but defines no text format. D01 implements a conservative default in `crates/aero-perception/src/observation.rs`: one line per rendered node, two spaces of indentation per rendered ancestor; controls as `@e<n> <role> "<name>"` plus ` value="…"` and fixed-order state flags; context as `heading <level> "…"`, `cell "…"`, `text "…"` and similar; structure as bare `table`, `row`, `list` or `listitem`. Every page-supplied string is quoted, with quotes, backslashes and control, line-separator, bidirectional and zero-width characters escaped, so page text can neither start a line nor forge a handle. Cut names and values are flagged `truncated`, and a final `[omitted N controls and M other lines]` line appears whenever anything was dropped. Handles are assigned after selection, in document order, so the text and the handle map always agree. Revising this is cheap while no planner consumes it; it must be settled before E03/E05 prompt construction.

#### DEC-014 — Running the nested browser sandbox under AppArmor user-namespace restrictions

Pending. ENV-005: on native Ubuntu 26.10 the stock AppArmor policy denies capabilities to everything started by Bubblewrap, so Chromium cannot start with its own sandbox inside ours. The Ubuntu 24.04 baseline enables the same restriction and is untested. Options:

1. A project-owned, narrowly attached AppArmor profile that lets the sandboxed browser use capabilities inside its own user namespace. The owner installs it with root on development hosts; the native package (I03) would ship it, in the way Ubuntu's `apparmor` package ships one for `/opt/google/chrome/chrome`. It needs a design spike, and loading it is a host security policy change only the owner applies. Not designed or tested.
2. Widen the stock policy locally through its `local/unpriv_bwrap` include. Quick, but it applies to every program started through Bubblewrap on the host. Not recommended.
3. Keep browser-backed work on the WSL2 host, where the restriction is absent, and use the native host for packages that need no browser. Produces no native evidence.
4. A disposable virtual machine for browser tests, ideally Ubuntu 24.04. It still needs option 1 or 2 inside it if the restriction applies there.

Owner direction (2026-11-10): the WSL2 environment “was default but no longer available. This linux environment will now be the default development environment”, with the instruction to “make changes to the code to get things working in this env.” Option 3 is therefore withdrawn. Option 1 is prepared: `packaging/apparmor/aerobrowser-bwrap` is the stock `bwrap-userns-restrict` policy attached only to `/usr/local/libexec/aerobrowser/bwrap` (a root-owned copy of `/usr/bin/bwrap`), with the one difference that programs it starts keep capabilities inside the user namespaces they create. `scripts/install-apparmor-profile.sh install [GROUP]` creates that launcher in a root-owned directory only GROUP may enter and loads the profile; `remove` undoes both. Tests select the launcher with `AERO_BWRAP`. What it permits: any member of GROUP can use user namespaces without the distribution's restriction by running that launcher; no other program's policy changes, and Chromium's own sandbox stays on. The owner's running of the installer is the acceptance (accepted 2026-11-10). Installed and verified 2026-11-10: the owner ran `sudo bash scripts/install-apparmor-profile.sh install`. Read back: `/usr/local/libexec/aerobrowser` is `root:davianc` mode 0750; the launcher is byte-identical to `/usr/bin/bwrap`; `/etc/apparmor.d/aerobrowser-bwrap` equals the repository file. A process started by the launcher is labelled `aerobrowser-bwrap//&aerobrowser-sandboxed (enforce)` and `unshare -U -r -m -p -f -n true` succeeds inside it; under `/usr/bin/bwrap` the label is still `bwrap//&unpriv_bwrap` and the same `unshare` still fails. With `AERO_BWRAP` set to the launcher, `cargo test -p aero-cdp --locked --test sandbox --features browser-tests` passed 11/11 (including `launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`, `readiness_failure_reaps_child`) and `cargo test -p aero-agent --locked --features browser-tests --test scenarios` passed 13/13, in six consecutive runs of each; no browser process remained. Open points for later: a narrower capability list for the child profile, refreshing the copy when the bubblewrap package updates, and the packaged (I03) form.

Original recommendation: option 1, with option 3 in the meantime if the WSL2 machine is still available. In every case doctor should detect and explain the condition (§7.8). Excluded in every option: `--no-sandbox`, switching the restriction off system-wide, or running the browser unconfined.

## 5. Initial design hazards already covered by the plan

These are review guardrails, not claims of reproduced application bugs:

| Hazard | Existing correction / requirement | Package and required proof |
|---|---|---|
| Agent approves its own action through MCP or JSON role | SPEC-25, REQ-GRANT-03 | N06/N07/N08/G15: model cannot issue grants; unknown/high-impact effects remain gated across paths. |
| Model says done despite missing output | REQ-VERIFY-01/02 | N04/N05/E05: required postconditions/artifacts and unresolved-effect gate. |
| Crash repeats a submit/purchase | SPEC-09/10, REQ-RECOVERY-01 | B05/F10: prepared intent preserved; independent fixture counter remains exactly 1. |
| Resume resets resources/authority | SPEC-13, REQ-TASK-02 | N02/N03/F11: immutable task/run/revision and cumulative reservation tests. |
| Local-only mode leaks through optional adapter/fallback | REQ-DATA-01/02 | N13/N14/E04: recording transport sees zero unauthorized transfers. |
| Native browser/viewport confused with Tauri WebKit | SPEC-03, REQ-VIEW-01 | N10/H03/H06: actual controlled target and native measured UX matrix. |
| Recovery claims lossless state/unsafe field matching | SPEC-04–08/16–19 | D04–D07/F02–F06/N19: exact fixtures and visible partial restore. |
| Secret/classification or path/sandbox fallback bypass | SPEC-06/20/21 | F01/F07/C03–C04/G06/N15–N16: sentinels, canaries, ownership/path tests, no insecure fallback. |

If actual implementation reproduces one, allocate a BUG ID and link the existing correction/requirement; do not mark it fixed merely because this hazard table exists.

## 6. Copyable record templates

### Bug report and fix record

```text
### BUG-NNN — short observable title
Type: bug
State: reported
Severity / priority: choose S0–S3 / P0–P2 and explain impact
Owner / reviewer: unassigned
Requirement / correction / plan package:
Affected environment, browser/app revision, features, evaluation case:
Reproduction preconditions and sanitized fixture:
Exact command or numbered manual actions:
Expected behavior:
Observed behavior and exit/test result:
Sanitized evidence location:
Safety/privacy impact and immediate containment:
Root cause and sibling paths checked:
Regression test path/name and observed red command/result:
Minimum fix and exact changed paths:
Observed green command/result and affected-suite results:
Independent artifact/effect/readback verification:
Remaining limitations / not run / blocker IDs:
Review outcome and approval reference:
Commit approval reference and actual SHA, or uncommitted:
Closure evidence and integrating reviewer:
Reopen history:
```

### Environment blocker

```text
### ENV-NNN — missing or unverified prerequisite
Type: environment
State: blocked or needs_validation
Priority / severity: P0–P2 / N/A
Owner:
Observed environment and read-only discovery evidence:
Blocked packages/gates:
Dependency or approval IDs:
Next permitted action and exact provisioning/validation scope:
Unblock command/checklist and required observed result:
Actual result / limitations / reviewer:
Closure or continued blocker:
```

### Owner decision

```text
### DEC-NNN — bounded decision
Type: decision
State: pending
Approver: project owner
Dependent packages/gates:
Options and recommended lean default:
Security/privacy/cost/platform tradeoff:
Decision deadline: before the named dependent action, not an invented date
Actual owner response and approval reference:
Accepted scope and explicitly excluded actions:
Follow-up implementation/validation record:
Superseded-by reference if applicable:
```

## 7. Evidence, maintenance, and closure rules

- Store only minimal synthetic/redacted reproduction evidence. Never attach real profile data, cookies, credentials, private documents, hidden model reasoning, account identifiers, or unrestricted screenshots to this tracker or a remote issue.
- Exact commands/results are observations; expected output belongs in a separate expectation field. Missing prerequisites, zero matched tests, scripted evaluations, actual-model runs, and native manual checks have distinct labels.
- Link issue IDs from the plan ledger and current handoff; keep detailed issue status here rather than duplicating it elsewhere. Update index and full record together when states change.
- Required gate exceptions need an explicit scope/requirements decision; security-critical regressions stay blocked until fixed or the affected capability is not shipped. Never hide a failed required case as optional after observing it.
- Do not erase failures, rewrite prior review history, fabricate commit/evaluation IDs, or close on an agent's claim alone. Read back the exact target and inspect real evidence.
- After every reviewed batch, reconcile this tracker, the execution ledger, and handoff, then report what is verified, what remains blocked, and the exact next safe action.
