//! E01/N07: the commit boundary. Every proposed action passes [`Capabilities::gate`]
//! before any input is dispatched; only the gate creates a [`Clearance`].
//!
//! [`required_path`] classifies every `Action` variant with an exhaustive
//! match, so a new variant cannot compile without a decision:
//!
//! - local actions with no browser or external effect clear directly;
//! - navigation, new tabs, metadata fetches and extraction are envelope
//!   effects authorized against N06 grants;
//! - clicks, typing, selects, checkboxes and key presses are classified by the
//!   host from the resolved target: as an envelope effect (a link that
//!   navigates) or as a commit, including an unknown effect;
//! - credential and OTP fills and downloads are commits.
//!
//! A commit clears only by consuming a trusted user's approval of a preview
//! whose material digest, run, revision, document, target and origin still
//! match, in one journal transaction. Without an approval ID the gate
//! returns the preview for review (prepare) and dispatches nothing. The
//! model's rationale, page text and observation content are not inputs.
//!
//! A clearance cannot be forged or duplicated:
//! ```compile_fail,E0451
//! let _ = aero_agent::approval::Clearance { cleared_by: aero_agent::approval::ClearedBy::Local };
//! ```
//! ```compile_fail,E0599
//! fn twice(c: aero_agent::approval::Clearance) { let _ = (c.clone(), c); }
//! ```

use crate::capabilities::{Authorization, Capabilities, ResolvedEffect, ReviewReason};
use crate::policy::{UnsafeTarget, allowed_navigation};
use crate::preview::Preview;
use crate::store::StoreError;
use aero_core::actions::Action;
use aero_core::task::Effect;

/// How an action must be authorized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionPath {
    /// No browser or external effect.
    Local,
    /// An envelope effect on the action's own URL(s) or current page.
    Envelope(Effect),
    /// The host decides from the resolved target: an envelope effect or a commit.
    HostClassified,
    /// Always a commit needing approval.
    Commit(Effect),
}

/// Classify `action`. Exhaustive by design: no wildcard arm.
pub fn required_path(action: &Action) -> ActionPath {
    match action {
        Action::Scroll { .. }
        | Action::Wait { .. }
        | Action::SwitchTab { .. }
        | Action::CloseTab { .. }
        | Action::AskUser { .. }
        | Action::Complete { .. } => ActionPath::Local,
        Action::Navigate { .. } | Action::NewTab { .. } | Action::BatchMetadata { .. } => {
            ActionPath::Envelope(Effect::Navigate)
        }
        Action::ExtractData { .. } => ActionPath::Envelope(Effect::Extract),
        Action::Click { .. }
        | Action::Type { .. }
        | Action::Select { .. }
        | Action::SetChecked { .. }
        | Action::PressKey { .. } => ActionPath::HostClassified,
        Action::FillCredentials { .. } | Action::FillTotp { .. } => {
            ActionPath::Commit(Effect::CredentialFill)
        }
        Action::DownloadFile { .. } => ActionPath::Commit(Effect::Download),
    }
}

/// What the host resolved for one proposed action.
// One short-lived value per proposed action: boxing the preview buys nothing.
#[allow(clippy::large_enum_variant)]
pub enum Resolved {
    Local,
    /// One effect per target: the action's URL(s) in order, or the current
    /// page for extraction or a host-classified link.
    Effects(Vec<ResolvedEffect>),
    /// A commit: the preview built for it and, once the trusted user has
    /// approved it, the approval to consume.
    Commit {
        approval_id: Option<String>,
        preview: Preview,
    },
}

/// What authorized a dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClearedBy {
    Local,
    Grants(Vec<String>),
    Approval(String),
}

/// Permission to dispatch one action once. Only [`Capabilities::gate`]
/// creates it; it is neither `Clone` nor constructible elsewhere.
#[derive(Debug, PartialEq, Eq)]
pub struct Clearance {
    cleared_by: ClearedBy,
}

impl Clearance {
    pub fn cleared_by(&self) -> &ClearedBy {
        &self.cleared_by
    }
}

/// Why an approval was not consumed. Nothing was dispatched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No such approval for this task.
    NotFound,
    Consumed,
    Expired,
    /// Another run, or the run ended.
    RunChanged,
    StaleRevision,
    /// The frame now shows another document.
    StaleDocument,
    /// The control was replaced.
    TargetChanged,
    OriginChanged,
    /// A material field differs from what was approved.
    MaterialChanged,
}

