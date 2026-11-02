//! In-memory, host-composed transfer gateway. This is not a sandbox or authentication:
//! unrestricted same-process code can instantiate a host, use networking directly, or
//! mislabel bytes. Never expose `GatewayHost`, payloads, grants, transports or a factory
//! to a restricted adapter. The host must obtain provenance, classification, run/revision,
//! mode and approval from trusted owners; this module does not establish them.
//!
//! Bytes are installed *after* encoding and never changed; size applies to those exact
//! bytes, not an image before base64 or an object before JSON. A transport is trusted to
//! send exactly those bytes to its installed recipient without redirects, retries to other
//! destinations, or payload logging. Nothing here enforces those transport obligations.
//! There are no production transports, redaction, durable grants, browser-egress control,
//! remote authentication, or N14 backend provisioning. Sensitive sources and secrets
//! fail closed; restricted-mode remote exceptions are not implemented.
//!
//! All mutations and synchronous dispatch hold the same mutex through transport return.
//! Revocation/revision changes wait for an in-flight send; they cannot undo a completed
//! send. An expiry is checked at dispatch admission, not throughout a long send. A stuck
//! or re-entrant transport can block/deadlock policy updates. A poisoned lock fails closed.
//! Host callers must prevent direct transport use and supervise transport deadlines.
//! A transport error does not prove that no bytes were sent. Expiry uses wall-clock
//! time, so clock rollback can extend validity; a reliable time source remains a host
//! requirement. Encoded lengths and lifetime identity counts are bounded, but
//! caller-supplied spare allocation capacity is not. Register each host with
//! `TaskAuthority::register_gateway`: the owner's `start_run` and
//! `revise_contract` then retire the task's gateways before the journal change
//! is submitted. Other cancellation paths must still call `retire_run`.
//! Same-revision replacement needs a new host. Retirement and
//! host drop close handles after in-flight transport returns; both can block on a
//! stuck transport. Transports must not retain strong host references: a cycle
//! prevents host drop. Explicit retirement breaks such cycles even after poison;
//! a retained handle alone does not retain the host.
//!
//! The restricted handle has no public constructor or fields:
//! ```compile_fail,E0451
//! use aero_agent::data_flow::DispatchHandle;
//! let handle = DispatchHandle { state: unreachable!(), context: unreachable!(), purpose: unreachable!() };
//! ```

pub use aero_core::task::Classification;
use aero_core::{
    checkpoint::{MAX_COUNTER, RunContext},
    ids::TaskId,
    task::ProcessingMode,
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::SystemTime,
};

