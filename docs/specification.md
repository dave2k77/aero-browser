# AeroBrowser — task-first product and technical specification

Revision: 2.0 — approved product direction incorporated 2026-10-22.
Status: specification for a greenfield implementation; no application features are implemented or verified by this document.

## 1. Authority and purpose

This is the authoritative, editable specification for AeroBrowser. The original `aero-browser-specification-chat.pdf` remains historical source material, not a record of working code. The [implementation plan](implementation-plan.md) defines implementation order, tests, and the live execution ledger; where an older PDF example conflicts with this specification, this specification controls. Change requirements here and their corresponding plan/test references together.

For the current verified checkpoint and next batch, read the [handoff](handoff.md). Follow the [contribution rules](../CONTRIBUTING.md) for TDD, review, and approval-gated commits, and use the [bug/fix tracker](bug-fix-tracking.md) for defects, environment blockers, and pending owner decisions. These documents do not imply application implementation or authorization for setup, commits, accounts, or publication.

The initial specification review covered all 96 PDF pages. Its 31 correctness corrections remain in implementation-plan §3, including MCP direction, actual UI/browser engine separation, JSON-safe forms, target/frame identity, conservative crash recovery, and secret handling. This revision adds the subsequently approved product recommendations; it does not withdraw those corrections or reinterpret illustrative code as tested implementation.

**Product promise:** A Linux-native browser for delegated work, with verifiable results, explicit permissions, and seamless human control.

**Goal:** Complete useful browser tasks correctly, show the evidence and artifacts, and keep the user in control without making them supervise every mechanical action.

MUST means a release requirement for the specified milestone. SHOULD indicates a preferred approach that needs a documented reason to change. “Best in class” is an aspiration evaluated through task quality and user experience, not a claim made before comparative measurements exist.

## 2. Intended users, scope, and first workflow

Initial users are individual Linux professionals conducting research, collecting structured information, and preparing browser-based work. AeroBrowser is a focused work browser, not initially a complete Chrome replacement, a remote-agent platform, or a replacement rendering engine.

Initial release target: Ubuntu 24.04 x86_64, with native X11 and Wayland validation. WSL is useful for development but does not prove either native desktop experience. The browser engine is a maintained Chromium build with its own renderer sandbox, supervised inside a tested Bubblewrap policy. Do not fork Chromium or assume Tauri/WebKitGTK embeds Chromium.

**First excellent workflow:** Given approved public supplier pages and explicit requirements, collect comparable supplier information, produce an ordered CSV and a concise sourced comparison, identify unknowns/exclusions, and allow the user to review evidence. The task may navigate approved origins and stage public documents; it may not create accounts, submit forms, disclose private data, or place orders. Use deterministic local supplier fixtures first; a real public-web smoke requires separate approval and is not a reproducible benchmark result.

A subsequent form-preparation workflow fills a synthetic multi-step application, produces a submission preview, survives interruption, and stops before final submission. Real account/payment operations are not needed to prove this architecture.

### Release scope

- **Foundation:** task contracts, scoped delegation, data-flow policy, typed outcomes/evidence, deterministic mechanics, browser lifecycle, initial evaluation harness, and early viewport feasibility.
- **First vertical slice:** verified supplier workflow, evidence-linked CSV/report, task workspace, pause/takeover, uncertainty handling, and durable history.
- **Resilient local product:** forms/frames/open shadow roots, best-effort encrypted recovery, guarded vault/TOTP, downloads/uploads, user-facing recovery reports, browser essentials, and optional tool integration.
- **Measured enhancements:** optional visual grounding, Parquet, bounded worker pool, procedure reuse, and opt-in retrieval. These enter a shipped feature set only after their acceptance tests and usefulness/cost evaluation pass.
- **Separate feasibility tracks:** native Chromium embedding, Flatpak compatibility, stronger network egress isolation, background service process, custom kernel hardening, distributed workers, and training. They are not prerequisites for declaring the first workflow useful.

## 3. Product principles

