# Development — environment and verification

Observed on 2026-10-22 in `/home/davia/projects/aero-browser`. See [handoff.md](handoff.md) for the current reviewed checkpoint and [implementation-plan.md](implementation-plan.md) for remaining work. This guide does not claim a working delegated browser/runtime yet.

## Environment and scope

- Development host: Ubuntu 26.04.1 LTS under WSL2, x86_64.
- Release baseline remains native Ubuntu 24.04 x86_64; native X11/Wayland, keyring and sandbox compatibility have not been established by this bootstrap.
- `rustc 1.99.1 (58cf8f9c3 2026-10-13)` and `cargo 1.99.1 (5f94df478 2026-08-27)` installed with the official rustup minimal profile; rustfmt and clippy installed. `rust-toolchain.toml` pins 1.99.1, not a moving stable channel. No older MSRV is claimed.
- C compiler: Ubuntu GCC 15.2.0-16ubuntu1. Bubblewrap 0.11.1 already available. Node v24.18.0/npm 12.2.0 were present; no frontend is implemented or required for this batch.
- The six crates are not completed CDP/perception/executor/agent subsystems. Current compiled behavior is B01–B03 pure core contracts and CLI prerequisite diagnostics; the bootstrap environment record above is historical, not an assertion that later core work is absent.
- Python 3 is required for `scripts/lock-browser.sh`, its adapter tests and the contributor documentation checker. They use only Python's standard library and do not require a `python` alias or pip installation. The helper checks `python3` before invoking Cargo; on a new development host verify `python3 --version` explicitly.
- User approved proceeding with the first phase after its setup/scaffold/doctor scope was explained. No commit/push, license choice, redistribution, real account use or weakening of host security is implied.

Rust was installed using the script downloaded from `https://sh.rustup.rs`, `--profile minimal --default-toolchain stable --no-modify-path`, then the observed compiler was pinned with `rustup toolchain install 1.99.1 --profile minimal --component rustfmt,clippy`. The installer did not edit shell startup files.

