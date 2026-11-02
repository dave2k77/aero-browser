//! Task-global accounting inputs, not dispatch permission or real billing data.
//! Prices are trusted configured upper bounds in the contract currency, including
//! all relevant charges. Unknown prices stay unknown. No FX or provider discovery.
//! Serde here is private journal storage, not a model-callable wire API; the
//! worker checks exact persisted shapes, versions, ownership and semantic bounds.
use crate::store::StoreError;
use crate::store::worker::Resources;
use aero_core::checkpoint::{MAX_COUNTER, RunContext};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Pricing {
    /// Explicit host assertion that this work has no inference component.
    NoInference,
    Unknown,
    /// Minor units per tokens_per_unit; input/output are rounded upward separately.
    UpperBound {
        currency: String,
        input_minor: u64,
        output_minor: u64,
        tokens_per_unit: u64,
    },
}
impl Pricing {
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        if let Self::UpperBound {
            currency,
            input_minor,
            output_minor,
            tokens_per_unit,
        } = self
            && (!valid_currency(currency)
                || *tokens_per_unit == 0
                || [*input_minor, *output_minor, *tokens_per_unit]
                    .into_iter()
                    .any(|v| v > MAX_COUNTER))
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
    pub fn quote(&self, resources: Resources, currency: &str) -> Result<Option<u64>, StoreError> {
        self.validate()?;
        resources.validate()?;
        if !valid_currency(currency) {
            return Err(StoreError::InvalidInput);
        }
        match self {
            Self::Unknown => Ok(None),
            Self::NoInference if resources.input_tokens == 0 && resources.output_tokens == 0 => {
                Ok(Some(0))
            }
            Self::NoInference => Err(StoreError::InvalidInput),
            Self::UpperBound {
                currency: quoted,
                input_minor,
                output_minor,
                tokens_per_unit,
            } => {
                if quoted != currency {
                    return Err(StoreError::InvalidInput);
                }
                let price = |tokens: u64, rate: u64| {
                    (tokens as u128 * rate as u128).div_ceil(*tokens_per_unit as u128)
                };
                let total = price(resources.input_tokens, *input_minor)
                    + price(resources.output_tokens, *output_minor);
                if total > MAX_COUNTER as u128 {
                    return Err(StoreError::BudgetExceeded);
                }
                Ok(Some(total as u64))
            }
        }
    }
}
fn valid_currency(s: &str) -> bool {
    s.len() == 3 && s.bytes().all(|x| x.is_ascii_uppercase())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Money {
    pub currency: String,
    pub minor_units: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Charges {
    pub inference: Pricing,
    pub purchase: Option<Money>,
}
impl Default for Charges {
    fn default() -> Self {
        Self {
            inference: Pricing::Unknown,
            purchase: None,
        }
    }
}
impl Charges {
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        self.inference.validate()?;
        if let Some(m) = &self.purchase
            && (!valid_currency(&m.currency) || m.minor_units > MAX_COUNTER)
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
    pub(crate) fn quote(
        &self,
        r: Resources,
        currency: &str,
    ) -> Result<(Option<u64>, u64), StoreError> {
        self.validate()?;
        let inference = self.inference.quote(r, currency)?;
        let purchase = match &self.purchase {
            None => 0,
            Some(p) if p.currency == currency && p.minor_units <= MAX_COUNTER => p.minor_units,
            Some(_) => return Err(StoreError::InvalidInput),
        };
        Ok((inference, purchase))
    }
}
/// Independent inference/read-only work. Browser actions use prepare_action's
/// durable action intent, not this API. IDs must never be reused within a task.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetRequest {
    pub context: RunContext,
    pub id: String,
    pub reserve: Resources,
    pub pricing: Pricing,
}
impl BudgetRequest {
    pub(crate) fn validate(&self) -> Result<(), StoreError> {
        self.context
            .validate()
            .map_err(|_| StoreError::InvalidInput)?;
        self.pricing.validate()?;
        for id in [
            self.context.task_id.as_str(),
            self.context.run_id.as_str(),
            &self.id,
        ] {
            if id.trim().is_empty() || id.len() > 256 || id.contains('\0') {
                return Err(StoreError::InvalidInput);
            }
        }
        self.reserve.validate()?;
        if self.reserve.input_tokens == 0
            && self.reserve.output_tokens == 0
            && self.reserve.active_ms == 0
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
}
/// Durable locator only, not authority; the writer rechecks context and identity.
#[derive(Clone)]
pub struct BudgetTicket {
    pub context: RunContext,
    pub id: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReservationState {
    Reserved,
    Settled,
    Uncertain,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrunReport {
    pub schema_version: u32,
    pub actual: Option<Resources>,
    pub purchase_actual: Option<u64>,
}

pub struct BudgetReceipt {
    pub state: ReservationState,
    pub reserved: Resources,
    pub actual: Option<Resources>,
    pub inference_reserved_upper_bound: Option<u64>,
    pub reported_overrun: Option<Resources>,
}
pub struct BudgetHistoryEntry {
    pub ticket: BudgetTicket,
    pub receipt: BudgetReceipt,
}
pub struct BudgetPage {
    pub entries: Vec<BudgetHistoryEntry>,
    pub has_more: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetSnapshot {
    pub currency: String,
    /// Counts of unknown-priced charges, not monetary amounts or token totals.
    pub unknown_inference_spent_count: u64,
    pub unknown_inference_reserved_count: u64,
    /// An unresolved inference operation cannot certify a complete monetary bound.
    pub uncertain_inference: bool,
    pub spent: Resources,
    pub reserved: Resources,
    pub action_attempts: u64,
    pub known_inference_spent_upper_bound: u64,
    pub known_inference_reserved_upper_bound: u64,
    /// None if any past or outstanding inference has unknown pricing.
    pub total_inference_upper_bound: Option<u64>,
    pub purchase_spent: u64,
    pub purchase_reserved: u64,
}

/// Process-local monotonic time; never persist clock ticks across restarts.
pub trait Clock {
    fn now(&self) -> Duration;
}
pub struct MonotonicClock {
    origin: Instant,
}
impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}
impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}
/// Host-controlled active intervals. Pauses exclude human waits. This does not
/// cancel work at a deadline; the future controller must enforce reserved bounds.
/// On crash retain the durable active-time reservation, never infer zero elapsed.
pub struct ActiveTimer<C: Clock> {
    clock: C,
    last: Duration,
    elapsed: Duration,
    active: bool,
}
impl<C: Clock> ActiveTimer<C> {
    pub fn new(clock: C) -> Self {
        let last = clock.now();
        Self {
            clock,
            last,
            elapsed: Duration::ZERO,
            active: false,
        }
    }
    pub fn resume(&mut self) -> Result<(), StoreError> {
        if self.active {
            return Err(StoreError::Conflict);
        }
        self.tick()?;
        self.active = true;
        Ok(())
    }
    pub fn pause(&mut self) -> Result<(), StoreError> {
        if !self.active {
            return Err(StoreError::Conflict);
        }
        self.tick()?;
        self.active = false;
        Ok(())
    }
    pub fn elapsed_ms(&mut self) -> Result<u64, StoreError> {
        self.tick()
    }
    fn tick(&mut self) -> Result<u64, StoreError> {
        let now = self.clock.now();
        let delta = now.checked_sub(self.last).ok_or(StoreError::InvalidInput)?;
        let elapsed = if self.active {
            self.elapsed
                .checked_add(delta)
                .ok_or(StoreError::BudgetExceeded)?
        } else {
            self.elapsed
        };
        let millis = elapsed.as_nanos().div_ceil(1_000_000);
        if millis > MAX_COUNTER as u128 {
            return Err(StoreError::BudgetExceeded);
        }
        self.elapsed = elapsed;
        self.last = now;
        Ok(millis as u64)
    }
}
