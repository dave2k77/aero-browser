//! N06: authorize a host-resolved effect against the task's durable grants.
//!
//! The host classifies an action from its actual resolved target, context and
//! adapter preconditions ([`ResolvedEffect`]); the model's action name or
//! rationale is never the classification, and an effect the host cannot
//! classify is `None`, which always needs review. Inside an issued envelope,
//! routine navigation and extraction are allowed without a prompt; anything
//! crossing it (origin or route, account, effect class, destination, quota,
//! expiry, contract revision) needs review. Unsafe targets are refused.
//!
//! Every decision is made in one journal transaction that also records the
//! quota use, so a restart re-reads the durable grants and their use; nothing
//! is reconstructed from history. Navigation grants cannot prove that every
//! GET is harmless: a site that mutates state on GET is outside this model.
//!
//! The model cannot construct the handle or decode an effect:
//! ```compile_fail,E0451
//! fn forge(journal: aero_agent::store::worker::JournalClient) {
//!     let _ = aero_agent::capabilities::Capabilities { journal, network: todo!() };
//! }
//! ```
//! ```compile_fail,E0277
//! let _: aero_agent::capabilities::ResolvedEffect = serde_json::from_str("{}").unwrap();
//! ```

use crate::policy::UnsafeTarget;
use crate::store::StoreError;
use crate::store::worker::{JournalClient, Reply};
use aero_core::capability::{CapabilityGrant, path_within};
use aero_core::ids::TaskId;
use aero_core::task::{Effect, RouteScope};
use url::Url;

/// An effect the host resolved for one proposed action.
#[derive(Clone)]
pub struct ResolvedEffect {
    pub task_id: TaskId,
    /// The contract revision the action was planned under.
    pub contract_revision: u64,
    /// `None`: the host cannot tell what the action does.
    pub effect: Option<Effect>,
    /// The effective target (frame) URL.
    pub target: Url,
    /// The trusted account/profile the action runs as, if any.
    pub account_ref: Option<String>,
    /// Where produced data goes, if anywhere.
    pub destination: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Authorization {
    /// Inside the named grant; one unit of its quota is now used.
    Allowed { grant_id: String, remaining: u64 },
    /// Ask the trusted user; nothing was dispatched or consumed.
    ReviewRequired(ReviewReason),
    /// Never acceptable, whatever is granted.
    Denied(UnsafeTarget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewReason {
    UnknownEffect,
    /// No live grant for the task.
    NoGrant,
    /// The closest live grant does not cover the effect.
    Boundary(Mismatch),
}

/// Why a grant does not cover an effect, in check order: a later variant
/// means the grant matched every earlier check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mismatch {
    StaleRevision,
    Expired,
    EffectClass,
    Route,
    Account,
    Destination,
    QuotaExhausted,
}

/// Whether targets on loopback and private networks may be authorized at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkAccess {
    PublicOnly,
    /// Test fixtures, or an explicit local-network permission.
    PrivateNetwork,
}

/// The host's authorization handle, issued by `TaskAuthority::capabilities`.
#[derive(Clone)]
pub struct Capabilities {
    journal: JournalClient,
    network: NetworkAccess,
}

impl Capabilities {
    pub(crate) fn new(journal: JournalClient, network: NetworkAccess) -> Self {
        Self { journal, network }
    }

    /// Decide `effect` immediately before dispatch. `Allowed` has consumed
    /// quota; call it once per dispatch, never speculatively.
    pub fn authorize(&self, effect: ResolvedEffect) -> Result<Reply<Authorization>, StoreError> {
        self.journal
            .authorize_effect(effect, self.network == NetworkAccess::PrivateNetwork)
    }
}

impl Capabilities {
    pub(crate) fn private_network(&self) -> bool {
        self.network == NetworkAccess::PrivateNetwork
    }
    /// Consume approval `id` for `current` (N07); see `approval`.
    pub(crate) fn consume(
        &self,
        id: String,
        current: crate::preview::Preview,
    ) -> Result<Reply<Result<(), crate::approval::Refusal>>, StoreError> {
        self.journal.consume_approval(id, current)
    }
}

/// Does `grant` (with `quota_used` spent) cover `effect` now?
pub(crate) fn evaluate(
    grant: &CapabilityGrant,
    quota_used: u64,
    effect: &ResolvedEffect,
    latest_revision: u64,
    now_ms: u64,
) -> Result<(), Mismatch> {
    let fail = |mismatch, failed: bool| if failed { Err(mismatch) } else { Ok(()) };
    fail(
        Mismatch::StaleRevision,
        grant.contract_revision != latest_revision || effect.contract_revision != latest_revision,
    )?;
    fail(Mismatch::Expired, now_ms >= grant.expires_at_ms)?;
    fail(
        Mismatch::EffectClass,
        !effect
            .effect
            .is_some_and(|kind| grant.effects.contains(&kind)),
    )?;
    fail(
        Mismatch::Route,
        !grant
            .routes
            .iter()
            .any(|route| route_covers(route, &effect.target)),
    )?;
    fail(Mismatch::Account, grant.account_ref != effect.account_ref)?;
    fail(
        Mismatch::Destination,
        effect
            .destination
            .as_ref()
            .is_some_and(|destination| !grant.destinations.contains(destination)),
    )?;
    fail(Mismatch::QuotaExhausted, quota_used >= grant.quota)
}

/// `url` is on `route`'s origin and within its path prefix. Paths with
/// encoded separators or dots are never within a narrower prefix, since a
/// server may decode them into another route.
pub(crate) fn route_covers(route: &RouteScope, url: &Url) -> bool {
    if url.origin().ascii_serialization() != route.origin {
        return false;
    }
    let path = url.path();
    if route.path_prefix != "/" {
        let lower = path.to_ascii_lowercase();
        if ["%2f", "%5c", "%2e"]
            .iter()
            .any(|encoded| lower.contains(encoded))
        {
            return false;
        }
    }
    path_within(path, &route.path_prefix)
}