1. Task state belongs to the application, not an LLM conversation, UI process, or inference vendor.
2. Proposal, permission, execution, observation, and verification are different events.
3. A dispatched action is not a verified effect. A model's `Complete` response is a completion proposal, not task success.
4. Delegate a bounded envelope once, rather than requesting approval for every ordinary mechanical step. Ambiguity or a boundary crossing triggers review.
5. Human approval can be requested by an agent but never issued by that agent, a page, a retrieved procedure, or an MCP tool caller.
6. Mechanically execute known operations; use semantic grounding before visual inference; request human help when reliable targeting or verification is unavailable.
7. Preserve uncertainty. Neither a local transaction nor a screenshot proves exactly-once external effects or universal website success.
8. Data movement is an explicit permission, not a side effect hidden behind a provider or tool setting.
9. Prefer an excellent small workflow over many impressive but unverified subsystems.
10. Treat manual browsing, accessibility, and safe handoff as product requirements, not demonstration polish.

## 4. First-class domain model

### Task, run, and browser session

A **Task** is the durable unit of user intent. Its immutable, versioned **TaskContract** contains:

- Opaque task ID, contract revision, objective, user-visible title, and typed completion criteria.
- Expected artifacts: required format, schema/fields, row/content constraints, and provenance requirements.
- Allowed origins/routes, profile/account references, permitted effect classes, and forbidden operations.
- Data-flow mode, approved processor destinations, source classifications, retention/export settings, and screenshot permissions.
- Cumulative action, elapsed-execution, token, and optional priced-inference budgets; spending cap defaults to zero for purchases.
- Dependencies, explicit stop conditions, unresolved questions, and the selected close/background behavior.

A **Run** is an execution attempt against a particular contract revision; it owns browser sessions, a controller, and an append-only effect journal. Multiple runs may belong to one task, but a restart/resume MUST NOT reset the task's budgets or overwrite earlier uncertainty. A contract revision is created only through the trusted user channel; the planner may propose changes that remain pending. Changing materially relevant scope revokes affected approvals and requires revalidation.

A **BrowserSession** identifies a runtime/profile/tab set. Browser target, frame, execution-context, and element handles are transient and scoped to runtime/document/observation epochs. They are never durable task identity or sufficient replay locators.

Existing implementation-plan `sessions` rows represent runs, not the entire Task. Add a `tasks` owner record and immutable contract revisions before implementing the store; this is greenfield schema design, not a reason to add a fake migration for an unshipped database.

### State and completion

Task states: `draft`, `ready`, `running`, `awaiting_user`, `paused`, `needs_review`, `completed`, `failed`, `cancelled`. `ready` requires a valid contract, executable plan, and required grants. Run states may omit draft/ready. Only one active input owner may control a browser session.

Each completion criterion has `not_checked`, `passed`, `failed`, or `inconclusive` status, plus evidence references and a verification method. Subjective criteria may require an explicit user attestation; that is recorded as `user_confirmed`, not independent mechanical proof. A task may enter `completed` only when every required criterion and artifact requirement passes or receives its explicitly permitted user attestation, no external effect remains unresolved, and budget/policy records are consistent. Model-reported completion without those conditions remains unverified and cannot render a green success state.

## 5. Architecture and ownership

Keep one Rust workspace, pure shared models, an owned CDP runtime, separate perception/input libraries, and an application library. Define a local session-service interface immediately, but do not add a daemon, network service, or extra crate merely to draw a process diagram.

```text
Trusted CLI / trusted desktop / restricted MCP adapter
                         |
          Local application / session-service interface
          task contracts and budgets | policy and approvals
          planner | independent verification | evidence/artifacts
          journal | recovery | human-input ownership
                         |
                Browser runtime supervisor
                         |
             Sandboxed Chromium runtime(s)

Untrusted page/provider/tool results enter as data, never as authority.
```

- `aero-core`: versioned Task/Run contracts, IDs, criteria/evidence/artifact DTOs, action models, validation, and typed public events. No database, provider, GUI, or process dependency.
- `aero-cdp`: process/transport ownership, targets/frames/geometry, lifecycle, input-supporting protocol methods, browser permission configuration, downloads, and bounded workers. No planner or business policy.
- `aero-perception`: bounded AX/DOM observations, shared locator strategy, source capture, extraction, privacy classification, optional visual marks. Observation does not mutate the website.
- `aero-executor`: deterministic mechanics and fresh grounded input. It requires an authorization token from the trusted controller; it does not ask an LLM for permission.
- `aero-agent`: explicit modules for tasks, session-service interface, planner, policy/capabilities, budgets, data-flow gateway, execution strategies, verification registry, evidence/artifact repository, journal/recovery, and optional adapters. Do not hide all behavior in `orchestrator.rs`.
- `aero-cli` and `aero-shell`: thin adapters to the same application interface; neither contains a second task engine.

