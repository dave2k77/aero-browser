//! Authorized, freshly grounded browser input boundary.
//!
//! D02: [`target`] grounds an observation handle in a fresh, visible,
//! unobstructed point; [`dispatcher`] sends exactly one input sequence to it.
//! Authorization (grants, approvals) is the caller's and is not checked here.

pub mod dispatcher;
pub mod target;

use aero_cdp::page::Page;
use aero_core::ids::{ObservationId, RuntimeEpoch};
use aero_perception::observation::HandleMap;

use crate::dispatcher::DispatchError;
use crate::target::{GroundedTarget, GroundingError};

/// Why a click was not performed, or may have been performed only in part.
#[derive(Debug, PartialEq)]
pub enum ClickError {
    /// Refused before any input was sent.
    Grounding(GroundingError),
    Dispatch(DispatchError),
}

/// Ground `handle` and click it once.
pub async fn click_handle(
    page: &Page,
    handles: &HandleMap,
    observation: &ObservationId,
    runtime: &RuntimeEpoch,
    handle: &str,
) -> Result<GroundedTarget, ClickError> {
    let target = target::ground(page, handles, observation, runtime, handle)
        .await
        .map_err(ClickError::Grounding)?;
    dispatcher::click(page, target.point)
        .await
        .map_err(ClickError::Dispatch)?;
    Ok(target)
}
