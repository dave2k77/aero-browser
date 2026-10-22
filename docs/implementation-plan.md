# AeroBrowser — task-first implementation plan and specification corrections

Revision 2.3 — 2026-10-22. BATCH-00 is committed; the approved B01 primitives are ready for review, uncommitted. Authoritative product/technical requirements remain in [specification.md](specification.md); the original PDF is historical source. Tested core primitives/scaffolding/diagnostics do not imply task/browser subsystem completion or release readiness.

## Goal

Build a Linux-native browser for delegated work that produces verified task results and evidence-linked artifacts under explicit permissions, with reliable human control and honest recovery.

## 0. Execution entry point and live ledger

### Document authority and read order

1. Read [the specification](specification.md) for product requirements and invariants.
2. Read [the handoff](handoff.md) for the current verified state, approvals, and next batch.
3. Follow this document for dependencies, contracts, work packages, and acceptance commands.
4. Follow [CONTRIBUTING.md](../CONTRIBUTING.md) for TDD, review, scope, and commit rules.
5. Use [the bug/fix tracker](bug-fix-tracking.md) for defects, environment blockers, and pending owner decisions.

This is the single live implementation plan. The former `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md` path redirects here; do not maintain two copies. Requirements belong to the specification, package status belongs to the ledger below, current-session transfer belongs to the handoff, and defect/decision status belongs to the tracker. A duplicated status must link to its authority rather than compete with it.

### Historical starting point — before BATCH-00 on 2026-10-22

Documentation only. No source tree, Cargo manifest/lockfile, Git repository, application tests, benchmark results, or release artifacts exist. The development host reports Ubuntu 26.04.1 LTS under WSL2; the release baseline remains native Ubuntu 24.04 x86_64. Available: Bubblewrap 0.11.1, Node 24.18.0, npm 12.2.0. Rust toolchain, Linux Chromium, and pkg-config are missing from PATH. Native X11/Wayland, sandbox compatibility, and Secret Service have not been exercised. See ENV-001 through ENV-004 in the tracker. This document does not authorize installations or implementation.

### Priorities and release checkpoints

| Priority / checkpoint | Work and dependency boundary | Exit evidence |
|---|---|---|
| P0 / BATCH-00 — review and setup | Complete A01 traceability/ADRs after document review; obtain DEC-001 setup and DEC-002 Git approval; execute A02 in approved environment; scaffold A03; complete A04 before browser-dependent work. | Requirement/correction coverage, actual compiler/tool versions, six-member Cargo metadata, core import test and truthful doctor/browser provenance. |
| P0 / G0 — trustworthy foundations | B01–B06 with N01–N03; E01–E02/N06–N08; N04/N07/N13–N14/N22; C01–C04/C07/C05–C06, D01–D03 and early F01/F08 guards. | Meaningful red/green tests, cumulative budgets, role/data-flow denials, owned browser lifecycle, sandbox canary and renderer sandbox proof. No production unguarded action path. |
| P0 / GV — desktop architecture gate | N10 follows actual-target/input/ownership support; DEC-003 accepts measured presentation. It can run alongside CLI work. | Native interaction/a11y/IME/dialog/takeover matrix. H01 stays blocked if the presentation is not accepted. |
| P0 / G1 — first useful CLI workflow | N11; E03–E04; N17; G07–G08; N18; N05/E05–E07; baseline G11 bounded history; N09 baseline close policy; N23 required G1 evaluation and I01 CI foundation. | Real local supplier-fixture CSV/report/manifest, source-value checks, zero unauthorized effects/false success/leaks, honest unknowns, pause/cancel/close and durable history. Scripted integration is not a model benchmark. |
| P1 / G2 — resilient core | D04–D07; F02–F11; N19; progressively extend N23 crash/frames/forms workload. | Exact form/storage hydration and independent fixture counter=1 after uncertain effect; bounded runtime rebuild; no budget/authority reset; partial recovery report. |
| P1 / G3 — guarded local breadth | G01–G06; N15–N16; G11 complete audit/history, G14 offline replay; retain privacy gates on every adapter. | Guarded vault/resources/uploads/downloads/tabs, isolated retention/deletion, side-effect-free audit; each shipped capability has a passing required workload. |
| P1 / G4 — usable task desktop | H01–H06 and N20–N21 after accepted GV, reusing one service and completion gate. | Native X11/Wayland evidence for claimed backends, accessible task controls, same controlled Chromium target, exclusive handoff and truthful close behavior. |
| P1 / G5 — release | I01–I06 with N23–N24, selected native package and owner acceptance. | Required workflow-by-workflow proof, security/dependency review, frozen quality criteria, held-out status, clean-machine install/doctor/workflow and actual artifact provenance. |
| P2 / measured options | N12; G09–G10/G12–G13/G15–G16; N09 optional service extraction; J01–J06 separately gated. | Owner selects scope, measurement/feasibility justifies it, and feature-specific provisioning/tests pass. Not required merely to deliver G1. |

The checkpoint table is a delivery roadmap, not permission to ignore the more precise §8 dependency schedule. Partial work on G11, F08, N09, or I01 is recorded at subtask level; the package does not become verified until every selected required part passes. Optional capabilities are explicitly recorded as not shipped until approved and tested.

### Ordering clarifications and correctness gates

- Implement the baseline N09 pause/flush/cleanup contract with E02/N08 and before G1 acceptance. Only N09's process extraction is optional/later. Do not wait for the desktop to define close behavior.
- F08's phase cancellation/deadline protection is a foundation requirement; real recovery/rebuild integration follows F09. The initial CLI must not ship with uninterruptible CDP/provider waits just because F08 appears under recovery.
- Implement the F10 journal-only `uncertain_effect_is_not_repeated` regression after B05 and before enabling E05 dispatch. Execute F10's real browser/server crash scenario when that runtime exists, before claiming the corresponding safety gate. G1 must fail closed on recovered prepared intents even before automated restoration ships.
- Apply browser permission deny defaults from C04 onward. N15 later introduces narrowly scoped grants/uploads; it is not permission to leave native device permissions permissive until then.
- Early G11 bounded history is needed by E03, independently of FTS/semantic retrieval. Baseline local task-owned cleanup is required when G1 starts persisting artifacts; N16 extends it across all later derived stores. Do not promise full deletion while any current store is excluded.
- N10 may report a development probe under WSL, but native backend claims require actual native runs. It cannot block pure CLI/library progress or be marked passed by screenshots alone.
- A02 can be split into approved compiler/native-tool setup and separately approved browser provisioning. Pure Rust tasks may proceed once their own prerequisites exist; browser-dependent tests remain blocked. A04's doctor implementation requires A03; browser provenance discovery can precede it. Never fabricate a publisher checksum if the distribution channel does not supply one: record the channel and verification policy for owner review.

### Work-package lifecycle and update discipline

States: `not_started` → `in_progress` → `ready_for_review` → `verified` → `committed`. `blocked` preserves the prior state and links to a tracker entry; `deferred` requires an explicit scope decision. Verified means actual focused and affected-suite results, artifact/readback proof where applicable, and completed review. Committed additionally requires explicit user approval and an actual commit SHA; an approved decision or written test name is not execution proof.

Before editing, claim one package/subtask, owner, exact file scope, dependencies, and verification commands in this ledger. Only one writer owns a file at a time. Child agents return findings/diffs and real output; the integrating implementer verifies results and updates status. Shared controller/store/runtime changes are serialized. A package containing a baseline and optional backend may be `verified` only when its scope explicitly identifies the completed baseline and the deferred backend; absent that note it remains partial.

Record microtasks under the package as `ID.1`, `ID.2`, etc.: one named failing behavior, exact test/implementation paths, red command/result, minimal change, green command/result, review. Preserve these evidence records with the reviewed diff/commit; link their stable location in the last column. Limit focused edits to 2–5 minutes; browser provisioning/native manual sessions are explicit longer operations, not fictitious five-minute tests. The detailed §8 rows are work packages, not timing estimates.

The user approved the reviewed BATCH-00 commit/push. Root commit 5a4fac91e5638735aa1cd762c183f63c9efa39de records its baseline; this checkpoint update records completed bootstrap scope, not browser-runtime or product completion. `—` means no assigned owner/evidence. Verify current remote delivery from Git. Later batches require separate review/approval.

