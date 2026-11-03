//! C05: the one owner of browser target discovery and page attachment.
//!
//! [`TargetRegistry::start`] subscribes to events *before* enabling target
//! discovery, then merges a `Target.getTargets` snapshot. An owned listener
//! task applies browser-level `Target.*` events; a lagged subscription is
//! resynchronized from a fresh snapshot rather than continued with a stale map.
//! Tab attachment happens only through the registry (explicit, flattened,
//! one session per target); browser-level auto-attach is never used, so there
//! is no second attachment authority for tabs. A page's out-of-process
//! iframes are attached through session-level auto-attach on that page's own
//! sessions and belong to its frame manager (D04).
//!
//! Target IDs are never reused by a browser, so a destroyed ID is remembered
//! and never re-added by a snapshot that was taken before the destruction.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use aero_core::ids::{CdpSessionId, TargetId};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::{CdpConnection, CdpError, CdpEvent, EventStream};

/// Deadline for registry CDP calls.
pub(crate) const CALL: Duration = Duration::from_secs(10);

/// What a target is, from its CDP `type`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Page,
    Iframe,
    /// `worker`, `shared_worker` or `service_worker`.
    Worker,
    /// Chromium's own UI surfaces (`browser_ui`, `tab`, `background_page`, ...).
    Other(String),
}

/// One target as last reported by the browser. All text is untrusted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetInfo {
    pub target_id: TargetId,
    pub kind: TargetKind,
    pub url: String,
    pub title: String,
    /// Attached by any client (as reported by the browser).
    pub attached: bool,
    pub opener: Option<TargetId>,
}

impl TargetInfo {
    /// A user-visible tab: a page that is not a DevTools front end.
    pub fn is_tab(&self) -> bool {
        self.kind == TargetKind::Page && !self.url.starts_with("devtools://")
    }
}

/// Registry contents. Pure: events and snapshots in, state out.
#[derive(Default)]
pub(crate) struct RegistryState {
    targets: HashMap<TargetId, (u64, TargetInfo)>,
    next_sequence: u64,
    destroyed: HashSet<TargetId>,
    /// Our attachments: `None` while an attach call is in flight.
    attached: HashMap<TargetId, Option<CdpSessionId>>,
    resyncs: u64,
    /// Counts ended sessions of ours; published so in-flight page calls can
    /// fail promptly instead of waiting for a reply that never comes.
    detachments: u64,
    /// Why the listener stopped, once it has.
    closed: Option<String>,
}

impl RegistryState {
    /// Apply one event. Only browser-level (`session_id == None`) target
    /// events change the registry.
    pub(crate) fn apply(&mut self, event: &CdpEvent) {
        if event.session_id.is_some() {
            return;
        }
        let params = &event.params;
        match event.method.as_str() {
            "Target.targetCreated" | "Target.targetInfoChanged" | "Target.attachedToTarget" => {
                if let Some(info) = parse_info(&params["targetInfo"]) {
                    self.upsert(info);
                }
            }
            "Target.targetDestroyed" => {
                if let Some(id) = params["targetId"].as_str() {
                    self.remove(&TargetId::from(id));
                }
            }
            "Target.detachedFromTarget" => {
                if let Some(session) = params["sessionId"].as_str() {
                    let before = self.attached.len();
                    self.attached
                        .retain(|_, ours| ours.as_ref().map(CdpSessionId::as_str) != Some(session));
                    self.detachments += (before - self.attached.len()) as u64;
                }
            }
            _ => {}
        }
    }

    /// Marks the start of a snapshot request; pass it to [`Self::apply_snapshot`].
    pub(crate) fn snapshot_mark(&self) -> u64 {
        self.next_sequence
    }

