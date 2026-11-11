# AeroBrowser — implementation handoff

Document-set revision: 2.37. Current checkpoint: D05 (frame-aware grounded clicks), below — committed and pushed on 2026-11-10 at the owner’s “commit, push and then proceed” (take the SHA from Git); not independently reviewed. Everything through D05 is committed and pushed. Prior checkpoints below are historical where their next actions or test counts differ.

## Current checkpoint — D05 frame-aware grounded clicks (2026-11-10)

The owner approved the [D05 planning record](#d05-planning-record-approved-2026-11-10) with “yes, proceed.” Committed and pushed on 2026-11-10 as `fix(frames): correlate owners and normalize viewport coordinates` (the plan row's message) at the owner's “commit, push and then proceed”; take the SHA from Git.

Changed:

- New `crates/aero-cdp/src/geometry.rs` (pure): `axis_aligned` (moved from `target.rs`), `FrameLink`/`FrameLevel`/`FramePoint` and `frame_point`, which clips the target's fragments to every frame viewport on the path and the root viewport, maps an out-of-process frame's point into its parent's session as `owner content origin + point × (owner content size ÷ child innerWidth/innerHeight)`, takes same-process frames' quads as already in root coordinates, and returns the root point plus a frame-local point per level. Seven unit tests use the spike's measured numbers.
- `crates/aero-executor/src/target.rs`: `ground` accepts handles in child frames. Checks, each failing before any input: the target's frame is still in the graph (else `StaleDocument`) and, for a child frame, still shows the observed document by its own session's `Page.getFrameTree` (`StaleDocument`); every frame on the path to the main frame is known (`FramesChanged`); a fresh isolated world per frame; node connected (`NodeGone`); not disabled (read in the owning session); the node scrolled into view in its frame, then each owner into view in its parent; a presentation barrier (below); measured afterwards; every owner content box axis-aligned (new `UnsupportedFrameTransform`); each frame's visual viewport unscaled (`ViewportScaled`); a visible point after clipping at every level; a hit test at every level in that frame's isolated world and frame-local coordinates (the target frame hits the node or a descendant, each parent exactly the owner `<iframe>`; otherwise `Occluded`); the frame-graph version unchanged since the start (`FramesChanged`). New `GroundingError::FramesChanged`; frame-gone, detached and rebuilding-graph errors map to it.
- Dispatch is unchanged: exactly one move/press/release on the page session at the root point, routed by the browser's own hit testing. `ground_node` (typing, select, checkbox) still refuses child-frame handles with `UnsupportedFrame` until D06.
- Deviation from the approved plan, found by the red tests: a presentation barrier. After the scrolls, grounding awaits two `requestAnimationFrame`s in each local root on the path (main frame and each out-of-process frame, in their isolated worlds). Without it the calibration scenario's nested leaf failed 2 of 2 runs: once its move events reached the parent frame's `<iframe>` element instead, once the whole click did, although every isolated-world hit test had agreed on the leaf's button. A 500 ms pause before each click passed 3 of 3; with the barrier it passed 6, 3 and 5 further full-suite runs. Interpretation (not proven): grounding the earlier targets scrolled the main page, and the press raced the compositor's hit-test data for the nested out-of-process frame. The barrier is a frame-timing heuristic, not a proof that routing matches; there is still no post-dispatch check of where a press landed.
- Fixtures: `merchant.html` (decoy iframe first with an identically named button; cross-site `gateway.html` on `localhost`), `merchant-decoy.html`, `calibration.html` (same-process frame with border, padding and inner scroll; same-process `scale(0.5)`; a frame clipping half its button; cross-site frame with a nested `127.0.0.1` leaf; cross-site `scale(0.5)`), `calibration-inner.html`, `calibration-edge.html`, `calibration-cross.html`, `transformed.html` (rotated same-process and cross-site frames; a cross-site frame under a 1 % opaque overlay). Frames record pointer/mouse/click events per element id and click coordinates with the element's box; the gateway and inner frames break `elementFromPoint`/`contains` in the page world.
- Scenarios: `two_iframes_map_to_correct_owners` (owners read in the parent session; the gateway's button gets one full event sequence and the merchant and decoy none; then the decoy's button, and nothing else clicks; typing into the gateway is `UnsupportedFrame` with no events); `nested_frame_coordinate_calibration` (page scroll 120, frame scroll 50, six targets at device scale 1 and 2; each click lands once inside its button and within 2 px of its centre, except the clipped one, which must land in the visible half; no click events elsewhere); `transformed_frame_fails_closed` (both rotated frames `UnsupportedFrameTransform`, the covered frame `Occluded`, zero events in every frame). D04's `same_process_frame_is_observable` now expects the in-frame click to succeed and, after the iframe navigates, the old handle to be refused as `StaleDocument`.

Evidence (native host, Chrome 156 in the profiled launcher):

- Red: the seven geometry tests failed against a stub; the three new scenarios and the changed D04 scenario failed against the unchanged grounding (`UnsupportedFrame`). The `StaleDocument` assertion and the centre tolerance were added after the implementation and are evidenced by mutation only.
- Green: `cargo fmt --all -- --check`; clippy with `-D warnings` with and without `aero-agent/browser-tests`; workspace 325 passed (including 13 doctests); sandbox 11/11; scenarios 16/16 in five consecutive runs; no browser processes left afterwards.
- Mutation (throwaway script outside the repository): nine of ten killed — owner scale ignored, no clipping at frame levels, no owner hit tests, bounding box instead of the axis check, no presentation barrier (killed within three runs), non-click input allowed in frames, frame document not rechecked, target hit test at the root point, and frame layout size replaced by visible size (killed only after adding the 2 px centre assertion). Survivor: dropping the final frame-graph version re-check, a race guard no scenario triggers deterministically.

Limits: clicks only; scroll containers other than frame viewports, CSS `zoom`, iframes in shadow trees and `clip-path` are untested (clipping by an ancestor fails closed through the owner hit test, untested); the barrier never resolves in a frame that does not render, which ends as the call deadline's runtime error; [BUG-007](bug-fix-tracking.md#bug-007--device-scale-override-does-not-reach-cross-site-frames) (device scale override not reaching cross-site frames) is filed, not fixed; not independently reviewed.

Next: owner review of D05; meanwhile the G0/G1 critical path (E01–E02, N06–N08, N14, N22, F01 redaction guard) as agreed, not D06/D07.

## Previous checkpoint — host migration and native baseline (2026-11-10)

The owner asked to sync with the repository and plan the next steps, then said “Okay, let's proceed as you recommend”: re-provision this machine (pinned Rust and the pinned test browser), re-run the baseline, reconcile the status documents, then D05. After the baseline report the owner said the WSL2 environment “was default but no longer available. This linux environment will now be the default development environment. I want you to migrate the entire project to this linux env, sort the packages required, and make changes to the code to get things working in this env.” Everything listed under “Changed” below was committed on 2026-11-10 in three commits (fix, sandbox launcher, docs) and pushed at the owner's “yes”; take the SHAs from Git.

Git:

- This checkout is a copy of the 2026-10-22 state (af88105) on a new machine, `/home/davianc/Documents/projects/aero-browser`. Remote main was 24 commits ahead; local main was fast-forwarded to ddc1c40 with no merge commit. D04 (CH-17) is therefore committed and pushed as ddc1c40; the CH-17 section below was written before that commit.
- The copy had flipped 35 tracked files from mode 644 to 755 with no content change; the committed modes were restored before the fast-forward.
- This machine has no SSH key or `known_hosts`, so the SSH remote failed. The first sync used anonymous HTTPS. After the owner logged `gh` in (HTTPS protocol, keyring) `origin` was switched to https://github.com/dave2k77/aero-browser.git; fetch and push use those credentials. No Git identity is configured on this host, so these commits used the identity of all earlier commits (dave2k77 <davianc@gmail.com>) through per-command `git -c`, not a configuration change.
- The anonymous fetch succeeded, so the repository was publicly readable on 2026-11-10 although DEC-002 records it as private. Not changed here; the owner should confirm which is intended.
- Work left uncommitted on the WSL2 machine after ddc1c40, and the mutation scripts kept in that machine's scratch directory (`aero-d01-mutants.py` … `aero-d04-mutants.py`), are not on this machine.

Host (first native host; all earlier evidence is WSL2): Ubuntu 26.10, kernel 7.3.0-10-generic, x86_64, GNOME on Wayland; GCC 15.3.0, Bubblewrap 0.12.0, pkg-config 2.5.1, Python 3.14.9. This is not the Ubuntu 24.04 release baseline.

Provisioned with the owner's approval, both outside the repository:

- Rust 1.99.1 (`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, `cargo 1.99.1 (5f94df478 2026-08-27)`), minimal profile with rustfmt and clippy, through the snap-packaged rustup 1.29.2. No toolchain existed before, so rustup also made 1.99.1 its default.
- Chrome for Testing 156.0.8078.47 from the lock's download URL into `$HOME/.cache/aerobrowser/browsers/156.0.8078.47/`. The archive SHA-256 and the executable SHA-256 both equal the values in `packaging/browser-lock.json`; the archive was checked for absolute, parent-traversing and symlink entries before extraction and deleted afterwards. `--version` prints `Google Chrome for Testing 156.0.8078.47`.

Baseline at ddc1c40 on this host:

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo build --workspace --locked` exit 0. The contributor documentation checker passes (12 documents).
- `cargo test --workspace --locked`: 301 tests and 13 doctests pass, none failed or ignored, with `TMPDIR` set to a private directory under `target/`. With `TMPDIR` under `~/.hermes/cache` (mode 0775 here, umask 0002) 26 aero-agent unit tests fail with `UnsafePath`: the private-path checks refuse a group-writable, non-sticky ancestor, as designed. See [development.md](development.md#native-ubuntu-2610-host-2026-11-10).
- `cargo test -p aero-cdp --locked --test sandbox --features browser-tests`: 8 pass, 3 fail (`launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`). The browser exits 134 with Chromium's “No usable sandbox!”. `readiness_failure_reaps_child` passes.
- `cargo test -p aero-agent --locked --features browser-tests --test scenarios`: 0 pass, 13 fail, each at browser start with the same message. No sandbox or browser process remained afterwards.
- Cause, filed as [ENV-005](bug-fix-tracking.md#env-005--apparmor-denies-the-nested-chromium-sandbox-on-native-ubuntu): this host's AppArmor policy runs everything started by Bubblewrap under a profile that denies all capabilities, so Chromium cannot build its own namespace sandbox inside ours. Nothing was changed to work around it: no `--no-sandbox`, no AppArmor or sysctl edit.
- Pin helper: `bash scripts/lock-browser.sh "$AERO_CHROMIUM"` printed FAIL here because the debug-profile doctor did not finish hashing the 300,842,312-byte executable within its two-second limit (3 of 3 runs), while the release-profile doctor reported `matched_local_pin`. Filed as [BUG-006](bug-fix-tracking.md#bug-006--debug-profile-doctor-cannot-hash-the-pinned-browser-within-its-deadline) and fixed below.

Migration work after the owner's second instruction:

- Packages: everything the current code needs is installed (build-essential/GCC, pkg-config, bubblewrap, apparmor with `apparmor_parser`, util-linux, Python 3, git, curl, CA certificates). Not installed and not needed yet: `libdbus-1-dev` (vault feature, G01), `xvfb`, `apparmor-utils`. Nothing was installed with root; `sudo` needs the owner's password.
- BUG-006 fix: new test `integrity_hashes_a_browser_sized_file_within_the_deadline` in `crates/aero-cli/src/doctor.rs` (a sparse 320 MiB file must hash to its known digest inside the existing limit). `cargo test -p aero-cli --locked --bin aero integrity_hashes_a_browser_sized -- --nocapture` ran it once and failed with `TimedOut` after 2.00 s. Root `Cargo.toml` now builds `sha2` optimized in the dev profile (`[profile.dev.package.sha2] opt-level = 3`); the same command passed three times in 0.16–0.17 s, and `bash scripts/lock-browser.sh "$AERO_CHROMIUM"` prints PASS in the dev profile. `Cargo.lock` is unchanged; the two-second limit and the 1 GiB bound are unchanged.
- ENV-005 remedy, prepared but not applied: `packaging/apparmor/aerobrowser-bwrap` (the stock Bubblewrap policy, attached only to `/usr/local/libexec/aerobrowser/bwrap`, whose children keep capabilities inside their user namespaces) and `scripts/install-apparmor-profile.sh` (`install [GROUP]` / `remove`, root only). The profile compiles with `apparmor_parser --skip-kernel-load --skip-cache`; the installer refuses to run without root. Neither has been loaded or run with root, so whether Chromium's sandbox then starts is untested. No launcher or test code needed changing: the tests already take the Bubblewrap path from `AERO_BWRAP`.
- `scripts/dev-env.sh` (to be sourced) exports `AERO_CHROMIUM` for the pinned browser and `AERO_BWRAP` when the launcher is installed, and says what is missing.
- Gates after these changes: formatting, warning-denied Clippy and the build pass; `cargo test --workspace --locked` passes 302 tests and 13 doctests, none failed or ignored. Browser suites: see item 0 under Next (11/11 and 13/13, six runs each, after the profile was installed).

Limits: one run of each suite; no repeat runs or mutation passes on this host; browser suites passed six consecutive runs natively but no mutation pass was run here; display, input, accessibility and keyring remain unexercised (ENV-004). The BUG-006 test is a timing test: it passed with a twelvefold margin here, and a much slower machine could still miss the limit, which would be a true report for that machine.

Changed. Code and packaging: `Cargo.toml`, `crates/aero-cli/src/doctor.rs`, new `packaging/apparmor/aerobrowser-bwrap`, new `scripts/install-apparmor-profile.sh`, new `scripts/dev-env.sh`. Documentation: this section and the CH-17 heading and note below; the plan header and the ledger states of A04, B02–B06, N01–N04 and D04 (`ready_for_review` → `committed`, with their commit IDs); tracker ENV-005, DEC-014, BUG-006, updates to BUG-001–BUG-005, ENV-001, ENV-002, ENV-004 and DEC-002; the new host section in development.md. README.md and the CONTRIBUTING.md status paragraph still describe the 2026-10-28 state and were not touched.

Next:

0. Done 2026-11-10: the owner said “okay done. sudo tasks have been run.” Installed and verified 2026-11-10: the owner ran `sudo bash scripts/install-apparmor-profile.sh install`. Read back: `/usr/local/libexec/aerobrowser` is `root:davianc` mode 0750; the launcher is byte-identical to `/usr/bin/bwrap`; `/etc/apparmor.d/aerobrowser-bwrap` equals the repository file. A process started by the launcher is labelled `aerobrowser-bwrap//&aerobrowser-sandboxed (enforce)` and `unshare -U -r -m -p -f -n true` succeeds inside it; under `/usr/bin/bwrap` the label is still `bwrap//&unpriv_bwrap` and the same `unshare` still fails. With `AERO_BWRAP` set to the launcher, `cargo test -p aero-cdp --locked --test sandbox --features browser-tests` passed 11/11 (including `launch_reaches_cdp`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`, `readiness_failure_reaps_child`) and `cargo test -p aero-agent --locked --features browser-tests --test scenarios` passed 13/13, in six consecutive runs of each; no browser process remained. ENV-005 is verified.
1. Superseded owner action for [DEC-014](bug-fix-tracking.md#dec-014--running-the-nested-browser-sandbox-under-apparmor-user-namespace-restrictions): review the profile and run `sudo bash scripts/install-apparmor-profile.sh install`. Then `source scripts/dev-env.sh` and run both browser suites; ENV-005 closes only if `renderer_sandbox_remains_enabled` and the rest pass. If they do not, read the AppArmor denials (`sudo journalctl -k -g apparmor`) before changing the profile. D05–D07 and every other browser-backed package wait on this.
2. D05 frame owners and viewport coordinates, starting with its plan and spike as for D02 and D04 (`two_iframes_map_to_correct_owners`, `nested_frame_coordinate_calibration`, `transformed_frame_fails_closed`), once the sandbox starts here. Re-run D01–D04's repeat runs natively first; their mutation scripts would have to be rewritten.
3. Packages that need no browser can proceed on this host meanwhile; by the plan's dependency schedule those are E01–E02, N06–N08, N14 and N22.
4. Doctor AppArmor detection (§7.8), after the migration commits and push (main at 7233dd3), committed and pushed at the owner's “yes, commit and continue” (SHA in Git): `crates/aero-cli/src/doctor.rs` `user_namespace_check` reports `unrestricted`, `restricted` (with the launcher install command), `restricted_project_launcher` or `unknown` from `kernel.apparmor_restrict_unprivileged_userns` and the resolved Bubblewrap path. Three unit tests `userns_check_*` failed against a stub returning `not_checked` (status assertions), then passed. Real doctor on this host: `restricted_project_launcher` with the launcher, `restricted` with `AERO_BWRAP=/usr/bin/bwrap`. Workspace 305 tests + 13 doctests, fmt and warning-denied Clippy pass. Limit: the loaded-profile list needs root, so doctor recognises the launcher path but does not confirm its profile is loaded; `sandbox` stays `not_checked`.
5. Owner confirmations still pending: DEC-012 (report claims format), DEC-013 (observation text format), closure of BUG-001–BUG-005, review of the BUG-006 fix, and whether Ubuntu 24.04 stays the release baseline now that development is on 26.10.

## D05 planning record (approved 2026-11-10)

The owner said “yes, commit and continue” after the doctor change (committed and pushed as e004db0). Following the D02/D04 practice, a spike came first.

Spike (throwaway test `spike_d05.rs` and three fixtures, deleted afterwards; design input, not test evidence). Native host, Chrome 156 in the profiled launcher. The top page on `127.0.0.1` (scrolled 120 px) contained a decoy same-process iframe first, a same-process iframe with a 7 px border and 5 px padding (scrolled 50 px inside), a same-process `scale(0.5)` iframe, a same-process `rotate(10deg)` iframe, a cross-site OOPIF on `localhost` (border 3, padding 9, scrolled 50) holding a nested `127.0.0.1` leaf OOPIF, a cross-site OOPIF under a nearly transparent overlay, and a cross-site `scale(0.5)` OOPIF. Each frame logged received clicks.

- `DOM.getContentQuads` answers in the coordinates of the session's local root. For same-process frames that is the root viewport, already including page and frame scroll, owner borders and padding, and scaling. For an OOPIF it is the OOPIF's own viewport, and a nested OOPIF's owner box is reported in its parent OOPIF's viewport.
- Mapping an OOPIF point up one level as `owner content-box origin (DOM.getBoxModel in the parent's session) + point × (content width ÷ child innerWidth)`, repeated per local root, gave root points at which a page-session click landed exactly on the intended button: the plain OOPIF, the scaled OOPIF (scale 0.5) and the nested leaf (two steps). `DOM.getContentQuads` on an iframe returns its border box, not its content box.
- A page-session `Input.dispatchMouseEvent` at a root point is routed by the browser's own hit testing; the overlay received the click. The same click dispatched on the OOPIF's child session at frame-local coordinates reached the button underneath the overlay. Child-session input bypasses parent-page occlusion and must not be used.
- `elementFromPoint` takes frame-local coordinates; with root coordinates inside a same-process child frame it returned nothing. Occlusion must be checked at every level: the top document must hit the first iframe owner, each intermediate frame the next owner, and the target frame the node.
- A rotated iframe gives non-axis-aligned owner and content quads: fail closed.
- With `Emulation.setDeviceMetricsOverride` device scale factor 2 on the page session, same-process frames and the same-site leaf report `devicePixelRatio` 2 but the cross-site OOPIFs stay at 1. All quads stay in CSS pixels and every mapped click still landed. Not a coordinate problem; recorded as a candidate defect for screenshots (a BUG-005 sibling), to be filed when D05 is reviewed.

Proposed D05 (approved by the owner’s “yes, proceed.”; implemented as recorded in the current checkpoint):

1. `crates/aero-cdp/src/geometry.rs` (named in the plan row): pure, unit-tested frame-link geometry. Axis-aligned owner content boxes (`UnsupportedFrameTransform` otherwise), per-link scale, mapping a local-root point into its parent session, frame-local points for hit tests, and clipping the target box to every frame viewport and the root viewport along the path.
2. `crates/aero-executor/src/target.rs` `ground`: accept handles in child frames. Checks in order: the frame still exists and shows the observed document; the frame graph version is unchanged; node resolved in an isolated world of its own frame; not disabled; scrolled into view in its frame and each owner into view in its parent; measured afterwards; every owner box axis-aligned; a visible point after clipping at every level; hit test at every level in isolated worlds (exact owner elements, then the node or a descendant). New `GroundingError::UnsupportedFrameTransform`; occlusion anywhere on the path is `Occluded`.
3. Dispatch stays on the page session at the root point: exactly one move/press/release, as in D02. Typing, select and checkbox in frames stay refused (`UnsupportedFrame`) until D06 proves cross-site OOPIF input.
4. Fixtures `merchant.html` (decoy iframe first, with an identically named button) and `gateway.html` (cross-site on `localhost`), plus calibration fixtures as needed. Scenarios: `two_iframes_map_to_correct_owners` (the gateway button is clicked exactly once, zero events in decoy and merchant); `nested_frame_coordinate_calibration` (page and frame scroll, borders and padding, same-process and OOPIF `scale(0.5)`, nested OOPIF, a target half clipped by its frame, device scale 1 and 2; each click lands once inside the button in frame-local coordinates); `transformed_frame_fails_closed` (rotated same-process and rotated OOPIF frames refuse with zero events; an overlay over an OOPIF is `Occluded` with zero events). Each red against stubs first.
5. Not tested by the spike and to be settled by red tests: whether `DOM.scrollIntoViewIfNeeded` in an OOPIF scrolls its ancestors, and scroll containers other than the frame viewport.

## Previous chain batch CH-17 checkpoint (committed ddc1c40)

Committed and pushed as ddc1c40 (remote main read at that SHA on 2026-11-10). The text below was written before the commit.

The owner approved the D04 plan below (“yes, go ahead”), including using `DOM.getFrameOwner` for observation links in D04 and leaving coordinates, the decoy frame and input inside frames to D05. A session-lease interruption stopped the first attempt mid-implementation; the owner said “continue” and the work resumed from the uncommitted tree.

CH-17 — D04 frame and execution-context identity, WSL2 evidence (details in the D04 ledger row):

- `crates/aero-cdp/src/frame_manager.rs` (new): pure `FrameGraph` plus a per-page `FrameManager` (listener, OOPIF configuration and resume, non-iframe rejection, rebuild, auto-attach off on drop).
- `crates/aero-cdp/src/page.rs`: `frames`, `frame_call`, `frame_ax_tree`, `frame_owner`, `resync_frames`, `frame_resyncs`; `runtime.rs`: `RuntimeError::{FrameGone, FramesUnknown}`, `attach` starts the frame manager; `target_registry.rs`: module docs on the two attachment scopes.
- `crates/aero-perception/src/observation.rs`: multi-frame `observe_page` with `observable_frames`.
- `crates/aero-executor/src/target.rs`: `GroundingError::UnsupportedFrame`.
- Found while testing: Chromium rejects `Target.setAutoAttach` with a filter when disabling (“Target filter should be empty when disabling auto-attach”); the rebuild and drop paths now send no filter when disabling.
- Tests, repeat runs and mutation results: see the D04 ledger row (6 + 1 unit tests, 2 scenarios, 15/15 repeat runs, 30/35 mutants with the survivors explained).

Limits: frames without a committed document are omitted; any frame change during an observation refuses it; acting inside frames is D05; lag triggers a full rebuild; WSL2 only.

Next by plan order: D05 frame owners and viewport coordinates (`two_iframes_map_to_correct_owners`, `nested_frame_coordinate_calibration`, `transformed_frame_fails_closed`).

## D04 planning record

The owner approved committing D03 and proceeding. D03 was committed as d014e8c and pushed (remote main read back at the same SHA).

Spike (throwaway test and five fixtures, deleted afterwards; design input, not test evidence) — Chrome 156 in the Bubblewrap sandbox; a top page on `127.0.0.1` with a same-origin iframe and a cross-site iframe on `localhost` (same port) that itself embeds a `127.0.0.1` leaf iframe, plus a dedicated worker:

- `localhost` and `127.0.0.1` are different sites, so the cross-site frame and the nested A→B→A leaf both became out-of-process frames (iframe targets) without extra launch flags.
- `Target.setAutoAttach {autoAttach, waitForDebuggerOnStart, flatten, filter: [{type: "iframe"}]}` on the page session attaches each OOPIF with a child session (event `Target.attachedToTarget` on the parent's session, `waitingForDebugger: true`). Configuring the child (enable domains, set the same auto-attach recursively) and then `Runtime.runIfWaitingForDebugger` attached the nested leaf from the child session. The filter kept the dedicated worker unattached and running.
- `Page.getFrameTree` on a session lists only that process's local frames: the top tree held the main and same-origin frames; each child session's tree was rooted at its OOPIF frame with `parentId` naming the parent frame. The parent briefly reported the OOPIF frame via `frameAttached` and then `frameDetached` with reason `swap`.
- OOPIF target IDs equalled their frame IDs in this build; the plan forbids relying on that, so a child's frame comes from its own session's frame tree.
- `Runtime.executionContextCreated` carries `auxData {frameId, isDefault, type}` and a `uniqueId`; numeric context IDs are per session (a child session's first context was `1`, as the top's had been), so contexts are keyed by session and ID.
- Accessibility: the top tree's `Iframe` nodes have no children; `Accessibility.getFullAXTree {frameId}` on the top session returns a same-process child frame's tree; an OOPIF's tree comes from its own session.
- Ownership: `DOM.getFrameOwner {frameId}` on the parent frame's session returned the exact `<iframe>` backend node for the same-process child, the OOPIF and the nested leaf (the leaf's owner only on its parent's child session, not on the top session) — exactly the D01 `FrameLink` input.
- Navigating the same-process iframe changed only that frame's `loaderId`; its old backend node then failed with "Node with given id does not belong to the document".
- Removing the cross-site iframe gave, deepest first, `Page.frameDetached` (reason `remove`) and `Target.detachedFromTarget` on each parent's session, then browser-level `Target.targetDestroyed`.
- A call on an ended child session gets an unsessioned error reply, which `CdpConnection` reports as `InvalidMessage("reply from a different session")` (per request; the connection stays usable). Frame calls therefore need the same detach race as `Page` calls.
- Turning auto-attach off detached the existing OOPIF sessions; turning it on again re-attached them with new sessions and `waitingForDebugger: false` — a full resync path after lost events.

## Previous chain batch CH-16 checkpoint (committed d014e8c)

The owner said “yes, go ahead” to committing D02 and starting D03. D02 was committed as 5c272d5 and pushed (remote main read back at the same SHA). D03 followed the plan row directly (no spike: it reuses D02 grounding).

CH-16 — D03 Unicode text and control actions, uncommitted, WSL2 evidence:

- `crates/aero-executor/src/target.rs`: `ground_node` = `ground` plus the node's object in the isolated world; `call_on` awaits promises.
- `crates/aero-executor/src/dispatcher.rs`: `type_text`, `press_key`, `select_values`, `set_checked` and `InputError` (see the D03 ledger row for the ordered checks). Typing never sends pointer input; it focuses once, verifies the active element, clears with one Ctrl+A and one Backspace, inserts with `Input.insertText` and verifies the value after two animation frames. Enter is a dedicated key event sent only on request. `press_key` takes exactly the B02 named keys.
- Fixture `crates/aero-agent/tests/fixtures/forms.html` with a controlled input, refusing fields, a self-reverting select, a click-cancelling checkbox and a per-element event log.
- Tests: `named_keys_have_definitions_and_nothing_else_does`; scenarios `unicode_type_clear_and_enter`, `typing_does_not_double_click`, `select_and_checkbox_are_verified` (red against stubs first). 15 consecutive full scenario runs passed. Mutation pass (`/home/davia/.hermes/cache/scratch/aero-d03-mutants.py`): 22/22 caught after adding the self-reverting select.

Limits: main frame only; contenteditable, number/date inputs and IME composition unsupported; selects are changed in the isolated world with synthetic `input`/`change` events, not through the native popup; whether Enter may submit is the caller's authorization decision; WSL2 only.

Next by plan order: D04 iframe observation.

## Previous chain batch CH-15 checkpoint (committed 5c272d5)

The owner said “yes, go ahead” to committing D01 and starting D02, then approved the D02 plan (including the viewport fix and reading the plan's "geometry only for retained action candidates" as "measure only the target being acted on"). D01 was committed as a465acc and pushed (remote main read back at the same SHA).

CH-15 — D02 grounded clicks, uncommitted, WSL2 evidence:

- `crates/aero-cdp`: `BrowserRuntime::start(spec, limits, epoch)` keeps the B06 `RuntimeEpoch` (`epoch()`); `SandboxSpec::viewport()`; `BrowserRuntime::attach` applies it with `Emulation.setDeviceMetricsOverride` (BUG-005); `Page::main_frame() -> MainFrame { frame, document }`, the document epoch being the main frame's `loaderId`.
- `crates/aero-perception/src/observation.rs`: `observe_page(page, id, runtime)` reads `main_frame`, `Accessibility.getFullAXTree`, then `main_frame` again, refusing `DocumentChanged` in between; main frame only.
- `crates/aero-executor/src/target.rs`: `ground` and its ordered refusals (`Handle`, `StaleDocument`, `NodeGone`, `Disabled`, `NotRendered`, `ViewportScaled`, `UnsupportedTransform`, `OffViewport`, `Occluded`, `Runtime`), each before any input. The node is resolved into a fresh isolated world; the hit test (`elementFromPoint` on the node's root, then identity or `contains`) runs there, so page script replacing those primitives cannot fake it. `visible_point`/`axis_aligned` are pure and unit-tested.
- `crates/aero-executor/src/dispatcher.rs`: exactly one `mouseMoved`/`mousePressed`/`mouseReleased` (left, click count 1); failure before the press is `NotStarted`, after it `Uncertain` (never retried). `aero_executor::click_handle` = ground + click.
- Fixture `crates/aero-agent/tests/fixtures/click.html`: capture-phase per-element counters for pointer, mouse, click, dblclick, auxclick and contextmenu events plus a click log (target tag, coordinates, box at click time); hostile replacements of `Document.prototype.elementFromPoint` and `Node.prototype.contains`.
- Tests: geometry unit tests `grounding_uses_the_centre_of_an_axis_aligned_box`, `grounding_clips_to_the_viewport_and_skips_hidden_parts`, `grounding_rejects_transforms_missing_boxes_and_bad_replies`; scenarios `click_has_one_event_sequence`, `scroll_remeasures_target`, `stale_observation_cannot_click`, `occluded_target_is_rejected`. Red was observed in layers: unit tests and the viewport assertion against stubs, the scenarios first at the `observe_page` stub, then (after it was implemented) at the `ground` stub. Added after the implementation, so coverage only: the 2× page-scale refusal and the press landing on a filling `<span>` child. 20 consecutive full scenario runs passed (15, then 5 after the fixture change).
- Mutation pass (`/home/davia/.hermes/cache/scratch/aero-d02-mutants.py`, background, per-mutant sandbox cleanup): 18 defects, 17 caught. "Descendant hits refused" was missed at first because the plain button's centre lands on the button itself; the filled-child case now catches it. The survivor is `observe_page`'s mid-read navigation guard: a race no deterministic test reaches.
- Viewport finding filed as BUG-005 (tracker) and fixed here.

Limits: main frame only (D04/D05); the hit test and dispatch are separate protocol calls, so a page can still move the target between them; each grounding creates a new isolated world; WSL2 only (ENV-004); the `Uncertain` dispatch path is untested (it needs an injected protocol failure).

Next by plan order: D03 Unicode text and control actions (`unicode_type_clear_and_enter`, `typing_does_not_double_click`, `select_and_checkbox_are_verified`).

### D02 planning record

The owner said “yes, go ahead” to committing D01 and starting D02. D01 was committed as a465acc and pushed (remote main read back at the same SHA).

Spike (throwaway test and fixture, deleted in the same command; design input, not test evidence) — Chrome 156, a page with a plain button containing a `<span>`, a button under a translucent overlay, a button in a `rotate(30deg)` container, a disabled button, a button 2,000 px below the fold, capture-phase counters for pointer/mouse/click events, and page script replacing `Document.prototype.elementFromPoint` and `Node.prototype.contains`:

- `DOM.getContentQuads` is in root-viewport CSS pixels and changes after `DOM.scrollIntoViewIfNeeded` (the far button moved from y≈2008 to y≈588), so geometry must be measured after scrolling.
- A transformed element yields a non-axis-aligned quad; SPEC-19 requires rejecting unsupported transforms rather than clicking the quad centre.
- `DOM.getNodeForLocation` reported "No node found" for points that were visibly inside elements after scrolling, so it is not a reliable hit test here. `document.elementFromPoint` plus `contains` evaluated on the node resolved into an isolated world (`DOM.resolveNode` with that world's `executionContextId`) gave the right answer for every case, including the overlay (hit `cover`), and was unaffected by the page replacing both prototypes.
- One `Input.dispatchMouseEvent` move/press/release sequence produced exactly one `pointermove`, `mousemove`, `pointerdown`, `mousedown`, `pointerup`, `mouseup` and `click` on the target. On the covered button every event went to the overlay; on the disabled button pointer events fired but no `mousedown`/`click`.
- A removed node still resolves (`isConnected` false) and has no quads. After navigation the main frame's `loaderId` changes and old backend node IDs fail with "Node with given id does not belong to the document", so the main-frame `loaderId` can serve as the document epoch.
- The layout viewport was 1265×657 CSS px although the launcher passes `--window-size=1280,800`: the plan's 1280×800 root viewport default (§4) is not actually in effect.

## Previous chain batch CH-14 checkpoint (committed a465acc)

The owner said “yes, go ahead” to committing C06 and starting D01 with a plan first. C06 was committed as 8f7fb54 and pushed (remote main read back at the same SHA). With C01–C07 committed, Batch C is complete. The owner then approved the D01 plan with the provisional DEC-013 text format.

CH-14 — D01 bounded semantic observations, uncommitted, unit evidence (pure code, no browser):

- `crates/aero-core/src/types.rs`: `ElementHandle` — canonical `@e<n>` (`u32`, no sign, leading zeros or whitespace); parsing proves shape only.
- `crates/aero-perception/src/axtree.rs`: `parse_tree` reads one frame's `getFullAXTree` reply. Strict shapes (non-scalar values, non-boolean flags, non-integer level, non-string name are errors), at most `MAX_AX_NODES` = 50,000 nodes, duplicate node IDs, a missing parentless `RootWebArea`, a `childIds` entry naming no node and a node reachable twice are errors. An explicit stack walks DOM pre-order; ignored wrappers are skipped with their children re-parented to the nearest kept ancestor; `InlineTextBox` is dropped.
- `crates/aero-perception/src/observation.rs`: `observe(ObservationInput)` merges frames at `FrameLink { parent, iframe_backend_node_id, child }` (≤ `MAX_FRAMES` = 64 and ≤ `MAX_OBSERVED_NODES` = 200,000; unknown root, duplicate frame, link to an unknown frame, a frame linked twice or as root, and one iframe linked twice are errors; unlinked frames are left out and not listed). Classification: controls get handles only with a DOM node and outside a native select (whose options are context); `StaticText` is dropped under a control, a named heading/cell or a label; headings keep their level; tables, rows, lists, dialogs, named images and iframes are rendered as context or structure; other roles are walked through. Rendering, selection and handle assignment follow DEC-013 (see the tracker): escaped quoted strings, 256-scalar cuts flagged `truncated`, 16 KiB with a 96-byte trailer reserve, priority focus/dialog → controls → nearby context (neighbours and the nearest heading above a control) → rest, `[omitted N controls and M other lines]`, handles assigned after selection so text and map agree. `HandleMap::resolve` rejects malformed handles, another observation, another runtime epoch and unissued handles; targets carry frame, document epoch, backend node ID, role and the full (uncut) name.
- Tests: `element_handle_shape` and nine `axtree_*` tests (three parser, six observation). All were observed red against stubs; the observation tests first failed only because the parser was a stub, so the parser was implemented first and they were observed failing against their own `observe` stub before it was implemented. Mutation pass (`/home/davia/.hermes/cache/scratch/aero-d01-mutants.py`): 27 defects. The first full run caught 23; the four misses were real test gaps (short focus/dialog lines squeezed into leftover space, a direct child hid the dialog-ancestry defect, page order hid the nearby-context priority, and handle-width slack hid the trailer reserve), fixed by 128-byte lines, a wrapper inside the dialog, context placed after 400 paragraphs and a 300-line text-only case. Rerun: 27/27 caught, sources verified restored.
- Process note: two earlier mutation runs were killed from outside (tool time limit, then an outer `timeout`) during a mutant that loops forever on a cyclic tree; the kill skipped cleanup and left a mutant in `types.rs` and then `axtree.rs`, both restored from pre-run backups and verified by `diff`. The script now runs each test in its own process group with a 120 s deadline and an 8 GiB address-space limit, restores on SIGTERM/SIGINT/SIGHUP, and refuses to start while an interrupted run's marker exists.
- `Cargo.lock`: aero-perception gains its `serde_json` dependency line; no new packages.

Limits: no real `RuntimeEpoch`/`DocumentEpoch` producer yet (callers supply them; D02/D04); per-frame tree fetching and iframe-to-frame linking are D04/D05; no browser-backed observation test yet (the spike below shaped the parser); DEC-013 is pending owner confirmation; nothing feeds observations into evidence capture yet.

Next by plan order: D02 grounded click (`click_has_one_event_sequence`, `scroll_remeasures_target`, `stale_observation_cannot_click`, `occluded_target_is_rejected`).

### D01 spike findings

Spike (throwaway test and fixture, deleted in the same command; design input, not test evidence) — `Accessibility.getFullAXTree` from Chrome 156 on a fixture with heading, paragraph+link, labelled/disabled/readonly inputs, a native and a `role=button` div, an `aria-hidden` and a `display:none` block, a table, a select, a checkbox and a same-origin iframe:

- Nodes arrive breadth-first, not in DOM order; DOM order must come from walking `childIds` from the `RootWebArea`.
- `aria-hidden`/`display:none` content is absent; layout wrappers are `ignored: true` (`role: none`, reason `uninteresting`) but still carry `childIds` that must be walked through.
- `InlineTextBox` children have negative node IDs and no `backendDOMNodeId`; every `StaticText` repeats the text of its heading/link/button/cell parent, so naive output doubles every label.
- States come as properties: `disabled`, `readonly`, `checked` ("true"/"false"/"mixed", a string), `level`, `selected`, `expanded`, `focusable`, `focused`, `required`, `settable`, `url`; form values come in `value` (textbox `"v"`, combobox `"B"`).
- Roles include Chromium-internal ones (`LabelText`, `generic`, `MenuListPopup`, `Iframe`, `RootWebArea`). The iframe appears as an `Iframe` leaf with no children: each frame's tree must be fetched separately (`frameId` parameter), so the frame merge is a real step.

## Previous chain batch CH-13 checkpoint (committed 8f7fb54)

The owner said “yes, go ahead” to committing C05 and starting C06. C05 was committed as bcc9961 and pushed (remote main read back at the same SHA).

CH-13 — C06 bounded page evaluation and lifecycle waits, uncommitted, WSL2 evidence:

- `crates/aero-cdp/src/evaluate.rs`: `Runtime.evaluate` with `returnByValue` and `awaitPromise`. `interpret` turns `exceptionDetails` (thrown value or rejected promise) into `EvaluateError::Exception` (the exception description, else the text; truncated to `MAX_EXCEPTION_CHARS` = 1024, untrusted), `unserializableValue` into `Unserializable`, a missing result/value into `Cdp(InvalidMessage)`, `undefined` into `null`, and a JSON result above `max_result_bytes` (default 1 MiB) into `ResultTooLarge`. `EvaluateOptions.deadline` (default 10 s) bounds the whole operation including retries. A destroyed context is `ContextDestroyed` unless the caller sets `context_destroyed_retries` (for idempotent expressions only), and only four exact protocol messages count as context loss; every other error propagates.
- `Page::evaluate`/`evaluate_with`, and `Page::navigate_and_wait(url, &NavigationLimits)` → `NavigationOutcome { same_document, url, status, stable }`. It subscribes before enabling Page, lifecycle events and Network and before navigating; `Page.navigate` without `loaderId` is a same-document change; otherwise it waits for this loader and frame's `DOMContentLoaded` and records this loader's `Document` response status. Failures are errors, not success: `errorText` → `NavigationFailed`, 4xx/5xx → `HttpStatus`, missed navigation deadline (default 30 s) → `NavigationTimeout` after a best-effort `Page.stopLoading`. Missed events during the wait are an error (no silent continuation). The reported URL is the current navigation-history entry.
- Stability runs in an isolated world (`Page.createIsolatedWorld`), so page scripts cannot replace its timers, animation frames or observer: after DOMContentLoaded, 200 ms with no DOM mutation, then two animation frames; the cap (default 5 s) yields `stable: false`, never an indefinite wait. Network idle is deliberately not a condition, so long-lived requests and sockets cannot hold it open. A context destroyed during the stability wait gets one fresh attempt, then `stable: false`.
- C05 defect found and fixed here: under repetition `page_target_navigation` failed 2 of 15 runs because a call sent just before the tab closed was never answered and waited the full 10 s. `RegistryState` now counts ended sessions (`detachments`), the listener publishes them on a `watch` channel, and every `Page` call races its request against `until_detached`, returning `Detached` promptly. The scenario now closes the tab while a never-settling call is in flight and requires `Detached` within 3 s. After the fix: 0 of 20 and 0 of 15 targeted repetitions failed. `BrowserRuntime::registry_failure()` exposes why the registry stopped, if it has.
- Harness: the fixture server accepts `/hold` and never answers it; new fixtures `dynamic.html` (mutates every 50 ms and fetches `/hold`) and `tampered.html` (replaces `setTimeout`, `clearTimeout`, `requestAnimationFrame` and `MutationObserver`).
- Tests: unit tests `values_and_undefined_are_returned`, `exceptions_are_errors_not_null`, `non_json_large_and_malformed_results_are_errors`, `only_context_loss_is_retryable` (observed red against stubs, then green); scenarios `evaluate_exception_is_error` and `navigation_and_stability_deadlines` (observed red against stub `navigate_and_wait`, then green) as listed in the plan ledger; five full scenario runs passed after the race fix was applied in the test. Mutation pass: 18 defects, 17 caught; the survivor (skipping `evaluate`'s up-front ownership check) is equivalent at the API level because the detach race resolves immediately for an unowned session.
- Gates: workspace 293 passed (289 + 4), 0 failed/ignored; aero-cdp + aero-agent with browser features and the browser: 205 passed; warning-denied Clippy (workspace, and both crates with browser features), fmt, warning-denied rustdoc, the documentation checker and `git diff --check` pass; Cargo.lock unchanged. Not independently reviewed.

Limits: WSL2 only (ENV-004). Blocked pages (DNS/policy blocks) beyond a refused connection are not exercised separately; history (`pushState`) SPA changes are not driven by `navigate_and_wait` (it reports fragment navigations it initiates); cross-document navigations started by the page itself are not awaited; no frames/OOPIF (D04). Downloads and dialogs during navigation are out of scope here.

Next by plan order: D01 observation capture (planned in the current checkpoint).

## Previous chain batch CH-12 checkpoint (committed bcc9961)

The owner said “yes, go ahead” to committing C07 and starting C05. C07 was committed as e538e20 and pushed (remote main read back at the same SHA).

CH-12 — C05 page target attachment and navigation, uncommitted, WSL2 evidence:

- `crates/aero-cdp/src/target_registry.rs`: `TargetRegistry::start` subscribes first, spawns the owned listener, enables `Target.setDiscoverTargets`, then merges a `Target.getTargets` snapshot. `RegistryState` is pure (events/snapshots in, targets out) and applies only browser-level `Target.*` events. A snapshot is authoritative only for targets known before its request (a sequence mark), because an event for a newer target can be applied before the older snapshot reply; destroyed target IDs are remembered and never re-added. On `EventLag` the listener resyncs from a fresh snapshot (counted in `resyncs()`); a failed resync or a closed stream stops it with a recorded `failure()` instead of continuing on a stale map. Snapshots also drop our session for a target the browser no longer reports attached, since detach events may be missed.
- Tabs: `TargetKind::Page` whose URL is not `devtools://`. Workers (`worker`, `shared_worker`, `service_worker`), iframes and Chromium UI (`browser_ui`, …) are discovered but are not tabs.
- One attachment authority: `attach` is the only path, explicit `Target.attachToTarget` with `flatten: true`, refused for unknown targets, non-tabs and targets already attached or being attached; the in-flight reservation is released by a guard on failure or a dropped future; a target destroyed during the call is reported, not recorded. Nothing enables auto-attach.
- `crates/aero-cdp/src/page.rs`: `Page::call` always carries the page's session and returns `Detached` once the registry no longer owns it; `navigate` returns `NavigationFailed(errorText)` for a navigation the browser rejects. Dropping a `Page` does not detach it (G05 owns detach/close/new tabs). Load/lifecycle waiting is C06.
- `crates/aero-cdp/src/runtime.rs`: `BrowserRuntime::start` (launch → connect → registry), `connection()` (browser level only), `targets/tabs/target/resyncs`, `wait_for_tab`, `attach`, `shutdown` (registry, connection, then pidfd-verified process). `RuntimeError` covers launch, CDP, attach, detached, navigation and no-tab failures.
- Harness: `BrowserHarness`/`Fixture` now own a `BrowserRuntime` (`fixture.runtime()`, `fixture.cdp()`); new fixture `crates/aero-agent/tests/fixtures/second.html`.
- Tests: registry unit tests `events_maintain_targets_in_discovery_order`, `tabs_exclude_workers_frames_ui_and_devtools`, `snapshots_are_authoritative_but_never_revive_destroyed_targets`, `detach_and_destroy_end_our_session` (observed red against stubs, then green) and `targets_discovered_after_the_snapshot_request_are_kept` (added with the implementation; coverage, not red/green). `page_target_navigation` (observed red against a stub `BrowserRuntime::start`): startup tab enumerated without creating one; every non-tab target excluded from tabs and a discovered non-tab refused (`NotATab`); second attach refused; a session distinct from the target ID; browser-level `Runtime.evaluate` rejected by the browser; two navigations through the page session change the real `document.title`/`location.href` and the registry follows the URL; navigating to a closed loopback port is `NavigationFailed(net::ERR_…)`; zero resyncs; closing the tab makes the page return `Detached` and removes the target. Five consecutive scenario runs passed. Mutation pass: 13 defects (session-scoped events applied, destroyed IDs revived, snapshot mark ignored, snapshot removal skipped, detach ignored, devtools pages as tabs, second or non-tab attach, unflattened attach, listener ignoring events, page call without session, ownership check skipped, navigation errors ignored) — all caught. Subscribe-before-enable ordering was not mutation-tested (the initial snapshot would partly mask it).
- Gates: workspace 289 passed (284 + 5), 0 failed/ignored; aero-cdp + aero-agent with browser features and the browser: 199 passed; warning-denied Clippy (workspace, and both crates with browser features), fmt, warning-denied rustdoc, the documentation checker and `git diff --check` pass; Cargo.lock unchanged. Not independently reviewed.

Limits: WSL2 only (ENV-004). Lag resync is unit-reasoned but not exercised against a real browser. No same-document/SPA navigation, load waiting or stability (C06); no frames/OOPIF sessions (D04); no detach, tab creation/close or popup ownership API (G05).

Next was C06 (CH-13 above).

### Earlier: chain batch CH-11 checkpoint (committed e538e20)

The owner said “yes, go ahead” to committing C04 and starting the next package. C04 was committed as 93c975d and pushed (remote main read back at the same SHA). The plan's Batch C order is C01–C04, then C07, then C05–C06, so the next package was C07 (fixture harness), not C05 as the previous report proposed.

CH-11 — C07 deterministic mandatory fixture harness, uncommitted, WSL2 evidence:

- Correction to committed C04: its browser tests printed a skip notice and passed unless `AERO_REQUIRE_BROWSER=1` was set, contrary to §6 ("No `return`/`Ok(())` escape on unavailable prerequisites") and SPEC-30. With `--features browser-tests` a missing `AERO_CHROMIUM` now fails all four (observed: 4 failed with the expected message).
- `crates/aero-agent/tests/support/fixture_server.rs`: test-only static server on tokio (already locked; aero-agent dev-dependency features `io-util, macros, net, rt, rt-multi-thread, sync, time`, no new crates) instead of Axum. Serves `tests/fixtures/<name>.html` for names `[a-z0-9-]+.html` only (404 otherwise, including `..` paths that would lead back to a real fixture), GET/HEAD (405 otherwise), 8 KiB/5 s request-head bound, `Connection: close`, request log. The accept loop owns a `JoinSet` of connections; `shutdown` stops and joins within 5 s (then aborts and errors); drop aborts.
- `crates/aero-agent/tests/support/browser.rs`: `BrowserHarness::launch` takes B06 private directories (`StateConfig::resolve` in a 0700 temp home → `acquire_task` → `create_runtime`), so C03's run-directory contract is exercised against the real B06 layout, then the C04 launcher. Missing `AERO_CHROMIUM`/Bubblewrap is an error the test reports. Field order drops the browser (blocking, pidfd-verified) before its directories and lease; `shutdown` runs teardown via `spawn_blocking` and requires `sandbox_emptied`.
- `crates/aero-agent/tests/support/mod.rs`: `Fixture { cdp, browser, server }` (drop order connection → browser → server; no async work in `Drop`), `serial()`, `wait_until` (event predicate; lag or a closed stream is an error, never a stale continue), `eventually` (bounded polling for state without a reliable event), `http_get`, `refuses_connections`, `sandbox_processes`. `crates/aero-agent/tests/fixtures/basic.html`. Other §5 fixtures arrive with their consumer packages.
- `fixture_harness_smoke` (`crates/aero-agent/tests/scenarios.rs`): observed red against a stub `Fixture::start`, then green. Asserts the server answers and refuses traversal/uppercase names; the browser answers CDP, creates a page that reaches the fixture URL (event) and the fixture title (polled `Target.getTargets`), and the server logged `GET /basic.html`; shutdown is verified; then a spawned scenario that starts a fixture and panics still leaves no sandbox process and a closed server port.
- Finding for C05: Chrome 156 did not emit `Target.targetInfoChanged` for the title change of an unattached page within 10 s (only the URL placeholder title), although `Target.getTargets` showed the loaded title. Page state must come from an attached session or polling, not that event.
- Mutation pass on the harness: name validation (caught after replacing a traversal path that the `.html` suffix rule already refused), browser leaked on drop (caught); server abort on drop is defence in depth — dropping the stop sender also ends the loop; removing both was first observed as a hang (unbounded shutdown join), so `shutdown` is now bounded and that double mutant fails cleanly. Not exercised by any test: `wait_until` lag handling and the harness's report of an unverified teardown (the underlying pidfd check is unit-tested in aero-cdp). A timed-out mutant run left a stray test binary, which was found and killed; no sandbox processes remained.
- Gates: workspace 284 passed (scenarios compile to no tests without the feature), 0 failed/ignored; aero-cdp + aero-agent with both `browser-tests` features and the browser: 193 passed; five extra smoke runs passed; warning-denied Clippy for the workspace and for both crates with browser features, fmt, warning-denied rustdoc, the documentation checker and `git diff --check` pass; Cargo.lock unchanged. Not independently reviewed.

Limits: WSL2 only (ENV-004). The harness is serial (one browser at a time). The server is HTTP/1.0-style one request per connection; no TLS, cookies or cross-origin fixtures yet (cross-site OOPIF scenarios need a second origin, e.g. another loopback port, when D04 arrives). `scripts/test-browser.sh` (the complete required browser suite on prepared native Linux) does not exist yet.

Next: C05 target attachment and navigation (`page_target_navigation` in `crates/aero-agent/tests/scenarios.rs`), then C06.

### Earlier: chain batch CH-10 checkpoint (committed 93c975d)

The owner said “yes, go ahead” to committing C03 and starting C04. C03 was committed as 34fca13 and pushed (remote main read back at the same SHA).

CH-10 — C04 owned browser startup and cleanup, uncommitted, WSL2 evidence only:

- Feasibility first (throwaway, not kept): the pinned Chrome for Testing 156.0.8078.47 in the C03 sandbox wrote `DevToolsActivePort` in about 0.9 s; Chromium's zygote, renderer and storage service ran in nested PID namespaces, renderer/storage with `Seccomp: 2`; stderr held only D-Bus connection errors (no bus is reachable). The network service runs with `--service-sandbox-type=none` and the GPU process unfiltered — Chromium's own headless defaults, not a flag of ours; recorded for the security review.
- `crates/aero-cdp/src/runtime.rs`: `BrowserProcess::launch(&SandboxSpec, LaunchLimits)` → `endpoint()` (`ws://127.0.0.1:PORT/devtools/browser/UUID`), `stderr_tail()`, `shutdown() -> ShutdownReport { status, sandbox_emptied }` (blocking; use `spawn_blocking`), teardown on drop. `LaunchError`: `StaleProfile` (nothing spawned or deleted), `Spawn`, `SandboxNotStarted` (no namespace init within 2 s), `EarlyExit { code, signal, stderr }`, `ReadinessTimeout { stderr }`, `TeardownIncomplete` (overrides the cause when an observed sandbox could not be verified empty). Every failure tears down before returning.
- Teardown: SIGKILL the namespace init through its pidfd (cannot hit a reused PID), then kill and reap Bubblewrap, then wait for the pidfd. The kernel completes the init's exit only after every process in its namespace, including Chromium's nested namespaces, has exited, so a readable pidfd proves the sandbox is empty.
- Defect found by the tests and fixed: the first implementation killed only Bubblewrap and relied on `--die-with-parent`. `readiness_failure_reaps_child` (1 ms deadline) reported `TeardownIncomplete`, and 15 Chromium processes really survived under an orphaned namespace init reparented to PID 321 — the init arms its parent-death signal only after it starts. The leftover sandbox was killed by its init only (after checking its command line and `NSpid`), and teardown now kills the init through the pidfd. 5 full-suite runs and 20 runs of that test since: no failures, no leftovers.
- Port-file handling: the profile is writable from inside the sandbox, so `DevToolsActivePort` is opened with `O_NOFOLLOW | O_NONBLOCK`, must be a regular file of at most 4096 bytes, and must match `PORT\n/devtools/browser/<lowercase UUID>` exactly (a torn write cannot validate). Liveness is rechecked after a valid read.
- Tests: `crates/aero-cdp/tests/sandbox.rs` — `host_probe_reports_this_hosts_runtime`, `stale_devtools_port_file_is_refused_before_spawning` (default suite) and, with `--features browser-tests`, `launch_reaches_cdp` (Browser.getVersion reports Chrome/156), `readiness_failure_reaps_child` (timeout and early exit, nothing left), `sandbox_cannot_read_host_canary` (a synthetic canary and its directory listing outside the mounts stay unreadable while a positive-control file in the staging mount is readable), `renderer_sandbox_remains_enabled` (`chrome://sandbox` says "You are adequately sandboxed"; independently every renderer has `Seccomp: 2` in a PID namespace other than the browser's). Runtime unit tests: strict port-file parsing, symlink/FIFO/size refusal, bounded redacted stderr, teardown claims empty only after the init exits (a pidfd on PID 1 stands in for an init that cannot be killed; skipped as root). Red/green: the unit tests and the browser tests (after the probe was implemented) failed against stubs, then passed; the first half of `readiness_failure_reaps_child` passed vacuously against the stub, its early-exit half was red. Mutation pass with leftover cleanup between mutants: 13 defects, all caught after two gaps were closed (ring trimming across pushes; teardown claiming empty without waiting). An earlier mutation run was discarded because the first mutant's leftovers contaminated the others.
- `AERO_REQUIRE_BROWSER=1` without `AERO_CHROMIUM` fails all four browser tests; without the flag they print a skip notice and pass.
- Dependencies: `libc` and `tempfile` (both already locked) now listed for aero-cdp; Cargo.lock changes only that dependency list.
- Gates: workspace 284 passed (278 + 6), 0 failed/ignored; `cargo test -p aero-cdp --features browser-tests` with the browser 32 passed; warning-denied Clippy for the workspace and for aero-cdp with all features, fmt, warning-denied rustdoc, the documentation checker and `git diff --check` pass. Not independently reviewed.

Limits: WSL2 only — native Ubuntu 24.04 with AppArmor's unprivileged user-namespace restriction is untested (ENV-004) and may need the distribution's Bubblewrap profile. Doctor still reports `runtime_ready=false` and does not run these checks. Teardown is SIGKILL only (no graceful `Browser.close`), so profiles are not cleanly shut down; per-run profiles make that acceptable for now. The browser executable is not re-hashed against `packaging/browser-lock.json` at launch. Stderr redaction removes URL queries/fragments and control characters only (F01 owns full redaction). Host network sharing remains reported, not isolated.

Next: C05 target attachment and navigation (one auto-attach authority, page sessions distinct from the browser session).

### Earlier: chain batch CH-9 checkpoint (committed 34fca13)

The owner (returning from Telegram-driven work in another session) said: “check the changes thoroughly and if all is well push and move on to the next task.” CH-8 was reviewed (see its review bullet below), committed as de6c2e0 and pushed with a540588; remote main read back at de6c2e0.

CH-9 — C03 pure sandbox command construction, uncommitted:

- `crates/aero-cdp/src/launcher.rs`: `SandboxSpec::build(&SandboxRequest) -> Result<SandboxSpec, SandboxError>`. Inputs are resolved facts only (canonical bwrap and Chromium executables, the B06 run directory, a `HostLayout` of `lstat` kinds for `/usr`, `MERGED_USR_PATHS`, `ETC_RUNTIME_PATHS`, a `Display`, viewport, locale); nothing is read from the filesystem. Output: program, ordered args, the host `DevToolsActivePort` path (`<run>/profile/DevToolsActivePort`) and `isolation_limits()` (`HostNetwork` always, `X11Display` for X11).
- Policy as built: user/PID/IPC/UTS namespaces, `--die-with-parent`, `--new-session`, `--cap-drop ALL`; `--clearenv` before `HOME=/home/aero`, `PATH=/usr/bin:/bin`, `LANG`; `/usr` read-only; merged-/usr links recreated with `--symlink` only if their normalized target resolves into `/usr` (directories bound; files, foreign or `..` targets refused); listed `/etc` runtime paths mounted only if present (a bind follows a symlinked source such as resolv.conf on the host); `--proc /proc`, `--dev /dev`, then tmpfs `/dev/shm`, `/tmp`, `/home/aero`; run `profile` and `staging` bound writable at `/home/aero/profile` and `/home/aero/staging`; Chromium's directory read-only at `/opt/aero/browser`; display mounts last (Wayland socket at `/run/aero/wayland-0` in a private tmpfs; X11 socket inside the private `/tmp` and authority at `/home/aero/.Xauthority`); `--chdir /home/aero`. Browser flags: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=0 --user-data-dir=/home/aero/profile --no-first-run --no-default-browser-check [--headless] --window-size=W,H about:blank`; no sandbox-weakening flag is ever produced and callers cannot add flags.
- Refusals: relative/unnormalized/NUL/root paths, Snap executables, an executable directly in `/`, a run directory equal to, inside or containing any read-only source (`/usr`, `/etc/...`, browser dir, display paths), viewport outside 1..=16384, locale outside `[A-Za-z0-9_.@-]{1,64}`, relative display paths.
- Design choice: the builder takes the run directory and derives `profile`/`staging` (matching B06 `RuntimeDirs`), so it cannot be handed an arbitrary writable host directory. The browser mount is the executable's parent directory, so an executable placed directly in a broad directory would expose that directory read-only; C04 must pass the pinned, hashed browser from `packaging/browser-lock.json`.
- Tests: `crates/aero-cdp/tests/sandbox.rs` — `command_policy`, `split_usr_directories_are_bound_and_missing_paths_skipped`, `unsupported_host_layouts_are_refused`, `unsafe_paths_are_refused`, `display_modes_expose_only_their_socket`; the test parser panics on any Bubblewrap option it does not know, so a new option (e.g. `--dev-bind`, `--bind-try`) fails the suite. All five observed red against a compiling scaffold (behavioral failures), then green; one assertion of mine was wrong (`--user-data-dir=/` matched the correct profile flag as a prefix) and was replaced by an exact single-profile check. Mutation pass: 22 defects, all caught.
- One-off smoke, deleted afterwards and not part of any gate: probing the real host layout and running the generated command through local bwrap 0.11.1 with `/usr/bin/true` as the browser exited 0 with empty stderr — the mount plan is accepted on this WSL host. This is not C04 proof (no Chromium, no renderer sandbox, no canary).
- Gates: workspace 278 passed (273 + 5), 0 failed/ignored; warning-denied Clippy, fmt, warning-denied rustdoc, the contributor documentation checker and `git diff --check` pass. No new dependency. Not independently reviewed.

Limits and notes for C04: no `/sys`, `/etc/passwd` or GPU device access is provided — C04 measures what Chromium actually needs and adds only that, with tests; whether Chromium's own namespace sandbox works nested in this user namespace is C04's `renderer_sandbox_remains_enabled`. Spawn bwrap with an empty environment. Host network sharing includes abstract Unix sockets, so a compromised renderer that escaped Chromium's sandbox could reach an abstract X11 socket even in headless mode; network/abstract-socket isolation remains future work and is reported via `IsolationLimit::HostNetwork`.

Next: C04 real launch (`launch_reaches_cdp`, `readiness_failure_reaps_child`, `sandbox_cannot_read_host_canary`, `renderer_sandbox_remains_enabled`) using the pinned Chrome for Testing 156.0.8078.47 build.

### Earlier: chain batch CH-8 checkpoint (committed de6c2e0)

The owner said “proceed to commit and move on to the next task”. CH-7 was rerun (workspace 256 passed, Clippy/fmt/rustdoc clean) and committed as a540588 — not pushed, because only a commit was requested. DEC-012 (claims format) remains pending; it blocks report generation, not the browser runtime.

CH-8 — C01/C02 CDP WebSocket transport, uncommitted:

- `crates/aero-cdp/src/connection.rs`: `CdpConnection::connect(endpoint, TransportLimits)` accepts only `ws://` on a literal loopback IP with an explicit nonzero port and no credentials (`localhost` is refused: name resolution is not trusted), with a handshake deadline and inbound message/frame size limits. One reader and one writer task, owned and joined by `close()`. `call(method, params, session, deadline)` routes by numeric ID; a reply carrying a different `sessionId` than the request fails that request with `InvalidMessage`; `error` objects become `Protocol {code, message}`; a reply with neither `result` nor `error` is `InvalidMessage`, never `null`; unknown/late IDs are ignored. Events (`subscribe()` → `EventStream`) carry method, params and session; lag is `EventLag(n)`; streams end with `Disconnected` when the connection ends.
- Cleanup: the pending map is locked only to insert/remove/drain; an RAII guard removes the entry on reply, timeout and dropped futures. Oversized requests are refused before sending (`TooLarge`). Any end (EOF, close frame, transport error, oversized inbound frame, writer failure) records the first reason and fails every pending call with it; later calls fail immediately. `close()` → `Cancelled`, close frame, bounded join (abort after 2 s). Dropping the connection signals both tasks and sends a close frame.
- `crates/aero-cdp/src/error.rs`: `Timeout`, `Disconnected`, `Protocol`, `InvalidMessage`, `EventLag`, `Cancelled`, plus `InvalidEndpoint` and `TooLarge`.
- Tests (`crates/aero-cdp/tests/transport.rs`, 17, local WebSocket peer on port 0): C01's seven observed red against a stub (six behavioral failures; the endpoint test passed vacuously), then green. C02's eight new tests observed red (eight failures; `late_reply_is_ignored` passed as a regression), then green. The stream-end assertions in `disconnect_fails_all_pending` and `slow_subscriber_reports_lag_instead_of_silently_skipping` were written after the implementation; six mutants (no pending guard, no event close, lag as disconnect, no session check, no size check, reader not ending) were run: all caught except the reader one, which the writer's own end path covers — removing both is caught by two tests. 20 repeated suite runs passed.
- Dependencies: workspace `tokio-tungstenite = 0.30.0` (default features off; `connect` only, no TLS) and `futures-util 0.3.34` (sink/std); aero-cdp enables tokio `macros, net, rt, sync, time`. Cargo.lock gains 28 package versions; none changed or removed. Downloaded from crates.io for this approved package.
- Gates: workspace 273 passed (256 + 17), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass.
- Second-session review (the owner said “check the changes thoroughly and if all is well push”): source, tests, manifests and docs read in full; gates rerun (273 passed, Clippy/fmt clean). Endpoint validation is stricter than the handshake's URI parser (literal loopback IP only, so `@`, percent-encoded or bracketed non-`::1` hosts are refused). Lock order is state → events with no lock held across an await or socket/channel send. New crates are all MIT and/or Apache-2.0 from crates.io with checksums; `sha1 0.11` (WebSocket handshake) duplicates the `digest`/`block-buffer`/`crypto-common`/`cpufeatures` stack beside `sha2 0.10` — harmless, removable later by moving sha2 to 0.11. Mutation pass, 15 defects one at a time with restore: 14 caught; the survivor (drop pending waiters instead of failing them) is equivalent, because a dropped reply channel makes the caller read the same recorded first reason. Non-blocking notes for later packages: handshake failures (including an HTTP refusal) all map to `Disconnected`, so C04 should keep its own readiness diagnostics; memory is bounded only by `event_queue × max_message_bytes` (64 GiB worst case at defaults) until C05/D01 set measured limits; a malformed reply is dropped and its caller waits for the deadline; the writer's "every sender gone" branch is unreachable because `Shared` holds the sender (shutdown is via the watch signal).

Limits: tested against a fake WebSocket peer only — no Chromium, no `DevToolsActivePort`, no target attachment (C03–C05). Same-session/browser routing rules beyond session-ID equality belong to C05. The 64 MiB default message limit is a placeholder until screenshot/large-DOM sizes are measured. Text frames only; binary frames are ignored. Event lag is reported, but no consumer yet resynchronizes (C05/G03).

Next: C03 pure sandbox command construction (`SandboxSpec::build`), then C04 real launch.

### Earlier: chain batch CH-7 checkpoint (committed a540588)

CH-6 was approved, committed as 960f50a and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-7.

CH-7 — evidence coverage in the completion gate (N05/N17), uncommitted:

- Format: DEC-012 (pending owner confirmation) — one fenced `aero-claims` block of strict JSON in a Markdown report; `evidence_id: null` marks an explicit unknown. The parser requires exactly one block, an object root with object claims (no positional arrays), no unknown/missing/duplicate fields, and schema version 1.
- `crates/aero-agent/src/verification/completion.rs`: `EvidenceCoverage` on Markdown outputs; the gate loads every distinct cited capture itself (committed record of this task and run, read back and decoded by `EvidenceStore::load`), compares each claim with `assess_structured_claim` (exact entity/field/value, conflicting captures inconclusive), fails on any contradiction, else Passed iff matched/total ≥ `required_fraction`; matched evidence IDs are recorded in the criterion result. Coverage on non-Markdown outputs remains Unsupported. `crates/aero-agent/src/tasks.rs`: `complete_run` now takes `&TaskLease` (opens both stores).
- Tests in `crates/aero-agent/tests/completion.rs`: two observed red (coverage Inconclusive) then green — matching captures complete the full documented fixture; contradicted, unknown, wrong-entity, other-task, nonexistent, missing/empty/malformed/duplicated blocks do not, and a correct report later completes. Two more added after a mutation pass: a 0.5 fraction where a contradiction still fails but an unknown is tolerated, and captures from an earlier run do not count. Seven coverage mutants: all caught; removing the gate's own same-run check alone survives because `assess_structured_claim` independently rejects other-run captures (confirmed by removing both).
- Gates: workspace 256 passed (243 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: prose outside the claims block is unchecked; coverage proves cited captures say the same values, not that sources are true or relevant beyond the exact entity/field; captures still have no real browser producer (D01). DEC-012 should be confirmed before report generation is built.

Chain status: all non-browser prerequisites for N05 now exist (artifact records and reconciliation, evidence capture, gateway lifecycle, coordinated deletion, completion gate with coverage). Next: C01–C04/D01 browser runtime for real capture, as agreed.

## Previous chain batch CH-6 checkpoint (committed 960f50a)

CH-5 was approved, committed as 71ed9d4 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-6.

CH-6 — N05 completion gate, uncommitted, per DEC-009 (no attestation):

- `crates/aero-agent/src/verification/completion.rs`: the host supplies only `OutputBinding`s (which committed artifact is which contract output). Phase 1 loads each record, requires it to belong to this run (else OwnershipMismatch), reads the bytes back, and evaluates every criterion of the accepted contract itself: `ArtifactSchema` (exact ordered CSV header) and `RowCount` (CSV data records) with a strict RFC 4180 reader; unbound outputs are Inconclusive/Unavailable, failed readback Inconclusive/InvalidReadback, malformed CSV Inconclusive/InvalidReadback, and `EvidenceCoverage` or non-CSV checks Inconclusive/Unsupported.
- `crates/aero-agent/src/store/worker.rs`: crate-private `finalize_run` — one transaction rechecks the run is open (running/paused) at the latest revision, no unresolved task work, usage within the contract, verified artifact records unchanged, and that results cover exactly the contract's criteria; upserts `criterion_results`; completes run and task only when every required criterion Passed and every required output verified. `start_run`/`revise_contract` refuse a completed task (Conflict); prepare already refuses non-open runs.
- `crates/aero-agent/src/tasks.rs`: `TaskAuthority::complete_run(&store, context, bindings) -> CompletionReport { completed, results }`.
- Schema (DEC-010 in place): completed runs and tasks cannot change status; a completed run's criterion results cannot be updated or deleted except by the task cascade.
- Tests: `crates/aero-agent/tests/completion.rs` (completes once and is then final; failed/unverifiable criteria and bad bindings leave the run open and a later correct output completes it; the full documented fixture never completes because evidence coverage is unsupported; unresolved work, a superseded run or revision, or another run's artifact block completion) and unit `finalize_rechecks_unresolved_work_record_identity_and_outputs`. Process note: these were written before the implementation but not run against a scaffold, so there is no red observation. Instead, seven deliberate defects were injected one at a time with automatic restore; two initially survived (skipped required-output check, skipped finalize unresolved-work recheck — both unreachable through the public API because contract validation and the phase-1 resume read already cover them), the unit test was added, and all seven are now caught.
- Gates: workspace 252 passed (239 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: passing proves byte shape, not truth or sourcing. The documented G1 fixture requires evidence coverage on a Markdown report and therefore cannot complete until CH-7 defines how report claims cite captures and compares each claim against loaded evidence (`load_evidence` + `EvidenceStore::load`, never caller facts). No orchestrator calls the gate yet. Blocking API (host thread). File bytes can still change after completion; the immutable record keeps the verified digest, so later readback detects it.

Next: CH-7 evidence coverage (claim/citation format, per-claim comparison), then C01–C04/D01 browser runtime for real capture.

### Earlier: chain batch CH-5 checkpoint (committed 71ed9d4)

CH-4 was approved, committed as fa09ba9 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-5.

CH-5 — coordinated task deletion (N16), uncommitted, per DEC-010 (in-place schema) and DEC-011 (tombstone):

- Schema: `retention_holds(task_id, id, created_at)` cascading with the task; `task_tombstones(task_key, state, created_at, updated_at)` with no task ID or content, permanent (no delete), immutable key/created time, state only `deleting` → `complete`, no replacement. Schema identity test now expects 14 tables.
- `crates/aero-agent/src/store/worker.rs`: `JournalClient::delete_task` is now the journal half — one transaction refuses Held (any hold) or NeedsReview (unresolved work), writes the `deleting` tombstone and deletes the task's records; repeating while deleting is Ok, after completion Deleted. `create_task` and `place_hold` refuse tombstoned IDs with the new `StoreError::Deleted`. Crate-private `complete_deletion`, `pending_deletions` (bounded 1024), `place_hold`, `release_hold`. New `StoreError::Held`.
- `crates/aero-agent/src/retention.rs`: crate-private `purge_task_files(&TaskLease)` removes everything except `controller.lock`, descriptor-relative, unlinking links without following them, refusing other-device directories, bounded to 100,000 entries and depth 32, then syncs the task directory. `config.rs`: `acquire_task_key` (by hashed key) and crate-private `TaskLease::shell`.
- `crates/aero-agent/src/tasks.rs`: `TaskAuthority::delete_task(&state, &task)` = retire gateways → nonblocking task lock (Busy, nothing changed, while any lease/store/runtime is live) → journal half → purge → complete; `resume_deletions(&state)` finishes `deleting` tombstones; owner-only `TaskClient::place_hold` / `release_hold`.
- Tests in new binary `crates/aero-agent/tests/deletion.rs`: purge of artifacts, evidence, runtime profile/staging and an outward symlink (target preserved) with the other task intact and the tombstone holding only key/state/timestamps; holds and a prepared action block deletion before it starts, and holds cannot be placed afterwards; journal-half-only deletion resumed after a journal restart, idempotently. All three observed red against stubs, then green. The old SQL-only test comment now describes the journal half.
- Gates: workspace 246 passed (233 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass; 15 repeated runs of the deletion, retention and task suites passed. Not independently reviewed.

Limits: deletion is blocking (host thread). Gateways are retired even if deletion is then refused. Filesystem leases do not consult tombstones: a later `acquire_task` for a deleted ID can create files, but the journal refuses all its writes and `delete_task` returns Deleted; such files are not purged automatically. Unlinking is not secure erasure, and exported/remote copies are not recalled. Tasks with unresolved effects cannot be deleted until reviewed (there is no reconciliation API yet). The hashed key is pseudonymous.

Next: CH-6 N05 atomic completion gate.

### Earlier: chain batch CH-4 checkpoint (committed fa09ba9)

CH-3 was approved, committed as 7f88df2 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-4.

CH-4 — gateway retirement wired to the run lifecycle (N13 slice), uncommitted:

- `crates/aero-agent/src/data_flow.rs`: retirement logic extracted to `retire_state` (unchanged behaviour: closes admission, waits for an admitted send, detaches state even after poison, drops transports after unlocking); new crate-private weak `GatewayLink` from `GatewayHost::link`.
- `crates/aero-agent/src/tasks.rs`: `TaskAuthority::register_gateway(&host)`; an owner-only registry (`Access::Owner` now carries it, model clients do not) with at most `MAX_GATEWAYS_PER_TASK` = 64 live links per task, dropped gateways pruned. The owner's `start_run` and `revise_contract` retire all registered gateways for the task, outside the registry lock, before submitting to the journal.
- Tests in new binary `crates/aero-agent/tests/gateway_lifecycle.rs` (kept apart from the data_flow suite, which spawns child processes): new-run retirement observed before the commit reply is awaited, other tasks unaffected and a new gateway for the new run works; contract revision retires; model start/revise are denied without retiring; registration bound and pruning. All three observed red against a no-op registration (after fixing a fixture that reused a journal-wide run ID), then green.
- Gates: workspace 243 passed (230 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: retirement happens before submission and is not undone if the journal rejects the request (fail closed: create a new gateway). It cannot recall a send already in flight and blocks on a stuck transport. Only gateways registered with the same `TaskAuthority` are covered; future N09 close and N16 deletion must use the same retirement. Still no real transport (DEC-004) or durable grants (N06).

Next: CH-5 N16 coordinated deletion with the DEC-011 tombstone.

### Earlier: chain batch CH-3 checkpoint (committed 7f88df2)

CH-2 was approved, committed as 3a9c3ce and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-3.

CH-3 — durable evidence capture (N17 slice), uncommitted:

- `crates/aero-agent/src/artifacts.rs`: the store now has a name namespace and subdirectory (`open_namespace`), `stage_with` for content built from the generated name, `check_evidence_owner`; artifact behaviour is unchanged. `crates/aero-agent/src/config.rs`: `artifact_directory` generalised to `task_subdirectory(name)`.
- `crates/aero-agent/src/evidence.rs`: `FactCapture`, `EvidenceStore::{open, capture, load}`, `StagedEvidence::record`. Capture validates the same bounds the comparison uses (scoped URL, nonempty excerpt, bounded text, nonzero time), refuses Unknown/Sensitive (no encryption yet), and writes strict versioned JSON embedding the generated `e1-` ID and run context. Load reads back against the journal record, requires an object, exact version/ID, and the record's run context (else OwnershipMismatch).
- `crates/aero-agent/src/store/worker.rs`: `EvidenceRecord` gains `bytes`; commit attachments and checkpoint loads require task-owned `e1-` IDs; new `JournalClient::load_evidence(task, id)`. Evidence is inserted inside the existing outcome transaction, so it commits atomically with the step.
- Schema (DEC-010, in place): evidence rows refuse update, replacement and individual deletion.
- Tests in `crates/aero-agent/tests/evidence.rs`: `journal_evidence_records_must_name_task_captures` observed red (fake ID accepted), then red again at the raw UPDATE, then green after the triggers; `captured_facts_round_trip_through_the_journal_and_store` was written together with the implementation, so it is not red/green evidence. Journal/worker fixtures now use well-formed `e1-` IDs with a byte length; the payload-corruption unit test drops the matching trigger to simulate corruption below SQL.
- Gates: workspace 240 passed (227 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: nothing yet produces captures from a real authorized observation (needs D01); Public/Redacted are trusted host assertions; the existing caller-constructible `CapturedFact` comparison remains for synthetic use and must not be fed caller facts by N05, which must load facts through `load_evidence` + `EvidenceStore::load`. No evidence reconciliation (orphaned captures) yet.

Next: CH-4 N13 gateway retirement wired to journal task cancellation.

### Earlier: chain batch CH-2 checkpoint (committed 3a9c3ce)

CH-1 was approved, committed as c2aa4a2 and pushed (remote main read back at the same SHA). The owner then said “yes, go ahead” to CH-2.

CH-2 — artifact reconciliation (N18 slice), uncommitted:

- `crates/aero-agent/src/store/worker.rs`: `JournalClient::load_artifacts(task)` returns the task's committed artifact records ordered by ID; more than `MAX_ARTIFACT_INVENTORY` (1024) is BudgetExceeded, never a truncated list. Rows are read with a 4 KiB per-row bound and validated by the same `checked_artifact` used by `load_artifact`.
- `crates/aero-agent/src/artifacts.rs`: `ArtifactStore::reconcile(&records)` returns `ArtifactReconciliation { verified, missing, mismatched, unrecorded, unexpected_entries }` with `is_consistent()`. Every recorded file is read back against its record; unrecorded means a regular file with this task's generated-name shape but no record; anything else (foreign/malformed names, directories, links) is unexpected. Records must belong to this task and be unique; more than 4096 directory entries is BudgetExceeded. Listing uses the live owned directory descriptor via /proc/self/fd, as the store opener already does. Nothing is deleted, repaired or re-recorded.
- Tests in `crates/aero-agent/tests/artifacts.rs`: `reconciliation_reports_unrecorded_missing_and_mismatched_files` observed red (empty inventory against the scaffold) then green; `inventory_limits_fail_closed_instead_of_truncating` was added after the implementation and passed on its first run, so it is coverage rather than red/green evidence.
- Gates: workspace 238 passed (225 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: the report is a snapshot; run it under the task lease while nothing is staging and no step is prepared, otherwise an in-flight stage appears as unrecorded. Hashing reads every recorded file (up to 16 MiB each). Cleanup of unrecorded or unexpected entries is N16's job. N18 still lacks the bundle manifest and the export grant.

Next: CH-3 N17 sealed capture interface and evidence rows bound into the outcome transaction.

### Earlier: chain batch CH-1 checkpoint (committed c2aa4a2)

Authorization (2026-11-02): the owner asked to resolve the blocked prerequisite chain (N05 ← N17/N18 + DEC-009; N17/N18 ← capture, N13, N16, schema). Decisions taken: DEC-009 accepted as no attestation; DEC-010 edit the unshipped v1 schema in place (no AeroBrowser state directory existed on the host, so no data is affected); DEC-011 minimal permanent tombstone, holds cannot stop a started deletion. Scope order: resolve all non-browser prerequisites in verified batches, then start C01–C04/D01 for real capture. The owner approved and the reviewed backlog was committed as seven per-package commits (each verified to build, test, lint and format on its own) and pushed: 0e7165b..5af0e9d.

CH-1 — journal-backed artifact records (N18 slice), uncommitted:

- `crates/aero-agent/src/artifacts.rs`: `StagedArtifact::record(context)`; `ArtifactStore::verify(&ArtifactRecord)` reads the file back against the journal record's task, length and SHA-256; shared `check_artifact_owner` for the `a1-<task key>-<nonce>` ID shape.
- `crates/aero-agent/src/store/worker.rs`: commit attachments and checkpoint loads require store-generated IDs owned by the run's task (malformed → InvalidInput, other task → OwnershipMismatch, persisted mismatch → CorruptDatabase); new `JournalClient::load_artifact(task, id)`.
- `crates/aero-agent/src/store/migrations/001_initial.sql` (in place per DEC-010): artifact rows cannot be updated, replaced or individually deleted; task cascade still removes them.
- Tests: three new integration tests in `crates/aero-agent/tests/artifacts.rs`, each observed red (journal accepted `artifact`; lookup unimplemented; raw UPDATE succeeded) then green. Journal/worker fixtures now use well-formed owned IDs; the payload-corruption unit test drops the new trigger to simulate corruption below SQL.
- Gates: `cargo test --workspace --locked --offline` 236 passed (223 behavior + 13 compile-fail doctests), 0 failed/ignored; warning-denied Clippy, fmt and warning-denied rustdoc pass. Not independently reviewed.

Limits: readback proves byte identity with the record, not correctness, relevance, classification or export permission. Orphaned staged files and records whose file is missing are detected only per record; no inventory reconciliation yet. Existing development journals with the previous schema are refused by the exact-schema check (expected under DEC-010).

Next batches (proposed, in order): CH-2 artifact reconciliation; CH-3 N17 sealed capture interface and evidence rows bound into the outcome transaction; CH-4 N13 retirement wired to journal cancellation; CH-5 N16 coordinated deletion with tombstone; CH-6 N05 atomic completion gate; then C01–C04/D01.

## Previous checkpoint (2026-10-28 stop)

Historical (revision 2.15): Session stopped 2026-10-28 16:18 +08 at the owner's request. N05, N13, N16, N17 and N18 remain blocked as complete packages; each has partial foundations described below. The recommended next batch is a preserving schema-evolution design and implementation tested only on disposable databases, subject to the owner's go-ahead. Do not migrate, reset or delete real user data; do not stage, commit or push without separate explicit approval. Browser/runtime/native qualification remains unverified. Prior checkpoints below are historical where their next actions or test counts differ from this current checkpoint.

## Next session: shared storage prerequisite

The most useful shared infrastructure dependency is safe journal schema evolution. `crates/aero-agent/src/store/mod.rs` currently accepts versions 0 and 1 and validates the exact supported schema; prior edits to the unshipped v1 schema can make older development databases fail to open. Evidence provenance, artifact reconciliation and durable deletion state need a way to evolve storage without losing existing records. The recommendation is to design and implement a preserving v1-to-v2 path using disposable fixture databases only, with explicit schema identity/version checks, preservation of representative task/run/journal records, transactional rollback on injected failure, unsupported-schema refusal, foreign-key/integrity checks and tests for interrupted or repeated attempts. First inventory the distinct v1 schemas actually supported: do not call every historical development schema valid or promise a migration from unknown versions without verifying their identities. Define backup/recovery and the opt-in boundary before any real migration. No user database operation was authorized by this recommendation.

This is a proposed next batch, not a completed migration or authorization to begin it. Obtain the owner's go-ahead before implementation; applying any migration to existing user databases is a separate decision. Schema evolution alone will not complete N05: N17 still needs a trusted real observation/capture owner, N18 needs verified artifact repository/readback and reconciliation, and any subjective attestation needs explicit permission and attribution. Finalization must atomically recheck accepted revision, run, unresolved work and trusted proof identities. N13 still needs real adapter/source/grant/lifecycle integration; N16 still needs trusted holds, coordinated admission, safe cleanup/recovery and privacy/cutover decisions. The retention design is in the N16 section of [the implementation plan](implementation-plan.md).

At stop, local branch `main` is at `af88105`; the workspace has substantial prior modified/untracked work and nothing staged. Preserve it. Most recent full workspace execution: `cargo test --workspace --locked --offline` passed 220 behavior tests and 13 compile-fail doctests, zero failures/ignored; format, warning-denied Clippy, offline build and rustdoc passed. The contributor documentation checker passed earlier in the session; a separate local link/anchor check was blocked by a consent timeout and was not retried. `git diff --check` passed after the checkpoint. No tests were rerun for this handoff-only edit. N13's latest poison/cycle repair passed its regression but has not been independently re-reviewed.

## Current N13/N16 work in progress

The owner said “Proceed with N13 and N16 as required.” N13 now has an in-memory host-issued gateway and fake-recipient tests; it is not connected to real adapters, a durable grant owner or trusted source capture. The parent reproduced two authorization/lifecycle gaps (retained handle after host drop and task-content telemetry), added red/green regressions and closed both in this slice; payload bytes and identity counts are also bounded. Independent review identified unscoped purposes and missing retirement: the parent added purpose-scoped handles and explicit irreversible retirement with red/green tests after review. A follow-up review found shutdown resource-retention and reentrant destructor deadlock defects; the parent reproduced both with bounded tests and fixed cleanup to detach values under lock and drop them after unlock, even after poison. A further review reproduced retention after poisoned retirement and a transport/host strong-reference cycle that prevents automatic host drop. The parent added a red/green regression: retirement now detaches resources even when poisoned while still returning `Unavailable`; the host must explicitly retire and transports must not retain strong host references. This last fix has not been independently re-reviewed. Size limits bound encoded lengths rather than caller-owned allocation capacity. Retirement is not wired to journal cancellation and cannot interrupt an in-flight send. N16 has a filesystem-only preflight that retains an existing exclusive task lock but always reports unknown hold/journal status and cannot delete or authorize deletion. Independent review found no false deletion eligibility in that scope; later cross-process tests cover TaskLease/preflight exclusion in both directions. A synthetic test confirms the existing `JournalClient::delete_task` removes SQL only while an artifact writer remains active; it must not be exposed as complete deletion. The test and API docs describe this limitation, not a repair. A proposed crash-safe coordinated deletion design and its unresolved privacy/hold/cutover decisions are recorded in the N16 plan. N13/N16 are blocked as complete packages. The current workspace passes 220 behavior tests and 13 compile-fail doctests; fmt, warning-denied Clippy, build, rustdoc and tracked diff whitespace checks also pass. Parallel fixture process spawning caused intermittent Busy on reopened locks; N16 retention and existing budget test binaries now serialize independent fixture tests while retaining within-test and cross-process assertions. Stress runs passed 40 retention suites and 20 budget suites before the last added retention test. Preserve all existing databases and user data; no migration/reset, real task-data deletion, Git staging, commit or push is authorized. N17/N18 and N05 remain blocked.

## Current N17/N18 blocked checkpoint

The owner said “Proceed with N17 and N18.” Independent design audits found no live observation owner, real evidence source or artifact repository, and existing journal evidence rows are digest-only; exact-schema checks reject in-place DDL changes to old databases. N17 has an explicitly non-authoritative, in-memory exact structured-value comparison in `crates/aero-agent/src/evidence.rs`: a scratch harness observed its planned named test red against an Inconclusive scaffold and green after implementation. It is not a repository, authorized capture, durable provenance, secret redaction, actual artifact readback or completion proof; `CapturedFact` is caller-constructible and must not be accepted as authority. N18 has a task-lease-bound, create-only private byte store with bounded readback and digest checks in `crates/aero-agent/src/artifacts.rs`. `Public`/`Redacted` are trusted-host assertions, not classification or redaction; `Unknown`/`Sensitive` fail before writing. The store is not an atomic bundle, journal-backed, crash-recoverable, encrypted, retention-managed, exported, or evidence-verified artifact repository. An independent adversarial review found no remaining confirmed defect in its reviewed snapshot; the reviewer reproduced the plaintext-write issue before the parent added the Unknown/Sensitive refusal. The review also identified absent-excerpt and conflicting-capture cases; the parent added tests and made those inputs inconclusive afterward, without a second independent review. Incomplete writes may leave private orphan files and aggregate storage is not bounded. N13 policy, N16 retention, real observation capture and a preserving schema/migration design remain necessary. No database reset, migration, N05 completion, attestation, publication or export is authorized.

Verification: final workspace `cargo test --workspace --locked --offline` passed 191 behavior and 12 compile-fail tests, with zero failed or ignored. Formatting, warning-denied Clippy, offline build, warning-denied rustdoc, documentation checker and tracked-diff whitespace checks passed. No changes staged or committed.

Historical next-step note: N13 and N16 were subsequently authorized and partially implemented; see the current checkpoint above. A preserving schema strategy and real observation ownership remain necessary. Do not migrate a user's database or claim N17/N18 complete merely from these foundations.

## Current N05 checkpoint

The owner authorized N05. The pure `completion_gate` predicate was first tested against an always-false implementation (the all-passed case failed), then implemented and the exact focused test passed. A separate regression records a model Complete proposal through the real journal and verifies the task remains running with a prepared unresolved intent, no artifact and no criterion records. This is not an orchestrator test, an inconclusive-criterion test, or proof of an implemented completion gate. An earlier assertion incorrectly expected paused; actual prepare changes the task to running, and the assertion was corrected. The final workspace gate passes 181 behavior and 12 compile-fail tests; format, warning-denied Clippy, build, rustdoc and the documentation checker pass.

Independent design review confirmed that the plan schedules N17 evidence and N18 artifact repositories before the full N05 gate. Existing evidence/artifact journal records only contain IDs, context, hashes and byte counts; they cannot establish output-name binding, source relevance, freshness or actual artifact readback. TaskContract has no subjective criterion/attestation permission, attributed user decision, or decision-evidence record. N04 reports are forgeable data, not verifier capabilities. The current resume view is a snapshot, so a separate check-then-complete write would race new work or revision changes. No production completion transition, attestation path or synthetic production success has been added.

Before enabling completion, implement the missing N17/N18 infrastructure and obtain authorization for a trusted attestation contract. N05 must bind trusted results to the persisted accepted task/revision and issued artifact identity; revalidate proof freshness, current run, task-wide unresolved actions/reservations and budget consistency atomically with task/run completion. External readbacks themselves cannot be made atomic by SQLite; bind immutable/versioned captures and recheck their identities. Ensure later admission cannot reopen or overwrite a completed run. Preserve existing databases; N17/N18 were subsequently authorized and partially implemented, but no schema reset, database migration, staging, commit or push has been approved. The pure predicate is a boolean helper, not an authorization boundary.

## Current N04 checkpoint

Owner authorized N04 independent verification. The fixed registry compares host-installed readbacks for navigation, nonsecret field state, boolean secret state, ordered artifact schema, row count and evidence-reference presence; unsupported checks and unavailable, malformed or wrong-scope readbacks are inconclusive. Readback requests carry identities but not expected values. Reports are versioned, bounded after decoding and carry status, method, references, time and static reason. Test-local dispatcher and readback show acknowledged wrong state fails and matching state passes. A compile-fail example checks the restricted TaskClient has no criterion-result writer. Independent source review found the original dispatch test did not call a dispatcher; the parent added and ran the test-local dispatch path. Latest workspace: 179 behavior and 12 compile-fail tests pass, including seven N04 integration tests; fmt, Clippy, build, rustdoc and documentation checks pass.

Critical limits: the registry is in-process host-owned infrastructure, not a sandbox; adapters must establish ownership/freshness and reject raw secret reads. There is no real readback adapter, accepted-output-name-to-artifact binding, evidence relevance/coverage, persisted criterion provenance, N05 completion gate, or untrusted postcondition ingress. Postconditions have no version envelope, and limits apply after deserialization; any future ingress must enforce a byte cap and version first. An independently reproduced forged Passed/Matched report validates structurally; decoded DTOs and raw host JournalClient StepOutcome reports must never be treated as verifier provenance. The N05 controller must bind the accepted task/revision/criterion, trusted verifier and artifact identity and independently check freshness, unresolved effects and required outputs before completion. An evidence-reference presence pass is not evidence coverage or truth. URLs and IDs may themselves be private; outputs require privacy policy, not just structural validation. N03's schema/performance caveats remain below. No N05, staging, commit or push is authorized.

## Current N03 checkpoint

The 2026-10-26 N03 session was interrupted after implementation began. The owner subsequently approved finishing the work and its review. N03 adds task-wide work/action reservations, pricing and separate purchase accounting, an injected active timer, and storage/recovery tests. Independent review found action overrun reports could be overwritten by smaller settlements and spent counters were not reconciled to durable history. The parent reproduced both; action overruns now retain uncertain holds and action settlement records reconcile spent counters. Focused regressions pass, including restart and injected rollback for overrun, and counter/receipt mutation tests. This is local storage/accounting evidence, not application acceptance.

Independent review also found attachment insertion could roll back overrun quarantine, settled work receipts could be read despite inconsistent totals, and overrun amounts were discarded. The attachment-conflict regression was observed red then green; work receipts now reject inconsistent totals and settled work records have an immutability trigger. Overrun amounts are retained separately from accepted settlement and survive restart in focused tests. Further review found malformed attachments blocked quarantine, unexpected tokens could still report a zero inference upper bound, and overrun reports/work records could be individually erased. Parent observed red/green regressions and added attachment-independent quarantine, conservative unknown inference, immutable reports and task-owned delete guards. Attachment validation errors for non-overruns are now delivered through the writer reply; malformed attachments are discarded on overrun. The final independent review verified those repairs but identified invalid outcome metadata blocking quarantine and a potential SQLite replacement bypass. The parent reproduced both as failing tests, fixed them, and reran the complete workspace: 172 behavior tests and 11 compile-fail doctests pass; format, warning-denied Clippy, build and rustdoc pass. Invalid outcomes on overrun are omitted rather than fabricated; invalid attachments are discarded, and non-overrun metadata errors are returned by the writer. Replacement is refused even when recursive triggers are off. The integrating parent reviewed and tested these last repairs; they were not independently re-reviewed after the changes.

New indexes use task/state searches, but full settled-history reconciliation still scales with lifetime history and has no hard deadline; performance and bounded retention remain open. ID-ordered history pages are not a concurrent snapshot and can miss inserts before the cursor. An actual SQL/I/O failure can roll back quarantine: the host must stop dispatch and reconcile rather than retry a smaller report.

The unshipped v1 schema was changed again; older development databases fail exact-schema identity checks. There is no automatic migration or approved deletion/reset. Preserve existing data. Accounting records are trusted host inputs, not grants or proof of provider billing; real provider/browser dispatch, active deadline enforcement and native qualification remain unimplemented. At the N03 checkpoint, N04, staging, commit and push had not yet been authorized; N04 was subsequently authorized as recorded above.

## Current N02 checkpoint

Authorization: the user said, “Awesome, let's continue with N02.” This covers the host-issued task capability boundary, immutable revision/run ownership and its tests, not N03 budgeting, N06 effect-grant issuance, N08 adapter authentication/controllers, browser/provider execution or publication.

Implemented paths: new `crates/aero-agent/src/tasks.rs` and `crates/aero-agent/tests/tasks.rs`; module export in agent `src/lib.rs`; scoped worker visibility/start/status/read-view changes and static errors in `src/store/worker.rs` and `src/store/mod.rs`. Existing journal integration fixtures explicitly route task mutations through a real TaskAuthority owner client; private worker fixtures supply synthetic StartPrerequisites and test denial against a full queue. Config, initial SQL schema, core/CLI/audit code, manifests/lock, pin and specification remain byte-for-byte preserved. No dependency or schema change.

Behavior: TaskAuthority is privileged host bootstrap bound to one Journal. It issues immutable, non-wire TaskClient owner or model-read-only capabilities; model clients cannot create/revise/start tasks and can read only the host-selected task. Persisted run/task/revision checks reject forged contexts. Raw JournalClient task creation/revision/start methods are crate-private. Owner writes reuse B05 exact-next immutable revision transactions, grant/approval invalidation, history and cumulative budget preservation. Start validates the stored contract and host prerequisites in the same transaction before retiring/inserting runs. Accepted intent stays draft; a new run record starts paused, not browser-ready. ResumeState now reports current persisted run status separately from historical checkpoint status.

Evidence:

- Required `cargo test -p aero-agent --test tasks contract_revision_requires_trusted_owner -- --exact` executed and failed on model revision being accepted against the permissive guard scaffold, then passed after owner checks. `start_checks_persisted_contract_prerequisites_without_writes` likewise failed Ok versus StartBlocked before persisted transaction-local checks. `intent_and_run_records_do_not_claim_execution_readiness` failed ready versus draft before correcting draft/paused bookkeeping. Scratch red logs use `aero-n02-*-red.log` and `aero-n02-readiness-red.log`.
- Nine task integration tests pass: owner/model denial, immutable history and grant invalidation, cross-task/forged run/revision reads, concurrent revision sequencing, cumulative history across runs/reconnect, persisted start gates and rollback, journal-bound scope/close behavior, invalid-model payload denial before closed-queue admission, and lost start acknowledgements. Compile-fail tests reject deserialized/externally constructed clients and direct raw task writes. The earlier bounded-worker test also verifies model denial takes precedence over QueueFull.
- Final workspace: 143 behavior tests and eleven compile-fail doctests pass; no failed/ignored/filtered tests. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc pass. Agent tests include 13 unit, 34 journal, nine task integration and three compile-fail tests. Final logs are `aero-n02-final-gate-0.log` through `-4.log`. Documentation/link/traceability, source-preservation and whitespace/index checks pass.
- Design review confirmed the boundary and identified readiness/status-view clarifications; draft/paused state, current_status and explicit privilege/lifetime documentation cover them. Independent source review reported no actionable N02 defects and independently ran the then-current seven task tests plus existing agent suites/Clippy. Parent subsequently ran all nine task tests and final workspace gates. No review finding remains outstanding.

Limits: factory possession is host-root composition, not proof of human/peer authentication. Never give a model adapter TaskAuthority, Journal, raw JournalClient, owner clients or an authority-minting callback. Scope is journal plus stable TaskId, not user/revision/run or revocable multi-user login; clones intentionally retain reads across revisions/new runs. Delete/recreate of a delegated ID within the same journal lifetime would reuse scope, so the host must not reuse IDs for unrelated work. Closing the Journal ends admission; accepted requests can survive lost replies and require reconciliation. Start prerequisites are trusted observations, not actual provider/background readiness, dependency completion, grants or launch. Scoped history is private data, not authorized provider egress. B06 leases remain independent filesystem ownership; runtime controllers, effect permissions, monetary budgets, encryption and N13 transfers remain later owners. ENV-002/ENV-004 unchanged.

Stop boundary: N02 is ready_for_review, uncommitted. No staging/commit/push, N03, browser/provider/account operation or new service process. Next separately approved batch is N03 cumulative reserved budgets (`crates/aero-agent/src/budget.rs`, `crates/aero-agent/tests/budget.rs`, writer integration): concurrent reservations, active-versus-paused time, known/unknown pricing and separate purchase limits, preserving B05 counters. Historical checkpoint sections below are evidence, not current next-action instructions.

## Current B06 checkpoint

Historical checkpoint: N02 above supersedes this section's next-action/status text; B06 evidence is retained.

Authorization: the user said, “Fantastic, proceed to B06.” Scope is private state/task/run/runtime layout and cooperative lifetime controller locks, not N02 roles, actual browser/input ownership, encryption, cleanup or publication.

Implemented paths: new `crates/aero-agent/src/config.rs`; scoped changes to agent `src/lib.rs`, `Cargo.toml`, `src/store/mod.rs`, `tests/journal.rs`, and `Cargo.lock`. The root manifest, worker implementation/tests, initial SQL schema, core/CLI/audit sources, browser pin and specification are preserved against the B06 baseline. Agent adds an edge to existing locked sha2 0.10.9; no package/version change is needed. Shared filesystem helpers retain B04's final-component-only open contract while supporting safe recursive configuration creation and exclusive runtime allocation.

Behavior: pure OS-native XDG_STATE_HOME or HOME/.local/state resolution, private `aerobrowser` root, separate journal/tasks directories, exact-byte domain-separated task/run/epoch path keys and a persistent per-task controller.lock. Owned 0700 directories and 0600 single-link regular locks are required; existing permissions are refused, not repaired. Runtime epochs allocate exclusively and never reuse earlier or partially created profiles/staging. RuntimeDirs retains its owner so dropping TaskLease cannot unlock while derived runtime handles remain. Drop never deletes files or resets task counters.

Verification and review:

- Required `cargo test -p aero-agent --test journal private_state_and_task_lock -- --exact` ran against the compilable no-lock scaffold and failed on duplicate same-task acquisition instead of Busy. The same test passes after lifetime flock enforcement; scratch evidence is `aero-b06-required-red.log`.
- Eight B06 integration tests pass: required permissions/ownership, XDG default/precedence/error rules, real cross-process exclusion/release and independent tasks, opaque traversal-looking/Unicode/byte-boundary IDs, symlink/hardlink/permission refusal, preserved B05 journal history across runtime generations, concurrent/partial epoch refusal with sentinel preservation, and non-UTF-8 state paths. The cross-process test also passed three repeated runs.
- Final agent suites: 13 module and 34 journal integration tests. Final workspace: 134 behavior tests and eight compile-fail doctests, none failed/ignored/filtered. Build, formatting, warning-denied Clippy and warning-denied rustdoc pass. Logs: `aero-b06-final-gate-0.log` through `-4.log`. Documentation/link/traceability, preservation, dependency, whitespace and empty-index checks pass.
- Read-only design review clarified exclusive epochs, every application-owned directory's permissions, lease/path distinctions, journal-versus-task authority, and persistent-state/environment assumptions. Those contracts are implemented or explicitly deferred to their runtime owners. Independent final source review found no actionable in-scope defect and reported rerunning the then-current 32 integration tests; the parent subsequently ran all 34 including the two additional boundary tests. Review self-report is not the final gate evidence.

Integration limits: task identity is scoped to the configured state root; hashes are not encryption or protection against guessing IDs. Local Linux filesystem semantics are a deployment requirement, not a locality detector. Acquire Journal first, then task lease, then runtime directories; on failure drop acquired guards without unlinking locks. Stop/join all browser/runtime consumers before releasing RuntimeDirs—copied path strings are not leases. This does not establish journal run authority, actual single-input dispatch/takeover, browser sandboxing or hostile same-UID/root isolation. Persistent profiles/staging can be sensitive; N16 owns future validated cleanup. No global /tmp or XDG_RUNTIME_DIR fallback, implicit deletion, browser/provider/account operation, or N02 implementation occurred. ENV-002/ENV-004 remain unchanged.

Stop boundary: B06 is ready_for_review, not committed. Next separately approved batch is N02 immutable revisions/run ownership, starting with `crates/aero-agent/tests/tasks.rs :: contract_revision_requires_trusted_owner`; then `new_run_preserves_task_identity_and_history`. Reuse B05 transactions and B06 lifetime primitives behind host-constructed caller capabilities rather than exposing them as model authority. No automatic commit/push or N02 work. Historical checkpoint sections below preserve evidence only.

## Current B05 checkpoint

Historical checkpoint: the B06 record above supersedes this section's next-action/status text; B05 evidence is retained.

Authorization: the user said, “Ok, let's go with B05.” Scope is the bounded writer and transactional task/revision/run/prepare/commit/load/delete storage boundary, not B06 configuration/controller locks, N02 role enforcement, real dispatch, encryption, providers or publication. The prior core/CLI/audit sources and specification are preserved byte-for-byte against the B05 baseline.

Implemented paths: new `crates/aero-agent/src/store/worker.rs` and private `crates/aero-agent/src/store/worker_tests.rs`; scoped changes to `crates/aero-agent/src/store/mod.rs`, `crates/aero-agent/src/store/migrations/001_initial.sql`, `crates/aero-agent/tests/journal.rs`, root/agent manifests and lockfile. Documentation is synchronized across the plan, requirements, README, contributor guide, tracker, development guide, ADR0003 and this handoff. Tokio 1.54.1 enables only sync for oneshot replies; pin-project-lite 0.2.17 is its new dependency. Existing serde/serde_json are direct agent edges. No Tokio executor, network or macro feature is enabled.

Implemented behavior: one bounded worker, immediate QueueFull on rejected admission, async/synchronous replies, accepted-work drain and synchronous join on close/drop. Task/revision/run writes are transactional; revisions preserve spent resources and invalidate old grants/approvals; new runs retire old active/paused runs. Prepare atomically reserves one attempt/step and directional tokens/active time. Commit atomically writes outcome, usage/reservation settlement, typed evidence/artifact descriptors, metadata-only checkpoint and statuses without advancing the cursor twice. Unknown consumption/effects remain reserved/needs_review. Startup classifies prepared orphans before exposing clients; load_resume refuses live prepared work instead of racing it. Audit is bounded/read-only; task deletion affects SQL-owned records only.

The initial schema is still unshipped. Directional token and active-ms reservation columns refine version 1 directly; old B04-shaped development databases fail exact-schema validation rather than being silently reset or reinterpreted. The full B03 form-containing Checkpoint is never serialized to plaintext metadata. Sensitive contracts and explicit host-classified RequiresEncryption decisions are refused; the host must supply already-redacted permitted contract/Decision data and opaque identifiers. No automatic redaction, encryption, provenance or authorization is established by these types.

Observed red/green and review:

- `writer_persists_validated_task_contract` first failed on the compilable stub, then passed. The required `cargo test -p aero-agent --test journal step_and_checkpoint_are_atomic -- --exact` subsequently demonstrated the actual partial-write defect: a duplicate artifact failure left the action Committed rather than Prepared without a surrounding transaction. Wrapping the full commit fixed that exact regression. A subsequent private matrix injects failures after four prepare and eight commit write points; all rollback assertions pass.
- Design review led to parent-observed red/green tests for stale run reuse after a newer run settles, and load_resume racing live prepared work. New runs now retire old runs; orphan classification occurs only under exclusive startup. Repeated resume retains reservation/cursor and cannot reconcile uncertainty.
- Parent hardening tests reproduced and fixed mismatched persisted reservation SQL ownership versus payload, and checkpoint usage failing to cover its saved cursor.
- Final read-only source review found two P2 persisted-corruption gaps, not remote authorization bypasses: contradictory action-state/outcome pairs and unchecked referenced evidence/artifact payload identity. Both were reproduced in repository tests before fixes. Audit now checks valid state/outcome combinations; checkpoint references bounded-decode and validate embedded ID/context/digest/byte count for both metadata tables. No review finding remains outstanding in this batch.
- Final agent suite: 13 module tests plus 26 journal integration tests, all passing. Workspace: 126 behavior tests and eight compile-fail doctests, no failures/ignored/filtered tests. Build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Five additional repeated worker-suite runs pass. Contributor documentation checks, local links/anchors, traceability, preservation and whitespace/index checks pass. Scratch logs are `aero-b05-*-red.log`, `aero-b05-atomic-behavior-red.log`, and `aero-b05-final-gate-0.log` through `-5.log`.

Limits and next integration obligations: JournalClient is trusted host storage plumbing, not model-callable N02 service authority. Digests and verification statuses are caller reports, not actual file/source checks; no task-completed setter exists. Monetary pricing/purchases and provider-specific charging remain N03; independent criteria/completion remain N04/N05. No reconciliation API, browser restoration or real crash/server proof is implemented. Synchronous start/close/drop must run on a blocking host thread; the queue is bounded but kernel filesystem waits are not cancelable deadlines. Lost replies require reconciliation, never assumed cancellation or repeated external dispatch. SQL deletion is neither secure erasure nor N16 artifact/profile cleanup. ENV-002/ENV-004 remain unchanged.

Stop boundary: B05 is ready_for_review, not committed. No staging/commit/push, browser/server/provider operation or B06 implementation occurred. Next separately approved batch is B06 private XDG state/task-controller isolation, beginning with `crates/aero-agent/tests/journal.rs :: private_state_and_task_lock`; N02–N03 follow B06. F10's later journal/executor guard and real crash matrix remain required before dispatch acceptance. Historical records below preserve previous evidence but no longer define current next actions.

## Current B04 checkpoint

Historical checkpoint: the B05 record above supersedes this section's next-action/status text; B04 evidence is retained.

Authorization: the user said, “Ok, let's go with B04”. This authorizes the initial task-owned schema/private-open batch, its bundled SQLite dependency and synchronized documentation, not B05 transactions, B06 controller/configuration, N02 authority, browser/provider/account operations or publication. Prior core/audit source and specification are preserved byte-for-byte against the B04 baseline.

Implemented paths: `crates/aero-agent/src/store/mod.rs`, `src/store/migrations/001_initial.sql`, `src/lib.rs`, `tests/journal.rs`, root and agent `Cargo.toml`, and `Cargo.lock`. Synchronized docs: README, contributor guide, plan, requirements, development, tracker, ADR0003 and this handoff. Bundled rusqlite 0.41.0 (default features off) resolves ten new package versions; no existing external package versions are removed or changed. Agent reuses existing libc and dev tempfile dependencies. The earlier CLI libc edge remains prior audit work.

Implemented boundary: eleven STRICT tables; task/run/revision composite ownership; immutable revision and grant-payload update/replacement guards; bounded counters/statuses/JSON-object syntax; transactional schema/application/version initialization; integrity/FK/schema identity checks and typed refusal of foreign/future/partial/corrupt stores. Dedicated Linux private paths, no-follow component traversal, owned 0700/0600 storage, safe auxiliary-file metadata checks and a cooperative lifetime flock protect the synchronous connection. SQLite pragmas are verified through Store settings. Store exports no raw connection or domain-write API.

Observed evidence:

- `cargo test -p aero-agent --test journal schema_and_foreign_keys -- --exact` first executed and failed exit 101 with schema version 0 versus expected 1 against the compilable scaffold; the same test now passes. Logs remain under the session scratch directory as `aero-b04-required-red.log`.
- Two hardening regressions were independently observed red then green: schema identity must not hide `sqliteXforeign` through LIKE's underscore wildcard; nonsticky writable ancestors must be rejected before creating the leaf. The implementation uses literal-prefix GLOB and trusted ancestry checks. Test parents are explicitly private, not a reason to weaken production checks.
- Final `cargo test -p aero-agent --locked`: two module tests and fifteen integration tests pass. Coverage includes migration rollback, actual SQLite-created WAL/SHM permissions, same-/cross-process owner exclusion and release, reopen persistence, cross-owner/revision rejection, immutable revisions, task cascades, invalid counters/JSON/ciphertext triples, FK corruption despite integrity_check=ok, schema/version/corruption refusal, failed-open lock release and auxiliary-file refusal without changed database bytes.
- Final `cargo test --workspace --locked`: 104 behavior tests and eight compile-fail doctests pass, no failures/ignored/filtered tests. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Logs: `aero-b04-final-gate-0.log` through `-5.log` in session scratch. Contributor documentation checks and final source/scope/whitespace validation pass.
- Read-only design review was source-only (its probes did not run); its ownership, FK-check, replacement, expiry and sidecar points are integrated or explicitly assigned to future trusted lifecycle enforcement. Independent final source review reported no actionable in-scope defect. Its failed-open/sidecar byte-preservation probe suggestions are now assertions in the parent-run repository suite; review self-report does not substitute for these executed gates.

Limits: structural SQL acceptance is not trusted TaskContract semantics, grant issuance, revocation/quota monotonicity, criterion existence, payload-size enforcement or encryption. B05's private writer must enforce authorized lifecycle/DTO/payload ownership; N02/N03/N06 own authority and budgets. Never serialize the entire form-containing Checkpoint into plaintext metadata. Linux procfs/local Unix filesystem/WAL/advisory-lock assumptions are explicit; hostile same-UID/root access and production power-loss/browser recovery are not proven. No XDG/task-controller boundary, writer queue, prepared intent, commit_step, task execution, keyring, providers or browser launch has been implemented. ENV-002/ENV-004 remain unchanged.

Stop boundary: B04 is ready_for_review, not committed. Main remains at the earlier local-only documentation commit af88105; the index is empty. No browser/server/database service was started by B04; tests use disposable synthetic files. Next separately approved batch is B05, starting with `crates/aero-agent/tests/journal.rs :: step_and_checkpoint_are_atomic`, then bounded writer/domain transactions and failure injection. Do not mark SPEC-09 fully implemented until that protocol is proven. B06 then N02–N03 remain later. Historical checkpoint sections below are evidence, not current next-action instructions.

## Current N01 checkpoint

Historical checkpoint: B04 above supersedes the next-action/status text below; the original N01 evidence is retained.

Authorization: after the verified audit and next-batch boundary were explained, the user said, “Alright, let's move on.” This authorizes N01's pure TaskContract batch, not a commit/push, B04 storage, N02 runtime ownership, providers, real accounts, browser execution or dependency installation. Existing audit files remain uncommitted and preserved.

Implemented files: `crates/aero-core/src/task.rs`, one `pub mod task` export in `src/lib.rs`, and `crates/aero-core/tests/task_contract.rs`. Documentation is synchronized in the plan, requirements, README, contributor guide, tracker and this handoff. All prior source (including actions/wire/CLI), manifests, lock, pin, scripts and specification are byte-for-byte unchanged from N01 start, except the intended lib.rs export. No dependency was added.

Contract: mandatory version 1, nominal TaskId and positive revision, title/objective/profile/account metadata, explicit origin/route/effect scope, processing mode with opaque processor/export references, source classifications, required typed criteria and named outputs, cumulative budget limits, dependencies, mandatory safety stops, questions, close behavior and bounded retention. Strict map-only structs and tagged criteria reject missing/unknown/duplicate/positional fields; nullable inference money is present explicitly. Static semantic errors and no raw aggregate Debug protect against accidental payload diagnostics, not all serialization/logging.

Design review resolved omissions in the earlier illustrative fixture against specification §4: add its title/routes/accounts/classifications/exports/dependencies/stops/questions explicitly; use budget_currency for both monetary caps. This is an unshipped initial format, so no migration or hidden legacy default. The documentation fixture now compiles through a test that checks exact JSON equality. Routes are deny-by-empty with explicit "/" for whole-origin scope; future policy matching is segment bounded and applies to parsed effective URLs. Mandatory stops cannot be omitted. Local-only export intent may reference a local grant without being mistaken for remote processing; actual recipient/purpose/data/task/revision/grant validity is still enforced by future trusted gateways. No automatic authority comes from references, source labels, selected effects or successful validation.

Observed TDD:

- The exact required command, `cargo test -p aero-core --test task_contract contract_is_strict_and_roundtrips -- --exact`, executed once and failed with exit 101 on duplicate criteria against the initial compilable validator scaffold. Full initial integration suite executed eight tests: seven failed, one nominal-ID proof passed. The same required test now executes and passes.
- Schema-review tests `contract_cannot_omit_mandatory_stop_conditions` and `local_export_intent_is_not_confused_with_remote_processing` each executed once and failed exit 101 before their corresponding validation fixes; both now pass. This records a pre-review design correction, not a shipped defect.
- Final source-review tests `contract_rejects_mapped_ipv6_origins_in_both_spellings` and `contract_required_row_counts_must_agree_per_output` each executed once and failed exit 101 before their fixes. Mapped IPv6 is now rejected in both dotted/hex spellings (ordinary canonical IPv6 remains valid); distinct required counts for the same output are rejected, while equal counts, different outputs and optional criteria remain valid. Parent reproduced/integrated both findings; no B02 parser change or dependency addition.
- Additional exact-bound/UTF-8, counter/money/fraction, canonical-origin/route/reference, processing/start-prerequisite, unknown-checker/authority, nested raw-duplicate/positional, privacy and documentation-fixture regressions pass. Positive zero/null/empty distinctions are explicit. CoverageFraction excludes NaN/infinity/out-of-range values through both decoding and private construction; no duplicate Task/Run aliases introduced.

Current executed gates: `cargo test -p aero-core --locked` passes 51 unit tests, one import, 17 TaskContract integration tests and eight compile-fail doctests. Workspace passes 87 behavior tests and eight compile-fail doctests, no failed/ignored/filtered tests in full runs. Workspace build, formatting, warning-denied Clippy and warning-denied rustdoc all exit 0. Source-preservation hashes pass. Contributor documentation command executed verbatim passes 12 documents/26 requirements/31 corrections/100 ledger packages; extended final checks pass 151 local links/anchors, 57 matrix rows, all 100 definitions, tracker references, JSON examples and 23 cumulative changed paths (prior work plus N01's two new source/test files). `git diff --check` passes and index is empty. Read-only design/source reviews are integrated, with both final source findings observed red/green and all gates rerun afterward. No remaining blocker was identified in the N01 pure-contract scope; no feature-level native/browser/provider/store proof is claimed.

Scope/limits: N01 validates representation/internal consistency and separately supplied host prerequisite facts, not actual provider availability or launch permission. Sensitive/local-only require local capability; background-service availability never substitutes for N09's explicit task grant/isolation. Questions block the start-prerequisite check, but dependency completion/cross-task cycles and trusted resolution remain runtime owners. Origins are a conservative canonical ASCII subset, not DNS/SSRF/redirect/egress policy. Criterion output references are names, not issued artifact records. No verifier/file writer, encrypted store, trusted immutable revision service, budget reservation or completion setter is implemented. N02–N05/N06/N09/N13–N18 remain responsible for those gates; ENV-002/ENV-004 are unchanged. No Hivemind use or external network/account operation.

Stop boundary: N01 is ready_for_review; stop for owner review/explicit scoped commit approval, not automatic publication or another implementation batch. Next separately authorized implementation is B04's task-owned initial SQLite journal schema, not N02 yet: dependency order is N01 → B04–B06 → N02–N03. B04's first named proof is `crates/aero-agent/tests/journal.rs :: schema_and_foreign_keys`. No staging, commit, push, B04 or N02 begun. Historical audit/B03/B02 evidence below is retained but no longer defines the current batch.

## Current pre-N01 audit checkpoint

Historical audit checkpoint: the current N01 record above supersedes its next-action/status text; the audit evidence remains valid.

Authorization: the user said, “Ok, let's resolve this bug, and let's find any other bugs or issues that needs to be fixed before moving on.” DEC-008 is accepted for BUG-001 and an audit/fix pass over implemented code, not N01/new features, installation, provider/account/browser operations or staging/commit/push. The bounded lifecycle repair reuses the already-locked libc package explicitly as documented below rather than introducing a new package/version.

BUG-001: two repository regressions were added before the change and each executed/faulted with exit 101 on positional Action arrays. Action now uses the existing map-only tagged helper, preserving valid object wire data, Debug/PartialEq and optional/null scroll target behavior. Both regressions pass across all 18 variants, direct and nested; existing core tests, workspace tests, build, formatting, warning-denied Clippy and core rustdoc pass. At that BUG-001-only step, manifests/lock/specification were unchanged; actions.rs was deliberately changed under the new authorization rather than covered by B03's historical preservation promise. The subsequent signal-ordering repair adjusts the root/CLI manifests and one existing lock edge, detailed below; core manifest, pin and specification remain unchanged.

Audit findings: three read-only audit streams covered B03 schemas/validation, B01 geometry/IDs/B02 parameter validation, and implemented CLI diagnostics/scripts. No additional concrete core defect was reported beyond BUG-001. The CLI audit identified three issues; the parent reproduced each against repository tests before fixing it. Review-reported stress/probe counts are supplemental, not a browser/model benchmark or a proof that no bugs exist. Parent owns all repository writes.

| Issue | Observed repository red | Local repair / green |
|---|---|---|
| BUG-001: Action accepts positional arrays | Two new core tests each ran once, failed exit 101 before the fix. | Shared map-only decoder; both pass across all 18 variants direct/nested, with valid objects/optional targets unchanged. |
| BUG-002: diagnostic descendants leak | Two descendant tests failed exit 101 after cleaning their fixtures. Review then identified signal-after-reap: parent ordering regression failed exit 101 (getpgid=-1); no rare PID-reuse collision was forced. | Dedicated groups; Linux waitid/WNOWAIT retains the leader until group signalling completes, then reaps it with no second group signal. Nondefault/auto-reaping SIGCHLD is refused before spawn; ECHILD drops signalling authority. Cleanup/order/policy tests pass. Not C04 runtime or hostile-executable containment. |
| BUG-003: helper requires absent python alias | Three new shell-adapter tests all failed exit 101 under a controlled Python-3-only/missing-Python PATH; helper returned 127. | Use/check python3 before Cargo; three tests pass, including mismatch rejection and Cargo-error propagation. Contributor check and prerequisite docs also use/name Python 3. Reports are synthetic adapter fixtures, not actual pin matches. |
| BUG-004: unbounded integrity read | Sparse 1 GiB+1 file failed prompt-rejection assertion, one test, exit 101. | Regular-file/1 GiB actual-byte bounds and a two-second integrity caller deadline. Size, preserved digest and blocked-job deadline tests pass. Blocked OS I/O cannot be canceled; timeout does not join the worker, which ends with this one-shot CLI process. |

Additional regression hardening: `wire::tests::recovery_and_event_objects_reject_field_corruption_systematically` now covers the recovery/event objects and every tagged variant, including each required field, invalid null, raw duplicates/null-first duplicates, unknown fields, trailing JSON, scalars and full positional arrays in serialized declaration order. It passed immediately against the existing implementation and is not claimed as another reproduced defect. Core has 51 unit tests plus one import and seven compile-fail doctests. Workspace has 70 behavior tests (51 core unit, one import, five CLI unit, three shell-helper and ten doctor) plus seven compile-fail doctests; all passed locally, no ignored/filtered tests in the full gate. Final full workspace tests/build/fmt/warning-denied Clippy and workspace rustdoc (`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`) pass. `bash -n` passes for the pin helper; the contributor Python3 check executes verbatim and passes 12 documents/26 requirements/31 corrections/100 packages. Extended final validation passes 148 local links/anchors, 57 matrix rows, all 100 definition/ledger mappings, JSON examples, tracker indexes and 21 exact reviewed changed paths, including previous B02/B03 work and the explicit manifest/lock edge. Index is empty, HEAD is still af88105, and the core manifest/pin/specification are unchanged. Source review's signal-ordering finding was parent-reproduced and fixed, and all Rust gates were rerun to the final 70+7 result; no remaining blocker was identified in the implemented pre-N01 scope. Both descendant tests also passed three repeated runs. The final lock readback proves only the existing libc edge changed, with package/version/checksum identities preserved.

Scope/preservation: production changes are Action's decoder, private wire helper reuse, CLI diagnostics and the pin helper; tests cover these and B03 negative shapes. Core manifest, specification and browser pin are unchanged. The root/CLI manifests now declare existing locked libc 0.2.191 explicitly for target-correct waitid/killpg/sigaction types/constants instead of hand-coded ABI layouts. Offline Cargo changed only aero-cli's libc edge: package/version/checksum identities are unchanged, no package download/install/update. Linux default SIGCHLD policy is queried, never modified. No provider/account/browser session/host-security setting or database touched. Diagnostic group cleanup is not renderer/runtime qualification; ENV-002/ENV-004 stay unverified.

Stop boundary: BUG-001–004 and the completed audit remain ready_for_review, not closed/committed; no remaining pre-N01 code blocker was identified after review and parent verification. No staging, commit, push or N01. After the fixes are reviewed/approved, N01 remains the next separate batch (`crates/aero-core/tests/task_contract.rs :: contract_is_strict_and_roundtrips`), before B04–B06 storage. Historical B03/B02 records below retain their original evidence but no longer describe current BUG-001 authorization/state or final audit totals.

## Current B03 checkpoint

Historical B03 checkpoint: superseded for BUG-001/DEC-008 and next action by the current audit record above; the B03 feature evidence remains valid.

Approval: after the B03 form/checkpoint/public-event scope and outstanding schema decisions were explained, the user said, “Great, let's proceed then.” This authorizes the pure B03 contracts and synchronized documentation, not commits/pushes (including B02), N01 TaskContract, persistence/encryption, browser capture/hydration, provider/account operations or UI work.

Implemented scope:

- `crates/aero-core/src/form.rs`: JSON array entries with strict locator/typed value objects, preserving empty text, Unicode, false checks, radio-none versus selected empty value and empty/multiple selections. Validators check locator shape/count/duplicate identity and control/value compatibility; no raw Debug on private aggregates.
- `crates/aero-core/src/ids.rs`: nominal TaskId/RunId/EvidenceId/ArtifactId using the existing opaque-string convention. IDs remain unnormalized and do not establish issuance, ownership or authority. N01 will reuse them, not introduce duplicate Task/Run types.
- `crates/aero-core/src/checkpoint.rs`: version-1 checkpoint, expected RunContext, cumulative task counters, SQL run statuses, explicit complete/partial/nonresumable capture report, runtime/document/observation generations and tab/frame/form inventory with evidence/artifact references. Scope/counter/cursor/uniqueness/aggregate limits are validated. Original encoded bytes have a separate ≤2 MiB size guard; inventory caps are 8 tabs, 20 origins and 2,000 total entries, never silently truncated.
- `crates/aero-core/src/events.rs`: version-1, sequenced public status/usage/step-outcome metadata. Dispatch and verification are separate; uncertain+passed is rejected. There is no raw form/action/diagnostic/authority payload field or executable completion setter. Opaque ID strings are still arbitrary data: trusted producers must issue/resolve them and apply privacy/provenance policy.
- `crates/aero-core/src/wire.rs` and `src/lib.rs`: private schema-once, map-only deserialization helper shared by B03 objects, with required explicit nullable fields, unknown/missing/duplicate/array rejection, supported-version checks and payload-free validation errors re-exported at the core root. B02's action parser and its public error type are unchanged.
- Final source-review repair: `strict_tagged_object!` gates FieldValue/EventKind through map visitation before a generated serde remote decoder, preserving one variant declaration and all duplicate/unknown/missing-field checks. Derived internally tagged enums alone accepted positional arrays even inside strict surrounding structs. This B03 fix leaves B02's analogous Action parser untouched; its separate reproduced defect is BUG-001, not a completed fix.
- Documentation scope: README, CONTRIBUTING, implementation plan, traceability matrix, tracker and this handoff. The plan links to compiled models and replaces its inconsistent checkbox/Text illustration. Future N01/N04 reuse existing IDs/verification status. Manifests, lockfile, specification and B02 actions.rs are byte-for-byte unchanged against the B03 start baseline.

Observed TDD evidence on existing pinned Rust/Cargo 1.99.1, default features, WSL development host:

| Subtask | Actual red | Actual green |
|---|---|---|
| B03.1 form representation | Tests first against a minimal compilable scaffold; `cargo test -p aero-core form_ -- --nocapture` ran three tests, all failed, including `form_json_preserves_empty_and_false`'s array assertion and invalid-input rejection. | The same tests pass after complete serialization, map-only parsing and structural validation; no permissive serializer/validator remains. |
| B03.2 durable references | Temporary aliases made `ids_are_distinct` fail (six identities instead of ten); the new Task/Run compile-fail example incorrectly compiled. Both commands exited 101. | Nominal wrappers pass the ID suite, exact Unicode/whitespace serialization, nonstring rejection/map ownership and the Task/Run compile-fail check. |
| B03.3 checkpoint contracts | After fixing a test-only borrow error, `cargo test -p aero-core checkpoint_ -- --nocapture` ran six tests: five failed, exposing unsupported-version/context/counter/inventory/size acceptance; structural helper tests already passed. | Required version and validators made the same six pass, including `checkpoint_version`, zero/max/gapped cursor cases and aggregate entry/byte limits. |
| B03.4 public events | `cargo test -p aero-core public_event_ -- --nocapture` ran five tests: four failed, exposing unsupported versions, wrong context/counters, private payload extras and uncertain+passed acceptance. | All five pass with strict version/kind fields, shared validation and distinct dispatch/verification. |
| B03.5 schema-review refinements | `form_value_requires_compatible_control_metadata` and `checkpoint_capture_completeness_is_explicit` each ran once and failed assertions, exit 101, before their changes. | Both pass after explicit capture-status serialization and control/value compatibility. Fixtures use distinct matching locators; no raw Debug was introduced. |
| B03.6 tagged-object enum repair | Final reviewer flagged derived tagged-enum sequences. Parent added direct/nested variant-array regressions; `form_objects_are_strict_at_every_level` and `public_event_objects_are_strict_and_required` each ran once and failed, exit 101, accepting Text and StatusChanged arrays respectively. | Shared map-only tagged decoding makes both pass for all four FieldValue/three EventKind variants, direct and nested; existing positive object roundtrips, nullable presence, duplicates and private-field rejections remain green. A now-unused serde import was removed after Clippy detected it. |

Current observed gates after those refinements:

- `cargo test -p aero-core --locked`: 48 unit tests, one import test and seven compile-fail doctests passed, no failed/ignored/filtered tests. B03 adds 17 unit tests and two compile-fail examples over B02.
- `cargo test --workspace --locked`: 57 behavior tests (48 core unit, one import, eight CLI) and seven compile-fail doctests passed. Empty runtime scaffold suites are not feature coverage.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: exited 0. Normal core dependency graph remains serde/derive only; serde_json stays test-only.
- All core/workspace/build/fmt/Clippy gates passed again after the final tagged-enum repair; `RUSTDOCFLAGS="-D warnings" cargo doc -p aero-core --no-deps --locked` also exited 0. The existing suite has no failed/ignored/filtered tests, but the separate known B02 scratch probes below still fail and block B02.
- Updated plan §7.10 completion-predicate example compiled against the actual existing verification-status enum in a scratch harness and passed its one test. This is documentation example evidence, not an implemented N04/N05 completion gate or an extra production test.
- Contributor-guide documentation command executed verbatim: PASS for 12 documents, 26 requirements, 31 corrections and 100 ledger packages. Extended final validation also passed 148 local links/anchors, 57 matrix rows, all 100 package definitions, two plan JSON examples, implemented matrix test names, tracker index/record equality, explicit B02 blocked/B03 ready_for_review/N01 not_started lifecycle checks and direct untracked-source whitespace checks; `git diff --check` passed. An initial extended-check regex missed bold/heading-style package definitions; correcting the checker, without changing valid package records, made full ledger/definition set comparison pass.

Review/proof boundaries: schema review confirmed the task/run/cursor design and led to capture completeness, control/value compatibility and explicit persistence/provenance boundaries. The first read-only source review found no defect outside those then-in-flight refinements; its three nonblocking suggestions are incorporated: raw duplicate nullable locator fields in both null orders, every NotDispatched verification report, and negative gapped/positive task-cumulative cursor cases. These test additions were already green against the existing implementation; they are not claimed as newly reproduced defects. Core/workspace/build/fmt/Clippy and preservation gates passed again afterward, with counts unchanged. Final whole-slice review found the tagged-enum array gap; the parent reproduced and repaired it in B03, checked sibling paths, and reran the gates. No remaining B03 source defect was identified; B02's reproduced sibling issue remains open, not covered by that statement. B03 reports capture status but does not run capture or prove that producer reports are truthful. Raw form/URL data is pre-encryption: the entire Checkpoint must never be written wholesale into plaintext SQL metadata_json or public events. B04/B05/F01/F05 own the safe metadata/authenticated-encrypted-payload split and missing-key failure; none is implemented here. Selectors, origins and captured IDs need real DOM/permission/freshness checks later. Context comparisons require a separately trusted expected value; reference existence, history/usage monotonicity, actual grants, event ordering/reconnect, independent verification and task completion remain future owners. F01/F02/F03 still own comprehensive capture eligibility, classification/redaction and hydration. Capture/size-limit failures must be reported as partial/nonresumable by future capture/recovery controllers, not silently dropped. ENV-002/ENV-004 are unchanged and unverified; Hivemind was not used.

Outstanding B02 sibling defect: a separate scratch harness compiled against the actual built library accepted Action `["click","target-1"]`, directly and in a valid version-1 Decision. `action_array_is_rejected` and `nested_action_array_is_rejected` both failed (exit 101). These two failing probes are outside the green workspace total and must not be hidden by it. Source/binary/log are `/home/davia/.hermes/cache/scratch/aero-b03-sibling-array-probe{.rs,,.log}` (local/ephemeral). BUG-001 blocks B02 readiness/commit; DEC-008 asks permission to change the explicitly preserved B02 parser/tests. The approval question timed out without response; no approval or rejection was inferred. B02 actions.rs, manifests, lockfile and specification remain unchanged.

Git/stop boundary: main remains af88105 with the earlier local-only README/handoff commit; B02 work was already uncommitted when B03 began. B03 adds uncommitted work without staging, committing or pushing either batch. No browser/server/provider/database was started. Read-only schema/source reviews have no write ownership. Next action is owner review and DEC-008's narrow B02-fix decision; fix/verify BUG-001 before B02 approval/commit and moving to separately approved N01, not automatic scope expansion or publication. After its separate approval, N01 begins at `crates/aero-core/tests/task_contract.rs :: contract_is_strict_and_roundtrips`; `cargo test -p aero-core --test task_contract contract_is_strict_and_roundtrips -- --exact` must eventually execute the actual test. N01 still precedes B04–B06 storage.

## Current B02 checkpoint

Retained B02 evidence: the current B03 record above supersedes its next-batch instructions and readiness claim. B02's action source remains unchanged and uncommitted, with newly reproduced BUG-001 blocking its object-only parser guarantee. Historical passing tests below did not cover tagged action arrays.

Approval: after the B02-only scope/limits/testing/stop boundary were explained, including the missing explicit wire version, the user said, “Ok, fantastic! Let's get started then.” This authorizes B02 implementation and its synchronized documentation, not a commit/push, B03, a new dependency, provider/account operation or browser/UI expansion.

Implemented scope:

- `crates/aero-core/src/actions.rs` and its `src/lib.rs` export: the 18 preplanned action variants, required `schema_version=1`, existing typed `ObservationId`, public rationale, strict object-only decision deserialization, strict action tags/fields, and pure `Decision::validate()` / `Action::validate()`.
- Validation: mandatory version, nonempty opaque references, named-key allowlist, conservative HTTP(S) syntax subset (ASCII DNS/canonical IPv4/bracketed IPv6 hosts, decimal u16 ports, well-formed percent escapes; no userinfo or browser-repair forms) and exact navigation-only `about:blank` exception, finite/bounded scroll, UTF-8 byte limit for typing, Unicode-scalar limits for rationale/summary, wait/extraction/batch bounds and csv/parquet format names. Validation errors contain static field/reason strings, never rejected payloads.
- Documentation: `README.md`, `CONTRIBUTING.md`, `docs/implementation-plan.md`, `docs/requirements.md`, `docs/bug-fix-tracking.md`, `docs/handoff.md`. The plan links to compiled Rust instead of retaining a competing uncompiled action definition. Product requirements and original source artifacts unchanged. No manifest, lockfile or dependency change.

Observed TDD evidence on Rust/Cargo 1.99.1 in the existing WSL workspace, default features:

| Subtask | Red observation | Green evidence |
|---|---|---|
| B02.1 strict parsing/version/validation | Tests were written first; initial missing-symbol compilation exited 101, then a deliberately permissive scaffold allowed real assertions to run. `cargo test -p aero-core decision_ -- --nocapture` executed 12 tests: 10 failed, including all three required names, with ignored fields/unknown actions/unsupported versions/unvalidated parameters exposed. | The same command passed all initial 12 tests after strict parsing and validators; deliberate unknown-action fallback and always-OK validator removed. |
| B02.2 object envelope | Review found serde's derived struct parser also accepted a positional JSON array. `cargo test -p aero-core decision_requires_an_object_envelope -- --nocapture` ran once and failed its rejection assertion, exit 101. | A map-only deserializer with required/duplicate/unknown-field checks made the identical test pass. Optional viewport scroll target also tested; the then-focused suite passed 14 tests. |
| B02.3 conservative URL syntax | Read-only contract review recommended rejecting malformed authorities rather than accepting any HTTP-shaped string. `cargo test -p aero-core decision_rejects_malformed_url_syntax -- --nocapture` ran once and failed, exit 101, exposing userinfo/bad-host/bad-port/bad-percent acceptance across navigation, new-tab and metadata variants. | Shared syntax helpers made the identical test pass without a parser dependency; Unicode paths, trailing-dot DNS, canonical IP hosts, port boundaries and valid escapes remain accepted. Unsupported raw Unicode hosts/IPv4 repair forms are explicit. Final focused decision suite: 15 passed, no ignored tests. |

Actual shared gates after both review repairs:

- `cargo test -p aero-core --locked`: 31 unit tests, one import test and five compile-fail doctests passed, no failed/ignored/filtered tests.
- `cargo test --workspace --locked`: 40 behavior tests (31 core unit tests, one core import test, eight CLI tests) and five compile-fail doctests passed; scaffold suites still contain no feature tests.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: all exited 0.
- `cargo tree -p aero-core --edges normal` still contains only serde and its derive machinery; no runtime/browser/storage/provider dependency was introduced.
- Contributor documentation command executed verbatim: PASS for 12 documents, links/fences/whitespace, 26 requirements, 31 corrections and 100 ledger packages. Extended checks passed for 12 tracked Markdown documents (including README), 140 local links/anchors, 57 unique matrix rows, two parsed JSON blocks, 18 action variants, 15 decision tests and the exact eight-file scope; only B02's ledger state changed and the Git index is empty. Direct new-source/whitespace inspection and `git diff --check` passed. These are documentation/scope checks, not extra application tests.

Proof boundaries: the DTOs do not execute actions, establish grants/freshness, fill credentials, export Parquet or mark a task successful. Callers must parse original response bytes directly then validate, and later trusted policy still owns authorization. The conservative URL subset is not full browser/WHATWG/IDNA parsing, origin/redirect/SSRF policy or network confinement; future URL consumers must parse/resolve and policy-check effective destinations. Raw Unicode hosts need already encoded ASCII IDNA; IPv6 zones and browser-repair forms are unsupported. B02 does not supply a global wire-payload cap or arbitrary-text redaction. Raw serde diagnostics and proposal Debug output can contain input and are not sanitized audit records. SPEC-26/SPEC-27 advance only partially; cross-surface privacy/history/replay acceptance remains future work. ENV-002/ENV-004 remain unverified and unchanged.

Review outcome: integrating self-review and the read-only source review found no remaining B02 blocker. The source reviewer excluded URL tests while those helpers were changing; the integrating implementer reviewed the conservative syntax repair and independently ran its focused regression and the complete workspace gates. Three suggested raw-JSON coverage cases were added for duplicate envelope rationale/action and duplicate scroll target after null; the existing implementation rejected all three without further production changes. The full focused/workspace/build/fmt/Clippy gates were rerun successfully after those fixtures. This is ready for owner review, not commit approval or product acceptance.

Git/publication: resumption checks found clean main at af88105, one commit ahead of origin/main. Independent `git ls-remote origin refs/heads/main` read back 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd. af88105 remains the previous local README/handoff commit, unpushed; B02 changes are additionally uncommitted. No staging, commit or remote write performed. No browser/server was started; read-only contract/source-review children were used, with no write ownership, and both have completed.

Next safe action: owner review of B02 and separate explicit commit/push approval if desired. Do not implement B03 automatically. After its own approval, B03 starts with `form_json_preserves_empty_and_false` in `crates/aero-core/src/form.rs`, then `checkpoint_version` checks per the plan; N01 TaskContract still precedes B04–B06 storage. No Rust/browser provisioning or B01 repeat is needed. Hivemind remains unavailable and was not used.

## End-of-session handoff

Historical 2026-10-22 stop record: the current B02 checkpoint above supersedes these restart instructions; preserve this section as B01 publication/approval evidence.

Stop instruction: the user said, “ok update the hand-off and we stop here tonight,” then requested, “update the main README.md and then do a final commit for the night.” This authorizes one final documentation commit containing README.md and this handoff, not B02 implementation or another push.

- At 2026-10-22 21:55 +08:00, local main and remote main were independently checked at 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd; the working tree was clean before this handoff edit. The preceding publication check also confirmed the same SHA through the GitHub API.
- Both B01 commits are pushed. The final documentation scope is `README.md` plus `docs/handoff.md`, committed locally under the user's final-night request. Use Git for that documentation commit's actual SHA; it is not self-embedded here. The last verified published checkpoint is 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd; this final documentation commit is not pushed. Recheck local/remote state next session rather than assuming they still match.
- Last application gates: 25 behavior tests and five compile-fail doctests passed, with workspace build, formatting and warning-denied Clippy. These were rerun before the approved B01 commit; they are not claimed rerun for this documentation-only stop note. Documentation validation is used for this edit.
- No background job, server, browser session or automatic continuation was started by this batch. No cleanup or native qualification is implied for unrelated machine processes.
- Remaining gates: ENV-002 browser/sandbox/provenance qualification and ENV-004 native display/keyring/platform evidence remain unverified. AeroBrowser is not yet a usable browser; handle-map freshness, task execution, permissions, storage/recovery, providers and desktop remain future work.

Next session:

1. Read this handoff and recheck Git status/log and local main versus origin/main. The final README/handoff commit is local-only; obtain a push request before publishing it. Do not repeat Rust provisioning, bootstrap, README or B01 work.
2. Obtain approval for B02 only: strict versioned action/decision contracts in `crates/aero-core/src/actions.rs` and its `src/lib.rs` export, following the canonical plan §7.1 and B02 assertions. B02 is not authorized by tonight's stop instruction.
3. Once approved, add the named decision tests first (`decision_is_strict_and_round_trips`, `decision_rejects_limits`, `decision_rejects_unknown_action`); run `cargo test -p aero-core decision_ -- --nocapture` and obtain meaningful red before minimal implementation. A zero-test run is not proof.
4. Run core/workspace/build/fmt/Clippy and documentation gates, update traceability, then stop for review before committing. Later sequence remains B03, N01, then B04–B06; no browser/provider/UI expansion is part of B02.

Environment reminder: Cargo may be absent from a fresh shell's PATH even though the pinned toolchain is installed. Use `export PATH="$HOME/.cargo/bin:$PATH"` before Rust checks; do not reinstall or change the pin to work around PATH.

## Current B01 checkpoint

This preserves the approved B01 checkpoint; the current B02 record above supersedes its next-session instructions. The BATCH-00 and documentation-only sections below are historical evidence, not instructions to repeat setup. The user approved B01 after a proposal to implement typed IDs and safe geometry, then stop for review before any commit. After the review report and passing checks, the user explicitly said “commit and push.” That authorizes publication of this B01 batch and its approval/status record, not implementation or publication of later packages.

Implemented scope:

- `crates/aero-core/src/ids.rs`: six distinct string-backed IDs, private inner strings, exact serde string round trips, owned map-key behavior, representative compile-fail cross-ID checks. These types do not generate or validate external identities, establish authority, or invalidate handles by themselves.
- `crates/aero-core/src/types.rs`: `CssRect::clipped_center`, finite/positive/representable bounds, visible intersection, overflow-resistant midpoint and strict interior checks; validated `FrameLocalRect`, `RootViewportPoint` and `ScreenshotPixelPoint` without implicit conversions. Clipping stays in its input space; there is no frame projection, DPR conversion or current-browser hit testing.
- Core `Cargo.toml`, `src/lib.rs`, root `Cargo.lock`: module exports, existing workspace serde dependency and test-only serde_json. No newly resolved package versions; normal core dependency tree remains serde and its derive machinery, with no browser/storage/provider runtime.
- Status/traceability: `README.md`, `CONTRIBUTING.md`, `docs/implementation-plan.md`, `docs/requirements.md`, `docs/handoff.md`, `docs/bug-fix-tracking.md`. Requirements unchanged. The plan now explicitly assigns the actual epoch-bound element handle map to its D01/D02 observation/input owners; B01 only supplies the types. Its mandatory freshness behavior is not waived or implemented here.

Observed TDD evidence (all deliberate red commands exited 101, not missing-tool failures):

| Subtask | Red observation | Green evidence |
|---|---|---|
| B01.1 identifiers | `cargo test -p aero-core ids_are_distinct -- --nocapture` failed against string aliases; `cargo test -p aero-core --doc` failed because cross-ID examples incorrectly compiled. | Four ID unit tests and three ID compile-fail doctests pass with nominal wrappers. |
| B01.2 clipping | `cargo test -p aero-core center_uses_visible_intersection -- --nocapture` failed against a method returning None. | Named test passes; nonfinite inputs, nonpositive dimensions, offscreen/empty area and overflowing bounds are covered. |
| B01.3 rounded boundaries | `cargo test -p aero-core center_rejects_unrepresentable_interior -- --nocapture` failed when the calculated midpoint rounded onto an edge. | Strict interior predicate rejects that case; `center_keeps_small_representable_intersections` still accepts representable subnormal interiors. Seven geometry tests pass. |
| B01.4 coordinate spaces | `cargo test -p aero-core coordinate_ -- --nocapture` exposed the screenshot/root alias and unvalidated points; `cargo test -p aero-core frame_local_rect_rejects_invalid_or_unrepresentable_bounds -- --nocapture` exposed the unchecked constructor; doctests exposed accepted cross-unit calls. | Five coordinate-space tests and two coordinate compile-fail doctests pass. |

Actual shared gates on Rust/Cargo 1.99.1 in the existing WSL workspace, default feature selection:

- `cargo test -p aero-core --locked`: 16 unit tests, one pre-existing import test, five compile-fail doctests passed.
- `cargo test --workspace --locked`: 25 behavior tests (including all eight pre-existing CLI tests) and five compile-fail doctests passed; no failed, ignored or filtered tests. Empty scaffold suites are not feature coverage.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --locked`: all exited 0.
- Contributor documentation checker: PASS for 12 documents, 26 requirements, 31 corrections and 100 ledger packages. Extended validation: 12 tracked documents, 135 local links/anchors, 57 unique matrix rows; only SPEC-17/SPEC-19 advanced partially. Updated §7.3 reference code separately compiled and passed its three tests; this is additional documentation evidence, not extra production tests. Final diff/whitespace and exact 11-file scope checks passed, including direct inspection of the two new source files; index empty and lock package versions/provenance unchanged. Application acceptance is not inferred from these checks.

Review/state: implementation self-review found no remaining B01 primitive defect; integrating checks above pass. The full workspace/build/fmt/Clippy and documentation/scope gates were rerun successfully immediately before committing the approved exact 11-file batch as 54e0a9573d8eb90746243f0f24056b05a3022988, `feat(core): define scoped IDs and CSS geometry`. Approval/status commit 1b2a3daa03f3ac36dea1c8be7620584bec96e3bd followed, and both were pushed to main. Local HEAD, origin/main, remote main and the GitHub API were read back at the same final SHA; the working tree was clean after publication. Ledger state is committed. The end-of-session section above records the subsequent local README/handoff documentation commit. No browser, provider, account or host-security operation was needed, and no long-running process was started by this batch. ENV-002/ENV-004 remain unverified native/runtime gates, not pure-core blockers.

Work is paused at the user's request. Next session, propose B02 strict action/decision contracts, then B03 snapshots/events and N01 TaskContract before B04–B06 storage. Do not begin those packages tonight or as part of finishing B01. They require separate approval.

## 0. Previous BATCH-00 checkpoint

This section supersedes the historical documentation-only/environment snapshot below. The user approved first-phase implementation and subsequently said, “ok, let's commit and push.” That authorizes the reviewed bootstrap baseline and checkpoint update to private main, not later features, real accounts/providers, license/redistribution choices, public release or host-security weakening.

- A01: parent read all five requirements/architecture/ADR documents and independently validated exactly 26 requirement and 31 correction rows, canonical package/test-name mappings, all local file links and anchors. Existing suite/manual gates and missing dedicated SPEC-23/SPEC-26 behavioral proof names are disclosed, not invented or considered application coverage. Requirements remain not_started until actual acceptance. Documentation is ready for user review.
- A02: official user-local Rust/Cargo 1.99.1 with rustfmt/clippy provisioned; compiler pinned in rust-toolchain.toml. GCC and Bubblewrap already present. User-installed pkg-config 2.5.1 verified at /usr/bin/pkg-config (canonical /usr/bin/pkgconf), resolving ENV-003 for bootstrap availability. Native platform/keyring/sandbox and future native-library gates remain unproven.
- A03: exact six-crate workspace implemented and metadata checked; minimal core import/version test passed after observed behavioral red. Manifests, lockfile and libraries are scaffolding, not implemented task/CDP subsystems.
- A04 baseline: real CLI --help/--version/argument handling and bounded prerequisite/version/hash diagnostics implemented. Eight CLI tests passed after observed red; explicit empty/missing path never falls back, hung versions are killed/waited, oversized output rejected, failed stderr hidden, child environment cleared. No browsing session is launched.
- Development browser: Chrome for Testing 156.0.8078.47 Linux64 was provisioned in the user's private cache. Real --version and Rust local executable-pin check passed. Vendor archive MD5 was compared against separately fetched Storage response headers; SHA-256 values are local byte pins, not independent vendor signatures. Redistribution is not approved. Details: [development.md](development.md).
- Intentional unready report: follow-up actual doctor JSON reports available browser, Bubblewrap and pkg-config 2.5.1, matched_local_pin, sandbox/user namespaces/keyring not_checked, display not_configured, runtime_ready=false; exit 2 is correct. This does not satisfy C04/G0/G1.
- Git: main, origin git@github.com:dave2k77/aero-browser.git; root commit 5a4fac91e5638735aa1cd762c183f63c9efa39de contains exactly the reviewed 32 text files. PDF, browser binaries, target/runtime data/secrets and .hermes artifacts are excluded. Cargo.lock is tracked. Check Git for the checkpoint-update commit and current push verification; do not infer remote delivery from this static document.

Actual red/green and verification evidence:

| Subtask | Observed result |
|---|---|
| A03.1 initial import introduction | cargo test -p aero-core --test import first exited 101 with missing manifest/module, then the named test ran and failed its empty VERSION versus 0.1.0 assertion. Replacing VERSION with env!(CARGO_PKG_VERSION) made the same test pass once. |
| A03.2 workspace boundary | cargo metadata --format-version 1 --no-deps parsed and checked exactly aero-core, aero-cdp, aero-perception, aero-executor, aero-agent and aero-cli. |
| A04.1 diagnostic behavior | cargo test -p aero-cli --test doctor ran eight named tests to behavioral red before implementation. An intermediate run passed seven and exposed Clap's rejection of an explicit empty PathBuf; OsString parsing fixed it without fallback. Same suite then passed all eight. |
| A04.2 integrity helper | bash -n scripts/lock-browser.sh passed; actual candidate check exited 0, nonexistent browser check exited 1. Neither result claims a working sandbox. |
| Shared quality gate | cargo test --workspace --locked passed the core import test and eight CLI tests; scaffold libraries have no behavior tests yet. cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked -- -D warnings, cargo build --workspace --locked all exited 0. |
| Documentation integration | Contributor checker executed: PASS for 12 documents, links/fences/whitespace, 26 requirements, 31 corrections and 100 ledger packages. Extended validation passed 123 local links including anchors, 57 unique matrix rows and canonical named proofs/explicit suite/manual gates. The parent's real checks supersede the subagent's blocked validator command; no child self-report was accepted as proof. |
| Working-tree boundaries | git diff --check passed (tracked diff is empty before first commit); independent whitespace/scope inspection checked all 32 project source/config/doc files. Historical PDF SHA-256 unchanged; target/PDF/local Hermes plan ignored; Git index empty and HEAD absent. GitHub read-back confirmed PRIVATE and isEmpty=true. |

Historical next action was B01 preparation; the current B01 checkpoint above supersedes it. Bootstrap review/commit approval did not authorize later commits. Native/runtime qualification remains incomplete.

## 1. Start here

Workspace: `/home/davia/projects/aero-browser`.

Read in this order:

1. [Specification](specification.md): authoritative requirements, product scope, and invariants.
2. [Implementation plan](implementation-plan.md): §0 priorities/live ledger, §3 source corrections, §6 TDD protocol, §7 reference contracts/code, §8 dependency-ordered packages, §9–10 acceptance.
3. [CONTRIBUTING.md](../CONTRIBUTING.md): ownership, security, verification, review, and commit rules.
4. [Bug/fix tracker](bug-fix-tracking.md): current environment blockers and pending owner decisions.

The old `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md` path redirects to the live plan. Do not rebuild a second plan or use the historical PDF as an implementation status report.

Document ownership:

- Specification: requirements and stable REQ-* IDs.
- Plan: task dependencies, exact implementation/test paths, contracts, live package states and owners.
- Tracker: actual bugs, environment blockers, decisions, regression/closure evidence.
- Handoff: current verified checkpoint, approvals, changed files, next safe action, and transfer notes.
- Contributor guide: recurring working rules, not another task/status backlog.

The project owner approves requirements/scope, setup, commits, and external actions. Hermes is the integrating implementer. Bootstrap, README and B01 were separately approved and committed; later batch approvals remain separate. A01's child owned only its five documentation paths and has completed that work.

## 2. Historical documentation baseline — superseded by §0

### What exists

| Artifact | State / use |
|---|---|
| `aero-browser-specification-chat.pdf` | Original 96-page historical input, preserved unchanged. The prior review recorded complete page coverage. Do not transcribe its Rust samples as tested source. |
| `docs/specification.md` | Authoritative revision 2.0; 26 stable REQ-* requirements and nine approved recommendation groups. Authority link now points to the live plan. |
| `docs/implementation-plan.md` | Published detailed plan: 31 SPEC-* corrections, 70 A–I packages, 24 N packages, six separately gated J tracks, complete initial 100-package ledger. All application work is unimplemented. |
| `docs/handoff.md` | This current checkpoint and transfer record. |
| `CONTRIBUTING.md` | TDD/review/approval/security/parallel ownership rules and executable documentation check. |
| `docs/bug-fix-tracking.md` | Local authoritative issue register. Four environment records and seven pending owner-decision records; no reproduced application bugs. |
| `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md` | Compatibility redirect, not another live plan. |

No source tree, Cargo manifest/lockfile, frontend manifest, build script, CI workflow, application test, benchmark report, package, Git repository, branch, or commit exists. All application paths in the plan are future paths. `git status --short` and `git branch --show-current` returned `fatal: not a git repository`; do not report the directory as a clean Git checkout.

### Observed environment — read-only inspection on 2026-10-22

| Item | Observed result |
|---|---|
| OS | `/etc/os-release`: Ubuntu 26.04.1 LTS, version ID 26.04. |
| Kernel/platform | Linux 6.18.33.1-microsoft-standard-WSL2, x86_64. |
| Release baseline | Native Ubuntu 24.04 x86_64 is the approved target, not the host just inspected. |
| Available tools | Bubblewrap 0.11.1; Node v24.18.0; npm 12.2.0. |
| Not found on PATH | cargo, rustc, rustup, chromium, chromium-browser, google-chrome, pkg-config, flatpak-builder, promtool. This is a PATH observation, not a full-filesystem inventory. |
| Native display/keyring/sandbox | Not exercised. WSL is not proof of native X11/Wayland, Secret Service, renderer sandbox, accessibility, or IME behavior. |
| Local instructions | No AGENTS.md found at `/home`, `/home/davia`, `/home/davia/projects`, or this root; no root CLAUDE.md or .cursorrules found at discovery. Recheck if later supplied. |
| Organization memory | Hivemind capture/recall disabled because organization credits are exhausted; not used as an implementation dependency. |

No dependency was installed, browser/provider/account operation performed, source code generated, Git initialized, commit made, or external system changed for this document-set delivery. No long-running process was started by this work; ambient machine processes were not exhaustively inventoried.

## 3. Decisions and invariants to preserve

Product: a Linux-native browser for delegated work with verified outcomes, explicit scoped permissions, evidence-linked artifacts, and reliable manual control. First useful workflow is deterministic local supplier research producing ordered CSV, sourced report, unknown/exclusion labels, manifest, and inspectable evidence. No account creation, submission, upload, or purchase is needed for that proof.

Architecture: six initial Rust crates; pure core models, owned CDP/runtime, separate perception/input libraries, one role-scoped application interface, and SQLite-first durability. Initial service is in-process; desktop/CLI/MCP are adapters. Tauri's trusted WebKit UI is not the controlled Chromium page. Run the actual-viewport feasibility gate before committing to substantial desktop work.

Non-negotiable invariants:

- Durable TaskContract/revisions and cumulative budgets survive runs, provider changes, reconnects, and recovery. Browser IDs/handles remain transient.
- Planner proposes; trusted policy authorizes; executor acts; independent verifier checks; journal records. Model completion/dispatch/citation presence alone never proves success.
- Every real effect path, including Enter/replay/MCP/workers, uses the same scope/preview/expiry/quota gate. Model/page/tool/procedure can request approval, never issue it.
- Intent and resource reservations are durable before dispatch. Unresolved external effects become `needs_review`, never silently replayed. Local DB transactions cannot promise exactly-once remote effects.
- Processor/tool/export transfers use the data-flow gateway. Local-only processing cannot fall back remotely and does not claim offline website browsing/full egress confinement.
- Secret sentinels are tested across planner/log/audit/screenshot/artifact/export surfaces; no raw credentials or private pages in docs, fixtures, bug reports, or traces.
- Pause/close blocks later dispatch, interrupts read-only/planner waits, reconciles bounded input, flushes state, and releases owned processes. Takeover requires acknowledged exclusive ownership; resume observes fresh state.
- Browser recovery is best-effort and visibly partial. No universal Undo, lossless JS/IndexedDB restoration, stealth, native hardware-input, or zero-exposure claim.
- Keep Chromium's own sandbox. No insecure fallback flag, permissive mount, plaintext snapshot fallback, or strict-mode downgrade to get tests green.

Lean defaults: CSV, sequential work, bounded history/SQLite, minimal default graph, native distribution first, pause-on-close. Heavy retrieval/vision/Parquet/workers/metrics/MCP, daemon extraction, Flatpak, remote workers, and training need separate selected scope and evidence; they are not first-workflow prerequisites.

### Ordering repairs incorporated into revision 2.1

The plan's new operational entry clarifies early baseline N09 close handling, early F08 cancellation/timeouts, a B05/F10 prepared-intent safety regression before dispatch, browser permission deny defaults before N15 expansion, bounded history before optional retrieval, and task-owned cleanup when artifacts first persist. Pure Rust bootstrap can proceed after its approved prerequisites without waiting for every optional native/browser tool. The geometry reference now rejects overflowed rectangle bounds and avoids overflow in finite midpoint calculation, with a named future regression test; it remains uncompiled reference code. These clarify existing requirements rather than add a second architecture.

## 4. First implementation batch

This is the original BATCH-00 checklist; actual implemented progress and remaining blockers are in §0. BATCH-00 was not permission to execute during the earlier documentation-only delivery; the later user approval and current scope are recorded above. Do not execute A–I–N alphabetically. Detailed dependencies and contracts remain in implementation-plan §8.

### 00.1 — owner review and authorization

Read the four living documents. Resolve applicable DEC-001 setup scope and DEC-002 local Git authorization. Record the owner's actual response and excluded actions in the tracker. Installing tools, initializing Git, committing, provider use, account operations, and publishing are separate actions; approval of this documentation is not all of those permissions.

Read-only preflight from the active workspace:

```bash
pwd
git status --short
git branch --show-current
python -c 'import shutil; print({x: shutil.which(x) for x in ("cargo", "rustc", "rustup", "bwrap", "pkg-config", "chromium", "chromium-browser", "google-chrome")})'
```

Expected at this baseline: the recorded workspace, Git's not-a-repository failures, missing tool paths as above, and an available bwrap path. These are observations to recheck, not failing application tests. If state differs, update this handoff before proceeding.

### 00.2 — A01 documentation traceability, in small edits

Create only the missing A01 paths, leaving accepted requirements intact:

1. `docs/requirements.md`: one row per REQ-* and SPEC-* ID, linked source, plan package(s), exact planned test path/name, required milestone/feature, and `not_started` until real proof exists. No completed checkbox based solely on document text.
2. `docs/architecture.md`: dependency direction and explicit Task/Run/BrowserSession, trusted service/role, gateway, journal, runtime, verifier, and artifact ownership. Reference rather than duplicate wire types/SQL.
3. `docs/adr/0001-ui-engine-boundary.md`: current native-companion/stream candidates and N10 acceptance matrix; decision remains pending actual feasibility, not a fabricated accepted result.
4. `docs/adr/0002-planner-and-mcp.md`: configured inference adapter versus restricted tool protocol; no agent approval issuer and no assumed MCP sampling inference server.
5. `docs/adr/0003-checkpoint-semantics.md`: prepared intent, cumulative reservations, atomic outcome/checkpoint/cursor, uncertain remote effects, bounded runtime rebuild, and explicit partial restoration.

Run the contributor guide's documentation checker and inspect the new matrix for 26 requirement and 31 correction IDs with no missing/duplicate ID. Extend the checker to include the new documents and their actual links when adding them. Expected: real passing document checks, not Rust/browser results. Submit this small documentation batch for review before treating A01 as verified or committing it.

### 00.3 — A02 setup in the approved environment

Perform only the owner-approved setup/download scope. Use that environment's actual OS/package manager, not a blind paste of Ubuntu 24.04 instructions onto Ubuntu 26.04 WSL. Capture `rustc --version`, `cargo --version`, `bwrap --version`, and `pkg-config --version` in `docs/development.md` when those tools exist. Browser source/version/provenance is a separately validated A04 dependency; do not fabricate upstream checksums or change sysctls/AppArmor to bypass a launch failure.

Update ENV-001/ENV-003 only with observed results. ENV-002 remains blocked until a real Linux browser and its required sandbox tests pass. Native platform evidence remains unverified until actual execution.

### 00.4 — A03 minimal scaffold, then a real core import test

Exact initial files: root `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, and `Cargo.toml` plus `src/lib.rs` under each of `crates/aero-core`, `crates/aero-cdp`, `crates/aero-perception`, `crates/aero-executor`, and `crates/aero-agent`; `crates/aero-cli/Cargo.toml` and `crates/aero-cli/src/main.rs`; resolve `Cargo.lock`. Add a focused import test at `crates/aero-core/tests/import.rs`. No desktop crate, provider integration, runtime database, downloaded browser tree, or optional feature stack belongs in this scaffold.

Use the chosen pinned compiler, edition 2024/resolver 3, exact six members, shared dependency declarations, and explicit `aero` binary. Introduce the import test first and observe its initial missing-workspace/module failure honestly; scaffold minimally so it runs, then introduce meaningful behavior tests before feature implementation.

```bash
cargo metadata --format-version 1 --no-deps
cargo test -p aero-core --test import -- --nocapture
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected after implementation: metadata reports exactly the six initial members, the named import test executes and passes, and all commands exit 0. Missing tool/dependency setup is a blocker, not a meaningful behavioral red. No result above has been produced at this documentation baseline.

### 00.5 — A04 truthful doctor and browser lock

Files: `packaging/browser-lock.json`, `scripts/lock-browser.sh`, `crates/aero-cli/src/doctor.rs`, `crates/aero-cli/tests/doctor.rs`, and the necessary exports/CLI routing in `crates/aero-cli/src/main.rs`/manifest. Implement one doctor check at a time: missing browser returns code 2 and `browser.available=false`, actual path/version/provenance readback, unsupported sandbox/display/keyring explicitly distinguished from passed support. Doctor never downloads a browser or changes security settings.

```bash
cargo test -p aero-cli --test doctor doctor_missing_browser_is_nonzero -- --exact --nocapture
cargo test -p aero-cli --test doctor
cargo run -p aero-cli -- doctor --json
```

Expected red/green: named test fails before the behavior exists, then runs once and passes; doctor reports actual configured prerequisites and returns nonzero when required ones are unavailable. It must not claim full launch/sandbox validation before C04 supplies the corresponding real checks. Document partial doctor readiness instead of calling ENV-002 closed early.

### Batch stop and first feature test

Stop at a reviewed bootstrap/doctor checkpoint, record exact changed files and real evidence, and ask for scoped commit approval. If any prerequisite remains blocked, report the verified subset without declaring the whole batch complete. After review, the next pure feature is B01 `center_uses_visible_intersection` in `crates/aero-core/src/types.rs`, with complete code/test in implementation-plan §7.3 and the same focused command in CONTRIBUTING.md. Integrate N01 before a session-only persistence design can become established.

## 5. Current blockers and approvals

The tracker owns statuses; this is a navigation summary, not a duplicate issue board:

- ENV-001: user-local pinned compiler verified. ENV-003: user-installed pkg-config 2.5.1 and doctor availability verified; bootstrap blocker resolved. DEC-001 accepted the first-phase scope. Future native-library/backend validation is not implied.
- ENV-002: development browser version/byte pin matches; native sandbox integration and production provenance/update/redistribution review remain required before dependent claims.
- ENV-004/DEC-003: native platform/viewport/keyring evidence needed before dependent support claims; does not stop independent pure Rust work.
- DEC-002: GitHub creation/local initialization accepted and completed; source/PDF licensing remains under DEC-005 and commits still need explicit batch approval.
- DEC-004: real inference/provider/data/budget configuration, only before actual-model work.
- DEC-005: application ID/rights/update/signing/release ownership, only before its dependent release actions.
- DEC-006: held-out source and threshold governance; current status is `holdout_not_available`, with no invented baseline.
- DEC-007: optional backend/feature scope; lean defaults remain off until justified.

GitHub creation and the explained first-phase setup/implementation were explicitly authorized by later user messages. The tracker records their scope. No commit/push, provider/account use, signing/publication, browser redistribution or optional-backend approval has been recorded; setup approval is not blanket authority.

## 6. Verification record for this documentation delivery

Actual documentation checks passed on 2026-10-22, using Python standard-library validation and Node syntax checking without creating probe files:

- The complete read-only documentation command embedded in CONTRIBUTING.md was extracted and executed from the workspace root. Observed output: `PASS: 6 documents, links/fences/whitespace, 26 requirements, 31 corrections, 100 ledger packages`; exit 0.
- Extended validation checked 29 local document links including fragment anchors; 100 unique work-package definitions exactly matched the 100 initial `not_started` ledger entries; all four ENV and seven DEC records matched their indexes and references.
- One JSON fenced example and two Rust raw JSON fixtures parsed. One JavaScript block passed `node --check -` through stdin. Parsing JSON inside Rust is not compiling Rust.
- The SQL schema was exercised in memory using synthetic records: schema version, foreign keys, immutable contract/grant payload, nonnegative budget, transactional rollback, deletion cascades, and final foreign-key consistency passed. No application database was created or tested.
- The original PDF's SHA-256 remained `31d4abdce5b505c8c1e31886077d301df47f516e2a8fe64d047824a4cbf39d17`. Workspace inventory contained only the six living/redirect Markdown documents plus the original PDF; no app source, Cargo manifest, or Git directory was created.

Application build/tests, Rust snippet compilation (including the overflow-safe geometry reference), browser integration, actual-model/held-out evaluation, native GUI/keyring, packaging, and remote CI have not been run. Expected commands elsewhere are not evidence of execution. No `git diff --check` result is claimed because this is not a Git repository.

## 7. Parallel work and transfer discipline

After setup/contracts stabilize, independent core-model tests and CDP transport tests may be delegated with explicit non-overlapping paths. Serialize shared manifests/module exports and all store/controller/runtime/recovery ownership changes through the integrating implementer. N10 native validation can proceed alongside the CLI slice once its actual input/runtime prerequisites exist. No child can approve grants/commits or close tracked work solely from its own summary.

Before handing off, record:

```text
Checkpoint / batch and exact package.subtask:
Current workspace, branch, HEAD or no repository:
Owner / reviewer and writable file scope:
Approved action scope and pending decision IDs:
Actual changed files, including uncommitted/untracked work:
Red test command/name/result:
Green and affected-suite commands/results:
Artifact/effect/readback evidence:
Not run, unsupported environments, remaining uncertainty:
Plan-ledger and tracker updates:
Processes started by this work and exact cleanup/ownership:
Next test file, test name, exact command, expected behavior:
Commit approval and actual SHA, or no commit:
```

Do not erase the prior checkpoint's evidence or describe a task as complete because the plan was completed. Retain concise batch history below and link larger reviewed evidence records from the plan ledger. Never include credentials, private pages, runtime profiles, or hidden model reasoning.

## 8. Batch history

### BATCH-00 commit approval

User request: “ok, let's commit and push.” Primary implementation commit: 5a4fac91e5638735aa1cd762c183f63c9efa39de, feat: bootstrap AeroBrowser workspace and readiness diagnostics. All bootstrap tests, build, formatting, warning-denied clippy and documentation/staged-scope checks passed. This accompanying documentation checkpoint records the real primary SHA rather than inventing a self-referential commit ID. Delivery verification compares final local HEAD with remote main; excluded artifacts remain local.

### BATCH-00 — initial implementation baseline, 2026-10-22

Follow-up: user reported pkg-config installed; independent version/path and doctor checks confirmed 2.5.1. Bootstrap tests/fmt/clippy passed. All A01–A04 bootstrap scopes are now ready_for_review; native runtime/feature qualification remains unverified. A new terminal initially lacked the user-local Cargo PATH; restoring the already documented PATH was sufficient, with no new installation or shell configuration change.

- Implemented and exercised: pinned Rust scaffold, core import/version contract, CLI doctor, development browser pin check and source/requirement/architecture documentation. Actual red/green and quality evidence is recorded in §0.
- Source scope: root Cargo.toml/Cargo.lock/rust-toolchain.toml/.gitignore; manifests and lib.rs for core/CDP/perception/executor/agent; core import.rs; CLI manifest/main.rs/doctor.rs/tests/doctor.rs; packaging/browser-lock.json; scripts/lock-browser.sh. No task/runtime/provider/GUI implementation added.
- Documentation scope: docs/requirements.md, docs/architecture.md, ADRs 0001–0003, docs/development.md, plus current plan/handoff/tracker/contributor updates. Specification product requirements and original PDF unchanged.
- Installed outside repository: user-local Rust and testing-browser cache only. No system package install, security-policy change, commit, staging, push or file upload. Native prerequisite and integration limitations remain explicit.
- Status: A01/A03/A04 baseline ready_for_review; A02 blocked on native pkg-config setup; all later packages not_started. Future dedicated SPEC-23/SPEC-26 proof names remain a documented plan refinement before their dependent feature acceptance, not hidden passing coverage.

### DOC-001 — contributor-facing document baseline, 2026-10-22

- Scope: publish the existing detailed plan with priorities/100-package ledger and ordering clarifications; add handoff, contributor guide, and local bug/fix/blocker/decision register; update specification authority link and previous plan redirect.
- Files: `docs/implementation-plan.md`, `docs/handoff.md`, `CONTRIBUTING.md`, `docs/bug-fix-tracking.md`, `docs/specification.md`, `.hermes/plans/2026-10-22_165525-aerobrowser-full-implementation.md`.
- Application work: none. Installs, Git initialization, commits, accounts, provider calls, release, and remote writes: none.
- Verification: documentation links/anchors, mappings, ledger/tracker coverage, JSON/JavaScript/SQL checks and PDF/scope checks passed; actual evidence and unrun application/native gates are recorded in §6.
- Next action: owner document review and bounded setup/Git decisions, then BATCH-00; no source implementation begins as a side effect of this delivery.
