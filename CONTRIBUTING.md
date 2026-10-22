# Contributing to AeroBrowser

Status: B01 pure primitives ready for review, uncommitted, 2026-10-22; see the [current handoff](docs/handoff.md#current-b01-checkpoint). The user approved committing and pushing the earlier bootstrap/README, not B01. Local main is linked to the private GitHub repository; verify current remote delivery from Git rather than this static snapshot. Browser execution, task/control/policy/store and native sandbox qualification remain future work. Every later batch still requires review and explicit commit/push approval.

## 1. Read first and respect document authority

1. [docs/handoff.md](docs/handoff.md) — current state, blockers, next batch, and handoff obligations.
2. [docs/specification.md](docs/specification.md) — authoritative product requirements and trust boundaries.
3. [docs/implementation-plan.md](docs/implementation-plan.md) — exact contracts, file paths, dependency schedule, work packages, and the live execution ledger.
4. [docs/bug-fix-tracking.md](docs/bug-fix-tracking.md) — defects, setup blockers, owner decisions, and fix evidence.

The PDF is historical design input, not tested code. Keep stable REQ-*, SPEC-*, and package IDs. A changed requirement must update its specification, plan/test traceability, affected acceptance tests, and current handoff together. Propose a conflicting requirement change for owner review; do not quietly weaken a security rule to unblock implementation.

## 2. Start a small, owned batch

- Recheck the active workspace, Git state, relevant instructions, manifests, definitions, and usages before editing. Do not read credential files or print secrets.
- Claim one package/subtask in the plan ledger: owner, dependencies, exact writable files, expected behavior, and verification commands. Reference a tracker entry for a blocker or bug.
- Implement only the current approved scope. No unrelated cleanup, generic plugin systems, daemon extraction, extra crates, or dependencies added for hypothetical future work.
- Every focused behavior is a 2–5-minute edit/check. A CDP transport, database, or desktop shell is a work package, not one microtask. Provisioning and native manual testing are longer, explicit operations.
- One writer owns a file at a time. The controller/store/runtime lifecycle has one integrating owner. Parallelize independent pure contracts/tests only after agreeing interfaces and non-overlapping writable paths.
- Agents/subagents may propose changes and return real evidence. They cannot approve their own work, close tracked bugs solely by self-report, grant application authority, or commit without user approval.

Before any implementation, resolve the applicable setup decision in the tracker. This guide itself does not approve dependency installation, Git initialization, account access, remote CI, external publication, or changing host security policy.

## 3. TDD and review cycle — every behavior

1. Write the complete failing test in the exact path named by the plan. Define any helper before it is used. Assert externally meaningful behavior, not just a mocked method invocation.
2. Run the focused command and capture the named test's failure. Missing compiler/browser/library is an environment blocker, not a useful red test. Initial module/import failure can establish a new module, but obtain a behavioral assertion failure before implementing its behavior.
3. Implement the minimum complete change; share the actual tested implementation. No parallel test-only JavaScript, production fake provider, placeholder response, ignored error, broad fallback, or TODO standing in for required behavior.
4. Run the identical focused test to green, then its package/affected integration suites and shared quality gate. A command that runs zero matching tests is not a pass. Required browser tests must fail missing prerequisites, not skip or return success.
5. Review the diff, test evidence, privacy/security impact, and affected acceptance criteria. Update the ledger/tracker/handoff with the real result and limitations. Enter `ready_for_review`; enter `verified` only after completed review and passing affected checks.
6. Stop for explicit user approval before a commit. Never commit a deliberate red stub or broad unreviewed changes just to achieve frequent commits.

Documentation-only changes use requirement/link/example checks rather than invented application tests. Bootstrap may verify metadata/import/build behavior before application behavior exists; record exactly what was exercised.

### Concrete first code cycle after A03

Use B01 in `docs/implementation-plan.md` §7.3 and §8. Put the complete `center_uses_visible_intersection` test beside `CssRect` in `crates/aero-core/src/types.rs`, export the module from `crates/aero-core/src/lib.rs`, and use a minimal incorrect method to obtain a real assertion failure before implementing the documented predicate.

```bash
cargo test -p aero-core center_uses_visible_intersection -- --nocapture
cargo test -p aero-core
```

Expected red: the named test executes and its visible-intersection assertion fails. Expected green: the named test executes once and passes; the core suite exits 0. Those were the prescribed red/green assertions; B01 has now exercised them, plus overflow/nonfinite and strict-interior rounding regressions. See the handoff for observed commands/results. They do not establish safe actual browser input.

## 4. Architecture constraints

- `aero-core`: pure versioned models/validation; no browser, database, provider, or GUI dependencies.
- `aero-cdp`: owned transport/process/target/frame lifecycle; no business policy, SQLite, or planner.
- `aero-perception`: observation/locators/classification; observation never mutates a page and never depends on `aero-executor`.
- `aero-executor`: fresh, deterministic input under a trusted authorization capability; no planner-driven permission grants.
- `aero-agent`: task/run ownership, grants, budgets, data flow, planner, independent verification, persistence, artifacts, and recovery. Keep orchestration thin and domains in their named modules.
- CLI, desktop, and restricted MCP use one role-scoped application interface. Do not add an alternate executor, frontend permission engine, or second WebKit browsing session.
- Begin with six Rust crates and minimal defaults. Pin the compiler and lock resolved dependencies; add a crate/feature only for a real boundary. Optional vault/Parquet/SOM/semantic-memory/MCP/desktop dependencies cannot inflate the minimal graph by accident.
- Define shared fixture/checker modules before consumers. CLI must not import integration-test-only helpers. The explicit nondefault evaluation feature owns the runner/fixture server/scripted support; dependency-light manifest validation remains testable without it.
- Preserve typed Task/Run/Target/Frame/session/epoch IDs. Never equate a transient browser handle with durable task identity or reuse stale handles after navigation/recovery/takeover.

## 5. Security and privacy rules

- Treat pages, provider responses, tool results, retrieved examples, and procedures as untrusted data, never authority.
- An agent can request approval, not issue it. Trusted-human-only grants and caller roles are host-constructed, never selected through model JSON or model-callable MCP tools.
- Policy checks the actual resolved effect, origin/account, material preview, expiry, quota, and fresh target immediately before dispatch. Apply the same gate to Enter, replay, workers, uploads, and external tools.
- Persist intent and reserve cumulative task resources before dispatch. A pending/uncertain effect after a crash becomes `needs_review`, not an automatic retry. SQLite does not provide exactly-once website effects.
- Dispatch acknowledgement and model `Complete` do not imply success. Independent postconditions, actual artifact readbacks, required criteria, and no unresolved effect control task completion.
- Mediate application-managed provider/OCR/embedding/tool/export transfers centrally. No unrestricted adapter HTTP client, remote fallback from local-only mode, or self-declassification by page/model content.
- Local-only processing is not offline browsing or complete website egress confinement. Fail unsupported strict modes; do not advertise URL checks as full network isolation.
- Never pass vault plaintext to the planner, ordinary logs, serialized audit, screenshots, metrics, fixtures, or bug reports. No Debug/Serialize for plaintext secret types. Redact echoed text, card/OTP fields, query tokens, tool results, and errors; suppress images when safe redaction is unavailable.
- Use synthetic sentinel values in tests. Do not access real accounts/payments to demonstrate integration. Native keyring tests use a dedicated test D-Bus/keyring environment.
- Private generated paths, exclusive directory-relative writes, no symlink following or overwrite, bounded payloads, and owner-validated cleanup. A model provides ArtifactID, not a host filename/path.
- Keep Chromium's renderer sandbox and the tested host sandbox. Never add `--no-sandbox`, disable web security, widen mounts/capabilities, change sysctls/AppArmor, or write plaintext snapshots to make a test pass.
- Pause cancels planning/read-only waits promptly, blocks later dispatch, and acknowledges exclusive input ownership after bounded input completion or explicit uncertainty. Resume takes a fresh observation.
- Retention includes every currently implemented local derived store. Disclose that local deletion cannot recall external copies and live browser profiles can remain sensitive.
- A streamed image is not proof of native accessibility/IME/dialog support; collect actual native evidence before claiming a backend.

A suspected active leak, unsafe dispatch, sandbox regression, duplicate external effect, or false success stops the affected path immediately. Create a sanitized S0/S1 tracker entry, preserve only necessary redacted evidence, and obtain review before resuming. Do not publish a vulnerability or raw browsing data to a public issue without approval. No security contact has been established yet; ask the project owner for an approved private channel rather than inventing one.

## 6. Verification commands and evidence

Run from the repository root after the referenced files/tools exist. Do not run bootstrap writes merely to make documentation commands succeed.

### Minimal Rust gate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all commands exit 0; named affected tests actually execute. The selected compiler, Cargo.lock, package/feature set, and any excluded platform are recorded. Do not use `--all-features` as a substitute for a supported feature matrix.

### Required browser and G1 workload gate

```bash
AERO_REQUIRE_BROWSER=1 bash scripts/test-browser.sh
AERO_REQUIRE_BROWSER=1 cargo run -p aero-cli --features evaluation --locked -- evaluate --manifest evals/manifest.json --suite g1 --planner scripted --output target/evaluation/g1.json
```

Expected: every mandatory local case executes, actual files/server truth match the checks, and safety-negative gates pass. Missing Chromium/bwrap/sandbox support is a failed/blocked gate, not a silent success. The report is labelled `scripted_browser`; it is not evidence of model intelligence or held-out performance. Actual-model evaluation requires approved provider/data/budget configuration and a separate report.

### Selected desktop gate, after accepted viewport feasibility

```bash
npm --prefix crates/aero-shell/ui ci
npm --prefix crates/aero-shell/ui run typecheck
npm --prefix crates/aero-shell/ui test -- --run
npm --prefix crates/aero-shell/ui run build
cargo test -p aero-shell --locked
```

Expected: automated checks exit 0. Separately record real X11/Wayland, keyboard/IME, screen-reader/focus, selection/dialog, pause/takeover/resume/close evidence for claimed support. Passing TypeScript tests cannot replace it. Optional features and packaging use their explicit plan gates; a binary's presence is not a clean-machine install test.

### Documentation checks, available before application bootstrap

Run this read-only check from the workspace root. It checks the living documentation links, fences, requirement mappings, and execution ledger without needing Rust or Git:

```bash
python - <<'PY'
from pathlib import Path
from urllib.parse import unquote
import re
root = Path.cwd()
paths = [Path('CONTRIBUTING.md'), *sorted(Path('docs').rglob('*.md'))]
legacy = Path('.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md')
if legacy.is_file():
    paths.append(legacy)
docs = {p: (root / p).read_text(encoding='utf-8') for p in paths}
for path, text in docs.items():
    assert text.endswith('\n'), f'{path}: missing final newline'
    assert all(line == line.rstrip() for line in text.splitlines()), f'{path}: trailing whitespace'
    inside = False
    prose = []
    for line in text.splitlines():
        if line.startswith('```'):
            inside = not inside
        elif not inside:
            prose.append(line)
    assert not inside, f'{path}: unclosed fence'
    for target in re.findall(r'\[[^\]\n]+\]\(([^)\s]+)\)', '\n'.join(prose)):
        if target.startswith(('https://', 'http://', 'mailto:', '#')):
            continue
        destination = unquote(target.split('#', 1)[0])
        assert (root / path.parent / destination).is_file(), f'{path}: broken link {target}'
def requirements(text):
    result = {}
    for line in text.splitlines():
        if line.startswith('| REQ-'):
            cells = [c.strip() for c in line.strip('|').split('|')]
            assert cells[0] not in result, f'duplicate {cells[0]}'
            result[cells[0]] = cells[1:]
    return result
spec = docs[Path('docs/specification.md')]
plan = docs[Path('docs/implementation-plan.md')]
assert requirements(spec) == requirements(plan)
assert len(requirements(spec)) == 26
corrections = re.findall(r'^\| (SPEC-\d{2}) \|', plan, re.M)
assert corrections == [f'SPEC-{i:02}' for i in range(1, 32)]
expected = {f'{prefix}{i:02}' for prefix, count in
            [('A',4),('B',6),('C',7),('D',7),('E',7),('F',11),
             ('G',16),('H',6),('I',6),('J',6),('N',24)]
            for i in range(1, count + 1)}
ledger = re.findall(r'^\| ([A-Z]\d{2}) \| (not_started|in_progress|ready_for_review|verified|committed|blocked|deferred) \|', plan, re.M)
assert len(ledger) == len(expected) == len({i for i, _ in ledger})
assert {i for i, _ in ledger} == expected
matrix = docs.get(Path('docs/requirements.md'))
if matrix is not None:
    identifiers = set(re.findall(r'\b(?:REQ-[A-Z]+-\d{2}|SPEC-\d{2})\b', matrix))
    assert identifiers == set(requirements(spec)) | set(corrections)
print(f'PASS: {len(docs)} documents, links/fences/whitespace, 26 requirements, 31 corrections, 100 ledger packages')
PY
```

Expected: the one PASS line and exit 0. This is documentation validation, not Rust/browser execution. Embedded JSON/SQL/JavaScript changes additionally need parsing/syntax and relevant synthetic constraint tests; record those separately. Do not treat this checker as a Markdown style framework or add a new dependency just to run it.

### Evidence record for every reviewed batch

Use this record in the plan ledger's linked review entry or handoff:

```text
Batch/package/subtasks:
Owner and exact writable paths:
Requirement/correction/bug IDs:
Environment and selected features:
Red command, named test, exit status, observed failure:
Minimal implementation summary:
Green command, executed count, exit status, observed result:
Affected-suite/artifact/readback evidence:
Not run / unavailable / limitations:
Reviewer and explicit commit approval reference:
Actual commit SHA or uncommitted:
Next safe action and remaining blocker IDs:
```

Capture concise output and exact commands; never invent timestamps, SHAs, test counts, CI runs, file hashes, or measurements. Do not paste secrets/private pages or full provider payloads into evidence.

## 7. Git, commits, and review

Git is initialized on main with origin at `git@github.com:dave2k77/aero-browser.git`. Do not initialize it again. The user explicitly approved the initial bootstrap commit and push; this does not grant ongoing commit/push permission for later batches. The historical PDF and `.hermes` session artifacts remain ignored pending source-inclusion review; the optional legacy redirect is not required in a fresh clone. Do not change global Git identity, commit the PDF, sign or publish a release as a setup side effect. License files require owner confirmation.

After approved initialization, inspect `git status --short`, `git branch --show-current`, and `git diff` before acting. Preserve the user's unrelated work. Use short-lived `feat/ID-description`, `fix/BUG-ID-description`, or `docs/description` branches when a branch is useful; no branch rename or history rewrite without authorization.

For an approved batch, stage only its reviewed exact paths. Example for B01 after its behavior and review pass:

```bash
git diff --check
git diff -- crates/aero-core/src/lib.rs crates/aero-core/src/ids.rs crates/aero-core/src/types.rs
git add -- crates/aero-core/src/lib.rs crates/aero-core/src/ids.rs crates/aero-core/src/types.rs
git commit -m "feat(core): define scoped IDs and CSS geometry"
git status --short
git log -1 --format='%h %s'
```

Expected: diff check exits 0, only the approved paths are committed, and the final log reports a real SHA/message. A first import/scaffold commit must be approved separately before this example. Do not use `git add .`, amend another contributor's commit, rebase shared history, force push, or include runtime profiles/databases/browser binaries/credentials/private evidence.

Review checklist:

- Scope and requirements are traceable; interfaces match their consumers and module exports/manifests exist.
- Meaningful red/green and affected suites are evidenced; zero tests/skips/missing dependencies are not called passes.
- Policy, budget, identity, crash uncertainty, privacy, file ownership, and cancellation invariants hold across sibling call paths.
- Source/claim checks use relevant captured values, not citation count alone; completion cannot bypass required results.
- Optional dependencies/features are justified and do not silently enter the minimal build.
- Current state, remaining blockers, tracker evidence, and exact next action are updated.
- Any commit, remote CI, release, account use, or setup action has the appropriate explicit approval.

## 8. Bugs and handoff

Use the [tracker's report and fix templates](docs/bug-fix-tracking.md#6-copyable-record-templates). Fix the root cause across related action paths, add the regression test first, preserve reproduction evidence, and let the integrating reviewer verify closure. A known design correction is not an implemented bug fix.

Before ending a batch or transferring work, update the package ledger, affected tracker entries, and [handoff.md](docs/handoff.md). Record actual changed files, commands/results, uncommitted state, running processes, exact next test, approvals, and unresolved uncertainty. Do not leave a speculative success claim or a promise to run tests later as a finished handoff.
