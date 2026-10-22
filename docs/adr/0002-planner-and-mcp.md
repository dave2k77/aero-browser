# ADR 0002: configured inference and restricted tool protocol

## Status

Accepted architectural boundary; provider/model/pricing configuration and optional MCP SDK/protocol selection remain implementation/provisioning gates. No inference, tool interop or application-test execution is certified by this ADR.

## Context

[SPEC-02](../implementation-plan.md#3-required-corrections-to-the-supplied-specification) corrects the historical use of an MCP client sending `sampling/createMessage` to an example tool server as an assumed inference API. The [specification](../specification.md#5-architecture-and-ownership) requires application-owned tasks and independent authority/verification. A protocol role is not permission to revise user intent or issue approvals.

## Decision: inference

`Planner` proposes a strict typed decision from an immutable sanitized observation, bounded history, contract revision, budgets and supported action schema. Trusted envelopes provide Task/Run/revision/caller role; model JSON cannot select those owners or claim verifier results.

E03/E04 implement separate planner adapters:

- `HttpPlanner` uses an explicitly configured base URL/model, TLS validation, bounded response/decision size, timeout/cancellation, secret-source authorization header, validated structured output and at most one bounded invalid-response repair attempt. No pretend default endpoint/model and no startup downloads.
- Provider task-content transfers consume a `DataFlowGateway` transport authorization tied to source labels, task/revision, exact recipient/purpose/type/size and trusted grant. The planner has no unrestricted task-content HTTP path. Local-only/sensitive failure cannot silently select a cloud fallback; approved website browsing is a separate network permission.
- Reserve task-wide requests/tokens/time and known priced upper bounds before inference. Unknown monetary price remains unknown, not free or a demonstrated money ceiling. Crash-time uncertain usage is not automatically refunded.
- `ScriptedPlanner` is test-only or explicitly enabled by the nondefault evaluation feature. Reports label `scripted_browser`; production has no fake fallback. A configured actual-model run is separate, requires its own grants/configuration and is not implied by passing a scripted fixture.

The planner cannot directly call executor/vault, grant/revise authority or write passed criteria. `AskUser` persists awaiting-user; `Complete` requests independent completion checking. Store only bounded public rationale, intended action, outcomes, evidence and usage; hidden reasoning is neither requested as a product contract nor displayed as an audit requirement.

## Decision: MCP tools

G15 is optional. Select and lock an official Rust SDK and actually supported protocol version when implementing it; this ADR does not pin an unverified version. Keep two distinct responsibilities:

1. **AeroBrowser as tool server:** initial stdio tools observe/step/start/status/pause/cancel adapt the common role-scoped `SessionService`. Validate schemas, task ownership and the same policy/budgets/journal/verifier path. Tool requests may return a pending approval request, never approval issuance or trusted human replies. There is no model-callable approve tool, trusted-role selector, arbitrary JS/shell/raw-CDP or host-file API. Stdio logs go to stderr, not the protocol stream.
2. **AeroBrowser as optional external-tool client:** capability negotiation, bounded messages/deadlines, cancellation, pagination and owned child cleanup are explicit. Tool input transfers pass the same data gateway; output is untrusted data and cannot declassify itself or mint permissions. An external tool effect is still policy/budget/journal controlled. Remote HTTP/OAuth is unavailable unless explicitly configured and tested.

If MCP sampling is separately supported later, follow its actual negotiated **server-to-client** request direction/capability semantics. Do not reinterpret sampling as this application's baseline inference transport. MCP interoperability is not model-quality evidence.

## Trusted channel and service consequences

N06–N08 establish host-constructed roles and immutable durable grants before adapters are enabled. CLI/desktop trusted human actions can accept revisions, approve exact material previews or reply to the user question; model/page/procedure/worker/tool callers can only request those decisions. A connection cannot self-declare a trusted role in JSON. Every task/run lookup checks authorization, not only ID shape.

Initial service is in-process. Optional process extraction follows N09's separate need/owner gate, using a private role-scoped Unix-domain service with explicit close/background policy, not an automatic network daemon. See [architecture](../architecture.md#service-and-lifecycle).

## Required future proof and limitations

Exact contracts live in [planner/approval/verification contracts](../implementation-plan.md#79-planner-approvals-and-verification-contracts), [N08](../implementation-plan.md#n08--shared-role-scoped-session-interface-req-service-01) and G15 in [Batch G](../implementation-plan.md#batch-g--local-product-subsystems-g3); coverage is indexed in [requirements](../requirements.md).

- `crates/aero-agent/tests/planner.rs :: scripted_planner_consumes_validated_observations`, `valid_decision`, `auth_is_not_logged`, `invalid_output_has_one_retry`, `timeout_and_cancel`, `oversize_is_rejected`, `extra_actions_are_rejected`, `local_policy_blocks_remote_request`.
- `crates/aero-agent/tests/data_flow.rs :: transfer_requires_exact_scoped_grant`, `local_processing_never_falls_back_remote`, `optional_adapter_cannot_bypass_mode`.
- `crates/aero-agent/tests/capabilities.rs :: model_cannot_grant_its_own_capability`; `crates/aero-agent/tests/verification.rs :: planner_cannot_write_passed_criteria` and `model_complete_cannot_bypass_criteria`.
- Optional `crates/aero-agent/tests/mcp.rs :: mcp_tools_list_and_call`, `mcp_invalid_input_is_protocol_error`, `mcp_policy_is_not_bypassed`, `mcp_stdio_is_clean`, `mcp_child_shutdown_is_reaped`; **manual/provisioning interop gate** with a real selected SDK client, no model required.
- N08 additionally specifies `crates/aero-agent/tests/service.rs` suite assertions for shared state/roles/reconnect but no individual test names. Configured-model/provider and pricing evidence, actual interop and any remote transport remain separate unrun gates, not filled with fabricated responses.