pub const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_GATEWAY_IDENTITIES: usize = 64;
const MAX_ID_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadKind {
    Text,
    Image,
    Document,
    Embedding,
    ToolPayload,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Inference,
    Ocr,
    Embedding,
    Tool,
    Export,
    Telemetry,
    Memory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipientKind {
    Local,
    Remote,
}

/// Host assertion; the recipient identity must be bound to its transport by the host.
pub struct Recipient {
    pub id: String,
    pub kind: RecipientKind,
}
/// Host-owned provenance and already-encoded bytes. Neither labels nor bytes are
/// accepted by a dispatch handle. No aggregate Debug/serialization is provided.
pub struct Payload {
    pub id: String,
    pub source: String,
    pub classification: Option<Classification>,
    pub secret: bool,
    pub kind: PayloadKind,
    pub encoded_bytes: Vec<u8>,
}
/// Host-issued exact permission. The expiry is exclusive and uses the system clock.
#[derive(Clone)]
pub struct Grant {
    pub id: String,
    pub context: RunContext,
    pub source: String,
    pub classification: Classification,
    pub payload_id: String,
    pub recipient_id: String,
    pub purpose: Purpose,
    pub payload_kind: PayloadKind,
    pub encoded_size: usize,
    pub expires_at: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchError {
    Denied,
    TransportFailed,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransportError;
/// Trusted host-installed, synchronous transport. It must not redirect or modify bytes.
pub trait Transport: Send + Sync {
    fn send(&self, bytes: &[u8]) -> Result<(), TransportError>;
}

struct InstalledRecipient {
    kind: RecipientKind,
    transport: Arc<dyn Transport>,
}
struct State {
    active: bool,
    context: RunContext,
    mode: ProcessingMode,
    recipients: HashMap<String, InstalledRecipient>,
    payloads: HashMap<String, Payload>,
    grants: HashMap<String, Grant>,
    used_payload_ids: HashSet<String>,
    used_grant_ids: HashSet<String>,
}
/// Trusted composition root; possession is not proof of a human's approval.
pub struct GatewayHost {
    state: Arc<Mutex<State>>,
}
/// Restricted to selecting installed identities and purpose; no payload, grant or
/// provenance can be installed through this handle. Bound to one run/revision
/// and one host-selected purpose, not a particular adapter identity.
#[derive(Clone)]
pub struct DispatchHandle {
    state: Arc<Mutex<State>>,
    context: RunContext,
    purpose: Purpose,
}

fn valid_id(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_ID_BYTES && !s.contains('\0')
}
fn valid_context(c: &RunContext) -> bool {
    valid_id(c.task_id.as_str())
        && valid_id(c.run_id.as_str())
        && c.contract_revision > 0
        && c.contract_revision <= MAX_COUNTER
}
fn lock(state: &Mutex<State>) -> Result<MutexGuard<'_, State>, DispatchError> {
    state.lock().map_err(|_| DispatchError::Unavailable)
}

impl GatewayHost {
    pub fn new(context: RunContext, mode: ProcessingMode) -> Result<Self, DispatchError> {
        if !valid_context(&context) {
            return Err(DispatchError::Denied);
        }
        Ok(Self {
            state: Arc::new(Mutex::new(State {
                active: true,
                context,
                mode,
                recipients: HashMap::new(),
                payloads: HashMap::new(),
                grants: HashMap::new(),
                used_payload_ids: HashSet::new(),
                used_grant_ids: HashSet::new(),
            })),
        })
    }
    /// Create an inference-only handle scoped to the current context.
    pub fn dispatch_handle(&self) -> DispatchHandle {
        self.dispatch_handle_for(Purpose::Inference)
    }
    /// Trusted host selects the one purpose the restricted adapter may request.
    pub fn dispatch_handle_for(&self, purpose: Purpose) -> DispatchHandle {
        // A poisoned state can be read only to construct a handle; dispatch itself fails closed.
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        DispatchHandle {
            state: Arc::clone(&self.state),
            context: state.context.clone(),
            purpose,
        }
    }
    pub fn install_recipient(
        &self,
        recipient: Recipient,
        transport: Arc<dyn Transport>,
    ) -> Result<(), DispatchError> {
        let mut s = lock(&self.state)?;
        if !s.active
            || !valid_id(&recipient.id)
            || s.recipients.len() >= MAX_GATEWAY_IDENTITIES
            || s.recipients.contains_key(&recipient.id)
        {
            return Err(DispatchError::Denied);
        }
        s.recipients.insert(
            recipient.id,
            InstalledRecipient {
                kind: recipient.kind,
                transport,
            },
        );
        Ok(())
    }
    pub fn install_payload(&self, payload: Payload) -> Result<(), DispatchError> {
        let mut s = lock(&self.state)?;
        let stored: usize = s.payloads.values().map(|p| p.encoded_bytes.len()).sum();
        if !s.active
            || !valid_id(&payload.id)
            || !valid_id(&payload.source)
            || payload.encoded_bytes.len() > MAX_PAYLOAD_BYTES
            || stored
                .checked_add(payload.encoded_bytes.len())
                .is_none_or(|n| n > MAX_TOTAL_PAYLOAD_BYTES)
            || s.used_payload_ids.len() >= MAX_GATEWAY_IDENTITIES
            || s.used_payload_ids.contains(&payload.id)
        {
            return Err(DispatchError::Denied);
        }
        s.used_payload_ids.insert(payload.id.clone());
        s.payloads.insert(payload.id.clone(), payload);
        Ok(())
    }
    pub fn install_grant(&self, grant: Grant) -> Result<(), DispatchError> {
        let mut s = lock(&self.state)?;
        if !s.active
            || !valid_id(&grant.id)
            || !valid_id(&grant.source)
            || !valid_id(&grant.payload_id)
            || !valid_id(&grant.recipient_id)
            || !valid_context(&grant.context)
            || grant.context != s.context
            || grant.encoded_size > MAX_PAYLOAD_BYTES
            || s.used_grant_ids.len() >= MAX_GATEWAY_IDENTITIES
            || s.used_grant_ids.contains(&grant.id)
        {
            return Err(DispatchError::Denied);
        }
        let payload = s
            .payloads
            .get(&grant.payload_id)
            .ok_or(DispatchError::Denied)?;
        if grant.source != payload.source
            || grant.classification != payload.classification.unwrap_or(Classification::Private)
            || grant.payload_kind != payload.kind
            || grant.encoded_size != payload.encoded_bytes.len()
            || !s.recipients.contains_key(&grant.recipient_id)
        {
            return Err(DispatchError::Denied);
        }
        s.used_grant_ids.insert(grant.id.clone());
        s.grants.insert(grant.id.clone(), grant);
        Ok(())
    }
    /// Irreversibly close this host's run. Waits for admitted sends; it cannot
    /// cancel or recall a transport already in flight. Registered gateways are
    /// also retired by the owner's run start and contract revision.
    /// If the mutex was poisoned, still release retained state but return
    /// `Unavailable`; all restricted operations remain permanently fail-closed.
    pub fn retire_run(&self) -> Result<(), DispatchError> {
        retire_state(&self.state)
    }
    /// Weak lifecycle link for the task authority; it never keeps the host,
    /// its transports or handles alive.
    pub(crate) fn link(&self) -> GatewayLink {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        GatewayLink {
            task: state.context.task_id.clone(),
            state: Arc::downgrade(&self.state),
        }
    }
    pub fn revoke_grant(&self, id: &str) -> Result<(), DispatchError> {
        let mut s = lock(&self.state)?;
        if s.grants.remove(id).is_none() {
            return Err(DispatchError::Denied);
        }
        Ok(())
    }
    /// Replace the run/revision, invalidating all prior handles, payloads and grants.
    /// IDs cannot be reused during this host's lifetime, even across revisions.
    pub fn advance_revision(
        &self,
        context: RunContext,
        mode: ProcessingMode,
    ) -> Result<(), DispatchError> {
        let mut s = lock(&self.state)?;
        if !s.active
            || !valid_context(&context)
            || context.task_id != s.context.task_id
            || context.contract_revision <= s.context.contract_revision
        {
            return Err(DispatchError::Denied);
        }
        s.context = context;
        s.mode = mode;
        s.grants.clear();
        s.payloads.clear();
        Ok(())
    }
}
/// Close admission and detach all retained state, even after poison. Waits for
/// an admitted send; drops transports only after releasing the mutex because a
/// transport destructor may re-enter a restricted handle.
fn retire_state(state: &Mutex<State>) -> Result<(), DispatchError> {
    let (removed, poisoned) = {
        let (mut state, poisoned) = match state.lock() {
            Ok(state) => (state, false),
            Err(error) => (error.into_inner(), true),
        };
        state.active = false;
        let removed = (
            std::mem::take(&mut state.grants),
            std::mem::take(&mut state.payloads),
            std::mem::take(&mut state.recipients),
        );
        (removed, poisoned)
    };
    drop(removed);
    if poisoned {
        Err(DispatchError::Unavailable)
    } else {
        Ok(())
    }
}

/// Crate-private weak reference used to retire a gateway on run lifecycle
/// changes. Not exposed to adapters; it grants no dispatch or install ability.
pub(crate) struct GatewayLink {
    task: TaskId,
    state: Weak<Mutex<State>>,
}
impl GatewayLink {
    pub(crate) fn task(&self) -> &TaskId {
        &self.task
    }
    pub(crate) fn is_live(&self) -> bool {
        self.state.strong_count() > 0
    }
    /// Irreversible, like `GatewayHost::retire_run`; a dead gateway is a no-op.
    pub(crate) fn retire(&self) {
        if let Some(state) = self.state.upgrade() {
            // Poison still closes and detaches; nothing further to report here.
            let _ = retire_state(&state);
        }
    }
}

impl Drop for GatewayHost {
    fn drop(&mut self) {
        // `retire_run` closes and detaches even on poison; it drops transports
        // only after releasing the mutex.
        let _ = self.retire_run();
    }
}
impl DispatchHandle {
    /// Inference convenience; every purpose, including inference, still requires an exact grant.
    pub fn dispatch(&self, payload_id: &str, grant_id: &str) -> Result<(), DispatchError> {
        self.dispatch_for(payload_id, grant_id, Purpose::Inference)
    }
    pub fn dispatch_for(
        &self,
        payload_id: &str,
        grant_id: &str,
        purpose: Purpose,
    ) -> Result<(), DispatchError> {
        let s = lock(&self.state)?;
        if !s.active || self.context != s.context || purpose != self.purpose {
            return Err(DispatchError::Denied);
        }
        let grant = s.grants.get(grant_id).ok_or(DispatchError::Denied)?;
        let payload = s.payloads.get(payload_id).ok_or(DispatchError::Denied)?;
        let recipient = s
            .recipients
            .get(&grant.recipient_id)
            .ok_or(DispatchError::Denied)?;
        let classification = payload.classification.unwrap_or(Classification::Private);
        if grant.context != s.context
            || grant.payload_id != payload_id
            || grant.source != payload.source
            || grant.classification != classification
            || grant.purpose != purpose
            || grant.payload_kind != payload.kind
            || grant.encoded_size != payload.encoded_bytes.len()
            || SystemTime::now() >= grant.expires_at
            || payload.secret
            || (purpose == Purpose::Telemetry && !payload.encoded_bytes.is_empty())
            || classification == Classification::Sensitive
            || (recipient.kind == RecipientKind::Remote && s.mode != ProcessingMode::ApprovedCloud)
        {
            return Err(DispatchError::Denied);
        }
        recipient
            .transport
            .send(&payload.encoded_bytes)
            .map_err(|_| DispatchError::TransportFailed)
    }
}