The planner cannot call the executor, vault, verifier result store, or unrestricted HTTP client directly. The verifier uses independent readbacks/checks; a second LLM is optional assistance, not the sole authority or sole safety mechanism. A verifier may not grant approvals or produce external mutations. Evidence capture that downloads/uploads or changes state must use the normal authorized effect path.

### Lifecycle and optional service process

Initial close behavior is `pause`: stop new automated dispatch, reconcile or mark uncertain in-flight effects, flush the journal, and release/terminate browser ownership safely. UI crashes must be detectable; an in-process crash may end the run, which resumes conservatively from the journal. Do not claim a task survives a crashed host process until the service process is implemented and tested.

If background work or UI crash isolation justifies extraction, run the same interface in a per-user local service. Use a private Unix-domain socket, owner/peer-credential checks, bounded versioned messages, role-specific methods, and event sequence recovery. No TCP listener by default. A model-facing MCP adapter has a narrower role than a trusted CLI/desktop peer; it cannot self-declare a trusted role. Protect against unintended client access, but do not claim that a private socket defeats a fully compromised same-user account.

Background mode requires an explicit per-task user grant, a persistent indicator, reconnect/status/cancel controls, and a clearly communicated close policy. Losing the UI does not remove task budgets or policy enforcement.

## 6. Delegation, effects, and transaction previews

A capability binds task/contract revision, origins, account/profile reference, effect classes, data destinations, quotas, and expiry. Grants come only from trusted user actions and are persisted with issuer, revision, expiry, revocation, and quota-use records. A restarted process must revalidate the durable grant, not recreate authority from model history. The controller checks capabilities immediately before dispatch. A trusted profile selector is not a model-settable filesystem path or account identifier.

Examples of useful grants:

- Navigate approved public supplier origins and extract specified fields.
- Stage public documents within size limits, but do not export them elsewhere.
- Fill eligible fields for this application/account, but never submit.
- Submit this exact preview once after explicit review.

Classify an action from its actual resolved target, context, and adapter preconditions, not just its action name or the LLM's rationale. A `Click` may navigate, toggle, purchase, delete, or be ambiguous. A `Type` may invoke autosave or trigger a search. Unknown effects are review-required; a site adapter cannot elevate permissions. Navigation grants cannot prove that all GET requests are harmless; document that limitation.

High-impact actions require a preview containing effective destination/frame origin, account, action/effect, relevant data fields, artifacts/recipients, amount/currency where applicable, evidence, and expiry. Bind approval to a canonical material-effect digest and relevant document/observation generation. Material destination/account/payload/price/recipient changes invalidate approval. Do not invalidate on unrelated animated DOM churn; do recheck semantics and target freshness immediately before dispatch.

Preparation and commit are distinct in the API/UI. There is no vault auto-submit flag. Human takeover is a separate input ownership mode: record user-performed actions as manual/unverified where appropriate, do not claim automation policy prevented every native-browser manual action, and require a fresh observation before automation resumes.

## 7. Verification, evidence, and artifacts

### Operation outcomes

Record `dispatch_state`, `verification_status`, `verification_method`, and evidence separately. The durable lifecycle is proposed → authorized → prepared → dispatched → verified or failed/inconclusive/uncertain → committed to the journal. An unknown result is never silently coerced into success.

Every supported operation declares a postcondition before dispatch:

| Operation | Minimum postcondition/evidence |
|---|---|
| Navigate | Correct effective destination and lifecycle or explicit page/network failure. |
| Type/select/check | Fresh target identity and expected state; secrets checked through safe boolean predicates. |
| Submit/commit | Observed confirmation or resulting record matched to intended account/payload; otherwise uncertain/review. |
| Download | Terminal completion, actual bytes, declared limits, plausible content, and requested source/artifact identity; MIME alone is insufficient. |
| Extraction | Ordered schema, limits, required fields/counts, source references, and successful readback of exported output. |
| Research/report | Required fields/claims link to captured source evidence; unsupported assertions are marked unknown, not invented. |

Use structured predicates and fixture server-side truth where available. An owned citation is not enough if its passage is irrelevant or contradicts the reported fact; compare structured source values where possible and mark unsupported semantic conclusions inconclusive. Website assertions and model statements remain untrusted evidence of appropriate strength; they do not become mathematical proof of external truth.