// One short-lived value per proposed action, like `Resolved`.
#[allow(clippy::large_enum_variant)]
pub enum Gate {
    Cleared(Clearance),
    /// An envelope effect outside every grant: ask the trusted user.
    ReviewRequired(ReviewReason),
    /// A commit without an approval: show this preview for review. Nothing
    /// was dispatched or consumed.
    NeedsApproval(Preview),
    Refused(Refusal),
    Denied(UnsafeTarget),
    /// The resolution does not fit the action (a host defect): refuse.
    Inconsistent,
}

impl Capabilities {
    /// Decide whether `action`, as the host resolved it, may be dispatched
    /// now. Envelope effects use grant quota and approvals are consumed only
    /// when the whole action clears; call once per dispatch.
    pub async fn gate(&self, action: &Action, resolved: Resolved) -> Result<Gate, StoreError> {
        let path = required_path(action);
        match (path, resolved) {
            (ActionPath::Local, Resolved::Local) => Ok(cleared(ClearedBy::Local)),
            (ActionPath::Envelope(kind), Resolved::Effects(effects)) => {
                let named: Option<&[String]> = match action {
                    Action::Navigate { url } | Action::NewTab { url } => {
                        Some(std::slice::from_ref(url))
                    }
                    Action::BatchMetadata { urls } => Some(urls),
                    _ => None,
                };
                match named {
                    Some(urls) => {
                        let mut targets = Vec::with_capacity(urls.len());
                        for raw in urls {
                            match allowed_navigation(raw, self.private_network()) {
                                Ok(url) => targets.push(url),
                                Err(why) => return Ok(Gate::Denied(why)),
                            }
                        }
                        if targets.len() != effects.len()
                            || targets
                                .iter()
                                .zip(&effects)
                                .any(|(url, e)| *url != e.target)
                        {
                            return Ok(Gate::Inconsistent);
                        }
                    }
                    // Extraction acts on the current page: exactly one target.
                    None if effects.len() != 1 => return Ok(Gate::Inconsistent),
                    None => {}
                }
                if effects.iter().any(|effect| effect.effect != Some(kind)) {
                    return Ok(Gate::Inconsistent);
                }
                self.authorize_all(effects).await
            }
            // A control the host resolved as a routine envelope effect (a link).
            (ActionPath::HostClassified, Resolved::Effects(effects)) => {
                if effects.len() != 1
                    || !matches!(effects[0].effect, Some(Effect::Navigate | Effect::Extract))
                {
                    return Ok(Gate::Inconsistent);
                }
                self.authorize_all(effects).await
            }
            (
                ActionPath::HostClassified,
                Resolved::Commit {
                    approval_id,
                    preview,
                },
            ) => self.commit(approval_id, preview).await,
            (
                ActionPath::Commit(kind),
                Resolved::Commit {
                    approval_id,
                    preview,
                },
            ) if preview.effect == Some(kind) => self.commit(approval_id, preview).await,
            _ => Ok(Gate::Inconsistent),
        }
    }

    /// Every effect must be allowed. Quota already used by earlier effects of
    /// a refused multi-target action is not returned (conservative).
    async fn authorize_all(&self, effects: Vec<ResolvedEffect>) -> Result<Gate, StoreError> {
        if effects.is_empty() {
            return Ok(Gate::Inconsistent);
        }
        let mut grants = Vec::with_capacity(effects.len());
        for effect in effects {
            match self.authorize(effect)?.recv().await? {
                Authorization::Allowed { grant_id, .. } => grants.push(grant_id),
                Authorization::ReviewRequired(reason) => return Ok(Gate::ReviewRequired(reason)),
                Authorization::Denied(why) => return Ok(Gate::Denied(why)),
            }
        }
        Ok(cleared(ClearedBy::Grants(grants)))
    }

    async fn commit(
        &self,
        approval_id: Option<String>,
        preview: Preview,
    ) -> Result<Gate, StoreError> {
        if preview.validate().is_err() {
            return Ok(Gate::Inconsistent);
        }
        let Some(id) = approval_id else {
            return Ok(Gate::NeedsApproval(preview));
        };
        Ok(match self.consume(id.clone(), preview)?.recv().await? {
            Ok(()) => cleared(ClearedBy::Approval(id)),
            Err(refusal) => Gate::Refused(refusal),
        })
    }
}

fn cleared(cleared_by: ClearedBy) -> Gate {
    Gate::Cleared(Clearance { cleared_by })
}