    /// Merge a `Target.getTargets` result requested at `mark`. Targets known
    /// before `mark` but missing from the snapshot are gone; targets
    /// discovered after `mark` (by events racing the reply) are kept.
    /// Destroyed IDs stay destroyed. Our sessions for targets the browser no
    /// longer reports as attached are dropped (detach events may be missed).
    pub(crate) fn apply_snapshot(&mut self, infos: &[Value], mark: u64) {
        let parsed: Vec<TargetInfo> = infos.iter().filter_map(parse_info).collect();
        let present: HashSet<&TargetId> = parsed.iter().map(|info| &info.target_id).collect();
        let gone: Vec<TargetId> = self
            .targets
            .iter()
            .filter(|(id, (sequence, _))| *sequence < mark && !present.contains(id))
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            self.remove(&id);
        }
        for info in &parsed {
            if !info.attached
                && let Some(Some(_)) = self.attached.get(&info.target_id)
            {
                self.attached.remove(&info.target_id);
                self.detachments += 1;
            }
        }
        for info in parsed {
            self.upsert(info);
        }
    }

    /// Targets in discovery order.
    pub(crate) fn targets(&self) -> Vec<TargetInfo> {
        let mut all: Vec<&(u64, TargetInfo)> = self.targets.values().collect();
        all.sort_by_key(|(sequence, _)| *sequence);
        all.into_iter().map(|(_, info)| info.clone()).collect()
    }

    fn upsert(&mut self, info: TargetInfo) {
        if self.destroyed.contains(&info.target_id) {
            return;
        }
        match self.targets.get_mut(&info.target_id) {
            Some((_, existing)) => *existing = info,
            None => {
                let sequence = self.next_sequence;
                self.next_sequence += 1;
                self.targets
                    .insert(info.target_id.clone(), (sequence, info));
            }
        }
    }

    fn remove(&mut self, id: &TargetId) {
        self.targets.remove(id);
        if let Some(Some(_)) = self.attached.remove(id) {
            self.detachments += 1;
        }
        self.destroyed.insert(id.clone());
    }

    pub(crate) fn session_of(&self, target: &TargetId) -> Option<&CdpSessionId> {
        self.attached.get(target)?.as_ref()
    }
}

/// Shared read access to the registry, for pages checking session ownership.
#[derive(Clone)]
pub(crate) struct TargetRegistryHandle {
    state: Arc<Mutex<RegistryState>>,
    detachments: watch::Receiver<u64>,
}

impl TargetRegistryHandle {
    pub(crate) fn owns(&self, target: &TargetId, session: &CdpSessionId) -> bool {
        lock(&self.state).session_of(target) == Some(session)
    }

    /// Resolves once the registry no longer owns `session` for `target`, or
    /// once the registry has stopped (ownership can then no longer be known).
    pub(crate) async fn until_detached(&self, target: &TargetId, session: &CdpSessionId) {
        let mut changes = self.detachments.clone();
        loop {
            changes.borrow_and_update();
            if !self.owns(target, session) {
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    }
}

/// Live target registry for one browser connection.
pub struct TargetRegistry {
    state: Arc<Mutex<RegistryState>>,
    detachments: watch::Receiver<u64>,
    connection: Arc<CdpConnection>,
    listener: Option<JoinHandle<()>>,
}

/// Why attaching failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachError {
    UnknownTarget,
    /// Only tabs are attached here; frames and workers have other owners.
    NotATab,
    /// This registry already owns (or is creating) a session for the target.
    AlreadyAttached,
    Cdp(CdpError),
    /// The browser replied without a session ID.
    InvalidReply,
}

impl TargetRegistry {
    /// Subscribe, start the owned listener, enable discovery, then merge a
    /// snapshot of existing targets.
    pub async fn start(connection: Arc<CdpConnection>) -> Result<Self, CdpError> {
        // Subscribe before enabling, so no discovery event can be missed.
        let events = connection.subscribe();
        let state = Arc::new(Mutex::new(RegistryState::default()));
        let (publish, detachments) = watch::channel(0);
        let listener = tokio::spawn(listen(
            events,
            Arc::clone(&state),
            Arc::downgrade(&connection),
            publish,
        ));
        // From here, dropping `registry` on an error aborts the listener.
        let registry = Self {
            state,
            detachments,
            connection,
            listener: Some(listener),
        };
        registry
            .connection
            .call(
                "Target.setDiscoverTargets",
                json!({ "discover": true }),
                None,
                CALL,
            )
            .await?;
        resync(&registry.connection, &registry.state).await?;
        Ok(registry)
    }

