# Architecture: one trusted task engine, separate browser mechanics

## Authority and scope

This is the A01 ownership map, not a second implementation plan or evidence of working features. The [specification](specification.md#5-architecture-and-ownership) owns product contracts; the [plan](implementation-plan.md#2-architecture--proposed-approach) owns package order, exact paths/tests and execution states. The [requirements matrix](requirements.md) preserves all product and correction obligations. Consult the [handoff](handoff.md) for current verified state; historical setup statements are not a live environment inventory.

First deliverable: approved local supplier fixtures → comparable ordered CSV, sourced report and read-back manifest, under bounded navigation/extraction grants. No real account creation, form submission, private-data disclosure or purchase is needed to prove that slice. Subsequent form preparation stops before submission. Broader features remain gated, not implicitly implemented by this architecture.

## Dependency and ownership boundaries

The initial workspace now has six scaffold members; their runtime/domain behavior below remains planned. Desktop is added only after the viewport decision. Arrows below point from consumer to dependency, not process containment.

```text
aero-cli ───────────────────────────┐
aero-shell/src-tauri (later) ───────┤
restricted MCP adapter (optional) ──┤
                                   v
                              aero-agent
                           /              \
                 aero-perception     aero-executor
                           \              /
                                aero-cdp
                                   |
                                aero-core
```

Upper layers may also consume pure `aero-core` DTOs directly. Perception must not depend on executor. The MCP adapter is an agent module, not an extra mandatory crate/service. See the [mandatory dependency direction](implementation-plan.md#dependency-direction--mandatory).

| Owner | Responsibility | Must not own or bypass |
|---|---|---|
| `aero-core` | Strict versioned Task/Run/action/criterion/evidence/artifact models, typed IDs, validation, public events | No workspace, process, provider, database or GUI dependency |
| `aero-cdp` | Owned Chromium process/transport, targets/frames/contexts, geometry, storage/network/download managers, optional worker supervisor | No planner, business authorization, keyring or UI state; no global connection |
| `aero-perception` | Bounded AX/DOM observations, shared locators, source extraction, classification/redaction, optional visual marks | Observation does not click, dismiss or otherwise mutate pages |
| `aero-executor` | Fresh grounded deterministic input, focus/hit testing and protocol mechanics | Requires controller authorization; cannot mint grants or infer authority from page/model text |
| `aero-agent` | Task ownership, service/controller, policy/capabilities/previews, budgets, transfer gateway, strategies/planner, read-only verification, evidence/artifacts, journal/recovery | Separate modules, not one all-purpose orchestrator; planner cannot directly invoke executor/vault, write passed results, or use unrestricted task-content HTTP |
| `aero-cli`, later desktop | Thin trusted-user adapters over the same service, with role-scoped commands and public events | No second task engine, frontend permission engine, or untrusted role string |

`BrowserRuntime` is the single owner of the browser child, CDP transport/listeners and all connection-bound managers. `SessionController` serializes a run and exclusive browser input ownership. A durable Task owns immutable contract revisions, runs, cumulative counters, grants, criteria and evidence/artifacts; browser handles never become durable task identity.

## Control, effects and independent proof

```text
trusted user → immutable TaskContract + scoped grants
                       |
              SessionService/controller
                       |
  fresh observation → strategy/planner proposal
                       |
 resolved effect + declared postcondition + policy/data checks
                       |
  writer: reserve task budget + unique step + durable prepared intent
                       |
       revalidate material approval/target → executor → Chromium
                       |
        independent read-only verifier + evidence/file readback
                       |
 writer: outcome/checkpoint/metadata/status transaction → completion gate
```

A `Complete` proposal invokes the required-criteria/artifact gate; it does not set success. Dispatch acknowledgement, a citation count, model confidence or a second model's opinion cannot independently pass a postcondition. Preserve not-checked, failed, inconclusive and passed outcomes. Allowed human attestations are attributed `user_confirmed`, not mislabelled mechanical verification. No task completes with an unresolved external effect.

Capabilities bind task/revision, origins/routes, account/profile reference, effect classes, destinations, quotas and expiry. Routine research inside a trusted envelope need not prompt per click. Unknown effects, autosave, boundary crossings and high-impact commit require review. One-use approvals bind canonical material destination/account/payload/recipient/amount and relevant target freshness, not unrelated DOM animation. All keyboard, worker, replay, strategy and tool paths share the gate. Trusted profile selection is not a model-supplied host path.

## Service and lifecycle

N08 introduces an in-process `SessionService` facade over one controller. Host-constructed caller capabilities distinguish trusted CLI/desktop from model-facing adapters; every task/run lookup checks ownership. The [planner/MCP ADR](adr/0002-planner-and-mcp.md) records inference and tool-role boundaries.

N09's **mandatory baseline** is pause/flush/reconcile on close: stop new dispatch, interrupt planning/read-only waits, finish bounded input or preserve its uncertainty, flush journal, and release/reap ownership. In-process host crash may end execution; it is not background survival. F08 phase cancellation/deadlines and F10 prepared-intent fail-closed behavior are early guards, not postponed until full recovery.

Only separately justified/approved extraction adds a per-user private Unix-domain service with owner/peer checks, versioned bounded messages, role-specific methods and event-sequence reconnect. No default TCP listener. Background continuation requires a per-task trusted grant, persistent status and reconnect/cancel controls. A private socket does not defeat a compromised same-user account.

## Data, browser and presentation trust boundaries

- Page/provider/tool/procedure outputs are untrusted classified data, never authority. Schemas and bounded inputs prevent unsupported commands; they do not prove universal prompt-injection immunity.
- Application-managed provider/tool/OCR/embedding/telemetry/memory/export transfers require a typed `TransferIntent` and gateway-owned authorized transport capability. Local-only failure pauses; approved-cloud grants name recipients and data types; sensitive mode denies by default. Unknown data is private. Provider credentials are separate from browser authentication and contract content.
- Approved website traffic is separate from processor transfers. Local processing is not offline browsing. URL checks/redaction do not confine an already-authorized hostile site's egress. Strict network promises require a tested confinement backend or refusal, never downgrade.
- Chromium keeps its own renderer sandbox inside a tested Bubblewrap policy. Private per-run roots, explicit mounts/env and owned cleanup exclude host home, keyring/provider credentials and unrestricted display access. Initial host-network and X11 limitations remain disclosed. Never disable a sandbox to make a test pass.
- Browser device/clipboard/filesystem/upload permissions default deny from startup. Narrow task/origin grants and trusted ArtifactIDs/picker tokens are required; no model host-file API. File inputs are not snapshot-restored.
- Tauri hosts only trusted local UI, not controlled websites. Actual controlled Chromium target presentation is a pending measured choice, detailed in the [UI/engine ADR](adr/0001-ui-engine-boundary.md). A second WebKit navigation or screenshot canvas cannot stand in for native interaction/accessibility evidence.

## Durable state, recovery and result storage

SQLite and one bounded blocking writer are authoritative for tasks/contracts, counters/reservations, runs, intents/outcomes, grants/approvals, evidence/artifact metadata and checkpoints. The canonical [store contract](implementation-plan.md#75-initial-schema--cratesaero-agentsrcstoremigrations001_initialsql) owns SQL and step allocation; do not duplicate it here. Sensitive snapshots require authenticated encryption with an available external key and no plaintext fallback. Live browser-managed profiles remain sensitive even when application snapshots are encrypted.

Persist intent before dispatch; outcome/checkpoint/cursor and owned metadata commit together afterward. Remote effects cannot share a SQLite transaction. Unresolved prepared/dispatched intent becomes `needs_review`, with no automatic repetition. Rebuild the entire runtime, invalidate transient epochs and revalidate immutable revisions/grants and task-global counters. Restoration is partial and reported, not JS-heap replay or universal Undo. Details and exact proof links are in the [checkpoint ADR](adr/0003-checkpoint-semantics.md).

Evidence links bounded relevant captures to task/run/operation, effective origin/URL, time, document generation, sensitivity and limitations. Hashes identify bytes, not truth or perpetual freshness. Artifacts use private generated IDs, bounded exclusive staging, atomic finalization and actual schema/count/hash readback. Export is a separate permission. Retention/deletion covers all owned snapshots/evidence/artifacts/derived memory/caches, cannot delete another task, and cannot recall copies already sent externally.

## Implementation and evidence gates

Follow the plan's [integrated schedule](implementation-plan.md#integrated-dependency-schedule--do-not-execute-alphabetically): documentation/bootstrap; strict Task/grant/budget/data/verification/evaluation foundations; owned browser and safe input; supplier bundle and early native viewport probe; resilient recovery and local breadth; accepted desktop; release proof. CLI may advance while native presentation access is blocked; H01 may not.

Reviewed deterministic adapters precede optional procedures, visual grounding, Parquet, retrieval and bounded workers. Every strategy uses the same authorization, cumulative reservations, journal and verification; fallback is no safety bypass. Enable optional features only after their real feature-specific tests/provisioning and measured usefulness pass.

Versioned independent fixture truth comes before model optimization. Required workloads fail missing/stub cases, zero executed cases or unavailable prerequisites. Scripted-browser integration, configured actual-model evaluation, native/manual UX evidence and owner-curated holdout review are distinct. Freeze mandatory cases/thresholds before tuning; no aggregate score can hide a failed required workflow. There are no benchmark, native-support, packaging or future code/test execution claims in this document.