### Evidence repository

An EvidenceRecord has opaque ID, task/run/operation, origin/effective URL, capture time, capture method, relevant excerpt/structured value, optional local content hash, document generation, sensitivity, retention, and limitations. Capture only required passages, not unlimited page archives. Hashes identify captured bytes; they do not prove the page's claims or publication time. Do not issue a second unauthorised authenticated fetch to reconstruct evidence.

### Artifact repository

An ArtifactRecord links a generated internal path to task/run/operation, format/MIME, schema/count where applicable, byte count/hash, source evidence IDs, creation time, sensitivity, retention, and verification result. Use private staging, bounded exclusive writes, readback checks, and atomic finalization. Publishing outside private task storage is a separate export permission. No model-supplied host paths.

The result bundle contains the report, structured data/documents, manifest, criteria outcomes, known limitations, and source references. UI/CLI must let users inspect provenance and freshness. Source changes may make a result stale; revalidation is explicit, not a false perpetual “verified” badge.

## 8. Data-flow and browser permission policy

Use a typed, deny-by-default gateway for application-managed transfers to providers, OCR/embedding services, external tools, telemetry, memory, and exports. Every request carries task/contract revision, source labels/origins, recipient identity, purpose, payload type/size, and grant reference. No provider/tool adapter bypasses this gateway with a general HTTP client. Gateways can approve a transfer without implying the recipient's outputs are trusted.

### User-visible processing modes

| Mode | Enforced behavior |
|---|---|
| `local_only` — Local-only processing | Task contents stay in local processing/storage. Local inference/embedding/OCR must be explicitly provisioned; if unavailable, pause with an actionable error. No remote fallback or task-content telemetry. Approved browser website requests still use the network; this is not an offline-browsing promise. |
| `approved_cloud` — Approved cloud processing | Only named processors may receive explicitly allowed data categories. Text, screenshots, documents, embeddings, and tool payloads have separate grants; no silent provider fallback. |
| `sensitive` — Restricted task | Local processing by default; cloud/screenshots/external tools/memory/exports denied unless the trusted user creates a narrowly scoped exception for the specified data and recipient. Secrets never enter ordinary model context. |

Unknown/unclassified task data defaults to private. A web page or model cannot label itself “public” to declassify data. User/configured source policy may classify approved public research pages. Provider credentials, task contracts, and browser authentication are separate secret domains.

Website traffic is governed by browsing/origin/permission policy, separately from application-managed processor transfers. Do not promise that application redaction prevents a malicious website from exfiltrating information supplied to that website. Strong browser-wide egress confinement requires its own tested proxy/network-namespace backend. Strict profiles that require such confinement must refuse to run without that backend; never silently weaken a selected policy mode.

Browser permissions default deny for camera, microphone, geolocation, clipboard read, filesystem, notifications, and automatic uploads. Explicit, task/origin-bound grants are required when supported. Export and upload use a trusted file picker or approved ArtifactID; validate type/size/destination, do not expose the user's home directory to the model or browser. Inspect nonsecret upload metadata and verify receipt when the workflow requires it. File inputs are never restored from snapshots; explicit reselection/reapproval is required after recovery.

Retention/deletion must cover tasks, evidence, artifacts, snapshots, derived memory/index rows, and application caches. Explain external recipients' retention separately; local deletion cannot revoke data already sent to a website/provider. Live browser profiles may contain sensitive browser-managed files despite application snapshot encryption.

## 9. Reliable and economical execution

Use deterministic operations/site adapters where their preconditions are satisfied, then AX/DOM-grounded targeting, then optional visual grounding, then human assistance. This is not a rigid prohibition on generic semantic execution; do not fail an ordinary supported task merely because no site-specific adapter exists. All strategies share policy, journal, budgets, verification, and fresh target resolution.

Only reviewed built-in adapters initially. A procedure records version, supported origin/routes, structural/semantic preconditions, typed parameter schema, steps, postconditions, effect/data requirements, provenance, and invalidation triggers. It contains no historical transient handles, secrets, arbitrary script, or authority grant. Failed preconditions stop or fall back to a newly planned authorized operation, not blind replay. User demonstrations can propose procedures for review; self-modifying production automation and an extension marketplace are out of scope.