    /// All known targets in discovery order.
    pub fn targets(&self) -> Vec<TargetInfo> {
        lock(&self.state).targets()
    }

    /// User-visible tabs in discovery order.
    pub fn tabs(&self) -> Vec<TargetInfo> {
        self.targets()
            .into_iter()
            .filter(TargetInfo::is_tab)
            .collect()
    }

    pub fn target(&self, id: &TargetId) -> Option<TargetInfo> {
        self.targets().into_iter().find(|t| &t.target_id == id)
    }

    /// Times the registry rebuilt itself after missing events.
    pub fn resyncs(&self) -> u64 {
        lock(&self.state).resyncs
    }

    /// Why the listener stopped, if it has (the registry is then frozen).
    pub fn failure(&self) -> Option<String> {
        lock(&self.state).closed.clone()
    }

    /// Attach to a tab with a flattened session owned by this registry.
    pub async fn attach(&self, target: &TargetId) -> Result<CdpSessionId, AttachError> {
        let mut reservation = {
            let mut state = lock(&self.state);
            let info = state
                .targets
                .get(target)
                .map(|(_, info)| info)
                .ok_or(AttachError::UnknownTarget)?;
            if !info.is_tab() {
                return Err(AttachError::NotATab);
            }
            if state.attached.contains_key(target) {
                return Err(AttachError::AlreadyAttached);
            }
            state.attached.insert(target.clone(), None);
            Reservation {
                state: &self.state,
                target,
                committed: false,
            }
        };
        let reply = self
            .connection
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target.as_str(), "flatten": true }),
                None,
                CALL,
            )
            .await
            .map_err(AttachError::Cdp)?;
        let session = reply["sessionId"]
            .as_str()
            .filter(|session| !session.is_empty())
            .map(CdpSessionId::from)
            .ok_or(AttachError::InvalidReply)?;
        let mut state = lock(&self.state);
        // The target may have been destroyed while the call was in flight.
        if state.attached.get(target) != Some(&None) {
            return Err(AttachError::UnknownTarget);
        }
        state.attached.insert(target.clone(), Some(session.clone()));
        reservation.committed = true;
        Ok(session)
    }

    pub(crate) fn handle(&self) -> TargetRegistryHandle {
        TargetRegistryHandle {
            state: Arc::clone(&self.state),
            detachments: self.detachments.clone(),
        }
    }

    pub(crate) fn connection(&self) -> &Arc<CdpConnection> {
        &self.connection
    }

    /// Stop the listener and wait for it.
    pub async fn stop(&mut self) {
        if let Some(listener) = self.listener.take() {
            listener.abort();
            let _ = listener.await;
        }
    }
}

impl Drop for TargetRegistry {
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            listener.abort();
        }
    }
}

/// Holds an in-flight attach slot; releases it unless the attach committed
/// (including when the attach future is dropped).
struct Reservation<'a> {
    state: &'a Mutex<RegistryState>,
    target: &'a TargetId,
    committed: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let mut state = lock(self.state);
            if state.attached.get(self.target) == Some(&None) {
                state.attached.remove(self.target);
            }
        }
    }
}

/// Apply events until the stream ends. Missed events trigger a snapshot
/// resync; a failed resync or a closed stream stops the registry with a
/// recorded reason instead of continuing on a stale map.
async fn listen(
    mut events: EventStream,
    state: Arc<Mutex<RegistryState>>,
    connection: Weak<CdpConnection>,
    publish: watch::Sender<u64>,
) {
    let announce = |state: &Mutex<RegistryState>| {
        let count = lock(state).detachments;
        publish.send_if_modified(|seen| {
            let changed = *seen != count;
            *seen = count;
            changed
        });
    };
    loop {
        match events.recv().await {
            Ok(event) => {
                lock(&state).apply(&event);
                announce(&state);
            }
            Err(CdpError::EventLag(_)) => {
                let Some(connection) = connection.upgrade() else {
                    lock(&state).closed = Some("connection dropped".into());
                    break;
                };
                if let Err(error) = resync(&connection, &state).await {
                    lock(&state).closed = Some(format!("resync failed: {error}"));
                    break;
                }
                lock(&state).resyncs += 1;
                announce(&state);
            }
            Err(error) => {
                lock(&state).closed = Some(error.to_string());
                break;
            }
        }
    }
}

