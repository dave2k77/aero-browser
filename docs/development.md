# Development — BATCH-00 bootstrap

Observed on 2026-10-22 in `/home/davia/projects/aero-browser`. See [handoff.md](handoff.md) for the current reviewed checkpoint and [implementation-plan.md](implementation-plan.md) for remaining work. This guide does not claim a working delegated browser/runtime yet.

## Environment and scope

- Development host: Ubuntu 26.04.1 LTS under WSL2, x86_64.
- Release baseline remains native Ubuntu 24.04 x86_64; native X11/Wayland, keyring and sandbox compatibility have not been established by this bootstrap.
- `rustc 1.99.1 (58cf8f9c3 2026-10-13)` and `cargo 1.99.1 (5f94df478 2026-08-27)` installed with the official rustup minimal profile; rustfmt and clippy installed. `rust-toolchain.toml` pins 1.99.1, not a moving stable channel. No older MSRV is claimed.
- C compiler: Ubuntu GCC 15.2.0-16ubuntu1. Bubblewrap 0.11.1 already available. Node v24.18.0/npm 12.2.0 were present; no frontend is implemented or required for this batch.
- Current six crates are a scaffold, not completed CDP/perception/executor/agent subsystems. Only the core version/import contract and CLI prerequisite diagnostics are implemented.
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
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
cargo run -p aero-cli --locked -- --help
```

Metadata must contain exactly aero-core, aero-cdp, aero-perception, aero-executor, aero-agent and aero-cli. The CLI binary is named `aero`. Do not treat the empty scaffold libraries' zero-test outputs as subsystem acceptance. Focused integration tests must actually execute. Test/measurement counts and red/green evidence belong in the handoff, not guessed from this guide.

No provider, database, keyring, desktop GUI, model download, real browsing account, or Chromium launch is needed for these bootstrap tests. Doctor tests use explicitly synthetic executable fixtures and do not prove a browser sandbox.

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
- Probes only trusted local executable `--version` commands: canonical paths, two-second phase deadline, bounded stdout, discarded stderr, cleared child environment and null stdin. It reports typed failure reasons rather than dumping arbitrary command errors or inherited secrets.
- PATH fallback searches known browser names, not credentials or the filesystem. AERO_BWRAP can explicitly select Bubblewrap; pkg-config is reported as a development prerequisite, not a runtime/compiler dependency.
- Local version/hash matching is integrity comparison, not executable authenticity, full shared-library validation, sandbox proof, or a browsing launch. The future runtime must own descendant cleanup, private mounts/profiles and renderer isolation; doctor version probing does not substitute for it.
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

Local `main` is linked to the private GitHub remote `git@github.com:dave2k77/aero-browser.git`. There are no approved commits/pushes yet. Review exact files and real results before an explicitly approved scoped commit. `.gitignore` excludes build/runtime data, credentials, `.hermes` session artifacts, and the historical PDF pending rights/size review; it does not ignore Cargo.lock. Runtime/browser provisioning remains outside the repository.