| Package | State | Owner | Evidence / blocker / approved scope |
|---|---|---|---|
| A01 | committed | Documentation subagent; Hermes integrates | 5a4fac9: source/requirements/ADRs and traceability checks reviewed and approved; future suite/manual proof gaps remain explicit. No application-feature acceptance claimed. |
| A02 | committed | User native setup / Hermes verifies | 5a4fac9 records verified bootstrap tooling/pins; installed Rust/browser/pkg-config remain external. ENV-003 resolved; ENV-002/ENV-004 native/runtime qualification remains unverified. |
| A03 | committed | Hermes | 5a4fac9: six-crate workspace, core import red/green, lockfile; build/test/fmt/clippy pass. |
| A04 | committed | Hermes | 5a4fac9: eight CLI tests and real development browser local-pin/helper checks. Baseline diagnostics only; runtime_ready=false and C04/native/production-provenance gates remain unverified. |
| B01 | ready_for_review | Hermes | Approved pure primitives: core ids.rs/types.rs/lib.rs, core Cargo.toml/Cargo.lock; synchronized README, CONTRIBUTING, plan/requirements/handoff/tracker. Six string IDs, clipped geometry and three coordinate-space types implemented after observed red/green. Workspace: 25 behavior tests + five compile-fail doctests, build/fmt/Clippy pass. [Evidence and limits](handoff.md#current-b01-checkpoint). No runtime handle map, browser/tasks/providers, commit or push. |
| B02 | not_started | — | — |
| B03 | not_started | — | — |
| B04 | not_started | — | — |
| B05 | not_started | — | — |
| B06 | not_started | — | — |
| C01 | not_started | — | — |
| C02 | not_started | — | — |
| C03 | not_started | — | — |
| C04 | not_started | — | — |
| C05 | not_started | — | — |
| C06 | not_started | — | — |
| C07 | not_started | — | — |
| D01 | not_started | — | — |
| D02 | not_started | — | — |
| D03 | not_started | — | — |
| D04 | not_started | — | — |
| D05 | not_started | — | — |
| D06 | not_started | — | — |
| D07 | not_started | — | — |
| E01 | not_started | — | — |
| E02 | not_started | — | — |
| E03 | not_started | — | — |
| E04 | not_started | — | — |
| E05 | not_started | — | — |
| E06 | not_started | — | — |
| E07 | not_started | — | — |
| F01 | not_started | — | — |
| F02 | not_started | — | — |
| F03 | not_started | — | — |
| F04 | not_started | — | — |
| F05 | not_started | — | — |
| F06 | not_started | — | — |
| F07 | not_started | — | — |
| F08 | not_started | — | — |
| F09 | not_started | — | — |
| F10 | not_started | — | — |
| F11 | not_started | — | — |
| G01 | not_started | — | — |
| G02 | not_started | — | — |
| G03 | not_started | — | — |
| G04 | not_started | — | — |
| G05 | not_started | — | — |
| G06 | not_started | — | — |
| G07 | not_started | — | — |
| G08 | not_started | — | — |
| G09 | not_started | — | — |
| G10 | not_started | — | — |
| G11 | not_started | — | — |
| G12 | not_started | — | — |
| G13 | not_started | — | — |
| G14 | not_started | — | — |
| G15 | not_started | — | — |
| G16 | not_started | — | — |
| H01 | not_started | — | — |
| H02 | not_started | — | — |
| H03 | not_started | — | — |
| H04 | not_started | — | — |
| H05 | not_started | — | — |
| H06 | not_started | — | — |
| I01 | not_started | — | — |
| I02 | not_started | — | — |
| I03 | not_started | — | — |
| I04 | not_started | — | — |
| I05 | not_started | — | — |
| I06 | not_started | — | — |
| J01 | not_started | — | — |
| J02 | not_started | — | — |
| J03 | not_started | — | — |
| J04 | not_started | — | — |
| J05 | not_started | — | — |
| J06 | not_started | — | — |
| N01 | not_started | — | — |
| N02 | not_started | — | — |
| N03 | not_started | — | — |
| N04 | not_started | — | — |
| N05 | not_started | — | — |
| N06 | not_started | — | — |
| N07 | not_started | — | — |
| N08 | not_started | — | — |
| N09 | not_started | — | — |
| N10 | not_started | — | — |
| N11 | not_started | — | — |
| N12 | not_started | — | — |
| N13 | not_started | — | — |
| N14 | not_started | — | — |
| N15 | not_started | — | — |
| N16 | not_started | — | — |
| N17 | not_started | — | — |
| N18 | not_started | — | — |
| N19 | not_started | — | — |
| N20 | not_started | — | — |
| N21 | not_started | — | — |
| N22 | not_started | — | — |
| N23 | not_started | — | — |
| N24 | not_started | — | — |

### First implementation batch — exact stop boundary

Read [handoff.md §4](handoff.md#4-first-implementation-batch) before starting BATCH-00. Review this document set, create A01's missing requirements/architecture/ADRs, obtain setup/Git decisions, then implement A02–A04 in separately reviewed slices. Stop after verified bootstrap/doctor readiness for user review; do not jump straight to a UI, provider call, real account, or optional feature.

First application behavior after the scaffold: B01's `center_uses_visible_intersection` in `crates/aero-core/src/types.rs`, using the complete §7.3 test. Establish red with a minimal incorrect implementation, implement the visible-intersection predicate, run that exact test to green, then the core suite. Check for arithmetic overflow in computed right/bottom/midpoint as an additional invalid-geometry test before browser input uses it. Do not copy a known-red stub into an approved commit. See CONTRIBUTING.md for approval-gated commit policy.

---

## 1. Current context and assumptions

### What was actually inspected

- Active workspace: `/home/davia/projects/aero-browser`.
- The original project input was `aero-browser-specification-chat.pdf`, a 96-page image/outlined-text export of a Gemini conversation titled **Building an Agent-First Linux Browser**. The approved Markdown specification now lives at `docs/specification.md`; this plan must be kept consistent with it.
- All pages 1–96 were read using in-memory Windows PDF rendering and OCR. Coverage was checked programmatically: 96 distinct pages, no missing pages, no duplicates. OCR is suitable for understanding the design, not for copying Rust punctuation or identifiers.
- There is no Git repository, source tree, Cargo manifest, frontend, build, or existing test suite. `git status` fails with “not a git repository.” The PDF's “Completed” checkboxes on pp. 29–30 are conversation claims, not evidence of an implementation.
- Planning environment: WSL2 Linux x86_64; Bubblewrap 0.11.1, Node 24.18.0, and npm 12.2.0 are available. `cargo`, `rustc`, `rustup`, Linux Chromium, `pkg-config`, Flatpak tooling, and `promtool` were not found on PATH. No dependencies were installed and no implementation commands were run in this planning turn.
- No source code, credentials, Git history, or organization memory was available. Hivemind was not used because recall is disabled.

### Interpretation and priority

1. This is a greenfield implementation, not production-hardening of an existing browser.
2. The deliverable is a useful local browser product, not a new rendering engine. Keep Chromium and its own renderer sandbox. Do not fork Chromium, embed CEF, write a distributed scheduler, or build a model-training pipeline in the initial product.
3. Prioritize one excellent verified supplier-research workflow, then guarded form preparation and product breadth. Retain all PDF subsystems in traceability, but do not make optional semantic memory, Parquet, visual inference, worker pools, or speculative extensions prerequisites for the first useful release. Advance them only with passing acceptance tests and measured benefit.
4. Initial supported release: Ubuntu 24.04 x86_64. Validate Debian and Arch after the native baseline works. WSL is a development environment, not proof of native Wayland support.
5. Proposed defaults in this plan are decisions, not unanswered implementation choices. Product decisions that genuinely need owner approval are isolated in §12. Until changed, use the defaults stated here.
6. No commits occur during planning. During implementation, make small reviewable batches; commit only after the user approves that batch. Commit commands below are templates for that approved point, not permission to commit automatically.

### PDF reading map

| PDF pages | Material | How to use it |
|---|---|---|
| 1–6 | Checkpoint capture, hydration, resume | Requirements and failure cases; replace the sample persistence/recovery implementation. |
| 7–14 | Form serialization and integration test | Retain native-setter/event idea; fix serialization, completeness, privacy, and assertions. |
| 15–20 | Nested open shadow-root recovery | Retain boundary-chain concept, not the duplicated interpolated JavaScript. |
| 20–28 | OOPIF perception/input and test | Rewrite frame correlation, coordinate conversion, and the test's origin setup. |
| 28–32 | Architecture, roadmap, README | Scope inventory only; none of the “completed” claims are verified. |
| 33–36 | Short master specification | High-level requirements; later monograph supplies more detail. |
| 36–89 | Crates, manifests, Rust samples, shell, CLI | Design input, not a compilable repository. |
| 90–92 | Repeated browser integration tests | Replace silent skips/fixed ports/sleeps with a mandatory deterministic harness. |
| 92–96 | AppImage, Flatpak, alerts, Grafana | Packaging intentions; replace placeholder artifacts and repair metrics math. |

## 2. Architecture / proposed approach

Use one Rust workspace with pure Task/Run contracts, an owned CDP/browser-runtime layer, separate perception/input libraries, and one application interface shared by CLI, desktop, and restricted MCP adapters. Keep task state, grants, data-flow policy, budgets, independent verification, and evidence/artifact metadata in the trusted control plane; keep browser mechanics in the execution plane and SQLite as the durable source of truth. Deliver a tested CLI workflow first, but run the native viewport feasibility gate before substantial frontend commitment: a trusted Tauri console may stream the actual Chromium target or accompany its native window, never pretend WebKit is that controlled Chromium instance.

### Dependency direction — mandatory

```text
aero-core                 Task/Run, action, criteria, evidence/artifact models
    ↑
aero-cdp                  process, transport, targets, frames, downloads
    ↑                 ↑
aero-perception       aero-executor
    ↑                 ↑
    └────── aero-agent ────┘  tasks/service, grants/data flow, planner,
                  ↑          verification, artifacts, journal/recovery
          aero-cli   aero-shell/src-tauri
                              ↑
                         aero-shell/ui
```

- `aero-core` depends on no workspace crate, browser, database, or GUI.
- `aero-perception` must not depend on `aero-executor`. Observation reports blockers and content; it does not mutate the website.
- `aero-cdp` has no planner/business policy, SQLite, Secret Service, or UI state.
- `BrowserRuntime` owns the child, CDP transport/listeners, targets, frame graph, network/download managers and worker supervisor. No global CDP connection.
- Durable `Task` owns immutable `TaskContract` revisions, typed criteria, cumulative budgets, grants, evidence/artifacts, and runs. Existing journal `sessions` are runs, not task identity. New runs/provider changes/UI reconnects cannot reset budgets or authority.
- `SessionController` serializes a run and browser input ownership. CLI, GUI and MCP submit typed commands through `SessionService`, initially an in-process interface with trusted-human versus model-adapter roles. Extract a local process only after N09's separate background-service gate; do not create a daemon/crate merely for a diagram.
- `Planner` proposes typed decisions but cannot call executor/vault or write verified outcomes/grants. An HTTP adapter performs configured inference; MCP is a tool protocol, not the assumed inference server.
- `VerifierRegistry` independently checks predeclared postconditions from safe readbacks/evidence. A second model may assist but is never the sole completion/safety authority.
- `DataFlowGateway` mediates application-managed provider/tool/OCR/embedding/memory/export transfers. All adapters use it; their private HTTP clients cannot bypass task modes or recipient/data-category grants.
- Split `aero-agent` into tasks, service, capabilities, budgets, data flow, strategies, verification, evidence/artifacts, store/recovery and adapters. Keep `orchestrator.rs` as coordination, not every domain's implementation. Do not add crates until a real ownership/build boundary earns one.
- Never expose arbitrary JavaScript, shell, host paths, raw CDP, trusted-role selection, or approval issuance as model actions.

### Data and state boundaries

```text
trusted TaskContract / explicit human grant
                     ↓
policy → durable intent → executor → browser side effect
           ↓                            ↓
       SQLite journal ← independent verifier ← fresh evidence
           ↓
      checkpoint + criteria/artifact outcomes

page/provider/tool output → untrusted, classified data → planner proposal
application-managed transfers → task-scoped DataFlowGateway
```

Browser and remote site state cannot be atomically committed together. Preserve uncertain effects without automatic replay. `Complete` proposes criterion evaluation; only the required-criteria/artifact gate can set task success. Application processor-transfer controls are distinct from browser website traffic; URL filters/redaction do not imply full egress isolation.

The initial host pauses/flushes on desktop close. A host crash may stop in-process tasks and is recovered conservatively from the journal, not falsely described as background survival. Optional background execution uses an explicitly enabled per-user Unix-domain service, owner/peer-credential checks, versioned bounded messages, narrower MCP roles, task-specific continue grants and visible reconnect/cancel controls. No default TCP server and no claim of security against a fully compromised same-user account.

## 3. Required corrections to the supplied specification

The following changes are part of implementation, not optional polish.

| ID | Source | Defect / overclaim | Required replacement |
|---|---|---|---|
| SPEC-01 | pp. 29–30 | “Completed” phases have no files/tests. | Start at foundation; maintain a requirement-to-test matrix. |
| SPEC-02 | pp. 77–79, 86, 88–89 | An MCP client sends `sampling/createMessage` to an example tool server as if it were an inference API. | Direct `Planner` adapter; separate MCP server/client responsibilities and supported protocol version. Sampling, if supported later, follows its actual server-to-client direction. |
| SPEC-03 | pp. 36, 85–88 | Tauri/WebKit/GTK4/CEF are presented as interchangeable; GUI viewport is placeholder text. | Tauri hosts trusted local UI, not controlled pages. Run N10 early, choose actual-target stream/native companion from interaction/a11y evidence; native CEF embedding requires a separate decision. |
| SPEC-04 | pp. 7–11, 41–43, 72–73 | `HashMap<FormFieldSignature, ...>` cannot become a JSON object with structural keys; hydrator assumes those keys were JSON strings. | `Vec<FormFieldEntry>` with explicit `locator` and `value`; JSON round-trip test is a release blocker. |
| SPEC-05 | pp. 8, 16, 72 | Non-empty-only capture loses cleared values; unselected radio groups/multi-selects/file inputs are mishandled. | Capture eligible controls including empty strings and false state; group radio selection explicitly; select by values, not index; omit files/hidden/secrets. |
| SPEC-06 | pp. 11, 48–49, 82 | “Zero exposure” excludes only password inputs; card/OTP text fields, screenshots, logs, JSON-RPC payloads, DOM echoes, and query strings leak. | Central classification/redaction and taint tracking across every model/audit/export path. Vault plaintext has no `Debug`/`Serialize`; screenshots are suppressed during secret entry unless verified redaction is possible. |
| SPEC-07 | pp. 3–6, 66, 84 | Storage is restored after navigation and fixed sleeps; app scripts have already consumed empty state. | Install origin/tab-scoped preload scripts before navigation, restore cookies by browser context, wait for actual lifecycle predicates, remove restore scripts after confirmed use. |
| SPEC-08 | pp. 1, 6, 32, 35 | Cookies/storage/forms cannot restore JS heap, wizard server state, IndexedDB, passkeys, file inputs, expired tokens, or arbitrary SPA state. | Best-effort browser restoration plus exact committed task history; report partial restoration and pause for re-authentication/unsupported state. Never guarantee lossless web session replay. |
| SPEC-09 | pp. 42–45, 83–84 | Separate DB writes can disagree; ignored errors and default-on-corrupt deserialize erase task state. | One SQLite DB and writer; schema migrations; transaction for step outcome + checkpoint + session cursor; propagate corruption/disk errors. |
| SPEC-10 | pp. 1, 5–6, 81–84 | Checkpoint boundaries disagree; a crash after purchase but before persistence can repeat purchase. | Durable pre-action intent; durable post-action outcome/checkpoint; pending intent on restart becomes `needs_review`, not an automatic retry. |
| SPEC-11 | pp. 50–51, 81 | Watchdog only queues a command read at the next loop iteration; a hung awaited operation never returns. | Supervisor races each operation against phase deadline, cancellation, and process/connection failure; recovery is outside the stalled operation. Monotonic time, capped recovery attempts. |
| SPEC-12 | pp. 6, 84 | Restart replaces only `cdp` and dispatcher; managers/listeners retain old connections and IDs. | Rebuild the entire owned `BrowserRuntime`; cancel and join old listeners; invalidate runtime/document epochs and element handles. |
| SPEC-13 | pp. 5–6, 88–89 | `--resume` changes an ID but still demands a goal, navigates to default URL, and starts a new task; step cursor not restored. | Dedicated `resume SESSION` route; load original goal/budget/cursor and active tabs; no new session insert/default navigation. |
| SPEC-14 | pp. 53–54 | CDP call holds pending-map lock across channel send/wait, has no deadline/cleanup, and does not fail pending requests on disconnect. | One reader, one writer, short map critical sections, per-request deadline/RAII removal, bounded queues, fail all pending requests on close. |
| SPEC-15 | pp. 57–60, 64 | Listeners subscribe after enabling events; duplicate auto-attach ownership; broadcast lag ends loops silently. | Subscribe before enabling; one target owner; reliable handling for paused requests and lifecycle events; lag triggers explicit resync/failure. |
| SPEC-16 | pp. 23, 60 | Iframe resolver ignores the requested frame ID and picks first visible iframe. | `Page.getFrameTree`/frame lifecycle mapping plus `DOM.getFrameOwner(frameId)` in the correct parent session. Never infer identity from first DOM match. |
| SPEC-17 | pp. 21–25, 59–60 | Target IDs are assumed to be frame IDs; same-process frames and nested OOPIF attachment are incomplete. | Distinct typed IDs; frame/context mapping from protocol events; attach recursively; include same-process frame execution contexts and detach/navigation cleanup. |
| SPEC-18 | pp. 25–28, 91–92 | Different ports imply cross-origin, not necessarily different sites/processes. Unlabelled card field also need not have the assumed accessible name. | `merchant.test` and `gateway.test`, host resolver mappings, `--site-per-process` in the OOPIF test, real labels, and explicit assertion of distinct target sessions. Also test same-site cross-origin frames separately. |
| SPEC-19 | pp. 24–25, 69, 75–76, 82 | Naive offsets can double-project coordinates; quad subtraction fails under rotation; stale bounds survive scrolling; duplicate clicks during typing. | Explicit frame-local CSS vs root CSS vs screenshot-pixel types; one geometry adapter; remeasure after scroll; hit-test/verify focus; click once; fail closed on unsupported transformed frame geometry. |
| SPEC-20 | pp. 34, 55–56 | Incomplete sandbox command, leaked env/display/host network, fixed profile/port, unread stderr, weak child ownership. | Explicit mounts/env, private per-run writable roots, port 0 and `DevToolsActivePort`, continuously drain bounded stderr, retain/reap child; no `--no-sandbox` fallback. Document host-network/display limits. |
| SPEC-21 | pp. 63–65, 83 | Download notification registration races instant downloads; unsafe suggested names/renames; host path not mounted in sandbox. | Subscribe and arm correlation before click, keep terminal outcomes cached, browser-context routing, host/sandbox path mapping, internal GUID storage and safe export names without overwrite or symlink following. |
| SPEC-22 | pp. 62–63 | `Arc::try_unwrap` cleanup fails with leaked references; cancellation leaks browser contexts; counter resets per clone. | Supervisor-owned leases/cleanup queue, shared counter, dispose-on-failure/cancel/drop, permit released only after disposal or whole-runtime teardown. |
| SPEC-23 | pp. 61–63 | Random WebGL/UA spoofing is inconsistent and advertised as stealth. | Stable factual profiles by default; isolation/locale/viewport configuration only. Do not claim undetectability or bypass anti-bot controls; pause for CAPTCHA. |
| SPEC-24 | pp. 70–71 | Overlay cleaner clicks arbitrary dialog buttons, accepts consent by default, removes DOM overlays, and rewrites document positioning. | Observation-only blocker detection; default reject nonessential tracking; scoped, approved dismissal action; no destructive universal “cleanup.” |
| SPEC-25 | pp. 83 | Three English keywords on Click do not gate Enter, submit, MCP or replay. | One gate checks actual resolved effects against task-scoped trusted capabilities; permitted research mechanics avoid per-click prompts, unknown/boundary/high-impact commits require review. |
| SPEC-26 | pp. 43, 77, 85–88 | Stores raw thoughts and promises streamed private reasoning. | Store/display short public rationale, intended action, outcome, evidence, and usage. Never require provider hidden reasoning or chain-of-thought. |
| SPEC-27 | pp. 83–85 | Debug strings saved as JSON; replay ignores click/type; transient handles cannot identify historical nodes. | Persist versioned typed actions and durable locator/evidence. Default replay is offline audit playback; live re-execution is a separate best-effort mode with fresh resolution and policy gates. |
| SPEC-28 | pp. 38–40, 45–50, 69–70, 89 | Missing dependencies/assets/module roots; Arrow families may mismatch; SOM encodes JPEG from RGBA; broad unconditional heavy dependencies. | Resolve/lock actual versions, keep optional features independent, derive schemas only where used, embed a licensed font, preserve PNG MIME or explicitly convert RGB for JPEG. |
| SPEC-29 | pp. 52, 94–96 | Metrics default binds all interfaces despite localhost docs; labels may include URLs; alert denominator uses comparison. | Loopback/off by default, fixed low-cardinality labels, numeric failure ratio and zero-traffic test, validate rules with `promtool`. |
| SPEC-30 | pp. 90–92 | Tests return success when Chromium/bwrap cannot start; fixed ports and sleeps mask bugs. | Required browser suite fails missing prerequisites; explicit separate CI job; dynamic ports, condition/event waits with deadlines, cleanup guards. |
| SPEC-31 | pp. 92–94 | AppImage omits Chromium/libs/update process; unverified downloaded tool/hash; Flatpak flags/paths/offline sources incomplete; nested sandbox not proven. | First native distro package, then AppImage with locked verified inputs; Flatpak after a real sandbox compatibility spike. No guessed hashes, obsolete runtime promises, or public signing claims. |

## 4. Scope, defaults, and non-goals

### Delivery gates

- **G0 — foundation:** workspace, Task/Run/criteria/grant/data-flow contracts, early independent evaluation, mandatory native sandbox/browser smoke, and usable page session.
- **GV — early viewport decision:** actual stream and native companion interaction/a11y/IME/dialog/focus/takeover tests before substantial frontend commitment. CLI work can continue independently; H01 cannot assume the winner.
- **G1 — first useful workflow:** supplier-research task produces independently verified CSV/report/evidence bundle within a user-granted browsing/extraction envelope and cumulative budgets. Local fixture truth first; a separately approved public smoke is not a benchmark substitute.
- **G2 — resilient core:** forms/frames/open shadow roots, eligible encrypted snapshots, bounded runtime rebuild, preserved task memory/budgets, no repeated uncertain effects, and user-visible partial recovery report.
- **G3 — local product breadth:** guarded vault/TOTP, downloads/uploads, tabs/profile clarity, browser/data permissions, provenance/artifact shelf, offline audit. Optional SOM/RAG/Parquet/workers/metrics/MCP are individually validated features, not first-workflow blockers.
- **G4 — task-first desktop:** selected actual-Chromium presentation; criteria/results/questions/artifacts workspace; browser essentials; accessible controls; acknowledged exclusive takeover; native X11/Wayland evidence. A screenshot is not an accessible native browser viewport.
- **G5 — releasable:** mandatory task workloads plus safety/false-success/privacy/recovery checks, CI/security review, chosen native distribution, verified artifacts/provenance/licenses/docs. Freeze workload thresholds before model tuning. AppImage/Flatpak/optional service have separate gates; do not hold the useful native baseline for every speculative backend.

### Explicit default limits

| Setting | Default |
|---|---|
| Delegation | No effect without a trusted task-scoped grant. Authorized research mechanics do not prompt per click; unknown/boundary-crossing/high-impact commits require review. |
| Root viewport | 1280 × 800 CSS px, device scale factor 1. |
| Deadlines | CDP 10 s; navigation 30 s; planner 90 s; download 60 s. Pause cancels planning/read-only waits promptly rather than waiting out those limits. |
| Stability | DOMContentLoaded + 200 ms mutation quiet + two animation frames; 5 s cap; network quiet advisory only. |
| Task budget | 30 action attempts across all runs; explicit active-time/token ceilings before run; purchase spending zero; priced inference limit only with configured upper-bound prices. Atomic reservations, no resume reset. |
| Recovery | 2 browser restarts per task; 250 ms then 1 s backoff; no automatic uncertain-effect retry. |
| Workers | Optional; start sequential. When enabled 2 contexts, configurable 1–8, queue ≤64; shared task budget. |
| Observation | 16 KiB UTF-8, names ≤256 Unicode scalar values; focus/dialog/actionable/nearby context priority; explicit truncation. |
| Snapshot | ≤2 MiB, 20 authorized origins, 8 tabs, 2,000 form entries; exceeding limits is explicit partial/nonresumable state. |
| Provider input/output | Response body ≤2 MiB; decision text ≤64 KiB; transfer requires matching data-flow grant. |
| Artifacts | Private staging; download ≤100 MiB; extraction ≤10,000 rows / 10 MiB; export separate from local staging permission. |
| Metrics | Off unless requested, loopback listener only, no task-content labels or telemetry bypass. |
| Persistence | Task/contracts/history on; raw screenshots off; sensitive snapshots opt-in encrypted with available key; no plaintext fallback. |
| Retention | Recoverable snapshots 7 days; task-specific evidence/artifact policy; task deletion purges derived local memory/index/caches, not already transmitted external data. |
| Processing | `local_only` until trusted approved-cloud recipient/data-category grant; missing local inference pauses. Approved website browsing can still use network. |
| Browser permissions | Deny camera/mic/geolocation/clipboard-read/filesystem/notifications/uploads without supported trusted task/origin grant. |
| Desktop close | Pause/flush/reconcile. Background continue unavailable until optional service backend passes tests and user opts in for task. |
| Completion | All required criteria/artifacts passed or allowed labelled user attestation; no unresolved effect. Model completion alone is not success. |

Application limits are not kernel CPU/RAM quotas. Browser contexts are not independent kernel sandboxes. Classifiers cannot guarantee an already-authorized hostile website will not exfiltrate its inputs. Strict browser-wide egress requires a separately tested proxy/network backend and must fail unavailable, never downgrade silently.

### Deferred / measurement-gated

Custom Landlock/seccomp, remote swarms, training, CEF, closed shadow recovery, arbitrary transformed iframe projection, guaranteed live replay, fingerprint spoofing, browser fork/extension marketplace, and lossless JS/IndexedDB recovery are not launch claims. Semantic infrastructure, Parquet, worker scaling and model routing require measured usefulness and individual gates. Keep every original subsystem in traceability; see §11 for research exits.

## 5. Target file layout and ownership

The documentation set now includes `docs/specification.md`, `docs/implementation-plan.md`, `docs/handoff.md`, `CONTRIBUTING.md`, and `docs/bug-fix-tracking.md`. Other paths below are **planned implementation paths**, not claims about existing files.

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
.gitignore
README.md
LICENSE-MIT
LICENSE-APACHE
docs/specification.md
docs/requirements.md
docs/evaluation.md
docs/viewport-feasibility.md
docs/architecture.md
docs/security.md
docs/development.md
docs/recovery.md
docs/releasing.md
docs/adr/0001-ui-engine-boundary.md
docs/adr/0002-planner-and-mcp.md
docs/adr/0003-checkpoint-semantics.md
crates/aero-core/{Cargo.toml,src/lib.rs,src/ids.rs,src/types.rs,
  src/actions.rs,src/form.rs,src/checkpoint.rs,src/events.rs,src/task.rs,
  src/capability.rs,src/verification.rs,src/evidence.rs,src/artifact.rs}
crates/aero-cdp/{Cargo.toml,src/lib.rs,src/error.rs,src/connection.rs,
  src/launcher.rs,src/runtime.rs,src/target_registry.rs,src/frame_manager.rs,
  src/page.rs,src/evaluate.rs,src/geometry.rs,src/storage_manager.rs,
  src/network/mod.rs,src/network/filter.rs,src/network/interceptor.rs,
  src/downloads.rs,src/pool.rs,src/screencast.rs}
crates/aero-perception/{Cargo.toml,src/lib.rs,src/axtree.rs,src/observation.rs,
  src/redaction.rs,src/overlays.rs,src/form_recovery.rs,src/extractor.rs,
  src/som.rs,js/locator.js,js/forms.js,js/extract.js,assets/font.ttf,
  assets/font.LICENSE}
crates/aero-executor/{Cargo.toml,src/lib.rs,src/dispatcher.rs,src/target.rs}
crates/aero-agent/{Cargo.toml,src/lib.rs,src/config.rs,src/controller.rs,
  src/orchestrator.rs,src/policy.rs,src/approval.rs,
  src/planner/mod.rs,src/planner/http.rs,src/planner/scripted.rs,
  src/store/mod.rs,src/store/worker.rs,src/store/migrations/001_initial.sql,
  src/snapshot_crypto.rs,src/recovery.rs,src/replay.rs,
  src/credentials.rs,src/totp.rs,src/export.rs,src/memory.rs,
  src/mcp/mod.rs,src/mcp/server.rs,src/mcp/client.rs,src/telemetry.rs,
  src/tasks.rs,src/budget.rs,src/capabilities.rs,src/preview.rs,
  src/service/mod.rs,src/service/local.rs,src/data_flow.rs,src/permissions.rs,
  src/strategies.rs,src/procedures.rs,src/verification/mod.rs,
  src/verification/registry.rs,src/verification/completion.rs,
  src/evidence.rs,src/artifacts.rs,src/recovery_report.rs,src/undo.rs}
crates/aero-cli/{Cargo.toml,src/main.rs,src/args.rs,src/doctor.rs}
crates/aero-shell/src-tauri/{Cargo.toml,build.rs,tauri.conf.json,
  capabilities/main.json,src/main.rs,src/commands.rs,src/state.rs}
crates/aero-shell/ui/{package.json,package-lock.json,index.html,
  tsconfig.json,vite.config.ts,src/main.tsx,src/App.tsx,src/styles.css,
  src/api.ts,src/types.ts,src/Viewport.tsx,src/Timeline.tsx,src/Approvals.tsx,
  src/TaskWorkspace.tsx,src/Artifacts.tsx,src/RecoveryReport.tsx,
  src/BrowserToolbar.tsx,src/Permissions.tsx}
crates/aero-agent/tests/support/{mod.rs,fixture_server.rs,browser.rs}
crates/aero-agent/tests/scenarios.rs
crates/aero-agent/tests/fixtures/{basic.html,forms.html,shadow.html,
  merchant.html,gateway.html,dynamic.html,secrets.html,table.html,
  downloads.html,prompt-injection.html,react-form.html}
crates/aero-cdp/tests/transport.rs
crates/aero-cdp/tests/sandbox.rs
crates/aero-agent/tests/{journal.rs,recovery.rs,policy.rs,planner.rs,
  vault.rs,export.rs,memory.rs,mcp.rs,telemetry.rs}
scripts/{check.sh,test-browser.sh,build-deb.sh,build_appimage.sh,
  check-artifacts.sh,lock-browser.sh}
packaging/browser-lock.json
packaging/appimage/{AppRun,aero.desktop}
packaging/debian/control
packaging/flatpak/com.aerobrowser.App.yml
packaging/monitoring/{alerts.yml,alerts.test.yml,grafana-dashboard.json}
.github/workflows/{ci.yml,release.yml}
evals/{manifest.json,README.md,cases/supplier-research.json,
  cases/form-prepare.json,cases/negative-safety.json}
crates/aero-agent/tests/{tasks.rs,budget.rs,verification.rs,capabilities.rs,
  service.rs,data_flow.rs,permissions.rs,procedures.rs,evidence.rs,
  artifacts.rs,recovery_report.rs,evaluation.rs}
scripts/evaluate.sh
crates/aero-agent/src/evaluation/{mod.rs,manifest.rs,runner.rs,fixture.rs}
crates/aero-agent/tests/fixtures/uploads.html
```

### Dependency policy

- Start with `serde`, `serde_json`, `thiserror`, `tokio` (only needed features), `tokio-util`, `futures-util`, `tokio-tungstenite`, `url`, `clap`, `reqwest` with Rustls, `tracing`, `tracing-subscriber`, `rusqlite` bundled, and `tempfile`/`assert_cmd`/`predicates` for tests. `async-trait` is acceptable for object-safe async adapters; do not invent a home-grown dynamic-future framework.
- Use an explicit chosen stable Rust toolchain and record its exact version in `rust-toolchain.toml`; edition 2024 and resolver 3. The first implementation task resolves the dependency graph and records the actual supported compiler/MSRV. Do not claim Rust 1.85 compatibility until that compiler is tested.
- Keep `vault` (`keyring` Secret Service backend, `zeroize`, `totp-rs`, authenticated encryption), `parquet` (one Arrow family), `semantic-memory` (LanceDB + FastEmbed), `som` (image/glyph libraries), `mcp` (official Rust SDK), `telemetry-otlp`, and `desktop` dependencies out of the default minimal graph.
- Basic encrypted sensitive snapshots and vault support can be enabled in the desktop/release feature set after their tests pass; minimal CLI can operate without a keyring and reports the resulting recovery limitations.
- Use `keyring`'s documented Secret Service feature, not `linux-native` alone. For the reviewed keyring 3.x API, `sync-secret-service` plus `crypto-rust` is a concrete option; run it through a blocking worker, not on the async executor. Test locked/unavailable stores, not just mock success.
- Commit `Cargo.lock` and frontend lockfile only at approved commits. `cargo build --locked` is the CI baseline. Resolve optional Arrow/LanceDB/FastEmbed compatibility together; do not duplicate incompatible Arrow types across an API boundary.
- No package downloads at application startup. Model artifacts and Chromium binaries are explicit, checksum-verified provisioning steps. Provider/model/base URL must be configured; there is no pretend default LLM.

## 6. Execution protocol for the implementer

### How task rows work

Rows A–I in §8 are work-package inventories, not literal five-minute tasks; Batch N gives explicit small edits/checks for the approved changes. For every behavior, perform a real small red/implementation/green cycle below. A generic template is not permission to implement a whole subsystem without a complete focused test:

1. **R1:** create/extend exactly the named test with the specified case and assertions. Reuse the fixture helpers and code in §7; do not copy the PDF.
2. **R2:** run the exact focused command. Expected red result: nonzero exit and the named assertion/unsupported method fails. A missing compiler/library/browser is an environment blocker, not a useful failing test. Initial module-not-found compile failure is acceptable only for the first introduction of a module; replace it with a real assertion failure before implementing behavior.
3. **I1…In:** implement the named method/branch/constant according to the exact contract in that row and §7. One row that needs more than five minutes of code is split by the semicolon-separated behaviors into separate test/implementation cycles. Do not label an entire browser subsystem a five-minute task.
4. **G1:** run the same focused command, now expecting the named test `... ok` and exit 0. Then run the package suite and shared gate below.
5. **Review checkpoint:** show the small diff and real red/green result. After explicit user approval, make the indicated commit. Never make a broken intermediate commit just to satisfy “frequent commits.”

A focused command matching zero tests is a failure of the plan execution. Integration tests must assert they exercised the requested backend. No `return`/`Ok(())` escape on unavailable prerequisites.

### Shared commands, run from workspace root

After bootstrap, the per-package green command is followed by:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Browser-enabled task verification uses:

```bash
AERO_REQUIRE_BROWSER=1 cargo test -p aero-agent --features browser-tests \
  --test scenarios CASE_NAME -- --exact --nocapture
```

Expected: the named test runs exactly once and passes. `browser-tests` is a feature that includes required native integration tests, not a mechanism to swallow browser launch errors. Basic library tests remain runnable without Chromium. `scripts/test-browser.sh` runs the complete required browser suite on prepared native Linux.

Commit template, **only after approval**, substituting the exact files from the task row:

```bash
git diff --check
git diff -- PATHS_FROM_TASK
git add -- PATHS_FROM_TASK
git commit -m "TASK_COMMIT_MESSAGE"
```

Do not use `git add .`; do not commit runtime profiles, DBs, downloaded browser/model binaries, credentials, or the source PDF without an explicit licensing/size decision. The first approved repository initialization is `git init -b main`; no remote, push, signing key generation, or release publication is implied.

## 7. Implementation contracts and copy-paste reference code

The snippets in this section belong **inside future files**. They are not executed by this plan. Use a single copy of each definition, export it from the named `src/lib.rs`, and test the definitions before building adapters around them.

### 7.1 Strict action wire format — `crates/aero-core/src/actions.rs`

The model supplies an observation ID with every decision; handles are opaque strings scoped to that observation. Trusted envelopes supply Task/Run/contract revision/caller role; model JSON cannot elevate them. No browser node/session IDs are exposed to the model. Add action variants only when their executor/policy/verification tests exist. `Complete` is a criterion-evaluation proposal, never a direct task-success setter.

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Click { target: String },
    Type { target: String, text: String, clear: bool, press_enter: bool },
    Select { target: String, values: Vec<String> },
    SetChecked { target: String, checked: bool },
    PressKey { key: String },
    Scroll { target: Option<String>, delta_x: f64, delta_y: f64 },
    Navigate { url: String },
    NewTab { url: String },
    SwitchTab { tab: String },
    CloseTab { tab: String },
    FillCredentials { username_target: String, password_target: String },
    FillTotp { target: String },
    ExtractData { target: String, columns: Vec<String>, limit: u32, format: String },
    DownloadFile { target: String },
    BatchMetadata { urls: Vec<String> },
    Wait { milliseconds: u32 },
    AskUser { question: String },
    Complete { summary: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub observation_id: String,
    pub rationale: String,
    pub action: Action,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_is_strict_and_round_trips() {
        let raw = r#"{"observation_id":"obs-1","rationale":"Open result","action":{"action":"click","target":"@e0"}}"#;
        let decision: Decision = serde_json::from_str(raw).unwrap();
        assert_eq!(decision.observation_id, "obs-1");
        assert_eq!(decision.action, Action::Click { target: "@e0".into() });
        let encoded = serde_json::to_string(&decision).unwrap();
        assert_eq!(serde_json::from_str::<Decision>(&encoded).unwrap(), decision);
        let bad = r#"{"observation_id":"obs-1","rationale":"x","action":{"action":"click","target":"@e0","javascript":"alert(1)"}}"#;
        assert!(serde_json::from_str::<Decision>(bad).is_err());
    }
}
```

The JSON fixtures above are complete one-line raw strings. No `auto_submit` option exists on vault actions. Submission is a separate policy-gated action.

Validation beyond deserialization is mandatory: allow only known key names, `http`/`https` navigation (plus internal `about:blank`), finite scroll deltas bounded to ±4000, type text ≤16 KiB, wait ≤5000 ms, extraction limit 1–10000, batch length 1–64, rationale ≤1024 characters, summary ≤8192 characters, and only `csv`/`parquet` export formats. A model never supplies a host output path.

### 7.2 JSON-safe form model — `crates/aero-core/src/form.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldLocator {
    pub frame_path: Vec<String>,
    pub origin: String,
    pub document_url: String,
    pub shadow_hosts: Vec<String>,
    pub form_selector: Option<String>,
    pub field_selector: String,
    pub tag: String,
    pub input_type: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldValue {
    Text { value: String },
    Checkbox { checked: bool },
    RadioGroup { selected_value: Option<String> },
    Select { values: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormFieldEntry {
    pub locator: FieldLocator,
    pub value: FieldValue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSnapshot {
    pub fields: Vec<FormFieldEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_json_preserves_empty_and_false() {
        let locator = FieldLocator {
            frame_path: vec![], origin: "https://merchant.test".into(),
            document_url: "https://merchant.test/form".into(),
            shadow_hosts: vec!["#profile-widget".into()],
            form_selector: None, field_selector: "#notify".into(),
            tag: "input".into(), input_type: "checkbox".into(), name: None,
        };
        let snapshot = FormSnapshot { fields: vec![
            FormFieldEntry { locator: locator.clone(), value: FieldValue::Checkbox { checked: false } },
            FormFieldEntry { locator, value: FieldValue::Text { value: String::new() } },
        ] };
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("\"fields\":["));
        assert_eq!(serde_json::from_str::<FormSnapshot>(&json).unwrap(), snapshot);
    }
}
```

Do not use this model's `Debug` output in logs: it represents pre-encryption browser data. A later privacy task replaces or restricts `Debug` on sensitive aggregate types. The example test must remain valid after that restriction.

### 7.3 Coordinate invariant — `crates/aero-core/src/types.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CssRect {
    pub fn clipped_center(self, viewport: (f64, f64)) -> Option<(f64, f64)> {
        let values = [self.x, self.y, self.width, self.height, viewport.0, viewport.1];
        if !values.iter().all(|v| v.is_finite())
            || self.width <= 0.0 || self.height <= 0.0
            || viewport.0 <= 0.0 || viewport.1 <= 0.0 {
            return None;
        }
        let raw_right = self.x + self.width;
        let raw_bottom = self.y + self.height;
        if !raw_right.is_finite() || !raw_bottom.is_finite() {
            return None;
        }
        let left = self.x.max(0.0);
        let top = self.y.max(0.0);
        let right = raw_right.min(viewport.0);
        let bottom = raw_bottom.min(viewport.1);
        if right <= left || bottom <= top {
            return None;
        }
        let x = left + (right - left) / 2.0;
        let y = top + (bottom - top) / 2.0;
        (x > left && x < right && y > top && y < bottom).then_some((x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn center_uses_visible_intersection() {
        let rect = CssRect { x: -100.0, y: 20.0, width: 120.0, height: 40.0 };
        assert_eq!(rect.clipped_center((800.0, 600.0)), Some((10.0, 40.0)));
        assert_eq!(CssRect { x: f64::NAN, ..rect }.clipped_center((800.0, 600.0)), None);
        assert_eq!(CssRect { x: 900.0, ..rect }.clipped_center((800.0, 600.0)), None);
    }

    #[test]
    fn center_rejects_overflow_and_keeps_finite_midpoints() {
        let overflowing = CssRect { x: f64::MAX, y: 0.0, width: f64::MAX, height: 10.0 };
        assert_eq!(overflowing.clipped_center((f64::MAX, 100.0)), None);
        let large = CssRect { x: 1.0e308, y: 0.0, width: 4.0e307, height: 10.0 };
        let (x, y) = large.clipped_center((f64::MAX, 100.0)).unwrap();
        assert!(x.is_finite() && y.is_finite());
        assert!(x > large.x && x < large.x + large.width);
    }

    #[test]
    fn center_rejects_unrepresentable_interior() {
        let rect = CssRect { x: 0.0, y: 0.0, width: f64::from_bits(1), height: 1.0 };
        assert_eq!(rect.clipped_center((1.0, 1.0)), None);
    }
}
```

The implemented B01 helper in `crates/aero-core/src/types.rs` additionally shares bounds validation with `FrameLocalRect`. Strict interior checks reject floating-point boundary rounding, not all tiny rectangles; a representable subnormal center remains valid. This unit-level predicate does not establish current browser geometry, visibility through occlusion, frame ownership or authority.

Use separate wrapper types for frame-local rectangles, root viewport points, and screenshot pixel points. For axis-aligned frames, map child local coordinates using the parent's **content** quad and child viewport dimensions; account for borders, scaling, scroll, and nested ancestry. If the quad is non-axis-aligned, return `UnsupportedFrameTransform` and request takeover; never click an approximate point. Do not add an offset to CDP coordinates that are already root-relative: the browser calibration scenario defines which conversion each protocol result uses.

### 7.4 Safe URL/path baseline — `crates/aero-agent/src/policy.rs` and `export.rs`

```rust
pub fn allowed_navigation(raw: &str, allow_loopback_fixture: bool) -> Result<url::Url, String> {
    let url = url::Url::parse(raw).map_err(|_| "invalid URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
        || !url.username().is_empty() || url.password().is_some() {
        return Err("unsupported or credential-bearing URL".into());
    }
    if !allow_loopback_fixture {
        match url.host() {
            Some(url::Host::Ipv4(ip)) if ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified() => {
                return Err("private address requires explicit network permission".into());
            }
            Some(url::Host::Ipv6(ip)) if ip.is_loopback() || ip.is_unspecified() || ip.is_unique_local() || ip.is_unicast_link_local() => {
                return Err("private address requires explicit network permission".into());
            }
            Some(url::Host::Domain(host)) if host == "localhost" || host.ends_with(".localhost") => {
                return Err("localhost requires explicit network permission".into());
            }
            _ => {}
        }
    }
    Ok(url)
}

pub fn safe_export_basename(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && name != "." && name != ".."
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
```

This baseline does **not** solve DNS rebinding, redirects to private IPs, unusual reserved ranges, or browser-initiated subrequests. Keep network permission enforcement and its limitations explicit: resolve/validate every approved navigation/redirect, block known metadata endpoints, and do not advertise full egress isolation without a network-namespace/proxy implementation. Desktop local-network browsing is an explicit separate capability.

Filesystem writes use a controller-created private directory and generated artifact IDs. Validate display names, then use directory-relative exclusive creation with no symlink following; do not rely on string validation or `canonicalize()` alone as the final race-free boundary.

### 7.5 Initial schema — `crates/aero-agent/src/store/migrations/001_initial.sql`

The database is unshipped: replace the initial design now instead of adding a fictitious upgrade migration. `tasks` owns cumulative budgets; `sessions` are runs of an immutable contract revision. The single writer validates revision/schema/cross-record ownership before transactions.

```sql
PRAGMA foreign_keys = ON;
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK(status IN
      ('draft','ready','running','awaiting_user','paused','needs_review','completed','failed','cancelled')),
    attempts_spent INTEGER NOT NULL DEFAULT 0 CHECK(attempts_spent >= 0),
    tokens_spent INTEGER NOT NULL DEFAULT 0 CHECK(tokens_spent >= 0),
    tokens_reserved INTEGER NOT NULL DEFAULT 0 CHECK(tokens_reserved >= 0),
    active_ms_spent INTEGER NOT NULL DEFAULT 0 CHECK(active_ms_spent >= 0),
    recovery_count INTEGER NOT NULL DEFAULT 0 CHECK(recovery_count >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE task_contracts (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision >= 1),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    contract_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY(task_id, revision)
);
CREATE TRIGGER immutable_contract BEFORE UPDATE ON task_contracts
BEGIN SELECT RAISE(ABORT, 'task contract revisions are immutable'); END;
CREATE TABLE capability_grants (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    issued_by TEXT NOT NULL,
    grant_json TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
    quota_used INTEGER NOT NULL DEFAULT 0 CHECK(quota_used >= 0),
    FOREIGN KEY(task_id, contract_revision)
      REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
);
CREATE TRIGGER immutable_grant_payload
BEFORE UPDATE OF task_id, contract_revision, issued_by, grant_json ON capability_grants
BEGIN SELECT RAISE(ABORT, 'capability grant payload is immutable'); END;
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    goal TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN
      ('running','awaiting_user','paused','needs_review','completed','failed','cancelled')),
    next_step INTEGER NOT NULL DEFAULT 1 CHECK(next_step >= 1),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(task_id, contract_revision)
      REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
);
CREATE TABLE actions (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    step INTEGER NOT NULL CHECK(step >= 1),
    observation_id TEXT NOT NULL,
    action_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('prepared','committed','uncertain','failed')),
    public_rationale TEXT NOT NULL,
    outcome_json TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY(session_id, step)
);
CREATE TABLE budget_reservations (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    reservation_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('reserved','settled','uncertain')),
    created_at TEXT NOT NULL
);
CREATE TABLE checkpoints (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    step INTEGER NOT NULL CHECK(step >= 0),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    metadata_json TEXT NOT NULL,
    snapshot_nonce BLOB,
    snapshot_ciphertext BLOB,
    key_id TEXT,
    created_at TEXT NOT NULL,
    CHECK((snapshot_ciphertext IS NULL AND snapshot_nonce IS NULL AND key_id IS NULL)
       OR (snapshot_ciphertext IS NOT NULL AND snapshot_nonce IS NOT NULL AND key_id IS NOT NULL)),
    PRIMARY KEY(session_id, step)
);
CREATE TABLE approvals (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    observation_id TEXT NOT NULL,
    material_effect_digest TEXT NOT NULL,
    preview_json TEXT NOT NULL,
    origin TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN (0,1)),
    FOREIGN KEY(task_id, contract_revision)
      REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
);
CREATE TABLE evidence (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE criterion_results (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    contract_revision INTEGER NOT NULL,
    criterion_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK(status IN ('not_checked','passed','failed','inconclusive')),
    result_json TEXT NOT NULL,
    PRIMARY KEY(task_id, contract_revision, criterion_id, session_id),
    FOREIGN KEY(task_id, contract_revision)
      REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
);
PRAGMA user_version = 1;
```

On open: private owner-only directory/file, `foreign_keys=ON`, `journal_mode=WAL`, `synchronous=FULL`, `busy_timeout=5000`. Refuse unsupported schema; future real upgrades migrate transactionally. One blocking thread owns `rusqlite::Connection`; bounded async requests receive oneshot replies; no `MutexGuard` across await. Cross-table task/session ownership is validated in the writer, not trusted from an adapter payload. Structured private content in contract/evidence/artifact metadata must follow the task's sensitivity/encryption/retention policy; secret payloads never enter these plaintext metadata columns.

Step protocol:

1. Load the run's TaskContract/grant revision, task-wide budget and fresh redacted observation.
2. Choose deterministic/semantic/optional visual strategy, declare its postcondition, validate policy/data destination and obtain matching scoped grant or trusted review. Unknown effects stay review-required.
3. Under one writer transaction atomically reserve budget, reserve `step = sessions.next_step`, increment next_step/run attempts/task attempts, and insert `prepared` intent **before** dispatch. Reject exhausted budgets or mismatched revision. Token/request reservations use the same task-wide serialization point; unknown crash-time consumption stays conservatively reserved until reconciled.
4. Revalidate target epoch, effective origin/account and material preview digest; dispatch one authorized action. Record task execution time cumulatively; paused human-wait time is separate from any wall-clock deadline.
5. Independent verifier observes the declared postcondition and records evidence/artifact readbacks or a typed inconclusive/failed result. Dispatch acknowledgement alone cannot pass.
6. Commit action outcome, eligible checkpoint, verification/evidence/artifact metadata and run/task status together. Record the already-reserved next_step without incrementing twice. Reconcile actual provider usage against reservations; never refund uncertain provider cost as zero.
7. A possibly executed effect without committed verification becomes uncertain/needs_review. Do not automatically repeat. Resume reads journal plus task counters, not only an older checkpoint; a new run does not reset limits.
8. `Complete` invokes the completion gate. Every required criterion/artifact must pass or receive its allowed labelled trusted user attestation, with no unresolved effect. Otherwise retain incomplete/awaiting_user/needs_review and explain why; model-reported completion is only proposal metadata.

SQLite atomicity cannot make external effects exactly once. Keep irreversible/unknown effects separate from reversible local edits; compensation is a new authorized action. Missing encryption keys, corrupt records, unsupported verifier, or unavailable strict data-flow backend are typed errors, never empty successful state.

### 7.6 Form JS placement and hydration algorithm

Keep scripts as ordinary JavaScript in `crates/aero-perception/js/locator.js` and `forms.js`, loaded with `include_str!`. Pass payloads via `Runtime.callFunctionOn.arguments` or JSON values; do not generate nested `format!` braces or interpolate selector strings into source.

Complete setter/event primitive for `forms.js`:

```javascript
(() => {
function setControl(control, property, value) {
  const view = control.ownerDocument.defaultView;
  const proto = control.tagName === 'TEXTAREA' ? view.HTMLTextAreaElement.prototype
    : control.tagName === 'SELECT' ? view.HTMLSelectElement.prototype
    : view.HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, property)?.set;
  if (typeof setter !== 'function') throw new Error('Unsupported native control');
  setter.call(control, value);
  control.dispatchEvent(new view.Event('input', { bubbles: true, composed: true }));
  control.dispatchEvent(new view.Event('change', { bubbles: true, composed: true }));
}
return { setControl };
})()
```

This is a complete ordinary-script IIFE: evaluate the identical source in the JS test harness and browser. In `forms.js`, add capture/restore functions inside this IIFE and return them in the same object; never maintain a separate test-only version or strip module syntax at runtime. In CDP, retain the returned object's remote object ID while calling these functions, then release its object group.

Capture algorithm, in order:

1. Enumerate authorized frames using the frame graph and execution contexts, not `iframe.contentDocument` across origins.
2. Traverse each `Document`/open `ShadowRoot` exactly once with a `TreeWalker`; on a shadow host descend into its root and record a selector relative to the current root. `getRootNode()` is the boundary. Do not recursively query the entire subtree for every descendant.
3. Eligible built-ins: input text/email/url/search/tel/number/date/time/month/week/color/range, textarea, checkbox, radio group, select-one/multiple. Skip password, file, hidden, submit/reset/button/image, disabled, and readonly controls; secret classification overrides eligibility.
4. Default snapshot includes eligible empty/false states. Later dirty optimization is permitted only with a persisted baseline and input/change tracking that preserves clearing. Do not infer dirty from nonempty.
5. Construct root-relative escaped selectors: unique ID if present, then unique form/name/type/value combination, then verified structural `:nth-of-type` path. `CSS.escape` is required. Radio-group identity includes form/root/name; selected option is stored by value, including `None`.
6. Exclude `autocomplete=current-password`, `new-password`, `one-time-code`, all `cc-*`, configured secret labels/names, and controls tainted by vault entry even if the type is text. Cross-origin payment forms are observable only after redaction and not persisted by default.
7. Return array entries and bounded capture diagnostics, never a structural-key map.

Restore algorithm:

1. Exact approved origin + expected tab/frame path + route must match. Reject redirect/origin mismatch and schema mismatch.
2. Wait for matching controls/custom elements via bounded observer/predicate (maximum 5 s); no fixed 500 ms assumption. Missing/ambiguous fields are reported, not silently matched to a neighbor.
3. Resolve each shadow host within its own current root, then the field in the final root. Verify tag/type/name/group identity again. Never cross from saved merchant form into a payment frame.
4. Set text/checkbox through native descriptors. For radio groups, clear/check the intended group and emit events for changed members. For multi-selects set option selection by saved values and then emit events on the select. Reordered options do not change semantic selection.
5. Re-read values, collect restored/missing/ambiguous/rejected counts, and test actual framework state. Synthetic `input`/`change` do not prove every application is compatible; custom controls are unsupported unless an adapter exists.
6. Never submit a form during hydration. If an input handler causes external effects, recovery must surface that risk under the user's restore permission.

### 7.7 CDP transport contract

`connection.rs` exposes `connect(endpoint, limits)`, `call(method, params, session, deadline)`, `subscribe()`, and `close()`. Errors distinguish `Timeout`, `Disconnected`, `Protocol {code, message}`, `InvalidMessage`, `EventLag`, and `Cancelled`.

- Parse the envelope first; match replies by request ID and check expected session when present. Unknown/late IDs do not fulfill another request.
- Insert pending entry under a short lock; drop lock; enqueue request; await with timeout/cancellation; remove entry on every completion path, including dropped call futures.
- Reader and writer tasks share a connection cancellation token and owned join handles. On either error/EOF, close channels and fail **all** pending replies, then terminate both tasks. Handle ping/pong/close and bounded message sizes.
- Subscribe listeners before enabling domains. For critical streams, a dedicated dispatcher must not silently drop paused network requests or detach events. A lag signal initiates resync/disable-interception rather than continuing with a stale map.
- Methods belonging to a page are always routed to an attached page/iframe session. Browser-wide methods use the browser connection. A browser WebSocket with `session_id=None` is not a page connection.

### 7.8 Launcher contract

`launcher.rs` builds a `SandboxSpec` then spawns it; command construction is independently unit-tested. Resolve the actual Chromium executable, not an Ubuntu Snap shell wrapper. Mount the executable and required runtime resources at paths the child can see; preserve symlink targets on merged-/usr systems.

Required policy: `--unshare-user`, `--unshare-pid`, `--unshare-ipc`, `--unshare-uts`, `--die-with-parent`, `--new-session`, dropped capabilities; private `/tmp`, `/dev/shm`, `/home/aero`, profile and staging mounts; read-only runtime libraries/fonts/certificates/DNS configuration; explicit working directory. Use `--clearenv` and rebuild only HOME, PATH, locale, and required browser/display variables. No host home, project root, SSH agent, keyring D-Bus socket, arbitrary XDG runtime directory, or provider API key is mounted/passed to Chromium.

Allow host network initially for browsing and loopback CDP; explicitly document this is not network isolation. Headless is the default. Windowed takeover mounts only the actual Wayland socket (derived from `WAYLAND_DISPLAY`) or required X11 socket and authorization file; X11 is a weaker security boundary and must be called out.

Chromium flags: `--remote-debugging-address=127.0.0.1`, `--remote-debugging-port=0`, private `--user-data-dir`, `--no-first-run`, `--no-default-browser-check`, and headless/viewport flags as selected. Never add `--no-sandbox`, `--disable-web-security`, or `--remote-allow-origins=*` to make tests pass. Read the newly created private profile's `DevToolsActivePort`, verify loopback endpoint and owning child liveness, retry within a 15 s deadline. Drain stderr concurrently into a redacted bounded ring. On any readiness error, kill **and wait** for the child and namespace descendants.

Keep Chromium's own renderer sandbox working inside Bubblewrap. If kernel/AppArmor/user-namespace policy prevents that combination, doctor must fail with an actionable explanation; don't disable a sandbox or change system security settings automatically.

### 7.9 Planner, approvals, and verification contracts

- `Planner::decide(PlanningInput, CancellationToken) -> Result<Decision, PlannerError>` receives a sanitized immutable observation, bounded history summary, action schema, and any opt-in retrieved examples.
- `ScriptedPlanner` consumes predefined typed decisions only under tests or the explicit nondefault `evaluation` build feature. Normal run/provider code cannot silently select it. Evaluation reports identify scripted-browser versus actual-model runs; scripted integration success is not a model benchmark result.
- `HttpPlanner` uses configured base URL/model, TLS verification, an authorization header from a secret source, request timeout/cancellation, and structured JSON output where supported. Parse exactly one decision, reject unknown fields/extra tool calls, validate schema/limits, and cap retries to one for an invalid response. Never include API tokens in traces or errors.
- Read-only operations: observation, local extraction into private staging, tab metadata, offline replay. A link navigation may be allowed by a bounded browsing permission; mutating GETs cannot be generically identified, so document that limitation and require approval for unknown action contexts.
- Mutating/ambiguous operations: clicks, typing, Enter, checkbox/select changes, form submission, credential fill, downloads with external side effects, live replay, and external MCP tools. Evaluate a capability or ask for approval. Never let the LLM's rationale, URL text, or page instructions mint that capability.
- Approval binds session, exact typed action digest, observation/document epoch, effective target origin, requested effect, expiry, and single use. Revalidate just before dispatch; navigation/DOM replacement invalidates it. Denial/timeout means no dispatch.
- Approval issuance is a separate trusted-human channel. The model, a page, a retrieved example, an MCP tool caller, and a background worker may request approval but may never grant it. Do not expose an approval-granting MCP tool to the same agent that proposes actions.
- `AskUser` persists `awaiting_user`, not completed. `Complete` proposes independent criterion/artifact evaluation. Store evidence, method and proposal attribution separately from criterion status (`not_checked`, `passed`, `failed`, `inconclusive`). `model_reported` may label a proposal but never satisfy required verification. Label permitted trusted user attestations `user_confirmed`; they are not mechanical proof.
- Verification is action-specific: typed value/focus with secret-safe boolean result, expected selected/checked state, navigation lifecycle and effective URL, download completion/file bytes, extraction schema/count, or explicit user confirmation. Mouse dispatch success is not click-effect success.

### 7.10 TaskContract, criteria, data-flow and outcome contracts

Implement authoritative Rust models in `crates/aero-core/src/task.rs`, `capability.rs`, `verification.rs`, `evidence.rs`, and `artifact.rs`, exporting from `crates/aero-core/src/lib.rs`. JSON strictness/versioning/limits follow the same rules as actions. Use this complete illustrative TaskContract fixture as `evals/cases/supplier-research.json`'s contract payload; fixture origins are explicitly rebound by the trusted evaluation harness, never by the model:

```json
{
  "schema_version": 1,
  "task_id": "supplier-research-fixture",
  "revision": 1,
  "objective": "Compare the two approved fixture suppliers and identify unknown lead times.",
  "profile_ref": "ephemeral-public-research",
  "allowed_origins": ["https://supplier-a.test", "https://supplier-b.test"],
  "allowed_effects": ["navigate", "extract", "stage_artifact"],
  "forbidden_effects": ["account_create", "submit", "purchase", "upload"],
  "processing_mode": "local_only",
  "processor_grants": [],
  "screenshot_transfer_allowed": false,
  "budgets": {
    "max_action_attempts": 30,
    "max_active_ms": 300000,
    "max_input_tokens": 20000,
    "max_output_tokens": 4000,
    "max_purchase_minor_units": 0,
    "purchase_currency": "USD",
    "max_inference_minor_units": null
  },
  "criteria": [
    {"id": "csv-schema", "kind": "artifact_schema", "required": true,
     "artifact": "supplier-comparison", "columns": ["supplier", "price", "lead_time", "source_url"]},
    {"id": "two-suppliers", "kind": "row_count", "required": true,
     "artifact": "supplier-comparison", "expected": 2},
    {"id": "source-coverage", "kind": "evidence_coverage", "required": true,
     "artifact": "supplier-report", "required_fraction": 1.0}
  ],
  "outputs": [
    {"name": "supplier-comparison", "format": "csv", "required": true},
    {"name": "supplier-report", "format": "markdown", "required": true}
  ],
  "close_behavior": "pause",
  "retention": {"snapshot_days": 7, "artifact_days": 30, "memory_opt_in": false}
}
```

This is a proposed test contract, not a claim of real supplier data, actual model prices, or benchmark results. `null` monetary inference ceiling means unknown/not enforced as money, not unlimited hidden spending; token/request/active-time caps still apply. Expiry/wall-clock limits are distinct from active task time. Unknown fields/kinds/negative/NaN budgets, duplicate criterion/output IDs, no required criterion, unsafe origins, contradictory allowed/forbidden effects, missing required outputs, or missing local provider fail validation before runtime launch. Criteria reference typed verifier IDs; the model cannot supply executable checker code.

A criterion result carries task/run/contract revision, criterion ID, `status`, `method`, evidence/artifact references, check timestamp and reason. Evidence/artifacts include origin/source capture time, sensitivity, retention and limitations; hashes identify captured bytes rather than prove website truth. Freshness belongs to the record; revalidation is explicit. `Complete` is evaluated by `completion.rs`, not forwarded to `Store::mark_status(completed)` by planner code.

Complete minimal completion predicate for `crates/aero-core/src/verification.rs` (schema/evidence/cross-record validation is the trusted caller's responsibility and gets its own tests):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus { NotChecked, Passed, Failed, Inconclusive }

pub fn completion_gate(
    required_checks: &[CheckStatus],
    required_artifacts_verified: bool,
    unresolved_effect: bool,
) -> bool {
    !required_checks.is_empty()
        && required_checks.iter().all(|s| *s == CheckStatus::Passed)
        && required_artifacts_verified
        && !unresolved_effect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_requires_checks_artifacts_and_resolved_effects() {
        assert!(!completion_gate(&[], true, false));
        for status in [CheckStatus::NotChecked, CheckStatus::Failed, CheckStatus::Inconclusive] {
            assert!(!completion_gate(&[CheckStatus::Passed, status], true, false));
        }
        assert!(!completion_gate(&[CheckStatus::Passed], false, false));
        assert!(!completion_gate(&[CheckStatus::Passed], true, true));
        assert!(completion_gate(&[CheckStatus::Passed], true, false));
    }
}
```

A subjective criterion's permitted user attestation is converted to Passed only by a trusted verifier recording `method=user_confirmed`, user attribution and evidence of the exact decision. A model's own report is never converted to Passed. Preserve that distinction in UI/metrics. Add serialization derives and the richer CriterionResult DTO around this predicate without changing its invariant.

### 7.11 Service roles, strategies, previews and transfer enforcement

- `SessionService` methods are `create_task`, `revise_contract`, `start_run`, `observe`, `propose_action`, `pause`, `resume`, `cancel`, `request_approval`, `grant_approval`, `reply_user`, `list_artifacts`, `get_evidence`, and `get_recovery_report`. Trusted CLI/GUI user adapters may create grants/revisions; model-facing adapters may request them only. Caller roles are host-constructed capabilities, not JSON role strings. Authenticate every task/session lookup; do not trust an opaque ID as authorization.
- `DataFlowGateway::authorize(TransferIntent)` checks task/revision, classified sources, exact processor/tool/export recipient, purpose, payload type/bytes, expiry and grant. Local-only processing denies all task-content remote inference/OCR/embeddings/tools/telemetry; approved browsing is separately network-authorized. Strict browser-wide egress requires a real backend and fails unavailable. Sensitive mode has deny defaults plus narrow trusted exceptions, never provider fallback.
- `Capabilities::authorize(ResolvedEffect)` uses actual target/account/route/adapter preconditions and effect class. Navigation/extraction inside an approved research envelope needs no per-click prompt. Unknown click/type/autosave/submission remains review-required. Procedures and model rationales cannot classify themselves harmless.
- `Preview` includes task/run/revision, effect class, target/account, recipients, relevant fields/artifact IDs, amount/currency, and expiry. Hash canonical typed material fields through a reviewed serializer; no float money. Re-check semantic material digest immediately before dispatch; changed payload/account/origin/amount invalidates approval, unrelated animation does not. Do not use a whole-DOM hash as either sole freshness check or sole effect identity.
- `ExecutionStrategy` selects deterministic mechanics/reviewed adapter when preconditions hold, otherwise AX/DOM, optional visual grounding, or human help. All paths return the same ResolvedEffect/Postcondition and go through grants/journal/verifier. Procedure preconditions failure stops/replans; no historical handles, embedded secrets, arbitrary code or authority grants in procedure data.
- `RecoveryReport` lists restored task history/storage/forms, missing/unsupported/expired auth, uncertain effects and required next decision. Local undo exists only for supported local effects; external compensation is a new scoped action. Do not show Undo for irreversible/unknown effects.

## 8. Step-by-step implementation backlog

Use the red/green/review cycle in §6 for each behavior. These A–I rows are work-package inventories, not promises that entire subsystems take five minutes; split them at the named test/branch boundary. Batch N supplies explicit small actions for the approved product changes. All commands describe expected future checks, not tests executed during documentation work.

### Integrated dependency schedule — do not execute alphabetically

1. A01–A04; B01–B03; N01; B04–B06; N02–N03. This establishes the Task/run split before implementing a session-only persistence design that would later need replacing.
2. E01–E02 with N06–N08; N04, N07, N13–N14 and N22. These pure/fake-adapter contracts establish grants, previews, verification, data flow, service roles and evaluation structure before provider/executor integration. N07's full preview integration follows N08 if needed; their pure DTO tests are independent.
3. C01–C04; C07; C05–C06; D01–D03; F01 privacy/redaction. Startup denies unsupported/ungiven browser permissions; first workflow does not require uploads/device access. N10 viewport feasibility follows these real input/ownership primitives and must pass before H01 commitment. CLI work can continue while native display testing is unavailable; desktop claims cannot.
4. N11; E03–E04 through the data-flow gateway; N17; G07–G08 baseline table/CSV; N18; N05; E05–E07. N04 tests use defined fake verifier adapters; real evidence/artifact checks are connected here. Complete supplier fixtures and the initial N23 G1 workload before calling the CLI useful.
5. Extend I01 CI from this first vertical slice and keep it active thereafter. N24 freezes baseline/held-out governance before model/strategy optimization; N12 procedure reuse follows baseline evidence rather than leading with a generic workflow DSL.
6. D04–D07; F02–F11 plus N19; G01–G06 and N15–N16 for recovery/privacy/browser breadth. No real vault/account/upload action before its corresponding guard tests pass. Re-run/extend N23 as each category becomes supported.
7. After N10's accepted presentation: H01–H04; N20–N21 integrated with H05–H06. Task workspace and browser essentials are required; the event timeline is secondary. N09 baseline pause-on-close is already required before G1; only process extraction is optional and must pass before enabling background continuation.
8. G09–G16 are individual feature decisions: Parquet/workers/semantic memory/visual marks/MCP/metrics enter the release matrix only with real acceptance/provisioning and measured benefit. Bounded history and offline audit remain core. Data-flow policy applies to every optional adapter.
9. I02–I06 release hardening/documentation/distribution with N23–N24 acceptance. J01–J06 remain separate feasibility tracks. Required workflow failures cannot be hidden by passing aggregate metrics or skipping unsupported backends.

The order is dependency-driven, not a license to expose an unguarded temporary product. Fake adapters are test-only and must be defined before use; an unavailable required backend is a blocker, not mock success in production.

### Batch A — prerequisites, source-of-truth, workspace (G0)

**A01 — synchronize source-of-truth and traceability.** Files: `docs/specification.md`, `docs/requirements.md`, `docs/architecture.md`, `docs/adr/0001-ui-engine-boundary.md`, `docs/adr/0002-planner-and-mcp.md`, `docs/adr/0003-checkpoint-semantics.md`. The specification already exists after this documentation revision: do not overwrite it with the PDF. Copy approved decisions and SPEC-01–SPEC-31 into requirement/ADR references; map every `REQ-*` register row to Batch N and concrete tests. Record all implementation statuses `not_started`. Verify relative links, ID uniqueness/coverage, and `git diff --check` only after approved initialization. Approved commit: `docs: record task-first requirements and corrected architecture`.

**A02 — provision an explicit build environment (approval required).** No tool-dependent implementation can begin until its prerequisites are available through approved installation or a supplied prepared environment. Compiler setup can precede browser provisioning; do not block pure tests on unrelated optional tools. Read `/etc/os-release`; use its package manager, not the PDF's combined distro instructions. On Ubuntu 24.04 the proposed preparation commands are:

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends build-essential pkg-config curl ca-certificates \
  bubblewrap libdbus-1-dev libssl-dev fonts-dejavu-core xvfb
```

Install Rust using the official rustup method only after approval; choose/pin the exact stable compiler at that point. Provision a real Linux Chromium executable through the browser-lock procedure in A04; do not call the Windows browser a Linux sandbox test. Desktop dependencies are deferred to H01. Verification: `rustc --version`, `cargo --version`, `bwrap --version`, `pkg-config --version`, `"$AERO_CHROMIUM" --version`; all exit 0 with actual versions. Capture these in `docs/development.md`. Do not edit sysctls/AppArmor to conceal an unsupported sandbox.

**A03 — initialize the lean workspace.** Files: root `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, and manifests/`src/lib.rs` for core, CDP, perception, executor, agent; `crates/aero-cli/{Cargo.toml,src/main.rs}`. Start with six crates; add shell only in H. Root members list exact paths, `default-members=["crates/aero-cli"]`, resolver 3. Each internal dependency is declared once via `[workspace.dependencies]`. Add a `[[bin]] name="aero"` in CLI. Ignore `target/`, profiles, artifacts, DB/WAL/SHM files, node_modules, dist, credentials, and caches. Test: `cargo metadata --format-version 1 --no-deps` exits 0 and reports exactly six initial members; `cargo test --workspace` exits 0. First small test in core asserts package can be imported. Do not install all optional dependencies yet. Commit: `build: scaffold minimal Rust workspace`.

**A04 — lock browser provenance and doctor checks.** Files: `packaging/browser-lock.json`, `scripts/lock-browser.sh`, `crates/aero-cli/src/doctor.rs`, `crates/aero-cli/tests/doctor.rs`. Contract: `aero doctor --json` reports compiler-independent runtime prerequisites, executable realpath/version, user namespace and sandbox test results, keyring/display status, and actionable failures; no browser downloads or security changes inside doctor. Browser lock contains actual version, platform, upstream URL, expected SHA-256 obtained independently from publisher metadata, retrieval/update policy, and license. Unit test `doctor_missing_browser_is_nonzero`: empty configured browser path → code 2 + JSON `browser.available=false`; configured test executable → truthful result. Command: `cargo test -p aero-cli --test doctor`. Commit: `feat(cli): add explicit runtime doctor and browser lock`.

### Batch B — pure contracts and persistent journal

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| B01 | `crates/aero-core/src/{lib.rs,ids.rs,types.rs}`, core `Cargo.toml` and `Cargo.lock`. Add distinct `TargetId`, `FrameId`, `CdpSessionId`, `RuntimeEpoch`, `DocumentEpoch`, `ObservationId`; geometry §7.3; separate `FrameLocalRect`, `RootViewportPoint`, `ScreenshotPixelPoint`. IDs serialize as strings but cannot be swapped in Rust APIs. Supply epoch types here; the actual epoch-bound element handle map belongs with D01 observation construction and D02 freshness enforcement, not this pure-primitives batch. | `cargo test -p aero-core center_uses_visible_intersection`; `cargo test -p aero-core ids_are_distinct`; `cargo test -p aero-core --doc`. Test exact string round trips, NaN/zero/offscreen rejection, finite/strict-interior clipping, validated coordinate constructors and compile-fail cross-type checks. Full core and workspace gates must pass. | `feat(core): define scoped IDs and CSS geometry` |
| B02 | `crates/aero-core/src/actions.rs`, `crates/aero-core/src/lib.rs`. Copy §7.1, simplify JSON fixtures to one-line raw strings, add strict `validate()` with all listed limits and finite number checks. Generate schema from the same type only when planner/MCP need it. | `cargo test -p aero-core decision_`; tests `decision_is_strict_and_round_trips`, `decision_rejects_limits`, `decision_rejects_unknown_action`. | `feat(core): add strict versioned action contracts` |
| B03 | `crates/aero-core/src/form.rs`, `crates/aero-core/src/checkpoint.rs`, `crates/aero-core/src/events.rs`, `crates/aero-core/src/lib.rs`. Copy §7.2 and add task/run/contract references to versioned checkpoint/events, transient epoch IDs, artifact/evidence references and public status/usage. Authoritative TaskContract remains N01. | `cargo test -p aero-core form_json_preserves_empty_and_false`; `cargo test -p aero-core checkpoint_version`; reject wrong task/revision/future versions and invalid counters. | `feat(core): define task-linked recovery and event models` |
| B04 | `crates/aero-agent/src/store/migrations/001_initial.sql`, `crates/aero-agent/src/store/mod.rs`, `crates/aero-agent/src/lib.rs`, `crates/aero-agent/tests/journal.rs`. Use revised §7.5 TaskContract/run/evidence/artifact schema, private paths and pragmas, known-version migrations only. Do not implement the superseded session-only schema. | `cargo test -p aero-agent --test journal schema_and_foreign_keys`; user_version=1, task→run foreign keys, immutable revisions, duplicate IDs, corruption/future schema errors, reopen persistence. | `feat(store): add task-owned transactional journal` |
| B05 | `crates/aero-agent/src/store/worker.rs`, `crates/aero-agent/src/store/mod.rs`, `crates/aero-agent/tests/journal.rs`. Bounded writer thread methods create_task/revise_contract/create_run/prepare_action/commit_step/load_resume/load_audit/delete_task. Checked task-wide budget reservation and never-reused run-step allocation precede dispatch; commit verification/artifact/checkpoint metadata atomically. | `cargo test -p aero-agent --test journal step_and_checkpoint_are_atomic`; inject failure, verify all-old/all-new state and prepared orphan→needs_review; no run/task cross-owner record accepted. | `feat(store): journal task budgets and verified effects atomically` |
| B06 | `crates/aero-agent/src/config.rs`, `crates/aero-agent/tests/journal.rs`. Private XDG task/run staging, no global /tmp profile. One task controller/input owner lock; distinct runtime/profile boundaries. Deletion is N16 task-owned cleanup, not deletion of a run to reset budgets/uncertainty/history. | `cargo test -p aero-agent --test journal private_state_and_task_lock`; 0700/0600 permissions, independent tasks, lock conflict, no symlink/cross-task escape. | `feat(store): isolate task data and controller ownership` |

### Batch C — transport, process ownership, page targets (G0)

Execution order inside this batch: C01–C04, then the standalone fixture-harness task C07, then C05–C06. C07 depends only on the launcher/transport, so browser scenarios never depend on a helper that has not yet been introduced.

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| C01 | `crates/aero-cdp/src/{error.rs,connection.rs,lib.rs}`, `crates/aero-cdp/tests/transport.rs`. Local test WebSocket server with dynamic listener. Implement envelope parse/ID routing/optional session and ping/pong per §7.7. | `cargo test -p aero-cdp --test transport out_of_order_replies`; two concurrent calls answered in reverse order receive correct results; page session preserved; protocol error returned, not Null. | `feat(cdp): multiplex requests by ID and session` |
| C02 | `crates/aero-cdp/src/error.rs`, `crates/aero-cdp/src/connection.rs`, `crates/aero-cdp/src/lib.rs`, `crates/aero-cdp/tests/transport.rs`. Per-call deadline, cancellation cleanup, bounded incoming/outgoing size, pending map short lock, shutdown signaling. | `cargo test -p aero-cdp --test transport`; include `timeout_cleans_pending`, `dropped_future_cleans_pending`, `disconnect_fails_all_pending`, `full_writer_queue_does_not_deadlock`, `late_reply_is_ignored`. | `fix(cdp): bound and cancel transport operations` |
| C03 | `crates/aero-cdp/src/launcher.rs`, `crates/aero-cdp/tests/sandbox.rs`. Pure `SandboxSpec::build` with explicit mounts/env/flags and host-to-sandbox paths. Follow §7.8, including executable realpath/resources and no dangerous fallback flags. | `cargo test -p aero-cdp --test sandbox command_policy`; assert private roots, clearenv, no host-home/dbus/API-key exposure, no fixed port, no `--no-sandbox`; merged-/usr optional mount cases. | `feat(sandbox): construct least-privilege browser launch` |
| C04 | `crates/aero-cdp/src/launcher.rs`, `crates/aero-cdp/tests/sandbox.rs`, `crates/aero-cdp/src/runtime.rs`. Spawn process, read fresh `DevToolsActivePort`, bounded stderr reader, owned shutdown/reap, readiness deadline/early-exit handling. | `AERO_REQUIRE_BROWSER=1 cargo test -p aero-cdp --test sandbox --features browser-tests -- --nocapture`; tests `launch_reaches_cdp`, `readiness_failure_reaps_child`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`. Required setup creates canary outside mounts, not a real secret. | `feat(sandbox): own browser startup and cleanup` |
| C05 | `crates/aero-cdp/src/target_registry.rs`, `crates/aero-cdp/src/page.rs`, `crates/aero-cdp/src/runtime.rs`; `crates/aero-agent/tests/scenarios.rs`. Subscribe first, enable discovery, enumerate existing targets, attach explicitly flattened, keep browser/page IDs distinct. Exclude workers/devtools from tabs; one auto-attach authority. | Browser command §6 with case `page_target_navigation`; assert root `Runtime.evaluate` is routed to page session and actual title/URL changes. | `feat(cdp): own page target attachment and navigation` |
| C06 | `crates/aero-cdp/src/evaluate.rs`, `crates/aero-cdp/src/page.rs`; `crates/aero-agent/tests/scenarios.rs`. Check `exceptionDetails`; return-by-value limits; execution-context destroyed retry is not blanket error swallowing. Navigate waits loader/frame lifecycle, handles same-document SPA changes and blocked/error pages; generic stability has overall cap. | Cases `evaluate_exception_is_error` and `navigation_and_stability_deadlines`; long-lived fetch/socket and continuous mutations cannot hang forever; HTTP/network failure not reported success. | `feat(cdp): add bounded page evaluation and lifecycle waits` |
| C07 | `crates/aero-agent/tests/support/{mod.rs,fixture_server.rs,browser.rs}`, `crates/aero-agent/tests/scenarios.rs`, fixtures listed in §5. Axum or existing selected HTTP server binds port 0, shutdown guard; browser harness supplies private dirs and cleanup, event/predicate `wait_until` helpers. | `cargo test -p aero-agent --features browser-tests --test scenarios fixture_harness_smoke -- --exact`; assert server/browser reachable and both reaped after failure path. Missing browser is a test failure. | `test(browser): add deterministic mandatory fixture harness` |

### Batch D — observation, frame identity, grounding, input

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| D01 | `crates/aero-perception/src/{axtree.rs,observation.rs,lib.rs}`, `crates/aero-core/src/types.rs`; unit tests beside parser. Parse AX role/name/properties/children/frame; keep headings/text/table context and actionable controls, not every static node as clickable. Stable DOM order; handle assignment once after frame merge; bind each handle map to the B01 observation/runtime/document epoch and frame types for D02 freshness checks; escape untrusted text in prompt serialization. | `cargo test -p aero-perception axtree_`; assert roles/disabled/readonly/heading level, text-only content kept, ignored nodes omitted, bounded deterministic output and handles consistent with map. D02's `stale_observation_cannot_click` must reject prior-generation handles before input. | `feat(perception): build bounded semantic observations` |
| D02 | `crates/aero-cdp/src/geometry.rs`, `crates/aero-perception/src/observation.rs`, `crates/aero-executor/src/{target.rs,dispatcher.rs,lib.rs}`. Query geometry only for retained action candidates, concurrency limit 8. Resolve current backend node in its session, scroll, remeasure, clip, hit-test then dispatch exactly one move/down/up sequence. Missing/stale/occluded/disabled node is error. | Cases `click_has_one_event_sequence`, `scroll_remeasures_target`, `stale_observation_cannot_click`, `occluded_target_is_rejected`; count click events exactly, not ≥1. | `feat(executor): ground actions in fresh visible targets` |
| D03 | `crates/aero-executor/src/dispatcher.rs`; fixtures `crates/aero-agent/tests/fixtures/basic.html`, `crates/aero-agent/tests/fixtures/forms.html`. Focus once, verify active element, clear using Ctrl+A/Backspace (or chosen safe native input method); insert Unicode through `Input.insertText`; dedicated Enter key event, not newline text. PressKey allowlist; select/checked are explicit typed operations. | Cases `unicode_type_clear_and_enter`, `typing_does_not_double_click`, `select_and_checkbox_are_verified`; test emoji/non-Latin, controlled input, Enter submits only when approved. | `feat(executor): implement Unicode text and control actions` |
| D04 | `crates/aero-cdp/src/frame_manager.rs`, `crates/aero-cdp/src/target_registry.rs`; `crates/aero-agent/tests/scenarios.rs`. Frame graph from frame tree/lifecycle and Runtime context auxData, not equality of target/frame IDs; recursively configure attached OOPIF sessions. Remove subtrees on detach and invalidate epochs on navigate. | Cases `same_process_frame_is_observable`, `nested_oopif_attaches_and_detaches`; assert distinct IDs stay distinct and old nodes fail after navigation. | `feat(cdp): track frame and execution-context identity` |
| D05 | `crates/aero-cdp/src/frame_manager.rs`, `crates/aero-cdp/src/target_registry.rs`, `crates/aero-cdp/src/geometry.rs`, `crates/aero-perception/src/observation.rs`; fixtures `crates/aero-agent/tests/fixtures/merchant.html`, `crates/aero-agent/tests/fixtures/gateway.html`. Get exact frame owner via `DOM.getFrameOwner` in parent context; convert local DOM rectangles through content boxes; support nested axis-aligned translation/scaling and viewport clipping; reject unsupported transform. | Cases `two_iframes_map_to_correct_owners`, `nested_frame_coordinate_calibration`, `transformed_frame_fails_closed`; first visible frame is deliberately a decoy. Test body/root scroll, borders, scaling, DPR 1/2; click the named target exactly. | `fix(frames): correlate owners and normalize viewport coordinates` |
| D06 | `crates/aero-agent/tests/scenarios.rs`, fixtures `crates/aero-agent/tests/fixtures/merchant.html`, `crates/aero-agent/tests/fixtures/gateway.html`. Resolver maps `merchant.test` and `gateway.test` to 127.0.0.1; browser test flag `--site-per-process`; true labels; assert separate sessions and combined AX handles. Same-site cross-origin test uses differing ports separately. | Cases `cross_site_oopif_perception_and_input` and `same_site_cross_origin_frame`; type nonsecret fixture text, assert gateway event/value and no input in merchant/decoy. | `test(frames): prove cross-site OOPIF input routing` |
| D07 | `crates/aero-perception/js/locator.js`, `crates/aero-perception/src/form_recovery.rs`; fixtures `crates/aero-agent/tests/fixtures/shadow.html`. Root-relative unique escaped selectors; open-root traversal/host chain; separate frame path. Reuse locator in perception, forms, and replay; no invented `>>>` native selector. | Cases `nested_shadow_target_resolution`, `duplicate_ids_across_shadow_roots`, `selector_metacharacters`; exact correct root/field receives input; closed roots produce unsupported diagnostics. | `feat(perception): resolve nested open shadow controls` |

### Batch E — controller, planner, policy, working CLI (G1)

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| E01 | `crates/aero-agent/src/policy.rs`, `crates/aero-agent/src/approval.rs`, `crates/aero-agent/tests/policy.rs`. Keep §7.4 URL/path baseline, then integrate N06 actual ResolvedEffect capability matching and N07 material preview binding. Unknown effects require review; authorized research navigation does not prompt per click. | `cargo test -p aero-agent --test policy`; all_effect_paths_are_gated, approval_cannot_be_reused, origin_change_invalidates_approval, page_text_cannot_grant_permission, unsafe_schemes_are_rejected. | `feat(policy): authorize bounded delegation and commit boundaries` |
| E02 | `crates/aero-agent/src/controller.rs`, `crates/aero-core/src/events.rs`; unit tests in controller. Typed commands Start/Pause/Resume/Cancel/Approve/Reply; only one active task per controller. Pause cancels planning/read-only waits immediately, then acknowledges once any current input sequence (maximum 500 ms) has finished or become explicitly uncertain; no later dispatch may start. Never wait through a 90 s planner timeout to acknowledge pause. Takeover is exclusive; resume invalidates observations. | `cargo test -p aero-agent controller_`; `controller_pause_is_acknowledged`, `controller_cancel_interrupts_wait`, `controller_double_start_is_rejected`, `controller_takeover_has_single_input_owner`. Use paused Tokio time and fake dispatcher, not sleep-based races. | `feat(agent): serialize task control and takeover` |
| E03 | `crates/aero-agent/src/planner/mod.rs`, `crates/aero-agent/src/planner/scripted.rs`, `crates/aero-agent/tests/planner.rs`. Internal trait input includes immutable TaskContract revision, sanitized observation, budgets and bounded history; scripted fake consumes predefined decisions only in tests. Model output cannot select task/role or write verifier outcomes. | `cargo test -p aero-agent --test planner scripted_planner_consumes_validated_observations`; exhaustion/stale observation/invalid revision errors; no model credential required. | `feat(planner): propose actions within application-owned tasks` |
| E04 | `crates/aero-agent/src/planner/http.rs`, `crates/aero-agent/tests/planner.rs`. Configured provider/mock contracts, TLS/body/deadline/retry limits and usage; all processor transfers pass N13/N14 and task-budget reservations. No fallback from local-only/sensitive policy; no default example server/model. | `cargo test -p aero-agent --test planner`; valid_decision, auth_is_not_logged, invalid_output_has_one_retry, timeout_and_cancel, oversize_is_rejected, extra_actions_are_rejected, local_policy_blocks_remote_request. | `feat(planner): enforce task data and cost policy on inference` |
| E05 | `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/src/verification/registry.rs`, `crates/aero-agent/src/verification/completion.rs`, `crates/aero-agent/tests/scenarios.rs`. Wire Task→Observe→Strategy/Plan→Grant→Prepare→Execute→IndependentVerify→Commit. Use N05 task completion gate, N17/N18 evidence/artifacts, immutable task/run binding and cumulative budgets; observation stays pure. | Cases scripted_local_task_commits_history, supplier_task_delivers_verified_bundle, ask_user_pauses_without_success, action_failure_is_recorded, model_complete_cannot_bypass_criteria. Inspect SQLite/files/fixture ground truth, not only stdout. | `feat(agent): deliver verified task outcomes and evidence bundles` |
| E06 | `crates/aero-cli/src/args.rs`, `crates/aero-cli/src/main.rs`, `crates/aero-cli/tests/cli.rs`. Task-first routes run/status/resume/replay/artifacts/evidence/recovery-report plus doctor/auth/totp and optional mcp/evaluate. Trusted CLI validates a TaskContract/grant, not an unbounded goal string. Parse before initializing browser/provider/keyring; no provider required for status/audit. | `cargo test -p aero-cli --test cli`; help_has_no_side_effects, invalid_args_exit_2, runtime_failure_exit_1, task_status_has_criteria_artifacts, resume_requires_no_goal; `cargo run -p aero-cli -- --help` exits 0. | `feat(cli): expose task-oriented execution and result inspection` |
| E07 | `crates/aero-cli/src/main.rs`, `crates/aero-agent/src/controller.rs`, `crates/aero-agent/tests/scenarios.rs`. Wire Ctrl+C/shutdown to cancellation, journal status/uncertain in-flight effect, join runtime/store/planner tasks, flush store; stdout JSON results separated from stderr logs. | Cases `cli_cancel_reaps_runtime`, `failed_task_has_nonzero_exit`, `minimal_cli_end_to_end`; run fixture goal with scripted provider only via explicit test configuration. | `fix(cli): propagate failures and shut down cleanly` |

### Batch F — privacy, complete form state, snapshots, recovery (G2)

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| F01 | `crates/aero-perception/src/redaction.rs`, `crates/aero-agent/src/{credentials.rs,orchestrator.rs}`, fixture `crates/aero-agent/tests/fixtures/secrets.html`. Sensitive classifier/taint set keyed by document+node; redact AX values, configured text echoes, URL queries, errors/audit payloads; no raw CDP payload logging. Omit sensitive screenshots by default. | Cases `secrets_never_reach_planner_or_audit`, `otp_and_card_text_fields_are_sensitive`, `secret_echo_disables_image_context`; sentinels checked across captured planner request, DB audit, logs, artifacts. Password masking alone fails. | `feat(privacy): centralize secret classification and redaction` |
| F02 | `crates/aero-perception/{js/forms.js,src/form_recovery.rs}`, `crates/aero-core/src/form.rs` in core; fixture `crates/aero-agent/tests/fixtures/forms.html`. Implement capture §7.6 as array; serialize empty/false/radio-none/multi-select by values; skip secrets/files/disabled/readonly; bounded diagnostics. | Cases `form_capture_preserves_cleared_values`, `form_capture_excludes_sensitive_controls`, `form_snapshot_json_roundtrip`; assert exact entry values, not `len >= 4`. | `feat(forms): capture complete eligible form state` |
| F03 | `crates/aero-perception/js/forms.js`, `crates/aero-perception/src/form_recovery.rs`, `crates/aero-core/src/form.rs`, `crates/aero-agent/tests/scenarios.rs`. Implement native setter/event primitive §7.6, selectors/identity/origin checks, bounded custom-element wait, per-entry restore diagnostics and readback; no submit. Fixture includes reordered select options and duplicate names. | Cases `form_hydration_restores_exact_values`, `reordered_multiselect_uses_values`, `ambiguous_field_is_not_restored`, `form_restore_refuses_wrong_origin`; event assertions count expected changed controls. | `feat(forms): restore controls without ambiguous matching` |
| F04 | `crates/aero-agent/tests/fixtures/react-form.html`, `crates/aero-agent/tests/scenarios.rs`, `crates/aero-perception/js/forms.js`. Bundle a local pinned React test fixture through dev tooling; assert application state/output, not only listener counters. Add nested open-shadow plus same-process/OOPIF per-origin cases. | Cases `react_controlled_form_recovers`, `shadow_form_recovers`, `iframe_form_recovery_respects_origin`; hard reload/wipe context, restore, compare actual app output and skipped-field diagnostics. | `test(forms): prove framework and frame recovery behavior` |
| F05 | `crates/aero-cdp/src/storage_manager.rs`, `crates/aero-core/src/checkpoint.rs`, fixtures `crates/aero-agent/tests/fixtures/dynamic.html`; `crates/aero-agent/tests/scenarios.rs`. Capture cookies for authorized browser context and origin storage for each authorized tab/frame; retain cookie partition fields supported by negotiated browser schema. sessionStorage is per tab+origin, not global. | Cases `snapshot_covers_authorized_origins`, `session_storage_is_tab_scoped`, `partitioned_cookie_roundtrip_or_explicit_unsupported`; no unrelated profile cookies captured; cookie metadata preserved. | `feat(recovery): capture scoped browser storage` |
| F06 | `crates/aero-cdp/src/storage_manager.rs`, `crates/aero-core/src/checkpoint.rs`, `crates/aero-agent/tests/scenarios.rs` plus `crates/aero-agent/src/recovery.rs`. Before navigation set context cookies and per-tab origin-guarded preload; attach OOPIF preload before runtime resumes when recovery requires it; remove script IDs after usage. Redirect mismatch never gets another origin's values. Forms restore after readiness. | Cases `storage_exists_before_application_boot`, `restore_redirect_does_not_leak_state`, `restore_scripts_are_removed`; fixture reads local/session storage synchronously in first script; original PDF implementation must fail this test. | `fix(recovery): hydrate storage before page scripts run` |
| F07 | `crates/aero-agent/src/{snapshot_crypto.rs,credentials.rs}`, `crates/aero-agent/tests/vault.rs`, `crates/aero-agent/tests/journal.rs`; Cargo `vault` feature. Use vetted AEAD (XChaCha20-Poly1305 acceptable), fresh cryptographic nonce per snapshot, version/session/step as authenticated associated data; key held in Secret Service outside browser. Metadata never contains raw secret payload. | `cargo test -p aero-agent --features vault --test vault`; tests `snapshot_ciphertext_hides_sentinels`, `tampered_or_wrong_session_snapshot_fails`, `missing_key_never_writes_plaintext`, `nonce_is_unique`; test failures propagate. | `feat(recovery): encrypt sensitive browser snapshots` |
| F08 | `crates/aero-agent/src/recovery.rs`, `crates/aero-agent/src/controller.rs`, `crates/aero-cdp/src/runtime.rs`; tests `recovery.rs`. Race phase future against deadline/cancellation/disconnect/process exit; monotonic deadline and independent supervisor. Keep no unrelated mutex across await. | `cargo test -p aero-agent --test recovery hung_operation_is_interruptible`; fake never-resolving CDP/planner futures are interrupted, appropriate phase-specific timeout recorded, paused/human-wait state doesn't restart. | `fix(agent): make stalled operations recoverable` |
| F09 | `crates/aero-agent/src/recovery.rs`, `crates/aero-agent/src/controller.rs`, `crates/aero-cdp/src/runtime.rs` plus `crates/aero-agent/src/orchestrator.rs`; tests `recovery.rs`, `scenarios.rs`. Rebuild all runtime-owned managers from retained config, cancel/join old generation, restore committed cursor/history/budget, cap restart count; new handles/epochs. | Cases `browser_kill_restores_committed_state`, `all_managers_use_new_runtime`, `recovery_budget_survives_resume`; old connection receives no post-restart requests and old handles cannot dispatch. | `feat(recovery): rebuild runtime and resume committed tasks` |
| F10 | `crates/aero-agent/src/{recovery.rs,store/worker.rs}`, `crates/aero-agent/tests/recovery.rs`; fixture endpoint records effects. Crash at every intent/dispatch/verify/commit boundary. If prepared intent may have executed, pause and require review; never retry inferred purchase/delete/submit. | `cargo test -p aero-agent --test recovery uncertain_effect_is_not_repeated`; browser variant case `kill_after_server_effect_before_commit`; server counter stays exactly 1, status `needs_review`. | `fix(recovery): refuse duplicate uncertain effects` |
| F11 | `crates/aero-cli/src/args.rs`, `crates/aero-cli/src/main.rs`, `crates/aero-agent/src/recovery.rs`, `crates/aero-cli/tests/cli.rs`, `crates/aero-agent/tests/scenarios.rs`. `aero resume ID` resolves task/run ownership and resumes original approved contract, journal/cursor/task-wide budgets with no default navigation or duplicate task. Render N19 report for expired auth/missing key/partial forms/uncertain effect. | Cases resume_requires_no_goal, resume_keeps_task_attempt_and_token_budget, partial_restore_requests_user; completed/unknown/corrupt IDs fail clearly. | `feat(cli): resume tasks with explicit recovery diagnostics` |

### Batch G — local product subsystems (G3)

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| G01 | `crates/aero-agent/src/{credentials.rs,totp.rs}`, `crates/aero-cli/src/{args.rs,main.rs}`, `crates/aero-agent/tests/vault.rs`. Secret Service worker, typed errors, exact-origin+account identity; hidden terminal entry for password/TOTP seed (no secret CLI argument/env echo). Store username+password atomically as one secret record, zeroize temporary buffers; TOTP RFC6238 with injected clock, 6 digits/30 s defaults and explicit algorithm settings. | `cargo test -p aero-agent --features vault --test vault`; RFC6238 published vectors; locked/unavailable keyring; account isolation; no Debug/Serialize of plaintext. Required native keyring test runs in dedicated D-Bus session, not user's real vault. | `feat(vault): add origin-scoped credentials and TOTP` |
| G02 | `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/src/policy.rs`, `crates/aero-executor/src/dispatcher.rs`, `crates/aero-agent/tests/scenarios.rs`. Approved fill verifies actual target frame origin, HTTPS, field type, fresh focus before every secret injection; re-check after navigation. No auto-submit; secret-safe boolean verification; mark field/echo taint before exposure. | Cases `vault_fill_is_blind_and_origin_bound`, `credential_frame_origin_mismatch_is_denied`, `totp_rollover_waits_for_fresh_code`; planner/log/DB scans find no sentinels. | `feat(vault): inject secrets through guarded browser actions` |
| G03 | `crates/aero-cdp/src/network/{mod.rs,filter.rs,interceptor.rs}`, `crates/aero-agent/tests/scenarios.rs`. Exact domain/subdomain rules; optional local hosts list, comments/aliases; avoid global `/ads/` substring blocking. Subscribe before Fetch.enable, route event's session, exactly one continue/fail per request, cancellation disables interception. Observe network policy for every attached target. | `cargo test -p aero-cdp network_filter_`; cases `interceptor_resolves_every_paused_request`, `interceptor_lag_fails_open_for_tracker_filter_only`, `network_permission_failures_fail_closed`. Ad blocking failure may disable optional ad filtering; security/network-permission enforcement may not fail open. | `feat(network): add bounded session-aware tracker filtering` |
| G04 | `crates/aero-perception/src/overlays.rs`, `crates/aero-agent/src/policy.rs`, `crates/aero-agent/tests/scenarios.rs`. Report known consent/modal controls as candidates; default reject nonessential cookies, require policy permission before dismissal; never remove arbitrary DOM nodes or rewrite body CSS. | Cases `observation_does_not_mutate_page`, `consent_defaults_to_reject`, `ambiguous_dialog_is_not_clicked`; confirm no call outside approved executor path. | `fix(perception): make overlay handling non-destructive` |
| G05 | `crates/aero-cdp/src/target_registry.rs`, `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/tests/scenarios.rs`. New/switch/close tabs with deterministic active selection, wait attach readiness, popup ownership, session/context filtering; close invalidates observations. | Cases `tabs_and_popups_route_to_active_page`, `worker_targets_are_not_user_tabs`, `closing_active_tab_selects_deterministically`; page B never receives page A action. | `feat(browser): add deterministic tab and popup lifecycle` |
| G06 | `crates/aero-cdp/src/downloads.rs`, `crates/aero-cdp/src/runtime.rs`, `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/tests/scenarios.rs`, fixture `crates/aero-agent/tests/fixtures/downloads.html`. Arm future before action; correlation by context/frame/start URL with ambiguity rejection; cache terminal states; GUID internal filenames in sandbox-mounted staging; reject cancel/timeout/oversize; explicit artifact export. | Cases `instant_download_is_not_missed`, `download_path_is_mounted`, `malicious_filename_cannot_escape`, `concurrent_downloads_do_not_cross_sessions`, `cancel_cleans_notifiers`; verify actual bytes/hash and no overwrite/symlink race. | `feat(downloads): track race-free isolated download artifacts` |
| G07 | `crates/aero-perception/{js/extract.js,src/extractor.rs}`, `crates/aero-agent/src/export.rs`, `crates/aero-agent/tests/export.rs`, fixture `crates/aero-agent/tests/fixtures/table.html`. Target resolved in its frame/session. Tables include thead/tbody/no-tbody, duplicate/empty headers, rowspan/colspan normalization and bounded cells/rows; explicit mappings for card lists, no generic nested-span duplication. Typed result has ordered columns and aligned nullable cells, not HashMap with duplicate header loss. | `cargo test -p aero-agent --test export`; cases `table_extraction_preserves_all_rows`, `duplicate_headers_are_deduplicated_stably`, `rowspan_colspan_are_aligned`, `extraction_limits_are_enforced`. Browser case `extracts_target_frame_table`. | `feat(extraction): normalize bounded DOM datasets` |
| G08 | `crates/aero-agent/src/export.rs`, `crates/aero-agent/tests/export.rs`; default CSV. Generated private artifact path; fixed column order, quote/newline/Unicode preservation, null-vs-empty documented; spreadsheet formula neutralization by default with explicit raw mode. Exclusive atomic publication and readback count. | `cargo test -p aero-agent --test export csv_`; `csv_roundtrip`, `csv_formula_is_neutralized`, `csv_failed_write_does_not_publish`, `csv_path_escape_is_rejected`. | `feat(export): write safe verified CSV artifacts` |
| G09 | `crates/aero-agent/src/export.rs`, `crates/aero-agent/tests/export.rs`; `parquet` feature adds one compatible Arrow/Parquet family. Batch rows, preserve nullable UTF-8 schema, Snappy only if enabled. Feature-off request returns unsupported error before browser action. | `cargo test -p aero-agent --features parquet --test export parquet_roundtrip`; read file back with Parquet reader, compare schema/count/nulls/Unicode; `cargo tree -d` review Arrow duplicates. | `feat(export): add optional batched Parquet output` |
| G10 | `crates/aero-cdp/src/pool.rs`, `crates/aero-cdp/src/runtime.rs`, `crates/aero-agent/tests/scenarios.rs`. Supervisor owns leases/context IDs/shared counter; acquire permit then context/target; dispose on each setup failure, job error, panic, cancellation, lease drop. JoinSet bounded scheduling and queued jobs ≤64; context isolation explicitly not process sandbox isolation. | Cases `worker_contexts_do_not_share_cookies`, `worker_cancellation_disposes_context`, `worker_failure_does_not_leak_permit`, `worker_limit_is_respected`; active contexts return to baseline before next lease. | `feat(workers): supervise bounded isolated browser contexts` |
| G11 | `crates/aero-agent/src/memory.rs`, `crates/aero-agent/tests/memory.rs`, `crates/aero-agent/src/store/worker.rs`. Core bounded history and offline audit; optional opt-in SQLite FTS retrieval of successful sanitized summaries only. N13/N14 data policy and N16 deletion apply; retrieved instructions cannot grant authority or replay old handles. | `cargo test -p aero-agent --test memory`; history_budget_is_bounded, only_successful_sanitized_examples_are_retrieved, memory_cannot_grant_permissions, task_delete_removes_memory. | `feat(memory): retain bounded task context without authority leakage` |
| G12 | `crates/aero-agent/src/memory.rs`, `crates/aero-agent/tests/memory.rs`, `crates/aero-agent/Cargo.toml` plus `semantic-memory` Cargo feature. LanceDB + FastEmbed adapter only after graph compiles; pin model ID/revision/dimension/checksum explicitly; provisioning separate from run, embedding on blocking worker, small-table exact search then measured index if needed. Missing model is disabled feature diagnostic, not network download. | `cargo test -p aero-agent --features semantic-memory --test memory`; deterministic fake embedder unit tests plus explicit provisioned local-model integration test `semantic_index_roundtrip`; assert vector dimension, deletion/upsert/no duplicates and prompt budget. | `feat(memory): add optional local semantic retrieval` |
| G13 | `crates/aero-perception/src/som.rs`, `crates/aero-perception/assets/{font.ttf,font.LICENSE}`, `crates/aero-agent/tests/scenarios.rs`. Add feature `som`, licensed bundled font, same observation handles, clip badge layout, CSS→pixel mapping from actual screenshot metadata not integer DPR assumptions, secret image suppression; PNG output MIME matches bytes. Only capture on explicit request/fallback, not every step. | `cargo test -p aero-perception --features som som_`; decoded image dimensions/format/markers at DPR 1,1.25,2 and offscreen bounds; browser case `som_uses_same_handles_as_observation`; no sensitive image emitted. | `feat(perception): add optional calibrated visual marks` |
| G14 | `crates/aero-agent/src/replay.rs`, `crates/aero-cli/src/args.rs`, `crates/aero-agent/tests/scenarios.rs`, `crates/aero-agent/tests/journal.rs`. `replay ID` is offline audit events with original timing optional and zero browser/provider initialization. Separate `replay ID --live` requires opt-in, fresh durable locator resolution, current policy/approval, and stop on divergence; no promise of determinism. | `cargo test -p aero-agent --test journal offline_replay_has_no_side_effects`; cases `live_replay_resolves_fresh_handles`, `live_replay_refuses_ambiguous_target`, `live_replay_requires_new_approval`. | `feat(replay): separate audit playback from guarded re-execution` |
| G15 | `crates/aero-agent/src/mcp/{mod.rs,server.rs,client.rs}`, `crates/aero-cli/src/args.rs`, `crates/aero-agent/tests/mcp.rs`. Select one official Rust MCP SDK/protocol version supported by target clients and lock it; expose stdio tools observe/step/start/status/pause/cancel via controller with session authorization and schema. Approval requests return a pending handle; approval grants and user replies are accepted only through the separate trusted CLI/GUI human channel, never as model-callable tools. Separate optional external-tool client negotiates capabilities, timeouts, cancellation, pagination and child lifecycle. No stdout logs; do not implement raw sampling-as-LLM. Remote HTTP/OAuth only when explicitly configured and tested. | `cargo test -p aero-agent --features mcp --test mcp`; `mcp_tools_list_and_call`, `mcp_invalid_input_is_protocol_error`, `mcp_policy_is_not_bypassed`, `mcp_stdio_is_clean`, `mcp_child_shutdown_is_reaped`; interop smoke with real selected SDK client, no model required. | `feat(mcp): expose guarded browser tools and optional tool client` |
| G16 | `crates/aero-agent/src/telemetry.rs`, `crates/aero-cli/src/main.rs`, `crates/aero-agent/tests/telemetry.rs`, `packaging/monitoring/{alerts.yml,alerts.test.yml,grafana-dashboard.json}`. Fixed metric names from PDF, bounded labels only action/phase/outcome; no URL/goal/session labels. Loopback opted-in listener, structured public events, OTLP optional. Failure ratio is `sum(rate(aero_step_errors_total[5m])) / clamp_min(sum(rate(aero_action_dispatched_total[5m])), 0.001) > 0.15`; instrument attempts consistently so numerator is subset of denominator. | `cargo test -p aero-agent --test telemetry`; `metrics_bind_loopback`, `secret_labels_are_impossible`, `counter_semantics_match`; `promtool check rules packaging/monitoring/alerts.yml`; `promtool test rules packaging/monitoring/alerts.test.yml` tests zero traffic and 20% errors; dashboard JSON parser exits 0 and panel queries reference existing metrics. | `feat(observability): add safe metrics and validated alerts` |

### Batch H — supervisory desktop (G4)

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| H01 | `crates/aero-shell/src-tauri/Cargo.toml`, `crates/aero-shell/src-tauri/build.rs`, `crates/aero-shell/src-tauri/tauri.conf.json`, `crates/aero-shell/src-tauri/capabilities/main.json`, `crates/aero-shell/src-tauri/src/main.rs`, `crates/aero-shell/ui/package.json`, `crates/aero-shell/ui/package-lock.json`, `crates/aero-shell/ui/index.html`, `crates/aero-shell/ui/tsconfig.json`, `crates/aero-shell/ui/vite.config.ts`, `crates/aero-shell/ui/src/main.tsx`, `crates/aero-shell/ui/src/styles.css`, `Cargo.toml`. Proceed only after accepted N10 presentation matrix; trusted Tauri local UI, locked React/TypeScript/plain CSS, explicit native prerequisites. | `npm --prefix crates/aero-shell/ui ci`; typecheck/build scripts; `cargo check -p aero-shell --locked`; no remote controlled URL loaded as privileged WebKit UI. | `feat(desktop): scaffold validated supervisory presentation` |
| H02 | `crates/aero-shell/src-tauri/src/state.rs`, `crates/aero-shell/src-tauri/src/commands.rs`, `crates/aero-shell/src-tauri/src/main.rs`, `crates/aero-shell/ui/src/api.ts`, `crates/aero-shell/ui/src/types.ts`. UI routes through N08 SessionService, generates authoritative DTO types, role-scoped commands and event sequence recovery; no duplicate engine/nested runtime. Initial host in-process/pause-on-close; optional service only after N09. | `cargo test -p aero-shell commands_`; `npm --prefix crates/aero-shell/ui test -- --run api.test.ts`; identity/role/ack/error/close policy tests. | `feat(desktop): adapt session-service contracts to trusted UI` |
| H03 | `crates/aero-cdp/src/screencast.rs`, `crates/aero-shell/src-tauri/src/commands.rs`, `crates/aero-shell/ui/src/Viewport.tsx`. Implement stream only where accepted by N10; native companion is valid selected primary arrangement. Actual controlled target, prompt frame acks, latest-frame bounds, tab/epoch metadata, sensitive-image suppression. Never use stream as proof of native a11y/selection/dialog support. | Cases screencast_is_same_controlled_target, screencast_ack_does_not_block_agent, tab_switch_replaces_screencast; UI Viewport.test.tsx rejects stale generation and honors selected native fallback. | `feat(desktop): present the actual controlled browser safely` |
| H04 | `crates/aero-shell/ui/src/Viewport.tsx`, `crates/aero-shell/src-tauri/src/commands.rs`, `crates/aero-agent/src/controller.rs`. Manual input only after acknowledged takeover; map image content box/letterboxing to root CSS pixels using frame metadata, no DPR double scaling; focus/keyboard Unicode; stale generation rejected. Browser native dialogs/file picker/CAPTCHA use supervised separate window fallback, not fake support in a screenshot canvas. | Case `takeover_blocks_agent_and_maps_input`; UI tests for resized/letterboxed view and click outside image; agent can resume only after fresh observation invalidates prior handles. | `feat(desktop): implement exclusive manual takeover` |
| H05 | `crates/aero-shell/ui/src/App.tsx`, `crates/aero-shell/ui/src/TaskWorkspace.tsx`, `crates/aero-shell/ui/src/Timeline.tsx`, `crates/aero-shell/ui/src/Approvals.tsx`, `crates/aero-shell/ui/src/Artifacts.tsx`, `crates/aero-shell/ui/src/RecoveryReport.tsx`, `crates/aero-shell/ui/src/styles.css`, `crates/aero-shell/src-tauri/src/commands.rs`. Integrate N20 task/criteria/result/artifact/questions workspace and N21 browser essentials; audit timeline secondary. Material commit previews, usage/budgets, explicit incomplete/uncertain state, bounded events and safe rendering. | `npm --prefix crates/aero-shell/ui test -- --run`; start failure/double start/pause ack/approval denial/unmount/gap resync plus required criterion failure never shown completed and keyboard-only artifact/recovery inspection. | `feat(desktop): deliver task-first work and result workspace` |
| H06 | `docs/development.md`, `docs/security.md`, desktop integration tests; no blanket capability expansion. CSP allows only local assets and required image/blob transport, forbids remote script/eval; invoke permissions scoped to local window; external links opened only through approved navigation. Test native X11 and Wayland independently on actual prepared systems. | `cargo test -p aero-shell`; UI typecheck/build/tests; native desktop smoke on both backends: run task, pause, manual type, resume, approve/deny, cancel, close app; all owned processes exit. Record actual environment/results and explicit unsupported native dialog limitations. | `test(desktop): validate native supervision and IPC boundaries` |

### Batch I — CI, distributions, documentation (G5)

| Task | Exact files and implementation | Red/green command; required assertions | Approved commit message |
|---|---|---|---|
| I01 | `scripts/check.sh`, `scripts/test-browser.sh`, `.github/workflows/ci.yml`. Start CI at G1, extend each feature: minimal Rust, required native browser/evaluation safety, selected desktop, explicitly shipped optional features and packaging. Real prerequisites, no required silent skip/continue-on-error; missing backend means not shipped or failed gate. | `bash scripts/check.sh`; `bash scripts/test-browser.sh`; N23 g1 evaluation then full declared shipped suite; feature commands only for implemented/provisioned selected features. Remote CI status remains unverified until actual approved remote run. | `ci: enforce task quality and native feature gates continuously` |
| I02 | `docs/security.md`, `docs/recovery.md`, `crates/aero-agent/tests/policy.rs`, `crates/aero-agent/tests/recovery.rs`, `crates/aero-cdp/tests/sandbox.rs`. Threat model includes hostile page/provider/tool, compromised renderer, local CDP access, host network/X11, persistence, trust in OS/keyring, encrypted payload limits. Test malicious URLs, redirect exfiltration, prompt injection, untrusted selectors, huge trees/bodies, poisoned retrieval, secret echoes, corrupted ciphertext, path races, cancellation. | Relevant unit/browser suites; `cargo audit` after installation approval; dependency/license report reviewed. No assertion that pattern redaction can prevent arbitrary encoded exfiltration by a hostile page. | `security: document and test trust boundaries` |
| I03 | `scripts/build-deb.sh`, `packaging/debian/control`, `docs/releasing.md`. Native package first: binary names `aero`/`aero-shell` exactly, runtime prerequisites (including actual Chromium/bwrap policy), `.desktop` metadata/icon/licenses, no credentials/models/profiles. Build on baseline distro. Do not bundle an outdated unmaintained Chromium. | `bash scripts/build-deb.sh`; `dpkg-deb --info dist/*.deb`; install into clean disposable VM and run `aero doctor --json`, browser smoke and GUI smoke. Expected clean package install and truthful runtime checks; unavailable browser isn't silently fetched. | `build: package native Linux release` |
| I04 | `packaging/appimage/{AppRun,aero.desktop}`, `scripts/{build_appimage.sh,check-artifacts.sh}`, browser lock. Shell script uses strict mode, controlled paths, quoted variables, and reviewed cleanup paths. AppRun executes packaged exact bin names and actually consumes AERO_CHROMIUM/AERO_BWRAP configuration. Tools/runtime assets pinned/checksummed, no downloads in AppRun, no unverified continuous tool. AppImage either includes an explicitly maintained browser bundle or clearly documented external dependency; choose bundled stable browser for portable release after licensing/update review. | `bash -n packaging/appimage/AppRun scripts/build_appimage.sh`; ShellCheck; package then `--appimage-extract` and run extracted `usr/bin/aero doctor --json` in clean VM, plus real task/GUI/sandbox smoke. Verify dependencies through `ldd` and required WebKit resources on baseline system. | `build: produce verified AppImage artifact` |
| I05 | `.github/workflows/release.yml`, `scripts/check-artifacts.sh`, `docs/releasing.md`, license files/assets. Build/release inputs immutable; checksums/SBOM/license manifest and provenance accompany artifacts; signing identity and secret storage supplied by owner. No release upload without explicit approval. Verify signatures against configured public identity on a clean machine. | Local artifact verification exits 0; `sha256sum -c dist/SHA256SUMS`; actual chosen signing tool verify command recorded after identity decision. No fabricated hash, key, signature, or published URL. | `release: add provenance and artifact verification gates` |
| I06 | `README.md`, `docs/specification.md`, `docs/architecture.md`, `docs/development.md`, `docs/recovery.md`, `docs/releasing.md`, `docs/requirements.md`, `docs/evaluation.md`, `docs/viewport-feasibility.md`. Exercise quickstart/task grants/local-only distinction/artifact-evidence/recovery/native presentation/limits/retention; documented optional features cannot masquerade as core completion. Replace stealth/hardware/lossless/universal undo/hidden reasoning claims. | Fresh baseline machine executes supported quickstart and required workflow; requirement register has actual passing proof or explicit not-shipped status; baseline/holdout/UX measurements recorded honestly. `git diff --check`. | `docs: publish verified task-first operator and developer guides` |

### Batch N — approved task-first product changes (integrated, not last)

These work packages implement `docs/specification.md`'s requirement register. Follow the dependency schedule above; the letter N does not mean “after release.” Each numbered sub-action below is a separate small edit/check. Execute one failing behavior at a time, then the same focused command to green; expand a complex adapter into another reviewed test/branch cycle rather than claiming an entire feature takes five minutes. All code belongs in the exact future paths named here; no project code was created by this documentation revision.

For each N package, final review uses `git diff --check`, `git diff --` followed by its explicitly listed files, and the §6 scoped commit template only after approval. Never grant an agent permission to commit automatically.

#### N01 — durable strict TaskContract (`REQ-TASK-01`)

Files: `crates/aero-core/src/task.rs`, `crates/aero-core/src/lib.rs`, `crates/aero-core/tests/task_contract.rs`.

1. Add a roundtrip test using the complete §7.10 contract JSON. Assert task ID/revision/criteria/output references survive serialization; unknown fields and duplicate criterion/output IDs fail.
2. Run `cargo test -p aero-core --test task_contract contract_is_strict_and_roundtrips -- --exact`; expected red: model/validator does not yet satisfy the asserted contract.
3. Add typed TaskContract/Budget/ProcessingMode/Criterion/OutputRequirement enums/structs with serde strictness and bounds. Export the module; then implement one validator branch per failing invalid fixture. No executable checker code from JSON.
4. Re-run the exact test, then `cargo test -p aero-core`; expected every added case runs/passes. Add separate `task_and_run_ids_are_distinct` to prevent treating browser/session ID as TaskID.

Approved commit: `feat(tasks): define strict durable task contracts`.

#### N02 — immutable revisions and run ownership (`REQ-TASK-01`, `REQ-TASK-02`)

Files: `crates/aero-agent/src/tasks.rs`, `crates/aero-agent/src/store/worker.rs`, `crates/aero-agent/tests/tasks.rs`.

1. Write `contract_revision_requires_trusted_owner`: trusted user creates revision 1; model caller cannot mutate/create revision 2; a trusted revision creates a new immutable row, invalidates affected grants, and cannot rewrite run history.
2. Run `cargo test -p aero-agent --test tasks contract_revision_requires_trusted_owner -- --exact` to red.
3. Implement create/revise/start-run through the single writer and host-constructed caller capability. Cross-task/session/revision mismatch is error; provider change/UI reconnect is not a new Task.
4. Add/run `new_run_preserves_task_identity_and_history` and the focused test to green. No hidden fixture helper may directly issue production grants.

Approved commit: `feat(tasks): persist immutable intent across runs`.

#### N03 — cumulative reserved budgets (`REQ-TASK-02`)

Files: `crates/aero-agent/src/budget.rs`, `crates/aero-agent/src/store/worker.rs`, `crates/aero-agent/tests/budget.rs`.

1. Test two concurrent reservations whose sum exceeds the contract limit: at most one succeeds; retry/resume/new run does not restore spent or uncertain reserved tokens/attempts. Add active-time versus paused-time case with injected clock.
2. Run `cargo test -p aero-agent --test budget`; expected named assertions fail before reservation implementation.
3. Implement checked integer reservation/settlement in a writer transaction, persisted unknown usage after crash, configured money upper bounds only where known, and separate zero-default purchase spending. Invalid/missing required limits fail before work.
4. Run the same suite to green; expected unknown-price cost remains null/unknown, never “free”; a budget failure dispatches no provider/action request.

Approved commit: `feat(budgets): reserve task-wide resources before execution`.

#### N04 — independent verifier registry (`REQ-VERIFY-01`)

Files: `crates/aero-core/src/verification.rs`, `crates/aero-agent/src/verification/mod.rs`, `crates/aero-agent/src/verification/registry.rs`, `crates/aero-agent/tests/verification.rs`, `crates/aero-agent/src/lib.rs`.

1. Define a test-local fake readback adapter with an explicit stored value. Test `dispatch_ack_is_not_verification`: dispatcher returns success but actual value is wrong; verifier must return Failed. Unknown checker returns Inconclusive/unsupported, never Passed.
2. Run `cargo test -p aero-agent --test verification dispatch_ack_is_not_verification -- --exact` to red.
3. Add typed Postcondition/CriterionResult and registry entries for navigation, field state, artifact schema/count and evidence references. Registry has read-only adapters; it cannot issue grants, call mutating actions, or use model confidence as proof.
4. Re-run exact test and suite to green; add `planner_cannot_write_passed_criteria`. Connect real browser/artifact readbacks in N05/N17/N18/E05, not fake production success.

Approved commit: `feat(verification): separate effects from independent checks`.

#### N05 — task completion gate (`REQ-VERIFY-02`)

Files: `crates/aero-core/src/verification.rs`, `crates/aero-agent/src/verification/completion.rs`, `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/tests/verification.rs`.

1. Copy §7.10's complete predicate and failing test first, initially with a minimal false implementation so expected successful case is red. Run `cargo test -p aero-core completion_requires_checks_artifacts_and_resolved_effects` and implement the shown predicate to green.
2. Add `model_complete_cannot_bypass_criteria`: submit a model Complete proposal with missing artifact, inconclusive criterion and unresolved intent; no completed transition may occur.
3. Run `cargo test -p aero-agent --test verification model_complete_cannot_bypass_criteria -- --exact` to red; implement validated task/revision-owned criterion lookup, required artifact readback and unresolved-effect check.
4. Add tests for explicit allowed user attestation with attribution, disallowed self-attestation, stale evidence/revision, and all-required-pass success. Run the same suite to green. UI/CLI/events consume this result, not a separate success boolean.

Approved commit: `feat(verification): enforce evidence-backed task completion`.

#### N06 — useful bounded delegation (`REQ-GRANT-01`, `REQ-GRANT-03`)

Files: `crates/aero-core/src/capability.rs`, `crates/aero-agent/src/capabilities.rs`, `crates/aero-agent/src/policy.rs`, `crates/aero-agent/tests/capabilities.rs`.

1. Test an approved public-research envelope: several permitted navigations/extractions need no new prompt; crossing origin/account/effect/quota/expiry requires review. A submit/purchase-shaped click and unknown autosave are not harmless merely because the model says so.
2. Run `cargo test -p aero-agent --test capabilities`; expected red assertions `research_grant_avoids_per_click_prompts` and `boundary_crossing_requires_review`.
3. Implement grant matching on host-resolved effect and trusted role; persist immutable grant payload/issuer plus expiry/revocation/quota use in capability_grants. Restart revalidates those records, never regenerates authority from model history. All strategy/replay/MCP/worker paths use it; page/LLM/procedure cannot issue a grant or select an account/profile path.
4. Re-run suite to green, including `model_cannot_grant_its_own_capability` and `expired_or_revised_grant_is_rejected`.

Approved commit: `feat(delegation): authorize bounded task envelopes`.

#### N07 — preparation and material commit preview (`REQ-GRANT-02`, `REQ-GRANT-03`)

Files: `crates/aero-agent/src/preview.rs`, `crates/aero-agent/src/approval.rs`, `crates/aero-agent/tests/capabilities.rs`.

1. Write a complete synthetic preview with destination/account/fields/artifact IDs/recipient/amount/currency/expiry. Mutate one material field at a time; old approval must fail. Unrelated animation metadata must not invalidate the material digest.
2. Run `cargo test -p aero-agent --test capabilities preview_change_invalidates_commit -- --exact` to red.
3. Canonicalize typed material data (integer minor-unit money), bind to task/run/revision and required target freshness, and consume trusted approval once immediately before dispatch. Re-check effective target origin/account/payload; no vault auto-submit or approve MCP tool.
4. Re-run to green; add `prepare_never_commits`, `approval_is_single_use`, and `unknown_amount_requires_review`. UI preview integration is N20, not duplicate frontend authorization logic.

Approved commit: `feat(approvals): bind commits to material transaction previews`.

#### N08 — shared role-scoped session interface (`REQ-SERVICE-01`)

Files: `crates/aero-agent/src/service/mod.rs`, `crates/aero-agent/src/service/local.rs`, `crates/aero-agent/src/controller.rs`, `crates/aero-agent/tests/service.rs`, `crates/aero-agent/src/lib.rs`.

1. Test that trusted CLI and GUI clients observe the same task/run/event sequence, while model adapter cannot revise contract/grant approval or access another task. Include duplicate start and reconnect-after-event-gap behavior.
2. Run `cargo test -p aero-agent --test service`; expected shared-state/role assertions red.
3. Introduce one in-process service facade and host-constructed caller capabilities around the existing controller. Methods are enumerated in §7.11; adapters have no alternate executor path. Do not add network sockets or a daemon here.
4. Re-run suite to green. Resume after manual ownership reacquisition takes a fresh observation and invalidates stale target grants.

Approved commit: `refactor(control): share one role-scoped application interface`.

#### N09 — explicit close policy and optional local service (`REQ-SERVICE-02`)

Files for baseline: `crates/aero-agent/src/service/local.rs`, `crates/aero-agent/src/controller.rs`, `crates/aero-agent/tests/service.rs`. Optional backend paths only after its gate: `crates/aero-agent/src/service/unix.rs`, `crates/aero-cli/src/service_host.rs`, `docs/adr/0008-local-service.md`.

1. Test `close_defaults_to_pause_and_flush`: no new automation after close ack, pending effects committed/marked uncertain, browser/store cleanup. Run `cargo test -p aero-agent --test service close_defaults_to_pause_and_flush -- --exact` to red, implement minimally, then green.
2. Record that in-process host crash stops execution and requires journal recovery. Do not implement a fake keep-running flag.
3. Only if approved background/UI-crash isolation is needed: write an ADR and failing `background_survives_ui_disconnect_only_with_grant` / `untrusted_peer_cannot_issue_approval` tests in `crates/aero-agent/tests/service.rs`; run the same service suite.
4. Implement private UDS/peer credentials/owner-only directory, role-separated authenticated connections, bounded versioned requests and event reconnect; no TCP. Test process restart/cleanup and explicit stop/cancel. Unsupported backend rejects `continue_on_close` before task launch.

Approved baseline commit: `feat(lifecycle): make task close behavior explicit`. Optional process extraction is a separately approved commit/gate, not mandatory initial scope.

#### N10 — early viewport feasibility (`REQ-VIEW-01`, `REQ-UX-02`)

Files: `docs/viewport-feasibility.md`, `docs/adr/0001-ui-engine-boundary.md`, `experiments/viewport/README.md`, `crates/aero-agent/tests/scenarios.rs`; any disposable probe code stays under `experiments/viewport/` and is not promoted to production unchanged.

1. Create a pass/fail/unsupported matrix for actual-target stream and native companion: text selection/copy, Unicode/IME, shortcut/focus, scroll/resize, keyboard/screen-reader access, popup/auth/file dialogs, takeover latency and app close. Include native X11 and Wayland environment/version columns.
2. Add/run browser smoke `AERO_REQUIRE_BROWSER=1 cargo test -p aero-agent --features browser-tests --test scenarios viewport_uses_controlled_target -- --exact`; expected red before the target/ownership probe exists; implement the minimum probe, then green.
3. Execute one manual matrix scenario at a time on prepared native systems. Record actual measurements and unsupported behavior, not anticipated passes. Provisioning/manual platform access is a blocker, not a five-minute completed implementation task.
4. Obtain owner acceptance of presentation/limitations before H01. If neither candidate supports required interaction/a11y, stop desktop commitment for a separate embedding feasibility decision; CLI G1 may proceed. A skipped platform does not count as dual-backend support.

Approved commit: `docs(viewport): record measured presentation feasibility` (results only after actual execution).

#### N11 — deterministic-first strategy (`REQ-EXEC-01`)

Files: `crates/aero-agent/src/strategies.rs`, `crates/aero-agent/src/orchestrator.rs`, `crates/aero-agent/tests/procedures.rs`.

1. Test an ordinary supported form/navigation/extraction mechanic performs no extra inference calls beyond the planner's intent; failed built-in preconditions choose fresh semantic grounding, then optional visual/human route with no blind retry.
2. Run `cargo test -p aero-agent --test procedures strategy_uses_cheapest_reliable_path -- --exact` to red.
3. Implement explicit strategy outcomes with shared ResolvedEffect/Postcondition/authorization interface. No separate “fast path” may bypass policy, budgets, journal or verification. Unsupported vision/local model is a truthful request for help.
4. Re-run to green; record measured calls/time/verified result in N23 before claiming a cost improvement. No universal site-adapter framework is required for the first workflow.

Approved commit: `feat(execution): select reliable deterministic mechanics first`.

#### N12 — reviewed reusable procedures (`REQ-EXEC-02`)

Files: `crates/aero-agent/src/procedures.rs`, `crates/aero-agent/tests/procedures.rs`, `docs/architecture.md`.

1. After a measured repeated workflow exists, test a versioned built-in procedure's origin/route/structural preconditions, typed parameters and postconditions. Alter the page shape or route; it must stop/replan and never dispatch a saved @e handle.
2. Run `cargo test -p aero-agent --test procedures changed_page_invalidates_procedure -- --exact` to red.
3. Add a minimal reviewed procedure representation with provenance/effect requirements and no arbitrary script/secrets/grants. Do not build a generic workflow language, self-updating adapter market, or silent user-demonstration recorder.
4. Re-run to green, including `procedure_cannot_elevate_permissions`; compare independent task success/cost before enabling it by default.

Approved commit: `feat(procedures): reuse validated workflows with preconditions`.

#### N13 — typed application data-flow gateway (`REQ-DATA-01`)

Files: `crates/aero-agent/src/data_flow.rs`, `crates/aero-agent/src/planner/http.rs`, `crates/aero-agent/tests/data_flow.rs`, `crates/aero-agent/src/lib.rs`.

1. Create a recording fake recipient transport. Test that a private text/image/document payload without matching source/recipient/purpose/type grant never reaches it; unchanged approved public text does.
2. Run `cargo test -p aero-agent --test data_flow transfer_requires_exact_scoped_grant -- --exact` to red.
3. Implement TransferIntent validation and a gateway-owned transport dispatch capability. Provider adapters require that capability instead of constructing unrestricted task-content HTTP requests. Unknown data defaults private; provider credentials stay separate from page/model payloads.
4. Re-run suite to green; add recipient redirect/grant expiry/revision/size/encoded screenshot and no-content telemetry cases. Optional MCP/OCR/embedding/export adapters join the same boundary before being enabled.

Approved commit: `feat(privacy): mediate typed application data transfers`.

#### N14 — enforce honest processing modes (`REQ-DATA-02`)

Files: `crates/aero-agent/src/data_flow.rs`, `crates/aero-agent/src/config.rs`, `crates/aero-agent/tests/data_flow.rs`, `docs/security.md`.

1. Test `local_processing_never_falls_back_remote`: configured local provider unavailable causes actionable pause, with zero remote requests/embedding/OCR/tool calls. Sensitive mode denies remote transfers except exact trusted exceptions.
2. Run `cargo test -p aero-agent --test data_flow local_processing_never_falls_back_remote -- --exact` to red.
3. Implement mode resolution at the gateway and explicit supported-backend discovery. Approved-cloud processor and screenshot permissions are separate. Approved website browsing remains possible; do not label local processing “offline browsing.” Strict browser egress mode fails if real confinement backend is missing.
4. Re-run suite to green, including `page_cannot_declassify_its_content`, `optional_adapter_cannot_bypass_mode`, and `strict_egress_cannot_downgrade`.

Approved commit: `feat(privacy): enforce local and restricted task modes`.

#### N15 — browser permissions and explicit uploads (`REQ-DATA-03`)

Files: `crates/aero-agent/src/permissions.rs`, `crates/aero-cdp/src/runtime.rs`, `crates/aero-executor/src/dispatcher.rs`, `crates/aero-agent/tests/permissions.rs`, `crates/aero-agent/tests/scenarios.rs`, `crates/aero-agent/tests/fixtures/uploads.html`.

1. Test deny defaults and task/origin-bound grant matching for camera/mic/geolocation/clipboard-read/filesystem/notifications/upload. Model cannot supply a path or grant itself access. Run `cargo test -p aero-agent --test permissions` to red; implement resolver/default-denial and run green.
2. Add one explicit upload action/DTO only after the guard exists: trusted ArtifactID or picker-selected file token, size/type/destination/revision validation. Export new model/import/schema fields once; no general host-file API.
3. Run `AERO_REQUIRE_BROWSER=1 cargo test -p aero-agent --features browser-tests --test scenarios upload_requires_artifact_and_origin_grant -- --exact` to red. Implement isolated staged file selection and a fixture upload endpoint; verify received bytes/receipt and no other host file is available.
4. Green includes wrong origin/file size/cancel paths and reload recovery requiring re-selection instead of restoring file inputs. Distinguish direct trusted human clipboard/manual-native behavior from agent authority.

Approved commit: `feat(permissions): guard browser resources and upload artifacts`.

#### N16 — retention across derived data (`REQ-DATA-04`)

Files: `crates/aero-agent/src/tasks.rs`, `crates/aero-agent/src/evidence.rs`, `crates/aero-agent/src/artifacts.rs`, `crates/aero-agent/src/memory.rs`, `crates/aero-agent/tests/tasks.rs`.

1. Test task deletion with synthetic artifact/evidence/snapshot/derived-index/cache records, including interrupted deletion. Another task's records and files must remain.
2. Run `cargo test -p aero-agent --test tasks delete_task_purges_owned_derived_data -- --exact` to red.
3. Implement owner-validated directory-relative cleanup and idempotent tombstone/retry where DB+filesystem cannot share a transaction. Respect retention holds/explicit exports; disclose external-copy limits. No raw secret in deletion log.
4. Re-run green; include symlink/cross-task escape/retention-expiry cases, and ensure reports do not promise revocation of already-sent external data.

Approved commit: `feat(retention): delete task data and derived local records`.

#### N17 — evidence and freshness repository (`REQ-EVIDENCE-01`)

Files: `crates/aero-core/src/evidence.rs`, `crates/aero-agent/src/evidence.rs`, `crates/aero-agent/src/store/worker.rs`, `crates/aero-agent/tests/evidence.rs`.

1. Test structured field/claim references resolve to captured origin/URL/time/excerpt/document generation; unknown, cross-task, irrelevant or contradictory evidence cannot validate a claim. For structured facts compare actual extracted source values with report/CSV values; citation presence/count alone is not truth. Unsupported semantic judgments remain inconclusive or require allowed user attestation. Changing the source version marks prior capture stale, not rewritten history.
2. Run `cargo test -p aero-agent --test evidence claims_require_owned_captured_sources -- --exact` to red.
3. Add bounded capture from authorized current observations, sensitivity/retention metadata and optional bytes hash. Never reconstruct source evidence through an unapproved second authenticated fetch. Classify unsupported claims unknown; no model-invented URLs/passages.
4. Re-run green, with secret-redaction/capture-limit/stale/cross-task cases. Persist evidence references in the same outcome transaction; hashes do not prove real-world truth.

Approved commit: `feat(evidence): preserve scoped source provenance and freshness`.

#### N18 — verified artifact bundles (`REQ-ARTIFACT-01`)

Files: `crates/aero-core/src/artifact.rs`, `crates/aero-agent/src/artifacts.rs`, `crates/aero-agent/src/export.rs`, `crates/aero-agent/tests/artifacts.rs`, `crates/aero-cli/src/args.rs`, `crates/aero-cli/src/main.rs`.

1. Test `bundle_manifest_matches_actual_outputs`: supplier CSV/report plus manifest reference actual file bytes/schema/count/evidence IDs, not an invented path. Missing/truncated/unverified file fails finalization.
2. Run `cargo test -p aero-agent --test artifacts bundle_manifest_matches_actual_outputs -- --exact` to red.
3. Implement generated ArtifactIDs, bounded private atomic staging/finalization, readback/hash and source/criteria metadata; separate export grant. Add CLI artifact/evidence inspection with truthful freshness/limitations.
4. Re-run green, including path/symlink/overwrite/cross-task/missing-evidence and no export-without-grant cases. This connects to G07/G08 before first G1 task, not late packaging polish.

Approved commit: `feat(artifacts): deliver verified evidence-linked result bundles`.

#### N19 — recovery report and truthful reversibility (`REQ-RECOVERY-01`, `REQ-RECOVERY-02`)

Files: `crates/aero-agent/src/recovery_report.rs`, `crates/aero-agent/src/undo.rs`, `crates/aero-agent/src/recovery.rs`, `crates/aero-agent/tests/recovery_report.rs`, `crates/aero-cli/src/main.rs`.

1. Test a snapshot with restored history, expired login, missing field and uncertain submit. Report each separately with next safe decision; overall status cannot claim full restoration. Run `cargo test -p aero-agent --test recovery_report partial_restore_is_visible` to red, implement typed report, then green.
2. Add `irreversible_effect_has_no_undo`: classifier distinguishes reversible local/compensatable external/irreversible/unknown; only supported local changes get undo commands.
3. Run `cargo test -p aero-agent --test recovery_report irreversible_effect_has_no_undo -- --exact` to red. Implement explicit support list; external compensation is a newly authorized journaled action, never record deletion or silent retry.
4. Re-run suite green. CLI/GUI render same report/method labels; no secret disclosure and no universal Undo claim.

Approved commit: `feat(recovery): expose partial restoration and honest undo`.

#### N20 — task-first accessible workspace (`REQ-TASK-03`, `REQ-UX-02`)

Files: `crates/aero-shell/ui/src/TaskWorkspace.tsx`, `crates/aero-shell/ui/src/Artifacts.tsx`, `crates/aero-shell/ui/src/RecoveryReport.tsx`, `crates/aero-shell/ui/src/Approvals.tsx`, `crates/aero-shell/ui/src/App.tsx`, `crates/aero-shell/ui/src/TaskWorkspace.test.tsx`.

1. Write UI fixtures for running, awaiting-user, partial recovery, failed criterion and completed-with-bundle states. Assert objective/criteria/results/questions/artifacts/budgets visible; timeline secondary; no unverified green success.
2. Run `npm --prefix crates/aero-shell/ui test -- --run TaskWorkspace.test.tsx` to red.
3. Implement components against generated service DTOs, trusted material preview, accessible focus/labels/live status and evidence navigation; no extra frontend permission engine. Safe text rendering, event gap resync and listener cleanup remain mandatory.
4. Re-run green plus typecheck. Add keyboard-only approval denial/artifact inspection/recovery-help case; actual native screen-reader/browser viewport behavior still requires N10/H06 evidence.

Approved commit: `feat(workspace): center tasks verified results and user decisions`.

#### N21 — baseline manual browser essentials (`REQ-UX-01`, `REQ-UX-02`)

Files: `crates/aero-shell/ui/src/BrowserToolbar.tsx`, `crates/aero-shell/ui/src/Permissions.tsx`, `crates/aero-shell/ui/src/BrowserToolbar.test.tsx`, `crates/aero-shell/src-tauri/src/commands.rs`, `crates/aero-agent/tests/scenarios.rs`, `docs/viewport-feasibility.md`.

1. Test address/search/back/forward/reload/stop/tabs/find/artifact shelf and explicit task/profile/account indicator; ambiguous profile switch cannot carry task grants/cookies into a new trust domain. Run `npm --prefix crates/aero-shell/ui test -- --run BrowserToolbar.test.tsx` to red.
2. Implement thin controls over selected native/stream arrangement and service, not a second WebKit browsing session. Trusted copy/paste/upload/site-permission UI follows N15; persistent profiles opt-in, no automatic cookie/password imports.
3. Green UI tests plus `AERO_REQUIRE_BROWSER=1 cargo test -p aero-agent --features browser-tests --test scenarios manual_takeover_preserves_single_input_owner -- --exact`. Agent cannot dispatch until resume acknowledgement/new observation.
4. Execute native keyboard/IME/selection/find/file-dialog/a11y checklist on X11/Wayland, recording known unsupported stream behavior and native fallback. A canvas substitute is not marked native-accessible.

Approved commit: `feat(browser-ui): add usable manual browsing and profile clarity`.

#### N22 — early independent evaluation structure (`REQ-EVAL-01`)

Files: `evals/manifest.json`, `evals/cases/supplier-research.json`, `evals/cases/form-prepare.json`, `evals/cases/negative-safety.json`, `evals/README.md`, `docs/evaluation.md`, `crates/aero-agent/src/evaluation/mod.rs`, `crates/aero-agent/src/evaluation/manifest.rs`, `crates/aero-agent/src/evaluation/runner.rs`, `crates/aero-agent/src/evaluation/fixture.rs`, `crates/aero-agent/tests/evaluation.rs`, `crates/aero-agent/Cargo.toml`, `crates/aero-cli/Cargo.toml`.

1. Version case IDs/categories/contracts/budgets/fixture ground truth/checker IDs and suite membership. G1 mandatory suite is supplier research plus its safety negatives; full-product suite grows explicitly as supported categories ship, never via silently skipped mandatory cases.
2. Test `evaluation_rejects_missing_or_self_judged_case`: duplicate IDs, absent result, model-only success, missing independent checker, zero executed cases and unversioned configuration are errors.
3. Run `cargo test -p aero-agent --test evaluation evaluation_rejects_missing_or_self_judged_case -- --exact` to red; implement strict manifest/result/checker validation with fake controlled case outcomes, then green. Define fakes in this test before use; no counterfeit benchmark data in docs/reports.
4. Include browser/app/model/config/version, explicit `evaluation_kind` and ground-truth provenance. Add a nondefault `evaluation` feature forwarding CLI→agent; fixture runner/server/checkers live in `crates/aero-agent/src/evaluation/` and tests reuse them instead of CLI trying to import a tests-only helper. Manifest/result validation and aggregation remain dependency-light and available to default unit tests; only runner/server/ScriptedPlanner and their optional dependencies are gated by that feature. The agent browser-tests feature forwards evaluation so C07 test wrappers reuse the same fixture server instead of duplicating it. Scripted planner is permitted explicitly in evaluation and labelled `scripted_browser`; normal run has no fake fallback. Initial dev fixtures are not a hidden set. N23 connects actual workloads after the first slice.

Approved commit: `test(evaluation): establish versioned independent task workloads`.

#### N23 — measured quality and safety release gate (`REQ-EVAL-02`)

Files: `crates/aero-agent/tests/evaluation.rs`, `crates/aero-agent/tests/scenarios.rs`, `crates/aero-cli/src/args.rs`, `crates/aero-cli/src/main.rs`, `scripts/evaluate.sh`, `docs/evaluation.md`, `.github/workflows/ci.yml`.

1. Test aggregation from explicitly synthetic unit fixtures: distinguish passed/failed/incomplete/inconclusive, false success, unauthorized effects, secret/data-policy violations, repeated uncertain effects, human prompts, time/tokens/priced-or-unknown cost. Missing/zero denominators render null/not-applicable, never 100% success.
2. Run `cargo test -p aero-agent --test evaluation` to red; implement validated aggregation and immutable run-config records, then green. Unit fixtures are not product benchmark measurements.
3. Connect the actual G1 workload/browser truth and run `AERO_REQUIRE_BROWSER=1 cargo run -p aero-cli --features evaluation -- evaluate --manifest evals/manifest.json --suite g1 --planner scripted --output target/evaluation/g1.json`. Expected future result: all mandatory scripted-browser cases execute and pass, report resolves evidence/config and labels `evaluation_kind=scripted_browser`; otherwise nonzero with truthful failures. Actual-model evaluation uses explicitly configured `--planner configured`, provider/data grants and budgets, a separately labelled report, and no implied model-quality claim from the scripted suite. Do not manufacture a report if browser/model prerequisites are missing.
4. Add deterministic negative gates requiring zero unauthorized effects/false success/leaks/repeated uncertain effects in the required suite. Extend with forms/uploads/frames/handoff/recovery as shipped. Freeze performance thresholds after actual baseline and before tuning; report native UX measurements separately, do not invent latency/memory SLO results.

Approved commit: `test(quality): gate releases on verified task outcomes`.

#### N24 — holdout governance and measured expansion (`REQ-EVAL-03`)

Files: `docs/evaluation.md`, `evals/README.md`, `crates/aero-agent/tests/evaluation.rs`, `docs/releasing.md`.

1. Record an owner-curated held-out source unavailable to optimization agents, access/review process, version/manifest receipt and benchmark freeze rules. If no such set is supplied, status is `holdout_not_available`, not “held-out passed”; repository-visible dev fixtures cannot count as hidden.
2. Test `comparison_requires_matching_workloads_and_configuration`: incomparable task sets/configurations/cost-unknown runs cannot produce a blanket improvement claim. Run `cargo test -p aero-agent --test evaluation comparison_requires_matching_workloads_and_configuration -- --exact` to red, implement comparison guards, then green.
3. Capture real baseline on the supported first workflow. Before enabling procedure reuse, new model routing, semantic retrieval, visual inference, concurrency or training, compare independent success/safety/interventions/latency/cost/resources under fixed budgets. A faster wrong result is not improvement.
4. Obtain owner review of holdout outcomes and specific release thresholds; retain failures and limitations. This task authorizes no collection/upload of private browsing data, training, or competitive superiority claim without measurements.

Approved commit: `docs(evaluation): govern held-out review and measured optimization`.

## 9. Full-product acceptance scenarios

All automated browser scenarios are local and deterministic, use fresh private profiles and dynamic ports, and fail missing prerequisites. They do not touch real accounts/payment sites. Sentinels are synthetic test values, never production secrets.

### Canonical harness shape — `crates/aero-agent/tests/scenarios.rs`

The helper types are introduced in C07; use this exact test structure, not silent skipping:

```rust
#[cfg(feature = "browser-tests")]
mod support;

#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn cross_site_oopif_perception_and_input() {
    let mut fixture = support::Fixture::start_cross_site().await
        .expect("required native browser fixture failed to start");
    fixture.navigate_merchant().await.expect("merchant navigation");
    fixture.wait_for_distinct_gateway_session().await.expect("real OOPIF attachment");
    let observation = fixture.observe().await.expect("unified observation");
    let target = observation.unique_handle("textbox", "Fixture account reference")
        .expect("exactly one accessible labelled gateway field");
    fixture.approve_and_type(&observation, target, "fixture-reference", true).await
        .expect("policy-approved grounded input");
    assert_eq!(fixture.gateway_value().await.unwrap(), "fixture-reference");
    assert_eq!(fixture.gateway_input_event_count().await.unwrap(), 1);
    assert_eq!(fixture.decoy_value().await.unwrap(), "");
    fixture.shutdown().await.expect("server and browser teardown");
}
```

The helper must additionally clean up in Drop/cancellation paths; explicit shutdown alone is not enough when an assertion panics. Use an owned cleanup supervisor and test that supervisor, rather than attempting async work directly inside Drop.

### Required task-first acceptance scenarios

- `supplier_task_delivers_verified_bundle`: local approved supplier origins, fixed fixture facts, missing lead time marked unknown, exactly expected ordered CSV rows, report claims linked to captured owned evidence, finalized manifest readback, all required criteria passed; no account/submit/upload/purchase effects.
- `model_complete_cannot_bypass_criteria`: planner says Complete while required artifact/check fails; task remains incomplete and the rejected completion proposal is recorded separately; the application false-success metric remains zero only because the success gate blocked it.
- `research_grant_avoids_per_click_prompts`: useful bounded delegation permits routine research without repeated approvals; origin/account/effect crossing asks and dispatches nothing before approval.
- `preview_change_invalidates_commit`: material account/recipient/payload/amount change rejects old approval; no final submit occurs from preparation alone.
- `local_processing_never_falls_back_remote`: local provider failure plus remote configured fallback still sends zero task-content requests; UI says local processing unavailable, not offline website browsing.
- `task_budget_survives_new_run`: restart/provider change/workers cannot reset or oversubscribe attempts/tokens/active-time; unknown usage remains reserved.
- `partial_restore_is_visible`: recovery report separates restored history from expired login, missing forms and uncertain submit. No universal Undo or blanket success.
- `manual_takeover_preserves_single_input_owner`: acknowledged pause prevents agent/native-wrapper input races; resume observes fresh state. Native user actions are labelled manual, not falsely described as all policy-mediated.
- `delete_task_purges_owned_derived_data`: task/evidence/artifact/snapshot/derived-memory/cache deletion is isolated and retryable, with external export/recipient limitations disclosed.

These add to—not replace—the frame/form/vault/transport/crash tests below. Each has an independent oracle/fixture and must appear in the evaluation manifest or a clearly linked required integration suite.

### Required crash matrix

| Injection point | Required persisted state | Required restart behavior |
|---|---|---|
| Before any intent | Latest committed checkpoint or initial snapshot | Fresh observation; retain total budget. |
| After intent, before dispatch | Prepared action | `needs_review`; cannot prove no effect. |
| During dispatch / connection loss | Prepared/uncertain action | Stop; no automatic retry. |
| After fixture server records effect, before verification | Prepared/uncertain action, server counter=1 | Needs review; counter remains 1. |
| During DB post-action transaction | Entire old or entire new transaction, never mixed | Either needs review or resumes from new committed cursor. |
| After commit | Committed action/checkpoint/cursor | Resume next step, not repeated prior step. |
| Missing/deleted snapshot key | Metadata intact, ciphertext unreadable | Explain nonrestorable auth state; never plaintext fallback. |
| Expired auth / changed route / missing shadow host | Partial browser restoration report | Pause/replan with explicit user visibility, never invent restored state. |
| Third browser failure | Persisted recovery_count reaches configured cap | Fail/needs_review; no infinite restart loop. |

### Privacy sentinel test surfaces

Check decoded planner HTTP requests, MCP outputs, serialized audit/events, SQLite non-encrypted columns, log collector, screenshots/artifact lists, user-visible errors and telemetry labels. Deliberately include password-like fields, `autocomplete=one-time-code`, `cc-number` text controls, secret echoed into ordinary text, URL query tokens, and a cross-origin decoy. Encrypted checkpoint ciphertext may carry these values only as authenticated encrypted data; raw browser profile storage is separately documented as sensitive and protected by per-user filesystem access, not claimed to be magically ciphertext.

## 10. Requirements traceability and final validation

| Requirement from PDF | Planned implementation | Required proof |
|---|---|---|
| Async Rust modular engine | A03, B01–B03, C01–C07 | Workspace tests, dependency direction review. |
| Bubblewrap isolation + Chromium sandbox | C03–C04, I02 | Host canary inaccessible; renderer sandbox active; child cleanup. |
| CDP multiplexed pages/tabs/frames | C01–C06, D04–D06, G05 | Concurrent routing, OOPIF/same-process fixtures, detach/epoch tests. |
| AX compaction and multimodal SOM | D01, G13 | Bounded deterministic text; calibrated same-handle images. |
| Grounded trusted-browser input | D02–D03, D05–D07 | Exact event/effect assertions, Unicode, no stale clicks. CDP input is not physical hardware. |
| Planner and observe/plan/execute/verify | E03–E05 | Real HTTP mock contract plus scripted real-browser task. |
| MCP integration | G15 | Actual SDK client ↔ server tool call, clean stdio, shared policy. |
| Safety/HITL | E01–E02, H04–H05 | All action paths gated; acknowledged pause; denial means zero dispatch. |
| Persistent history/checkpoints | B04–B06, F05–F11 | Atomic journal and crash matrix; resume restores original budget. |
| Forms, React events, open shadow roots | F02–F04, D07 | Exact empty/false/select/group app-state restore. |
| Vault and TOTP | F01, F07, G01–G02 | RFC vectors, Secret Service integration, full-surface sentinel scan. |
| Cookie/tracker/overlay handling | G03–G04 | Exactly-one Fetch resolution; no observation mutation; consent reject default. |
| Downloads + CSV/Parquet | G06–G09 | Actual bytes/readback, data alignment, races/path tests. |
| Background pool | G10 | Context isolation, bounded concurrency, cancellation/disposal. |
| Episodic/vector memory | G11–G12 | Safe successful examples, deterministic retrieval tests, no runtime downloads. |
| Replay | G14 | Offline no side effects; live guarded locator resolution and divergence stop. |
| Metrics/Grafana/alerts | G16 | Listener/privacy tests, promtool simulations, dashboard query checks. |
| Tauri supervisory shell | H01–H06 | Actual controlled target displayed, exclusive manual input, X11/Wayland evidence. |
| AppImage/Flatpak/distribution | I03–I05, J01–J03 | Clean-machine artifacts; Flatpak not marked ready until its separate gate passes. |
| Kernel hardening / swarms / model training | J04–J06 | Separate feasibility/measurement and owner approval; not bundled into v1 claims. |

### Approved product requirement traceability

The authoritative register below is synchronized with `docs/specification.md` §13. Batch N defines exact files/test names/commands and the integrated schedule places it before its consumers. Every row requires real execution evidence before status changes from unimplemented; no recommendation is satisfied only by an aspirational paragraph.

| ID | Requirement | Acceptance obligation | Plan additions |
|---|---|---|---|
| REQ-TASK-01 | Durable TaskContract separate from runs/browser sessions | Strict roundtrip, user-only revisions, stable identity across provider/UI changes | N01, N02 |
| REQ-TASK-02 | Typed completion criteria and task-global budgets | Required criteria enforced, no reset/oversubscription on retry/resume | N02, N03 |
| REQ-TASK-03 | Task workspace instead of conversation-centric UI | Criteria/results/questions/artifacts visible; audit secondary | N20 |
| REQ-VERIFY-01 | Independent operation verification | Dispatch success/model completion never imply passed postcondition | N04, N05 |
| REQ-VERIFY-02 | Verified task completion gate | Missing/failed/inconclusive criteria block success; user attestations labelled | N05 |
| REQ-GRANT-01 | Task-scoped delegation | Routine authorized navigation/extraction do not prompt per click; boundary crossing does | N06 |
| REQ-GRANT-02 | Prepare/commit and material preview binding | Changed account/destination/payload/amount invalidates approval | N07 |
| REQ-GRANT-03 | Trusted-human-only grants | MCP/page/model/procedure cannot issue authority | N06, N07 |
| REQ-SERVICE-01 | Shared local control-plane interface | CLI/UI/MCP use one controller; role-specific methods | N08 |
| REQ-SERVICE-02 | Explicit close/background lifecycle | Pause default; background opt-in only after process isolation proven | N09 |
| REQ-VIEW-01 | Early actual-viewport feasibility | Recorded native interaction/a11y matrix before frontend commitment | N10 |
| REQ-EXEC-01 | Least-expensive reliable strategy | Deterministic→semantic→visual/human fallback, same policy/verification | N11 |
| REQ-EXEC-02 | Preconditions for reusable procedures | Changed page invalidates procedure; no stale handles/permission reuse | N12 |
| REQ-DATA-01 | Typed processor/tool/export gateway | Destination/purpose/labels/grants checked; no adapter bypass | N13 |
| REQ-DATA-02 | Honest processing modes | Local/sensitive block remote fallback; permitted browsing not mislabelled offline | N14 |
| REQ-DATA-03 | Explicit browser permissions and uploads | Deny defaults; task/origin/ArtifactID-scoped upload and receipt checks | N15 |
| REQ-DATA-04 | Full retention/deletion scope | Evidence/artifacts/snapshots/derived memory purged, external limits explained | N16 |
| REQ-EVIDENCE-01 | Captured provenance and freshness | Claims/fields resolve to scoped evidence; stale/unknown marked | N17 |
| REQ-ARTIFACT-01 | Verified result bundle | Manifest and files read back, sources/hashes/criteria resolve; no unsafe paths | N18 |
| REQ-RECOVERY-01 | User-visible recovery report | Partial restore/auth/uncertain effect surfaced with safe next step | N19 |
| REQ-RECOVERY-02 | Truthful undo/compensation | No undo on irreversible/unknown effects; compensation reauthorized | N19 |
| REQ-UX-01 | Manual browser essentials/profile clarity | Native/navigation/find/copy/permissions/context workflows tested | N21 |
| REQ-UX-02 | Accessible exclusive human handoff | Keyboard/a11y matrix and no input race; fresh observe on resume | N10, N20, N21 |
| REQ-EVAL-01 | Early independent evaluation | Versioned independent ground truth; report rejects missing/stub cases | N22 |
| REQ-EVAL-02 | Safety and quality release gates | Safety negatives, false-success, costs/interventions and mandatory workflows reported | N23 |
| REQ-EVAL-03 | Held-out and measured optimization | Owner-curated holdout, baseline comparisons, no unmeasured superiority claims | N24 |

### Exact final verification commands

These are expected future checks, not execution results from this documentation update. Use the chosen shipped feature/backend matrix, not a requirement to provision every optional engine before a useful release.

Core and G1 required suite after implementation:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo test -p aero-core completion_requires_checks_artifacts_and_resolved_effects --locked
cargo test -p aero-agent --test tasks --locked
cargo test -p aero-agent --test budget --locked
cargo test -p aero-agent --test verification --locked
cargo test -p aero-agent --test capabilities --locked
cargo test -p aero-agent --test data_flow --locked
cargo test -p aero-agent --test evidence --locked
cargo test -p aero-agent --test artifacts --locked
cargo test -p aero-agent --test evaluation --locked
AERO_REQUIRE_BROWSER=1 bash scripts/test-browser.sh
AERO_REQUIRE_BROWSER=1 cargo run -p aero-cli --features evaluation --locked -- evaluate --manifest evals/manifest.json --suite g1 --planner scripted --output target/evaluation/g1.json
```

Expected: named cases execute, required workflow outputs match actual independent fixture truth, evidence/artifact links/readbacks resolve, no required skip, safety negatives/false-success/privacy/duplicate-effect checks pass. The report explicitly says scripted-browser integration, not model benchmark. An actual configured-model run is a separate declared evaluation with real provider/data grants/pricing-or-unknown cost. Missing provider/browser/holdout is a blocker or unavailable status, never synthesized report data.

For a selected desktop release after accepted N10 native presentation:

```bash
npm --prefix crates/aero-shell/ui ci
npm --prefix crates/aero-shell/ui run typecheck
npm --prefix crates/aero-shell/ui test -- --run
npm --prefix crates/aero-shell/ui run build
cargo build -p aero-cli -p aero-shell --release --locked
bash scripts/check-artifacts.sh
```

Require actual X11/Wayland interaction/a11y/handoff evidence for claimed support, including keyboard/IME/selection/dialogs and explicit limitations. Default host close pauses; optional background/service behavior needs its own N09 proof.

Only if shipped, execute corresponding optional gates:

```bash
cargo test -p aero-agent --features vault,parquet,mcp --locked
cargo test -p aero-perception --features som --locked
cargo test -p aero-agent --features semantic-memory --test memory --locked
promtool check rules packaging/monitoring/alerts.yml
promtool test rules packaging/monitoring/alerts.test.yml
```

Feature combinations must actually be supported; do not assume all-features is valid for mutually exclusive native backends. Semantic tests require explicit model provisioning; absence means that feature is unverified/not shipped. Packaging checks require running actual artifacts on clean supported machines, not merely finding a binary.

The release evidence includes mandatory workload manifest/case coverage, independently verified success versus incomplete/false success, interventions/approvals, uncertainty/recovery correctness, data-policy violations, actual time/tokens/cost (or unknown), resource measurements and native interaction matrix. Owner-curated held-out review is separate from repository-visible development fixtures. Freeze mandatory cases and release thresholds before tuning; each required workflow must pass regardless of aggregate score. No performance/superiority figure exists until measured.

## 11. Separately gated extensions

These remain part of the project roadmap, but cannot be honestly reduced to copy-paste production code before platform/protocol/ownership decisions are tested. Each extension begins with the concrete read-only/spike acceptance below and receives its own detailed implementation plan after its gate passes. Do not conflate these research gates with G0–G5 completion.

| Task | Exact files | Bite-sized initial tasks and verification |
|---|---|---|
| J01 — Flatpak feasibility | `packaging/flatpak/com.aerobrowser.App.yml`, `docs/adr/0004-flatpak-sandbox.md`, `docs/releasing.md` | Select supported runtime/SDK from actual Flathub metadata; reproduce CLI launch under Flatpak; test whether the distribution permits required Chromium namespaces and nested bwrap. Try a documented Flatpak-native sandbox backend, not `--no-sandbox`. Record pass/fail and the exact process/mount boundary. `flatpak-builder --force-clean .flatpak-build packaging/flatpak/com.aerobrowser.App.yml` is expected to fail until real sources/checksums/offline Cargo/npm inputs exist; no placeholder hash. |
| J02 — Flatpak implementation after J01 passes | Same manifest, `crates/aero-cdp/src/launcher.rs`, sandbox tests | Implement explicit `SandboxBackend::{Bubblewrap,Flatpak}` selected by runtime, proving equivalent declared boundary or documenting weaker one; no fictitious `--socket=ipc`; use portals for exports where possible, exact Secret Service permissions only for trusted host. Vendor immutable offline Rust/npm/browser sources. Run sandbox, vault, browser and GUI smoke inside installed sandbox. |
| J03 — Flathub submission | `docs/releasing.md`, packaging metadata/desktop/appstream files | Validate manifests/AppStream/icons/licenses, pass Flathub review requirements, build/install bundle on clean native machines. Publishing/signing/application ID ownership requires explicit owner approval; until then status is `not_published`. |
| J04 — Landlock/seccomp defense in depth | `docs/adr/0005-kernel-hardening.md`, future `crates/aero-cdp/src/hardening.rs` | First measure existing bwrap+Chromium sandbox privileges and kernel support; add a failing test for one specific denied host filesystem operation, then minimal policy; verify Chromium's own sandbox and downloads still work. Version policies by capability, not permissive catch-all fallback. Do not claim protection from all browser zero-days. |
| J05 — Remote workers | `docs/adr/0006-remote-workers.md`, future `crates/aero-worker/` | First document a measured local capacity bottleneck and authenticated remote threat model. Define cancellation, lease heartbeat, artifact transfer, authorization and secret locality before transport. UDS-first local service if needed; gRPC/TLS remote scope is a separate approved project. |
| J06 — Action-model fine-tuning | `docs/adr/0007-training-data.md`, future `tools/training/` | First create a versioned sanitized export of *approved* successful trajectories with provenance and rights; sentinel privacy test must fail on any secret. Define held-out local-browser benchmark, compare compact model vs baseline on success/safety/latency/cost, then decide whether training earns its maintenance cost. No unapproved data upload or training. |

## 12. Risks, tradeoffs, and owner decisions

### Open decisions before dependent gates

1. **Viewport:** N10 records actual stream/native-companion interaction, accessibility and display-backend evidence early. Accept a presentation with explicit limitations before H01; do not assume screencast is an excellent native browser. If neither candidate fits, commission separate embedding feasibility instead of quietly substituting WebKit or maintaining a Chromium fork.
2. **Inference:** configure actual local/cloud provider/model and pricing before real-model smoke/evaluation. Local-only failure cannot fall back remote. Scripted fixture results are explicitly integration results, not model comparisons.
3. **Data/retention:** baseline modes and opt-in encrypted snapshots are approved product direction. Confirm task-specific exceptions and stricter intranet/egress/retention obligations before using sensitive accounts. Strong egress needs a real backend; local-only processing is not offline browsing.
4. **Background/service:** start with one in-process control interface and pause-on-close. N09 process extraction requires a need/owner decision and actual disconnect/auth/restart tests before background continuation is exposed.
5. **Distribution/license:** confirm reverse-DNS app ID/domain, project/font/browser rights, browser maintenance/redistribution responsibility and signing identity before publication. `com.aerobrowser.App` is provisional, not proven owned.
6. **Quality:** owner curates held-out cases and freezes release workload/SLO thresholds after baseline. Do not invent holdout availability or competitive superiority. A required workflow failure blocks release even if aggregate metrics look good.

### Principal tradeoffs

- **Task scope versus browser clone:** task workspace plus polished navigation/tab/find/permissions/profile/handoff essentials; avoid cloning every Chrome extension/password-manager feature before useful delegated work exists.
- **Useful delegation versus blanket approvals:** one bounded trusted task envelope reduces friction; actual unknown/high-impact effects still require review. Page text/model rationale cannot classify its own effect harmless.
- **Independent verification versus self-reported success:** typed postconditions/artifact readbacks/source evidence cost engineering time but prevent false-green outcomes. Some external effects remain inconclusive and require human confirmation.
- **Application data gateway versus website egress:** enforce provider/tool/processor/export boundaries centrally; do not suggest it prevents an already authorized hostile website from leaking inputs through arbitrary encodings.
- **Service boundary versus premature daemon:** model roles/state/lifecycle now, separate process only for justified background/UI isolation; private UDS is not a defense against fully compromised same-user code.
- **Deterministic execution versus brittle procedures:** use mechanical primitives first, reviewed adapters only with checked preconditions, semantic/visual/human fallback without policy bypass. Measure before introducing generic procedure DSLs, vectors or model routing.
- **Direct CDP versus automation library:** narrow tested protocol surface retains lifecycle control; avoid reimplementing every domain or claiming hardware-native input.
- **SQLite-first versus immediate semantic stack:** retain reliable task journal/FTS/optional successful-summary retrieval; only enable heavy embeddings/Arrow/LanceDB when measured usefulness earns provisioning/binary/startup cost.
- **Honest recovery versus false exactness:** restore application-owned task intent/history reliably; web state is best-effort. Unknown submit needs reconciliation/review, not automatically replayed purchase. Compensation is a new action, never universal Undo.
- **Security versus portability:** no --no-sandbox, web-security disabling, hidden permission widening, plaintext fallback, or silent strict-mode downgrade to make tests pass.

## 13. Documentation verification and implementation handoff

The original review covered all 96 source pages and retained its 31 source-referenced corrections. This revision creates `docs/specification.md` and updates this plan to integrate the nine approved product recommendations, rather than appending an unrelated wishlist. Requirement IDs, Batch N, architecture, Task/run schema, completion semantics, ordering, first workflow, data-flow modes, early native viewport/evaluation gates, optional features and quality acceptance must remain synchronized.

Historical revision 2.0 changed only the specification and the then-private plan; revision 2.1 publishes this plan and adds the handoff, contributor guide, and bug/fix tracker, with authority links and a redirect from the previous plan path. No application package is completed by that publication. No application code, dependency installation, Git initialization/commit, project build/test, account operation or external publication is performed. Actual documentation checks passed: all nine approved recommendations mapped, 26 requirement IDs synchronized with the specification, 24 unique new N work packages, all 31 original corrections retained, balanced Markdown fences, relative file links and whitespace, embedded JSON parsing, and JavaScript syntax through stdin. The SQL example parsed and its foreign keys, immutable contract/grant payloads, transaction rollback and deletion cascades were exercised against synthetic in-memory records. These checks do not verify a future Rust store, browser or product implementation; Rust snippets and project tests were not compiled/run.

Begin with the integrated dependency schedule in §8, not alphabetic batches. Prioritize Task/grant/budget/data-flow/verification/evaluation foundations, then useful supplier artifacts and early viewport feasibility, then recovery/manual experience and measured breadth. Independent pure-model/transport work may be parallelized only after contracts/file ownership are agreed; lifecycle/controller/recovery edits need one owner. Review real focused results and exact artifacts before an approved commit. No delegated agent's success report substitutes for actual verification.

### Primary reference links

These technical API lookup references were checked during the original specification review; this documentation revision does not claim a fresh external API/version audit. Recheck supported SDK/protocol/browser versions before implementation, and do not copy third-party samples as tested project code.

- MCP sampling direction and declared capability: https://modelcontextprotocol.io/specification/2025-06-18/client/sampling
- Current tool-server model and safety semantics (select/lock actual SDK-supported protocol at implementation time): https://modelcontextprotocol.io/specification/latest/server/tools
- Linux Tauri webview engine: https://v2.tauri.app/reference/webview-versions/
- Tauri native prerequisites: https://v2.tauri.app/start/prerequisites/
- Reviewed keyring 3.x backend features (`linux-native` is keyutils, Secret Service is separate): https://docs.rs/keyring/3.6.3/keyring/
- DOM methods including `getFrameOwner`, `describeNode`, `getBoxModel`, `resolveNode`: https://chromedevtools.github.io/devtools-protocol/tot/DOM/
- Pre-document scripts and screencast lifecycle: https://chromedevtools.github.io/devtools-protocol/tot/Page/
- Actual CDP schema, to be pinned to the tested Chromium revision rather than treating `tot` as stable: https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/json/browser_protocol.json
- Bubblewrap security policy and limitations: https://raw.githubusercontent.com/containers/bubblewrap/main/README.md


### Revision 2.1 documentation checks

The live plan retains all 100 work packages (70 A–I, 24 N, six separately gated J), 31 source corrections, and 26 stable requirements. The initial execution ledger records every package once, with no fabricated completion/commit evidence. Verification for this document set checks relative links, requirement-table equality, package/ledger coverage, tracker references, Markdown structure, and embedded JSON/JavaScript/SQL examples. Actual results and limitations are recorded in the handoff; expected future Cargo/browser commands above are not run results.
