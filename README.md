# AeroBrowser

A Linux-native browser for delegated work, being built around verifiable results, explicit permissions, and reliable human control.

The goal is to let users delegate bounded browser tasks, inspect the resulting evidence and artifacts, and take over when judgment or manual interaction is needed. AeroBrowser uses an existing Chromium-family engine; it is not a new rendering engine or a Chromium fork.

## Current status

**Early development: bootstrap and B01 committed; B02/B03/N01 contracts, B04–B06 storage, N02 task access and audit repairs implemented locally, uncommitted. N03 local budgets and N04 synthetic verification are ready for owner review. This is not yet a usable browser or an autonomous browsing agent.**

Implemented:

- A six-crate Rust workspace with a pinned toolchain and dependency lockfile.
- An `aero` CLI with help, version information, and a `doctor` command.
- Bounded executable/version and integrity probes, diagnostic process-group cleanup, and comparison against a recorded development browser byte pin.
- Bootstrap tests, requirements traceability, architecture decisions, and contributor documentation.
- B01 core primitives: six distinct string identifiers for targets, frames, CDP sessions, runtime/document generations and observations; exact string serialization; clipped CSS geometry; and separate validated frame-local/root-CSS/screenshot coordinate types.
- B02 pure contracts (uncommitted): version-1 action/decision objects, object-only action parsing, typed observation references, parameter validation and payload-free validation errors. These proposals never grant execution permission.
- B03 pure contracts (uncommitted): JSON-safe form values, distinct task/run/evidence/artifact references, versioned checkpoints with explicit capture completeness, and public status/usage/outcome events. These represent data; they do not capture, persist or restore a browser.
- N01 TaskContract (uncommitted): strict versioned objectives/criteria/outputs, scoped intent, processing rules, bounded cumulative limits and explicit metadata. Validators do not grant authority, execute verifiers, persist revisions or start tasks.
- B04 storage foundation (uncommitted): strict task-owned SQLite schema, transactional initialization, ownership/immutability constraints, corruption/version refusal, and a private single-owner opener.
- B05 journal (uncommitted): bounded writer/async replies, transactional task/run records, pre-action reservations, atomic outcomes/checkpoints, conservative orphan recovery and paginated audit. No execution authority, encrypted snapshots or browser restoration.
- B06 private state (uncommitted): XDG state layout, isolated task/run/runtime profile and staging directories, and cooperative task locks retained while runtime handles live. No browser/input dispatcher or automatic cleanup.
- N02 task access (uncommitted): host-issued owner/model scopes, owner-only immutable revisions/run records, task-scoped history and transaction-local start checks. Accepted intent is draft and new runs are paused; no browser readiness or effect grants are implied.
- N03 local accounting and N04 verifier registry (uncommitted): cumulative reservations and synthetic readback comparisons. The registry is not connected to real browser/artifact sources or a completion gate; serialized results do not prove provenance.

Not implemented yet: browser process supervision and CDP transport, task execution, permission enforcement, encrypted/browser recovery, model-provider integration, or a desktop interface. The library crates establish boundaries; their presence does not mean those subsystems work.

The intended first useful workflow is supplier research against deterministic local fixtures: produce an ordered CSV, a sourced comparison report, and a verified artifact manifest while identifying unknowns and avoiding account creation, submission, uploads, or purchases. This is a planned acceptance scenario, not a feature available today.

### Latest checkpoint — 2026-10-28

Bootstrap and B01 are committed and pushed to `main`: `54e0a95` implements scoped identifiers and safe geometry; `1b2a3da` records the approved B01 checkpoint. Geometry tests cover visible intersections, invalid/nonfinite inputs, overflow and floating-point boundary rounding. Compile-fail tests check that identifiers and coordinate spaces cannot be implicitly mixed.

B02/B03, audit repairs, N01–N04 and B04–B06 are implemented locally; nothing from these batches is committed. N03 accounting and N04's synthetic verifier registry are ready for owner review. N13/N16 have partial in-memory policy and filesystem-preflight foundations, not real transfer enforcement or complete deletion. N05 completion and N17/N18 repositories remain blocked after partial foundations. The previous README/handoff commit `af88105` remains local-only. See the [current handoff](docs/handoff.md#current-n13n16-work-in-progress) for review findings, verification and remaining limitations.

## Quickstart

### Prerequisites

- Linux with a C compiler/linker and a Rust installation managed by rustup.
- Rust **1.99.1**, with rustfmt and Clippy, as pinned in [`rust-toolchain.toml`](rust-toolchain.toml).
- Git and access to the repository. The SSH clone command below requires GitHub SSH authentication.
- Python 3 for the pin-check helper, shell-adapter tests and documentation checks; no `python` alias or pip packages are required.

Rustup can provision the pinned toolchain when Cargo first runs. No Node.js, model provider, keyring, or browser binary is required to build and test the current bootstrap and core primitives.

```bash
git clone git@github.com:dave2k77/aero-browser.git
cd aero-browser
cargo build --workspace --locked
cargo run -p aero-cli --locked -- --help
cargo run -p aero-cli --locked -- --version
```

The binary is named `aero`. If a user-local Rust installation is not on PATH, add it for the current terminal:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

See the [development guide](docs/development.md) for the observed environment, provisioning details, browser candidate, and native prerequisites.

### Inspect prerequisites

```bash
cargo run -p aero-cli --locked -- doctor --json
```

Doctor reports browser and Bubblewrap availability, the development browser-pin comparison, pkg-config availability, and the status or limitations of native checks. Select a trusted browser executable with `--chromium` or `AERO_CHROMIUM`; the bootstrap does not download one automatically.

