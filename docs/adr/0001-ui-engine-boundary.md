# ADR 0001: trusted UI and controlled browser are separate engines

## Status

**Accepted:** engine and trust separation. **Pending:** desktop presentation choice and native interaction evidence. A01 records the accepted boundary; it does not pass N10, accept a candidate, unblock H01, or claim native backend support.

## Context

The [specification's viewport gate](../specification.md#early-viewport-decision-gate) and [SPEC-03](../implementation-plan.md#3-required-corrections-to-the-supplied-specification) correct the historical conflation of Tauri/WebKitGTK, GTK and Chromium/CEF. Tauri's trusted Linux local UI is not the Chromium instance controlled by CDP. Loading the same URL twice would produce different browser state, authentication and input ownership, not a viewport of the controlled target.

Native release acceptance targets Ubuntu 24.04 x86_64 with actual X11 and Wayland evidence. WSL probes are development evidence only. See [architecture](../architecture.md) for runtime/controller ownership and [traceability](../requirements.md) for planned proof.

## Decision: accepted now

- Keep maintained Chromium and its renderer sandbox under the owned, tested browser supervisor. Do not fork the renderer or silently replace controlled browsing with a privileged WebKit webview.
- Use Tauri only as a trusted local task console, thinly adapting the common `SessionService`. Remote page content does not receive its privileged invoke surface.
- Present the **actual controlled target** where streamed imagery is useful; a native Chromium companion window is an eligible primary interaction arrangement. Stream target/tab/runtime generation must correspond to controller state; stale images/input are rejected.
- Require acknowledged exclusive takeover before manual wrapper input. Pause blocks new agent dispatch; resume observes the user's actual state and invalidates old handles and materially stale approvals. Native manual actions are labelled manual/unverified where appropriate, not falsely promised to be universally policy-mediated.
- Keep browser permissions, upload file tokens, sensitive-image suppression, bounded frame acknowledgements and input coordinate calibration at their existing trusted owners. The frontend is not a second policy or execution engine.

## Decision still pending: presentation

N10 must compare actual-target streaming with a companion-console/native-window arrangement. The default **candidate**, not an accepted winner, is the Tauri task console with streaming where useful and native Chromium interaction where required. H01 and substantial packaging/frontend commitment wait for owner acceptance of measured behavior and limitations.

If neither candidate meets required interaction/accessibility, stop desktop commitment and obtain a separate CEF/native-embedding feasibility decision. Do not infer that embedding is implemented or mandatory, substitute another browser engine, promote disposable probe code unchanged, or maintain a Chromium fork. Pure CLI/library delivery may continue independently.

## Required future evidence

The canonical package is [N10](../implementation-plan.md#n10--early-viewport-feasibility-req-view-01-req-ux-02), with H01–H06/N20–N21 as consumers.

- Automated planned proof: `crates/aero-agent/tests/scenarios.rs :: viewport_uses_controlled_target`. Later H03 names `screencast_is_same_controlled_target`, `screencast_ack_does_not_block_agent` and `tab_switch_replaces_screencast` in that same file. These do not alone establish native usability.
- **Manual/provisioning gate:** record actual environment/versions and pass/fail/unsupported findings for text selection/copy, Unicode/IME, keyboard shortcuts, focus, scrolling/resizing, screen-reader/keyboard access, popups/auth/file dialogs, takeover/viewport latency and app close on native X11 and Wayland independently. Record selected fallback and known limitations.
- **Owner gate:** accept that matrix and supported arrangement before H01. A skipped backend cannot count as supported; a screenshot canvas cannot count as an accessible native browser viewport.
- Planned browser ownership proof: `crates/aero-agent/tests/scenarios.rs :: manual_takeover_preserves_single_input_owner`. UI suite files are `crates/aero-shell/ui/src/TaskWorkspace.test.tsx` and `crates/aero-shell/ui/src/BrowserToolbar.test.tsx`; the plan does not yet name their individual tests.

## Consequences and nonclaims

The console and controlled browser can evolve independently without duplicated task authority. A companion window may introduce focus/dialog coordination costs, and a stream may lack selection, IME or accessibility features; those tradeoffs must be observed rather than waved away. Native input evidence, performance measurements and owner acceptance remain unexecuted gates in this ADR. No code, browser test, native smoke or packaging result is reported here.