For a new terminal using that user installation:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version
cargo --version
```

Expected for this checkout: the exact pinned compiler above, or an explicit installation attempt if that version is not yet provisioned. Dependency resolution is recorded in `Cargo.lock`; do not regenerate it for unrelated changes.

## Build and test

From the workspace root:

```bash
cargo metadata --format-version 1 --no-deps
cargo test -p aero-core --test import -- --nocapture
cargo test -p aero-cli --test doctor -- --nocapture
cargo test -p aero-cli --test browser_pin_helper -- --nocapture
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo run -p aero-cli --locked -- --help
```

Metadata must contain exactly aero-core, aero-cdp, aero-perception, aero-executor, aero-agent and aero-cli. The CLI binary is named `aero`. Do not treat the empty scaffold libraries' zero-test outputs as subsystem acceptance. Focused integration tests must actually execute. Test/measurement counts and red/green evidence belong in the handoff, not guessed from this guide.

No provider, external database server, keyring, desktop GUI, model download, real browsing account, or Chromium launch is needed. B04 builds bundled SQLite through rusqlite 0.41.0 (default features disabled), using the existing C toolchain; journal tests create disposable synthetic databases. Doctor tests use explicitly synthetic executable fixtures and do not prove a browser sandbox.

### C01/C02 transport tests

`cargo test -p aero-cdp --test transport --locked` runs 17 tests against an in-process WebSocket peer bound to `127.0.0.1:0`; no browser or network beyond loopback is used. aero-cdp depends on tokio-tungstenite 0.30.0 with default features disabled (`connect` only, no TLS) and futures-util 0.3.34. Passing these tests proves transport routing and cleanup, not Chromium reachability or sandboxing.

### C03 sandbox command tests

`cargo test -p aero-cdp --test sandbox --locked` runs five pure tests of `SandboxSpec::build`; nothing is spawned and no real host path is read. They prove the constructed Bubblewrap argument vector (namespaces, cleared environment, private roots and mount order, merged-/usr handling, display sockets, refused inputs), not that the sandbox works: C04's browser tests own that.

### B04 storage prerequisites

`cargo test -p aero-agent --test journal --locked` exercises the initial schema and private opener. It requires Linux procfs and a local filesystem supporting SQLite WAL, advisory locks, ownership and Unix permission bits. Supply an absolute dedicated path; only its final missing directory is created. Existing directories/files must already be owned and private (0700/0600). Symlinks, hardlinks, foreign owners, writable nonsticky ancestors and unsupported schemas fail closed; the opener does not chmod existing data. Do not point it at Windows-mounted, network or arbitrary shared storage and assume equivalent guarantees. B06's config module now supplies private XDG/task paths; Store alone is not task-controller authority. The lock coordinates cooperating Store instances, not hostile same-UID/root SQL writers.

B05 adds `cargo test -p aero-agent --locked` coverage for the bounded Journal writer, private failure injection and lifecycle tests. Tokio 1.54.1 uses only sync/oneshot support; it does not introduce a runtime or network stack. Journal start/close/drop block and must be invoked on a blocking host thread. Initial version-1 directional-counter changes intentionally refuse older B04 development databases via exact schema matching; do not delete or repair user data to hide that refusal. Browser/runtime crash qualification and monetary provider accounting remain later gates.

B06 configuration resolves OS-native XDG_STATE_HOME, otherwise HOME/.local/state, with an `aerobrowser` suffix. Missing state ancestors can be created privately; explicit relative/parent-traversing paths or unsafe existing objects fail without fallback or chmod repair. No XDG_RUNTIME_DIR or global /tmp fallback exists. Profiles/staging persist until future N16 cleanup. Task/run/epoch names are domain-separated hashes of exact bounded opaque IDs, not user-provided filenames. RuntimeDirs must remain alive through subprocess shutdown to retain the cooperative task lease. `cargo test -p aero-agent --test journal private_state_and_task_lock -- --exact` is the focused proof, not a native browser/input test.

N02's task API is verified with `cargo test -p aero-agent --test tasks --locked`. Trusted bootstrap constructs TaskAuthority from the Journal and provides adapters only the appropriate fixed TaskClient. Raw task-write methods are no longer public on JournalClient; legacy journal fixtures use explicit trusted service adapters. No new dependency or schema migration is involved. New intent remains draft and new run records paused; start prerequisites are synthetic host observations in tests, not real provider/background-service qualification. N08 peer authentication and N06 effect grants remain future integration.

## User-local development browser candidate

A Chrome for Testing stable Linux x86_64 binary was provisioned outside the repository at:

```text
$HOME/.cache/aerobrowser/browsers/156.0.8078.47/chrome-linux64/chrome
```

Its observed `--version` output is `Google Chrome for Testing 156.0.8078.47`. This is a development testing candidate, not an owner-approved redistributable product browser or a native sandbox qualification.

Primary provisioning references:

- Availability: https://googlechromelabs.github.io/chrome-for-testing/
- Machine-readable metadata: https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json
- Vendor explanation: https://developer.chrome.com/blog/chrome-for-testing/

`packaging/browser-lock.json` records the actual version, revision, platform, upstream download URL, locally measured archive/executable SHA-256, license-reference URL, explicit update policy and `redistribution_approved=false`. Archive MD5 matched the separately fetched Google Storage object headers. The selected metadata supplies no independent publisher SHA-256: `publisher_sha256` remains null, not an invented signature/checksum. TLS/vendor metadata and local byte pins are not equivalent to independently authenticated release provenance. Owner review is required before production provisioning/redistribution, and the vendor terms were not replaced by a project license.

The original archive was downloaded to the configured Hermes scratch directory, checked against the response-header MD5, validated for path traversal/symlink entries and bounded extracted size, then extracted under the private per-user cache. No browser binary/profile/archive belongs in Git. Archive availability in scratch is temporary; the extracted development binary is outside scratch. No auto-update or application-startup download is implemented.

Configure the candidate explicitly; doctor does not silently search arbitrary caches:

```bash
export AERO_CHROMIUM="$HOME/.cache/aerobrowser/browsers/156.0.8078.47/chrome-linux64/chrome"
cargo run -p aero-cli --locked -- doctor --json
bash scripts/lock-browser.sh "$AERO_CHROMIUM"
```

Doctor currently returns exit 2 and `runtime_ready=false` even when the version/local executable pin matches. This is intentional: the real C04 Bubblewrap/renderer-sandbox/namespace/cleanup qualification is not implemented. Display environment presence is not native UX proof; Secret Service remains untested. Do not use this report to claim G0/G1 or release readiness.

`lock-browser.sh` is a check of the already reviewed development pin, not a downloader or a way to bless arbitrary new bytes. It invokes the same Rust doctor implementation, accepts doctor exit 2 as a readable unready report, and succeeds only for `browser.available=true` plus `browser_lock.status=matched_local_pin`. Success does not override `runtime_ready=false`. A missing/mismatching executable fails. Updating the lock requires reviewed actual source/version/hashes and regression tests, not automatically trusting the current local executable.

## Doctor behavior and boundaries

- `--help`/`--version` and invalid argument handling do not probe a browser or initialize a provider/runtime.
- `doctor --json --chromium PATH` overrides AERO_CHROMIUM/PATH. An explicit empty or missing path never silently falls back to a different browser.
- Probes only trusted local executable `--version` commands: canonical paths, two-second phase deadline, 4,096-byte stdout limit, discarded stderr, cleared child environment and null stdin. Each probe gets its own process group; normal/nonzero root exit and timeout/error paths kill that group and reap the direct child before accepting output. Completion is observed with Linux waitid/WNOWAIT so the leader stays unreaped until its group is signalled; only then is the direct child reaped, with no second group signal. A nondefault/auto-reaping inherited SIGCHLD policy is refused before a probe is spawned. Typed libc bindings replace the hand-written ABI declaration; the already-locked libc 0.2.191 is now an explicit CLI dependency, with no new package/version or download. This is not containment of hostile executables or descendants deliberately escaping their group.
- PATH fallback searches known browser names, not credentials or the filesystem. AERO_BWRAP can explicitly select Bubblewrap; pkg-config is reported as a development prerequisite, not a runtime/compiler dependency.
- Executable integrity checking has a separate two-second caller deadline and a 1 GiB regular-file/actual-byte limit. Oversized/growing or timed-out reads make the pin unavailable, not matched. Hash workers check the deadline between reads; already blocked kernel I/O cannot be canceled, so a timed-out worker is not joined and ends when the one-shot doctor process exits. This does not create a general long-lived hashing service or hard real-time guarantee.
- Local version/hash matching is integrity comparison, not executable authenticity, full shared-library validation, sandbox proof, or a browsing launch. The future runtime still owns full browser descendant lifecycle, private mounts/profiles and renderer isolation; diagnostic group cleanup does not qualify C04 or close ENV-002/ENV-004.
- Namespace/sandbox/keyring checks remain explicitly `not_checked`. No system settings are changed, no profiles loaded, and no real user vault accessed.

## Native prerequisites and remaining qualification

The user has installed pkg-config. Follow-up checks returned version 2.5.1 at /usr/bin/pkg-config; doctor resolves its canonical executable /usr/bin/pkgconf and reports available=true. Bootstrap tests, formatting and clippy pass with it present. The earlier interactive-sudo installation blocker is resolved; no sudo password was accepted in chat and no shell startup file was changed.

For another Ubuntu development environment, provisioning commands are below; they do not need repeating on this host merely because this example remains in the guide:

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends pkg-config
pkg-config --version
```

ENV-003 now records actual tool availability; this does not establish compatibility of future native libraries/features. Do not install desktop, keyring, model or Flatpak stacks speculatively. Ubuntu's available chromium-browser candidate is a Snap transition package; do not mistake that wrapper for a directly mounted Linux browser binary. Native sandbox/user-namespace, display/IME/accessibility, and dedicated Secret Service checks require their actual later test environments. Never add `--no-sandbox`, change AppArmor/sysctls or weaken permissions to hide a failure.

## Git and review

Local `main` is linked to the private GitHub remote `git@github.com:dave2k77/aero-browser.git`. The user approved the initial bootstrap commit/push; root commit 5a4fac91e5638735aa1cd762c183f63c9efa39de records the reviewed baseline. Check actual HEAD/origin status for delivery; later batches require their own review and scoped approval. `.gitignore` excludes build/runtime data, credentials, `.hermes` session artifacts and the historical PDF pending rights/size review; Cargo.lock remains tracked. Browser provisioning remains outside the repository.