Budgets are task-cumulative and reserved before dispatch/provider/tool work; concurrent workers cannot oversubscribe them. At most one bounded invalid-response repair attempt is permitted by default and still costs tokens/time. Inference cost ceilings require configured prices/upper bounds; if price is unknown, enforce token/request limits and report monetary cost as unknown. Purchase spending and inference cost are separate budgets. No invented savings, speed, or success figures.

## 10. Task workspace and manual browsing experience

The primary workspace shows task objective/criteria, progress, verified versus unverified results, questions/approvals, artifacts/evidence, budgets, and recovery status. Public action rationale is available in a secondary audit timeline; raw model reasoning or a scrolling log is not the main product.

Required baseline browser interactions: address/search navigation, back/forward/reload/stop, tabs, task/profile context indicator, find-in-page, copy/paste with trusted permission handling, downloads/artifact shelf, explicit uploads, clear site-permission controls, keyboard shortcuts, focus visibility, and keyboard/screen-reader accessible task controls. Persistent profiles are opt-in, separate from ephemeral task sessions, and never automatically import another browser's cookies/passwords. No password-manager or extension ecosystem clone is required for the first release.

### Early viewport decision gate

Before substantial frontend or packaging work, test the actual controlled Chromium target through a streamed view and a companion-console/native-window arrangement. Evaluate text selection/copy, Unicode/IME, scrolling/resizing, keyboard shortcuts, accessibility, popups/auth/file dialogs, focus, takeover, and X11/Wayland behavior. Record limitations, measured latency, and user acceptance. A screenshot canvas alone must not be called an accessible native browser viewport.

Choose the supported arrangement from evidence. Default candidate is a trusted Tauri console with a stream where useful and native Chromium for interactions the stream cannot support. If native in-process Chromium embedding is mandatory and simpler arrangements fail, stop desktop commitment and obtain a separate CEF feasibility decision. Do not quietly substitute WebKit navigation of the same URL, maintain a fork, or call an untested arrangement “seamless.”

Pause interrupts planning/read-only waits promptly and blocks new automation; bounded in-flight input is finished or marked uncertain. Takeover requires acknowledged exclusive input ownership. Resume invalidates old handles/approvals where relevant and observes the actual state left by the user. Manual input must not race background automation on the same page.

## 11. Persistence, recovery, and truthful reversibility

SQLite is authoritative for tasks/contracts, cumulative budgets, runs, intent/outcome journal, approvals, evidence/artifact metadata, and checkpoints. Use one bounded writer owner; step outcome/checkpoint/cursor changes are transactional. Sensitive snapshot payloads require authenticated encryption and an available key outside the browser; no plaintext fallback. Atomic DB updates do not atomically commit a website operation.

Persist intent before external action. A crash with an unresolved prepared/dispatched effect means `needs_review`; do not repeat it automatically. Supported independent readback may reconcile a specific operation when it can identify the actual resulting record; otherwise require user review. Reconciliation is read-only and policy-bound, not a speculative retry.

Restore original task contract/run history, cumulative budgets, authorized tab/origin storage, and eligible forms. Rebuild all runtime managers and invalidate stale IDs. Restore origin-scoped storage before application boot scripts; form restoration uses exact verified locators/native setters and bounded readiness. Best-effort browser hydration does not restore arbitrary JS heaps, IndexedDB, expired tokens, files, passkeys, or remote wizard state.

Every recovery produces a structured report: task history restored, browser storage restored/skipped, fields restored/missing/rejected, authentication status, unsupported state, unresolved effects, next safe step, and required user decisions. Expose it in CLI and task workspace, not only debug logs.

Distinguish `reversible_local`, `compensatable_external`, `irreversible_external`, and `unknown` effects. Only promise local undo or external compensation when an implementation and postcondition exist. Compensation is a new authorized journaled action; it is not erasure of history. Offline audit replay is side-effect free; live re-execution is opt-in and stops on divergence. There is no universal Undo or deterministic live replay claim.

## 12. Evaluation and release quality

Create the evaluation harness before optimizing models or expanding features. Version task fixtures/contracts, ground truth, judge/checker implementation, browser/application/model configuration, seed where meaningful, and per-case budgets. Ground truth is independent of the planner; deterministic fixture server state is preferred for side effects. Never use public live websites as the only CI oracle.