**Doctor currently exits with code 2 and `runtime_ready=false`, even when the configured browser matches its local pin.** This is intentional: actual browser sandbox, namespace, process-cleanup, native display, and keyring qualification have not been implemented or established. A successful `--version` probe or matching byte hash is not proof of a working sandbox.

[`packaging/browser-lock.json`](packaging/browser-lock.json) records a development-only browser candidate. Its local SHA-256 pins are not independently published vendor signatures, and browser redistribution is not approved. Details and the pin-check helper are documented in the development guide.

## Build and validation

Run from the repository root:

```bash
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
```

The N02 workspace run on 2026-10-26 passed **143 behavior tests and eleven compile-fail doctests**. Build, formatting, warning-denied Clippy and rustdoc pass. Nine N02 integration tests and three compile-fail proofs cover host-issued authority, model/cross-task denials, immutable history, persisted start checks, scope lifetime and sealed task-write APIs. No dependency or schema change was needed. These are local tests, not hosted CI, remote-peer authentication or real-browser qualification.

These checks validate bootstrap, B01–B03/N01 pure contracts and B04–B06's storage boundaries, not delegated browsing, native sandbox behavior, model quality or working recovery. The types do not establish handle freshness, target hit testing or authorization. B02's URL guard is not a complete browser parser/network policy. B03's checkpoints hold private pre-encryption data, not plaintext SQL metadata; public-event structure is not sanitization of arbitrary ID strings. Trusted producers and future persistence/privacy/runtime owners remain necessary. Required real-browser/native suites will be introduced with their implementation; unavailable prerequisites must not silently count as a pass.

## Architecture

One application/control interface will own tasks, scoped permissions, cumulative budgets, verification, and durable history. CLI, desktop, and restricted tool adapters will share that interface rather than create independent execution paths.

| Crate | Intended responsibility |
|---|---|
| `aero-core` | Pure versioned contracts, IDs, validation, and public events |
| `aero-cdp` | Owned browser processes, transport, targets, frames, and lifecycle |
| `aero-perception` | Read-only semantic observations, extraction, and privacy classification |
| `aero-executor` | Authorized, freshly grounded deterministic browser input |
| `aero-agent` | Task coordination, policy, budgets, verification, storage, and recovery |
| `aero-cli` | Command-line adapter; currently implements prerequisite diagnostics |

The desktop shell is deferred until an early feasibility gate evaluates presentation of the actual controlled Chromium target. Tauri's trusted local UI must not be confused with that browser instance. See the [architecture document](docs/architecture.md) and its linked ADRs for ownership and trust boundaries.

## Design rules

These are requirements for the implementation, not claims that the corresponding safeguards already exist:

- The planner proposes; trusted policy authorizes; the executor acts; an independent verifier checks; the journal records.
- A dispatched action or model-reported completion is not verified task success.
- Grants come from the trusted human channel, never from a page, model, procedure, or model-callable approval tool.
- Task budgets and unresolved effects survive retries and restarts. Uncertain external actions must not be blindly repeated.
- Local-only processing must not silently fall back to remote providers. It is distinct from offline browsing or complete website-egress isolation.
- Human takeover requires acknowledged exclusive input ownership and fresh observation before automation resumes.
- Browser restoration is best-effort and must disclose partial recovery. There is no lossless-replay, universal Undo, stealth, or exactly-once remote-effect promise.
- Chromium's renderer sandbox must remain enabled; missing support is not a reason to introduce an insecure fallback.

## Roadmap

1. Core contracts, task/run identity, permissions, budgets, independent verification, and durable journaling.
2. Owned Chromium runtime, reliable observation/input, and the first verified supplier-research workflow.
3. Frames/forms, honest recovery, guarded local browser capabilities, and evidence-linked artifacts.
4. A task-first desktop with measured native interaction, accessibility, and exclusive handoff.
5. Required workload/security gates and verified native packaging before release.

Optional retrieval, visual grounding, concurrency, tool integration, background services, and alternative packaging have separate selection and validation gates. They are not prerequisites for the first useful workflow. Follow the [implementation plan](docs/implementation-plan.md) for the actual dependency order; its batches are not executed alphabetically.

Development has been exercised under WSL2. The planned release baseline is native Ubuntu 24.04 x86_64, with separate X11 and Wayland validation. Native support is not established by the current bootstrap tests.

## Documentation

| Document | Purpose |
|---|---|
| [Specification](docs/specification.md) | Authoritative product and technical requirements |
| [Implementation plan](docs/implementation-plan.md) | Contracts, dependencies, work packages, and execution ledger |
| [Requirements matrix](docs/requirements.md) | Requirement/correction-to-proof traceability |
| [Architecture](docs/architecture.md) | Component ownership and architectural decisions |
| [Development guide](docs/development.md) | Setup, commands, diagnostics, and environment limitations |
| [Handoff](docs/handoff.md) | Current checkpoint, execution evidence, and next safe action |
| [Bug/fix tracker](docs/bug-fix-tracking.md) | Defects, environment records, and owner decisions |
| [Contributing](CONTRIBUTING.md) | TDD, review, security, and scoped commit rules |

## Contributing and licensing

Use small, test-driven changes within the current approved scope. Do not commit credentials, browsing profiles, generated artifacts, downloaded browser binaries, or private evidence. Review and explicit approval are required before commits or pushes; this is not standing permission for future batches.

A project license has not yet been selected. Do not assume an open-source license or browser/asset redistribution rights from repository access. License and release-ownership decisions are tracked separately in the bug/fix tracker.
