//! N06: a bounded delegation envelope the trusted user issues for one task
//! revision. Durable data, not authority by itself: a grant takes effect only
//! once the trusted-user client has issued it into the journal, and every use
//! is re-checked there (expiry, revocation, quota, current revision).
//!
//! Envelopes cover routine effects only ([`Effect::envelope_grantable`]).
//! Commits such as submit, purchase, upload, account creation, export and
//! credential fill need a per-commit preview approval (N07) or their own
//! package's guard.

use crate::ids::TaskId;
use crate::task::{Effect, MAX_ITEMS, RouteScope, TaskContract, route_shape};
use crate::wire::{ValidationError, require, strict_object};
use std::collections::HashSet;

pub const GRANT_SCHEMA_VERSION: u32 = 1;
/// Most effects one grant may admit.
pub const MAX_GRANT_QUOTA: u64 = 10_000;

impl Effect {
    /// Effects a standing envelope may cover without a per-action preview.
    pub fn envelope_grantable(self) -> bool {
        matches!(self, Self::Navigate | Self::Extract | Self::StageArtifact)
    }
}

strict_object! {
    /// What the trusted user delegates. Every list must be non-empty except
    /// `destinations`; the grant must lie within its contract revision.
    pub struct CapabilityGrant {
        pub schema_version: u32,
        /// 1–128 of `A–Z a–z 0–9 . _ -`; unique in the journal.
        pub id: String,
        pub task_id: TaskId,
        pub contract_revision: u64,
        /// Each within one of the contract's `allowed_routes`.
        pub routes: Vec<RouteScope>,
        pub effects: Vec<Effect>,
        /// A trusted account/profile reference from the contract's
        /// `account_refs`, or `None` for anonymous browsing.
        pub account_ref: Option<String>,
        /// Data destinations from the contract's `export_grants`.
        pub destinations: Vec<String>,
        /// Effects this grant may admit in total, 1–[`MAX_GRANT_QUOTA`].
        pub quota: u64,
        /// Wall-clock expiry, milliseconds since the Unix epoch.
        pub expires_at_ms: u64,
    }
    version schema_version = GRANT_SCHEMA_VERSION;
}

impl CapabilityGrant {
    /// Shape and containment in `contract` (the same task and revision).
    /// Expiry against the clock is the journal's check.
    pub fn validate_against(&self, contract: &TaskContract) -> Result<(), ValidationError> {
        require(
            self.schema_version == GRANT_SCHEMA_VERSION,
            "grant.schema_version",
            "unsupported schema version",
        )?;
        require(
            (1..=128).contains(&self.id.len())
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')),
            "grant.id",
            "1-128 ASCII letters, digits, '.', '_' or '-'",
        )?;
        require(
            self.task_id == contract.task_id && self.contract_revision == contract.revision,
            "grant.contract_revision",
            "grant must name its contract's task and revision",
        )?;
        require(
            (1..=MAX_ITEMS).contains(&self.routes.len()),
            "grant.routes",
            "1-64 routes required",
        )?;
        let mut routes = HashSet::new();
        for route in &self.routes {
            require(
                route_shape(&route.path_prefix)
                    && contract.allowed_routes.iter().any(|allowed| {
                        allowed.origin == route.origin
                            && path_within(&route.path_prefix, &allowed.path_prefix)
                    }),
                "grant.routes",
                "route must lie within an allowed contract route",
            )?;
            require(
                routes.insert((&route.origin, &route.path_prefix)),
                "grant.routes",
                "duplicate route",
            )?;
        }
        require(
            (1..=MAX_ITEMS).contains(&self.effects.len()),
            "grant.effects",
            "1-64 effects required",
        )?;
        let mut effects = HashSet::new();
        for effect in &self.effects {
            require(
                effect.envelope_grantable()
                    && contract.allowed_effects.contains(effect)
                    && !contract.forbidden_effects.contains(effect),
                "grant.effects",
                "effect must be envelope-grantable and allowed by the contract",
            )?;
            require(effects.insert(effect), "grant.effects", "duplicate effect")?;
        }
        require(
            self.account_ref
                .as_ref()
                .is_none_or(|account| contract.account_refs.contains(account)),
            "grant.account_ref",
            "account must be one of the contract's account references",
        )?;
        require(
            self.destinations.len() <= MAX_ITEMS,
            "grant.destinations",
            "must not exceed 64 entries",
        )?;
        let mut destinations = HashSet::new();
        for destination in &self.destinations {
            require(
                contract.export_grants.contains(destination) && destinations.insert(destination),
                "grant.destinations",
                "destination must be a unique contract export reference",
            )?;
        }
        require(
            (1..=MAX_GRANT_QUOTA).contains(&self.quota),
            "grant.quota",
            "quota must be 1-10000",
        )?;
        require(
            self.expires_at_ms > 0,
            "grant.expires_at_ms",
            "expiry required",
        )
    }
}

/// `path` lies within the segment-bounded `prefix`: equal, or below it at a
/// `/` boundary (`/a` covers `/a` and `/a/b`, not `/ab`).
pub fn path_within(path: &str, prefix: &str) -> bool {
    path == prefix
        || (prefix.ends_with('/') && path.starts_with(prefix))
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}
