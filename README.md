# AeroBrowser

A Linux-native browser for delegated work, being built around verifiable results, explicit permissions, and reliable human control.

The goal is to let users delegate bounded browser tasks, inspect the resulting evidence and artifacts, and take over when judgment or manual interaction is needed. AeroBrowser uses an existing Chromium-family engine; it is not a new rendering engine or a Chromium fork.

## Current status

**Early development: bootstrap plus committed B01 core primitives. This is not yet a usable browser or an autonomous browsing agent.**

Implemented:

- A six-crate Rust workspace with a pinned toolchain and dependency lockfile.
- An `aero` CLI with help, version information, and a `doctor` command.
- Bounded executable/version probes and comparison against a recorded development browser byte pin.
- Bootstrap tests, requirements traceability, architecture decisions, and contributor documentation.
- Distinct string identifiers, clipped CSS geometry, and separate frame-local/root-CSS/screenshot coordinate types, with unit and compile-fail tests.

Not implemented yet: browser process supervision and CDP transport, task execution, permission enforcement, durable task storage/recovery, model-provider integration, or a desktop interface. The library crates establish boundaries; their presence does not mean those subsystems work.

The intended first useful workflow is supplier research against deterministic local fixtures: produce an ordered CSV, a sourced comparison report, and a verified artifact manifest while identifying unknowns and avoiding account creation, submission, uploads, or purchases. This is a planned acceptance scenario, not a feature available today.

## Quickstart

### Prerequisites

- Linux with a C compiler/linker and a Rust installation managed by rustup.
- Rust **1.99.1**, with rustfmt and Clippy, as pinned in [`rust-toolchain.toml`](rust-toolchain.toml).
- Git and access to the repository. The SSH clone command below requires GitHub SSH authentication.

Rustup can provision the pinned toolchain when Cargo first runs. No Node.js, model provider, keyring, or browser binary is required to build and test the current bootstrap.

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

These validate the implemented bootstrap, not delegated browsing, native sandbox behavior, or model quality. Required real-browser and native-platform suites will be introduced with their corresponding implementation; unavailable prerequisites must not silently count as a pass.

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
