# AeroBrowser — bug, fix, blocker, and decision tracker

Updated for the approved B01 commit on 2026-10-22. This remains the authoritative local issue register. Local main is linked to private dave2k77/aero-browser; bootstrap and README were separately approved and committed. B01 primitives are committed as 54e0a9573d8eb90746243f0f24056b05a3022988 after the user said “commit and push”; see the [current evidence record](handoff.md#current-b01-checkpoint). Hosted issue migration, later batch commits, public release and private-evidence publication are not approved. Verify actual remote delivery separately.

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

## 4. Live register — initial state

### Application defects

No BUG records yet. The B01 identifier/geometry/coordinate primitives, existing core import contract and eight CLI diagnostic tests pass: 25 behavior tests and five compile-fail doctests across the workspace. B01 TDD exposed and corrected floating-point midpoint boundary rounding before review; that deliberate red state is evidence in the handoff, not a fabricated shipped-product BUG. Browser/task execution and epoch-bound handle invalidation are not implemented. This is not a claim that the future product is bug-free. ENV-002/ENV-004 remain unverified native/runtime gates; pure-core B01 did not exercise or close them.

### Environment blockers

| ID | Priority | State | Owner | Observed condition | Blocks |
|---|---|---|---|---|---|
| ENV-001 | P0 | verified | Hermes | User-local Rust/Cargo 1.99.1 with rustfmt/clippy; core import and workspace build/test/lint pass | No longer blocks pure Rust work |
| ENV-002 | P0 | needs_validation | Hermes | User-local test browser matches version/local byte pin; no actual browser sandbox qualification or independent publisher SHA-256 | C04/C07 browser gates, production provenance review, G0/G1/GV |
| ENV-003 | P0 | verified | User installed; Hermes verified | pkg-config 2.5.1 available at /usr/bin/pkg-config (canonical /usr/bin/pkgconf); doctor detects it; bootstrap tests/fmt/clippy pass | Bootstrap tool-availability blocker resolved; future native library/backend qualification remains separate |
| ENV-004 | P1 | needs_validation | Unassigned | Only WSL2 development environment inspected; native Ubuntu 24.04 X11/Wayland and Secret Service/sandbox compatibility are untested | GV, native vault integration, G4/G5 support claims |

#### ENV-001 — Rust toolchain unavailable

- Type / severity: environment / N/A. Observed 2026-10-22, active workspace `/home/davia/projects/aero-browser`.
- Evidence: read-only `shutil.which` checks returned null for `cargo`, `rustc`, `rustup`; no Cargo manifest or source tree exists.
- Dependency: DEC-001 now records the user's approval to proceed with the explained first phase.
- Next action: approve and perform A02's compiler/tool setup, select/pin the exact supported compiler during A03, and document actual versions. Tool installation and lockfile resolution are not the same operation.
- Unblock proof: `rustc --version` and `cargo --version` exit 0; the approved compiler matches `rust-toolchain.toml`; A03 `cargo metadata --format-version 1 --no-deps` and import test execute successfully. Record outputs, not anticipated versions.
- Actual unblock evidence: official rustup minimal install followed by pinned 1.99.1 plus rustfmt/clippy; actual versions, six-member metadata, core import red/green, workspace build/test, formatting and warning-denied clippy passed. Parent reviewed real output. Initial evidence above is historical; see docs/development.md for current setup. No commit.

#### ENV-002 — Linux browser provisioning and provenance missing

- Type / severity: environment / N/A.
- Evidence: read-only PATH checks returned null for `chromium`, `chromium-browser`, and `google-chrome`. A separately supplied off-PATH executable has not been checked; do not claim no browser can exist anywhere on the machine.
- Dependencies: DEC-001 approved provisioning; A03 before doctor code; A04 browser provenance/update policy before required integration.
- Next action: choose a maintained real Linux Chromium executable, record channel/version/license and verifiable provenance in `packaging/browser-lock.json`, then test the supported sandbox combination. Do not treat a Windows browser, Snap wrapper, or HTTP-mocked process as that proof.
- Unblock proof: `"$AERO_CHROMIUM" --version`, truthful `aero doctor --json` after implementation, required `launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`, and failure-cleanup tests execute in the approved Linux environment. Record independently sourced checksums where the selected channel supplies them; absent publisher hashes require a documented reviewed channel policy, never an invented value.
- Provisioning update: vendor Stable metadata selected Chrome for Testing 156.0.8078.47 Linux64/revision 1706539; archive MD5 matched separately fetched Google Storage headers. Local SHA-256 pins are recorded in packaging/browser-lock.json. The real binary --version succeeds, and doctor/lock-browser.sh match the local pin. This is development-only, not redistribution approval or independently published SHA-256. Runtime sandbox/namespace/cleanup tests remain unrun. No browsing session or commit.

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

#### DEC-001 — Development setup authorization

Accepted scope: after setup, minimal six-crate scaffold and doctor checks were explained, the user said, “Ok, great. Let's proceed.” Work proceeds on the inspected WSL host with official user-local rustup and a development-only Linux testing browser; source/version/integrity and boundaries are recorded in docs/development.md. Native Ubuntu 24.04 release gates remain unproven. Sudo needs interactive user authentication. No commit/push, project license, redistribution, real account/provider use, host-security changes or optional backend is approved by this scope.

#### DEC-002 — Repository and source inclusion

Accepted repository scope: the user explicitly requested the GitHub repository before implementation. Created https://github.com/dave2k77/aero-browser as private/empty, initialized local main, connected origin via SSH, and read back those values. No commit, push, identity change or upload occurred. The original PDF, browser/runtime data and .hermes artifacts are ignored. Commits require explicit batch approval; PDF rights/inclusion and project/browser licensing remain undecided under DEC-005.

Follow-up approval: the user said, “ok, let's commit and push.” The reviewed BATCH-00 baseline was committed as 5a4fac91e5638735aa1cd762c183f63c9efa39de after passing bootstrap tests/build/fmt/clippy, document checks, exact staged-scope review and targeted credential-pattern scanning. This approval includes the accompanying checkpoint-record update, not unrelated feature work. Push completion must be verified by comparing remote main with local HEAD. No PDF, browser binary, profile, secret, target or Hermes session artifact is included.

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