async fn resync(connection: &CdpConnection, state: &Mutex<RegistryState>) -> Result<(), CdpError> {
    let mark = lock(state).snapshot_mark();
    let reply = connection
        .call("Target.getTargets", json!({}), None, CALL)
        .await?;
    let infos = reply["targetInfos"]
        .as_array()
        .ok_or_else(|| CdpError::InvalidMessage("getTargets reply without targetInfos".into()))?;
    lock(state).apply_snapshot(infos, mark);
    Ok(())
}

fn parse_info(value: &Value) -> Option<TargetInfo> {
    let id = value["targetId"].as_str().filter(|id| !id.is_empty())?;
    let kind = match value["type"].as_str()? {
        "page" => TargetKind::Page,
        "iframe" => TargetKind::Iframe,
        "worker" | "shared_worker" | "service_worker" => TargetKind::Worker,
        other => TargetKind::Other(other.to_owned()),
    };
    let text = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
    Some(TargetInfo {
        target_id: TargetId::from(id),
        kind,
        url: text("url"),
        title: text("title"),
        attached: value["attached"].as_bool().unwrap_or(false),
        opener: value["openerId"].as_str().map(TargetId::from),
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, kind: &str, url: &str) -> Value {
        json!({
            "targetId": id, "type": kind, "url": url, "title": url,
            "attached": false, "browserContextId": "ctx"
        })
    }
    fn event(method: &str, params: Value) -> CdpEvent {
        CdpEvent {
            method: method.into(),
            params,
            session_id: None,
        }
    }
    fn ids(state: &RegistryState) -> Vec<String> {
        state
            .targets()
            .iter()
            .map(|t| t.target_id.as_str().to_owned())
            .collect()
    }

    #[test]
    fn events_maintain_targets_in_discovery_order() {
        let mut state = RegistryState::default();
        state.apply(&event(
            "Target.targetCreated",
            json!({ "targetInfo": info("B", "page", "about:blank") }),
        ));
        state.apply(&event(
            "Target.targetCreated",
            json!({ "targetInfo": info("A", "service_worker", "chrome-extension://x/sw.js") }),
        ));
        assert_eq!(ids(&state), ["B", "A"]);
        let mut changed = info("B", "page", "http://127.0.0.1:1/a.html");
        changed["title"] = "Fixture".into();
        state.apply(&event(
            "Target.targetInfoChanged",
            json!({ "targetInfo": changed }),
        ));
        let b = &state.targets()[0];
        assert_eq!(
            (b.url.as_str(), b.title.as_str()),
            ("http://127.0.0.1:1/a.html", "Fixture")
        );
        assert_eq!(ids(&state), ["B", "A"], "an update keeps discovery order");
        state.apply(&event("Target.targetDestroyed", json!({ "targetId": "B" })));
        assert_eq!(ids(&state), ["A"]);
        // Malformed or session-scoped events change nothing.
        state.apply(&event(
            "Target.targetCreated",
            json!({ "targetInfo": { "type": "page" } }),
        ));
        let mut scoped = event(
            "Target.targetCreated",
            json!({ "targetInfo": info("C", "iframe", "x") }),
        );
        scoped.session_id = Some(CdpSessionId::from("page-session"));
        state.apply(&scoped);
        assert_eq!(ids(&state), ["A"]);
    }

    #[test]
    fn tabs_exclude_workers_frames_ui_and_devtools() {
        let mut state = RegistryState::default();
        let mark = state.snapshot_mark();
        state.apply_snapshot(
            &[
                info("page", "page", "http://127.0.0.1:1/"),
                info(
                    "devtools",
                    "page",
                    "devtools://devtools/bundled/inspector.html",
                ),
                info("sw", "service_worker", "chrome-extension://x/sw.js"),
                info("shared", "shared_worker", "http://127.0.0.1:1/w.js"),
                info("dedicated", "worker", "http://127.0.0.1:1/d.js"),
                info("frame", "iframe", "http://127.0.0.1:2/"),
                info("ui", "browser_ui", "chrome://omnibox-popup.top-chrome/"),
            ],
            mark,
        );
        let targets = state.targets();
        let tabs: Vec<&str> = targets
            .iter()
            .filter(|t| t.is_tab())
            .map(|t| t.target_id.as_str())
            .collect();
        assert_eq!(tabs, ["page"]);
        let kinds: Vec<TargetKind> = targets.iter().map(|t| t.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                TargetKind::Page,
                TargetKind::Page,
                TargetKind::Worker,
                TargetKind::Worker,
                TargetKind::Worker,
                TargetKind::Iframe,
                TargetKind::Other("browser_ui".into()),
            ]
        );
    }

    #[test]
    fn snapshots_are_authoritative_but_never_revive_destroyed_targets() {
        let mut state = RegistryState::default();
        let mark = state.snapshot_mark();
        state.apply_snapshot(&[info("A", "page", "a"), info("B", "page", "b")], mark);
        state.apply(&event("Target.targetDestroyed", json!({ "targetId": "A" })));
        // A snapshot taken before A's destruction still lists it.
        let mark = state.snapshot_mark();
        state.apply_snapshot(
            &[
                info("A", "page", "a"),
                info("B", "page", "b"),
                info("C", "page", "c"),
            ],
            mark,
        );
        assert_eq!(ids(&state), ["B", "C"]);
        // A later snapshot without B removes it.
        let mark = state.snapshot_mark();
        state.apply_snapshot(&[info("C", "page", "c")], mark);
        assert_eq!(ids(&state), ["C"]);
        state.apply(&event(
            "Target.targetCreated",
            json!({ "targetInfo": info("A", "page", "a") }),
        ));
        assert_eq!(ids(&state), ["C"], "destroyed IDs are never re-added");
    }

    #[test]
    fn targets_discovered_after_the_snapshot_request_are_kept() {
        let mut state = RegistryState::default();
        let mark = state.snapshot_mark();
        // An event for a new target is applied before the older snapshot.
        state.apply(&event(
            "Target.targetCreated",
            json!({ "targetInfo": info("new", "page", "about:blank") }),
        ));
        state.apply_snapshot(&[info("old", "page", "a")], mark);
        assert_eq!(ids(&state), ["new", "old"]);
    }

    #[test]
    fn detach_and_destroy_end_our_session() {
        let mut state = RegistryState::default();
        let mark = state.snapshot_mark();
        state.apply_snapshot(&[info("A", "page", "a"), info("B", "page", "b")], mark);
        let a = TargetId::from("A");
        let b = TargetId::from("B");
        state
            .attached
            .insert(a.clone(), Some(CdpSessionId::from("s-a")));
        state
            .attached
            .insert(b.clone(), Some(CdpSessionId::from("s-b")));
        state.apply(&event(
            "Target.detachedFromTarget",
            json!({ "sessionId": "s-a", "targetId": "A" }),
        ));
        assert_eq!(state.session_of(&a), None);
        state.apply(&event("Target.targetDestroyed", json!({ "targetId": "B" })));
        assert_eq!(state.session_of(&b), None);
        // A snapshot that drops a target also drops our session for it.
        state
            .attached
            .insert(a.clone(), Some(CdpSessionId::from("s-a2")));
        let mark = state.snapshot_mark();
        state.apply_snapshot(&[], mark);
        assert_eq!(state.session_of(&a), None);
        assert_eq!(state.detachments, 3, "every ended session is announced");
    }
}