Maintain categories for public research/provenance, forms/wizards, extraction/downloads/uploads, tabs/frames/auth boundaries, human handoff, crashes/network faults, prompt injection/unauthorized effects, privacy, and manual interaction quality. A development suite can be public in the repo. A held-out set must be separately curated and unavailable to optimization agents; do not pretend fixtures committed to the same accessible repository are hidden. Its creation/review is an owner-controlled release task, not permission to collect private browsing data.

Report separately:

- Verified task success; failed, incomplete, and inconclusive tasks.
- False-success count/rate against independent ground truth.
- Unauthorized effects, secret/data-policy violations, and repeated uncertain effects.
- Human interventions and prompts per completed task, including useful approvals versus avoidable interruptions.
- Recovery correctness and supported-state restoration completeness.
- Wall-clock time, provider calls/tokens, priced or unknown cost, and peak resources per verified task.
- Native interaction/a11y checklist and measured takeover/viewport responsiveness.

Primary metric is verified useful task completion with safety constraints, not clicks, tokens generated, or actions attempted. Required deterministic safety cases must have zero unauthorized effects, false success, secret leaks, and repeated uncertain effects; that is a test gate, not a universal real-world guarantee. Freeze the mandatory workload cases and acceptance thresholds before release/model tuning; missing required cases block release. Performance SLOs and comparative superiority claims require actual baseline measurements, environment disclosure, and repeatable results. Every required shipped workflow must pass its specific acceptance test; aggregate success cannot conceal a broken required workflow.

## 13. Requirement register and acceptance ownership

These IDs are stable references for implementation and validation. Each requirement is unimplemented until its named plan task and actual tests pass.

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

## 14. Retained technical requirements and non-goals

Retain typed epoch IDs, bounded CDP queues/timeouts/cancellation, request cleanup, subscribe-before-enable lifecycle, fresh post-scroll geometry, correct same-process/OOPIF mapping, open-shadow locators, JSON array form snapshots preserving empty/false/multi-select states, private paths, Secret Service/TOTP protections, context-scoped downloads, supervised worker cleanup, MIME-correct visual marks, and loopback opt-in observability from the corrected implementation plan.

Retain CSV as baseline, Parquet optional, SQLite-first memory, opt-in semantic retrieval, MCP as guarded tools rather than assumed inference, and signed/checksummed/provenance-verified packaging only with real configured identities/assets. Native packaging precedes optional Flatpak submission. External publication, signing identity creation, account access, dependency installation, and commits still require appropriate explicit user approval.

Do not prioritize stealth/fingerprint spoofing, swarms, universal native embedding, an extension marketplace, a browser fork, vector infrastructure, or model fine-tuning as launch claims. Do not advertise perfect prompt-injection prevention, exactly-once external effects, lossless browser restoration, universal zero-exposure secrets, full network isolation from URL checks, or universal Undo.

## 15. Open owner decisions, not implementation guesses

- Confirm actual inference provider/local model and pricing configuration before real inference smoke; tests use explicitly identified mocks/scripted planners.
- Accept the viewport feasibility evidence before choosing the desktop presentation; native embedding is a separate decision if simpler approaches fail.
- Confirm app ID/domain, license/asset rights, browser redistribution/update ownership, and signing identity before publication.
- Approve local setup separately; this specification revision does not install tools or initialize Git.
- Supply/review holdout tasks and freeze release-specific performance thresholds after baseline measurements.
- Decide whether intranet/strong-egress/background requirements justify their separate backends. Unsupported strict modes fail honestly instead of silently downgrading.

## 16. Approved recommendation coverage

| Recommendation | Specification sections | Implementation additions |
|---|---|---|
| Task-first product | 2–4, 10 | N01–N03, N20 |
| Independent verification | 4, 7, 12 | N04–N05, N22–N23 |
| Bounded delegation and commit preview | 6 | N06–N07 |
| Control/execution-plane separation | 5 | N08–N09 |
| Earlier viewport decision | 10 | N10 |
| Reliable low-cost mechanics/procedures | 9 | N11–N12 |
| Visible data-flow permissions | 8 | N13–N16 |
| Provenance/artifacts/recovery product UX | 7, 10–11 | N17–N21 |
| Early benchmark and measured expansion | 12 | N22–N24 |

Implementation starts with documentation/contract/evaluation foundations, then a bounded browser vertical slice and early viewport experiment, then recovery and product breadth. This is the approved direction, not evidence that any implementation has begun.
