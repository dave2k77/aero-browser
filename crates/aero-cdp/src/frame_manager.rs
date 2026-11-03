//! D04: the frame graph of one attached page.
//!
//! A page's frames live in several renderer processes. Same-process frames
//! are reported on the page's own session; each out-of-process iframe (OOPIF)
//! is attached through *session-level* auto-attach on its parent's session
//! (`Target.setAutoAttach` with an iframe-only filter and
//! `waitForDebuggerOnStart`), configured, and then resumed. Browser-level
//! auto-attach is never used, so tabs keep a single attachment authority
//! (the target registry); child sessions belong to the page's frame manager.
//!
//! The graph is built from frame trees, `Page.frame*` lifecycle events and
//! `Runtime.executionContext*` auxiliary data — never from target and frame
//! ID equality. Execution-context IDs are only unique per session, so a
//! context is identified by (session, ID). Navigating a frame replaces its
//! document epoch (`loaderId`) and drops its descendants; detaching a frame or
//! a child session drops the whole subtree. Missed events make the graph
//! unknown until it is rebuilt by turning auto-attach off and on again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use aero_core::ids::{CdpSessionId, DocumentEpoch, FrameId};
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::target_registry::CALL;
use crate::{CdpConnection, CdpError, CdpEvent, EventStream};

/// A JavaScript execution context. Numeric IDs repeat across sessions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExecutionContextRef {
    pub session: CdpSessionId,
    pub id: i64,
}

/// One frame as currently known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    pub frame: FrameId,
    pub parent: Option<FrameId>,
    /// The current document; `None` until one has committed in the session
    /// that owns the frame.
    pub document: Option<DocumentEpoch>,
    /// The session owning the frame (the page's own session for
    /// same-process frames).
    pub session: CdpSessionId,
    /// The first frame of its session: the main frame or an OOPIF root.
    pub local_root: bool,
    /// The frame's default (main-world) execution context, once created.
    pub context: Option<ExecutionContextRef>,
}

/// The frames reachable from the main frame, parents before children (in
/// discovery order), and the graph version they were read at. The version
/// changes whenever a frame, document or session changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSnapshot {
    pub version: u64,
    pub frames: Vec<FrameInfo>,
}

/// What the manager must do for a newly attached child target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// An iframe: configure it, then resume it if it is waiting.
    Configure {
        session: CdpSessionId,
        waiting: bool,
    },
    /// Anything else: resume it if waiting and detach it from `parent`.
    Reject {
        parent: CdpSessionId,
        session: CdpSessionId,
        waiting: bool,
    },
}

/// One known frame.
#[derive(Clone, Debug)]
struct Node {
    /// Discovery order, for a stable parents-first listing.
    sequence: u64,
    parent: Option<FrameId>,
    document: Option<DocumentEpoch>,
    session: CdpSessionId,
    /// The default context's ID, within `session`.
    context: Option<i64>,
}

/// Frame graph contents. Pure: events and frame trees in, state out.
pub(crate) struct FrameGraph {
    page_session: CdpSessionId,
    frames: HashMap<FrameId, Node>,
    next_sequence: u64,
    /// Our child sessions and the session each was attached from.
    children: HashMap<CdpSessionId, CdpSessionId>,
    version: u64,
    session_ends: u64,
    unknown: bool,
    resyncs: u64,
}

impl FrameGraph {
    pub(crate) fn new(page_session: CdpSessionId) -> Self {
        Self {
            page_session,
            frames: HashMap::new(),
            next_sequence: 0,
            children: HashMap::new(),
            version: 0,
            session_ends: 0,
            unknown: false,
            resyncs: 0,
        }
    }

    /// The page's session or a live child session of ours.
    pub(crate) fn owns_session(&self, session: &CdpSessionId) -> bool {
        session == &self.page_session || self.children.contains_key(session)
    }

    /// Apply one event; returns what to do for a newly attached child.
    pub(crate) fn apply(&mut self, event: &CdpEvent) -> Option<Action> {
        let session = event.session_id.as_ref()?;
        if !self.owns_session(session) {
            return None;
        }
        let params = &event.params;
        match event.method.as_str() {
            "Page.frameAttached" => {
                let frame: FrameId = text(&params["frameId"])?;
                let parent: FrameId = text(&params["parentFrameId"])?;
                if !self.frames.contains_key(&frame) {
                    self.insert(frame, Some(parent), None, session.clone());
                }
            }
            "Page.frameNavigated" => {
                let info = &params["frame"];
                let frame: FrameId = text(&info["id"])?;
                let document: DocumentEpoch = text(&info["loaderId"])?;
                let parent = text(&info["parentId"]);
                self.remove_descendants(&frame);
                match self.frames.get_mut(&frame) {
                    Some(node) => {
                        node.parent = parent;
                        node.document = Some(document);
                        node.session = session.clone();
                        node.context = None;
                    }
                    None => self.insert(frame, parent, Some(document), session.clone()),
                }
                self.version += 1;
            }
            "Page.frameDetached" => {
                let frame: FrameId = text(&params["frameId"])?;
                let owner = self.frames.get(&frame).map(|node| node.session.clone())?;
                if params["reason"].as_str() == Some("swap") {
                    // Moving to another process: only the session that still
                    // owns the frame loses its document and children.
                    if &owner == session {
                        self.remove_descendants(&frame);
                        if let Some(node) = self.frames.get_mut(&frame) {
                            node.document = None;
                            node.context = None;
                        }
                        self.version += 1;
                    }
                } else {
                    self.remove_subtree(&frame);
                }
            }
            "Runtime.executionContextCreated" => {
                let context = &params["context"];
                let id = context["id"].as_i64()?;
                let aux = &context["auxData"];
                if aux["isDefault"].as_bool() != Some(true) {
                    return None;
                }
                let frame: FrameId = text(&aux["frameId"])?;
                let node = self.frames.get_mut(&frame)?;
                if &node.session == session {
                    node.context = Some(id);
                }
            }
            "Runtime.executionContextDestroyed" => {
                let id = params["executionContextId"].as_i64()?;
                for node in self.frames.values_mut() {
                    if &node.session == session && node.context == Some(id) {
                        node.context = None;
                    }
                }
            }
            "Runtime.executionContextsCleared" => {
                for node in self.frames.values_mut() {
                    if &node.session == session {
                        node.context = None;
                    }
                }
            }
            "Target.attachedToTarget" => {
                let child: CdpSessionId = text(&params["sessionId"])?;
                let waiting = params["waitingForDebugger"].as_bool() == Some(true);
                if params["targetInfo"]["type"].as_str() != Some("iframe") {
                    return Some(Action::Reject {
                        parent: session.clone(),
                        session: child,
                        waiting,
                    });
                }
                self.children.insert(child.clone(), session.clone());
                self.version += 1;
                return Some(Action::Configure {
                    session: child,
                    waiting,
                });
            }
            "Target.detachedFromTarget" => {
                let child: CdpSessionId = text(&params["sessionId"])?;
                if self.children.get(&child) == Some(session) {
                    self.end_session(&child);
                }
            }
            _ => {}
        }
        None
    }

    /// Merge a `Page.getFrameTree` reply from `session`. Fills gaps only:
    /// state already derived from events is never overwritten, because those
    /// events may be newer than the tree.
    pub(crate) fn seed(&mut self, session: &CdpSessionId, tree: &Value) {
        if !self.owns_session(session) {
            return;
        }
        let mut pending = vec![&tree["frameTree"]];
        let mut visited = 0;
        while let Some(entry) = pending.pop() {
            visited += 1;
            if visited > MAX_SEEDED_FRAMES {
                break;
            }
            let info = &entry["frame"];
            let Some(frame) = text::<FrameId>(&info["id"]) else {
                continue;
            };
            let document: Option<DocumentEpoch> = text(&info["loaderId"]);
            match self.frames.get_mut(&frame) {
                Some(node) => {
                    if node.document.is_none() && &node.session == session {
                        node.document = document;
                        self.version += 1;
                    }
                }
                None => {
                    self.insert(frame, text(&info["parentId"]), document, session.clone());
                    self.version += 1;
                }
            }
            if let Some(children) = entry["childFrames"].as_array() {
                pending.extend(children.iter().rev());
            }
        }
    }

    /// Forget all frames and child sessions; the graph stays unknown until
    /// [`Self::finish_resync`].
    pub(crate) fn reset(&mut self) {
        self.frames.clear();
        if !self.children.is_empty() {
            self.children.clear();
            self.session_ends += 1;
        }
        self.unknown = true;
        self.version += 1;
    }

    pub(crate) fn finish_resync(&mut self) {
        self.unknown = false;
        self.resyncs += 1;
        self.version += 1;
    }

    /// The current frames, or `None` while the graph is unknown.
    pub(crate) fn snapshot(&self) -> Option<FrameSnapshot> {
        if self.unknown {
            return None;
        }
        let mut children: HashMap<Option<&FrameId>, Vec<(&FrameId, &Node)>> = HashMap::new();
        for (frame, node) in &self.frames {
            children
                .entry(node.parent.as_ref())
                .or_default()
                .push((frame, node));
        }
        for list in children.values_mut() {
            list.sort_by_key(|(_, node)| node.sequence);
        }
        let mut frames = Vec::new();
        let mut queue: std::collections::VecDeque<&FrameId> = children
            .get(&None)
            .into_iter()
            .flatten()
            .map(|(frame, _)| *frame)
            .collect();
        while let Some(frame) = queue.pop_front() {
            let node = &self.frames[frame];
            let local_root = match &node.parent {
                None => true,
                Some(parent) => self
                    .frames
                    .get(parent)
                    .is_none_or(|parent| parent.session != node.session),
            };
            frames.push(FrameInfo {
                frame: frame.clone(),
                parent: node.parent.clone(),
                document: node.document.clone(),
                session: node.session.clone(),
                local_root,
                context: node.context.map(|id| ExecutionContextRef {
                    session: node.session.clone(),
                    id,
                }),
            });
            queue.extend(
                children
                    .get(&Some(frame))
                    .into_iter()
                    .flatten()
                    .map(|(child, _)| *child),
            );
        }
        Some(FrameSnapshot {
            version: self.version,
            frames,
        })
    }

    /// Counts ended child sessions (published so calls fail promptly).
    pub(crate) fn session_ends(&self) -> u64 {
        self.session_ends
    }

    pub(crate) fn resyncs(&self) -> u64 {
        self.resyncs
    }

    fn insert(
        &mut self,
        frame: FrameId,
        parent: Option<FrameId>,
        document: Option<DocumentEpoch>,
        session: CdpSessionId,
    ) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.frames.insert(
            frame,
            Node {
                sequence,
                parent,
                document,
                session,
                context: None,
            },
        );
        self.version += 1;
    }

    /// Every frame below `frame` (not `frame` itself).
    fn descendants(&self, frame: &FrameId) -> Vec<FrameId> {
        let mut found = Vec::new();
        let mut pending = vec![frame.clone()];
        while let Some(parent) = pending.pop() {
            for (child, node) in &self.frames {
                if node.parent.as_ref() == Some(&parent) && !found.contains(child) {
                    found.push(child.clone());
                    pending.push(child.clone());
                }
            }
        }
        found
    }

    fn remove_descendants(&mut self, frame: &FrameId) {
        let below = self.descendants(frame);
        if !below.is_empty() {
            for child in below {
                self.frames.remove(&child);
            }
            self.version += 1;
        }
    }

    fn remove_subtree(&mut self, frame: &FrameId) {
        self.remove_descendants(frame);
        if self.frames.remove(frame).is_some() {
            self.version += 1;
        }
    }

    /// End a child session, the sessions attached through it, and every
    /// frame they own (with the frames below those).
    fn end_session(&mut self, child: &CdpSessionId) {
        let mut ended = vec![child.clone()];
        let mut index = 0;
        while index < ended.len() {
            let parent = ended[index].clone();
            ended.extend(
                self.children
                    .iter()
                    .filter(|(_, from)| **from == parent)
                    .map(|(session, _)| session.clone()),
            );
            index += 1;
        }
        for session in &ended {
            self.children.remove(session);
        }
        let owned: Vec<FrameId> = self
            .frames
            .iter()
            .filter(|(_, node)| ended.contains(&node.session))
            .map(|(frame, _)| frame.clone())
            .collect();
        for frame in owned {
            self.remove_subtree(&frame);
        }
        self.session_ends += 1;
        self.version += 1;
    }
}

/// Bound on frames read from one frame tree reply.
const MAX_SEEDED_FRAMES: usize = 1_000;

/// A non-empty string field as an ID.
fn text<T: for<'a> From<&'a str>>(value: &Value) -> Option<T> {
    value.as_str().filter(|text| !text.is_empty()).map(T::from)
}

/// State shared by the listener, configuration and resync tasks.
struct Shared {
    graph: Mutex<FrameGraph>,
    ends: watch::Sender<u64>,
    page_session: CdpSessionId,
    /// A rebuild is running (at most one at a time).
    resyncing: std::sync::atomic::AtomicBool,
}

impl Shared {
    /// Run `change` on the graph and publish ended sessions.
    fn update<T>(&self, change: impl FnOnce(&mut FrameGraph) -> T) -> T {
        let (result, ends) = {
            let mut graph = lock(&self.graph);
            let result = change(&mut graph);
            (result, graph.session_ends())
        };
        self.ends.send_if_modified(|published| {
            let changed = *published != ends;
            *published = ends;
            changed
        });
        result
    }
}

/// Session-level auto-attach: iframes only, paused until configured. The
/// browser refuses a filter when disabling.
fn auto_attach(enabled: bool) -> Value {
    if !enabled {
        return json!({ "autoAttach": false, "waitForDebuggerOnStart": false, "flatten": true });
    }
    json!({
        "autoAttach": true,
        "waitForDebuggerOnStart": true,
        "flatten": true,
        "filter": [{ "type": "iframe" }],
    })
}

/// Live frame graph for one page session.
pub(crate) struct FrameManager {
    shared: Arc<Shared>,
    ends: watch::Receiver<u64>,
    connection: Arc<CdpConnection>,
    listener: Option<JoinHandle<()>>,
}

impl FrameManager {
    /// Subscribe, seed the page session's frames, enable runtime events and
    /// session-level OOPIF auto-attach.
    pub(crate) async fn start(
        connection: Arc<CdpConnection>,
        page_session: CdpSessionId,
    ) -> Result<Self, CdpError> {
        // Subscribe before enabling anything, so no event can be missed.
        let events = connection.subscribe();
        let (ends_tx, ends) = watch::channel(0);
        let shared = Arc::new(Shared {
            graph: Mutex::new(FrameGraph::new(page_session.clone())),
            ends: ends_tx,
            page_session,
            resyncing: std::sync::atomic::AtomicBool::new(false),
        });
        let listener = tokio::spawn(listen(
            events,
            Arc::clone(&shared),
            Arc::downgrade(&connection),
        ));
        // From here, dropping `manager` on an error aborts the listener.
        let manager = Self {
            shared,
            ends,
            connection,
            listener: Some(listener),
        };
        build(&manager.connection, &manager.shared).await?;
        Ok(manager)
    }

    pub(crate) fn snapshot(&self) -> Option<FrameSnapshot> {
        lock(&self.shared.graph).snapshot()
    }

    pub(crate) fn owns_session(&self, session: &CdpSessionId) -> bool {
        lock(&self.shared.graph).owns_session(session)
    }

    /// Resolves once `session` is no longer one of ours.
    pub(crate) async fn until_ended(&self, session: &CdpSessionId) {
        let mut changes = self.ends.clone();
        loop {
            changes.borrow_and_update();
            if !self.owns_session(session) {
                return;
            }
            if changes.changed().await.is_err() {
                return;
            }
        }
    }

    /// Rebuild the graph from scratch.
    pub(crate) async fn resync(&self) -> Result<(), CdpError> {
        resync(&self.connection, &self.shared).await
    }

    pub(crate) fn resyncs(&self) -> u64 {
        lock(&self.shared.graph).resyncs()
    }
}

impl Drop for FrameManager {
    fn drop(&mut self) {
        if let Some(listener) = self.listener.take() {
            listener.abort();
        }
        // Without a listener, new OOPIFs would wait for a debugger forever:
        // turn auto-attach off (which also detaches our child sessions).
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let connection = Arc::clone(&self.connection);
            let session = self.shared.page_session.clone();
            runtime.spawn(async move {
                let _ = connection
                    .call(
                        "Target.setAutoAttach",
                        auto_attach(false),
                        Some(&session),
                        CALL,
                    )
                    .await;
            });
        }
    }
}

/// Seed the page session's frames, then enable the events that keep the
/// graph current. Frames are seeded before `Runtime.enable` so that the
/// context events it replays find their frames.
async fn build(connection: &CdpConnection, shared: &Shared) -> Result<(), CdpError> {
    let session = &shared.page_session;
    connection
        .call("Page.enable", json!({}), Some(session), CALL)
        .await?;
    let tree = connection
        .call("Page.getFrameTree", json!({}), Some(session), CALL)
        .await?;
    shared.update(|graph| graph.seed(session, &tree));
    connection
        .call("Runtime.enable", json!({}), Some(session), CALL)
        .await?;
    connection
        .call(
            "Target.setAutoAttach",
            auto_attach(true),
            Some(session),
            CALL,
        )
        .await?;
    Ok(())
}

/// Rebuild after missed events: detach every child session, forget the
/// graph, re-seed, replay contexts and re-attach existing OOPIFs.
async fn resync(connection: &CdpConnection, shared: &Shared) -> Result<(), CdpError> {
    use std::sync::atomic::Ordering;
    if shared.resyncing.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let session = &shared.page_session;
    let result = async {
        connection
            .call(
                "Target.setAutoAttach",
                auto_attach(false),
                Some(session),
                CALL,
            )
            .await?;
        shared.update(FrameGraph::reset);
        // Re-enabling replays the page session's existing contexts.
        connection
            .call("Runtime.disable", json!({}), Some(session), CALL)
            .await?;
        build(connection, shared).await
    }
    .await;
    if result.is_ok() {
        shared.update(FrameGraph::finish_resync);
    }
    shared.resyncing.store(false, Ordering::Release);
    result
}

/// Apply events in order; configure or reject new child targets; rebuild
/// after a lag.
async fn listen(mut events: EventStream, shared: Arc<Shared>, connection: Weak<CdpConnection>) {
    loop {
        match events.recv().await {
            Ok(event) => {
                let Some(action) = shared.update(|graph| graph.apply(&event)) else {
                    continue;
                };
                let Some(connection) = connection.upgrade() else {
                    return;
                };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move { act(&connection, &shared, action).await });
            }
            Err(CdpError::EventLag(_)) => {
                shared.update(FrameGraph::reset);
                let Some(connection) = connection.upgrade() else {
                    return;
                };
                let shared = Arc::clone(&shared);
                tokio::spawn(async move {
                    let _ = resync(&connection, &shared).await;
                });
            }
            Err(_) => {
                // The connection ended: nothing about the frames is known.
                shared.update(FrameGraph::reset);
                return;
            }
        }
    }
}

/// Configure an attached iframe (always resuming it), or reject anything
/// else.
async fn act(connection: &CdpConnection, shared: &Shared, action: Action) {
    match action {
        Action::Configure { session, waiting } => {
            let configured = async {
                connection
                    .call("Page.enable", json!({}), Some(&session), CALL)
                    .await?;
                let tree = connection
                    .call("Page.getFrameTree", json!({}), Some(&session), CALL)
                    .await?;
                shared.update(|graph| graph.seed(&session, &tree));
                connection
                    .call("Runtime.enable", json!({}), Some(&session), CALL)
                    .await?;
                connection
                    .call(
                        "Target.setAutoAttach",
                        auto_attach(true),
                        Some(&session),
                        CALL,
                    )
                    .await
            }
            .await;
            if waiting {
                let _ = connection
                    .call(
                        "Runtime.runIfWaitingForDebugger",
                        json!({}),
                        Some(&session),
                        CALL,
                    )
                    .await;
            }
            let _ = configured;
        }
        Action::Reject {
            parent,
            session,
            waiting,
        } => {
            if waiting {
                let _ = connection
                    .call(
                        "Runtime.runIfWaitingForDebugger",
                        json!({}),
                        Some(&session),
                        CALL,
                    )
                    .await;
            }
            let _ = connection
                .call(
                    "Target.detachFromTarget",
                    json!({ "sessionId": session.as_str() }),
                    Some(&parent),
                    CALL,
                )
                .await;
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> CdpSessionId {
        CdpSessionId::from(id)
    }

    fn event(on: Option<&str>, method: &str, params: Value) -> CdpEvent {
        CdpEvent {
            method: method.into(),
            params,
            session_id: on.map(CdpSessionId::from),
        }
    }

    fn navigated(on: &str, frame: &str, parent: Option<&str>, loader: &str) -> CdpEvent {
        let mut info = json!({ "id": frame, "loaderId": loader, "url": "http://x/" });
        if let Some(parent) = parent {
            info["parentId"] = json!(parent);
        }
        event(
            Some(on),
            "Page.frameNavigated",
            json!({ "frame": info, "type": "Navigation" }),
        )
    }

    fn attached_frame(on: &str, frame: &str, parent: &str) -> CdpEvent {
        event(
            Some(on),
            "Page.frameAttached",
            json!({ "frameId": frame, "parentFrameId": parent }),
        )
    }

    fn detached_frame(on: &str, frame: &str, reason: &str) -> CdpEvent {
        event(
            Some(on),
            "Page.frameDetached",
            json!({ "frameId": frame, "reason": reason }),
        )
    }

    fn context(on: &str, id: i64, frame: &str, default: bool) -> CdpEvent {
        event(
            Some(on),
            "Runtime.executionContextCreated",
            json!({ "context": { "id": id, "origin": "http://x", "name": "",
                "auxData": { "frameId": frame, "isDefault": default, "type": if default { "default" } else { "isolated" } } } }),
        )
    }

    fn attached_target(on: &str, child: &str, kind: &str, target: &str, waiting: bool) -> CdpEvent {
        event(
            Some(on),
            "Target.attachedToTarget",
            json!({ "sessionId": child, "waitingForDebugger": waiting,
                "targetInfo": { "targetId": target, "type": kind, "url": "", "title": "", "attached": true } }),
        )
    }

    fn detached_target(on: &str, child: &str) -> CdpEvent {
        event(
            Some(on),
            "Target.detachedFromTarget",
            json!({ "sessionId": child, "targetId": "T" }),
        )
    }

    /// `Page.getFrameTree` reply: (id, loader, children).
    fn tree(id: &str, parent: Option<&str>, loader: &str, children: Vec<Value>) -> Value {
        let mut frame = json!({ "id": id, "loaderId": loader, "url": "http://x/" });
        if let Some(parent) = parent {
            frame["parentId"] = json!(parent);
        }
        json!({ "frame": frame, "childFrames": children })
    }

    fn reply(frame_tree: Value) -> Value {
        json!({ "frameTree": frame_tree })
    }

    /// Main frame `M` (loader `L1`) with same-process child `S` (loader `L2`).
    fn seeded() -> FrameGraph {
        let mut graph = FrameGraph::new(session("P"));
        graph.seed(
            &session("P"),
            &reply(tree(
                "M",
                None,
                "L1",
                vec![tree("S", Some("M"), "L2", vec![])],
            )),
        );
        graph
    }

    fn frame<'a>(snapshot: &'a FrameSnapshot, id: &str) -> Option<&'a FrameInfo> {
        snapshot
            .frames
            .iter()
            .find(|info| info.frame.as_str() == id)
    }

    fn ids(snapshot: &FrameSnapshot) -> Vec<&str> {
        snapshot
            .frames
            .iter()
            .map(|info| info.frame.as_str())
            .collect()
    }

    fn ctx(on: &str, id: i64) -> Option<ExecutionContextRef> {
        Some(ExecutionContextRef {
            session: session(on),
            id,
        })
    }

    #[test]
    fn frames_follow_lifecycle_events_across_sessions() {
        let mut graph = seeded();
        assert_eq!(graph.apply(&context("P", 1, "M", true)), None);
        graph.apply(&context("P", 2, "S", true));
        // An isolated world is not the frame's context.
        graph.apply(&context("P", 3, "M", false));
        assert_eq!(
            graph.apply(&attached_target("P", "C", "iframe", "X", true)),
            Some(Action::Configure {
                session: session("C"),
                waiting: true
            })
        );
        assert!(graph.owns_session(&session("C")));
        graph.apply(&navigated("C", "X", Some("M"), "L3"));
        // The same numeric context ID in another session is another context.
        graph.apply(&context("C", 1, "X", true));
        // A context reported by a session that does not own the frame is not
        // the frame's context.
        graph.apply(&context("P", 9, "X", true));
        // Events from sessions that are not ours, and browser-level events,
        // change nothing.
        graph.apply(&attached_frame("Z", "Q", "M"));
        graph.apply(&event(
            None,
            "Page.frameAttached",
            json!({ "frameId": "R", "parentFrameId": "M" }),
        ));

        let snapshot = graph.snapshot().expect("known");
        assert_eq!(ids(&snapshot), ["M", "S", "X"]);
        let main = frame(&snapshot, "M").unwrap();
        assert_eq!(main.parent, None);
        assert_eq!(main.document, Some(DocumentEpoch::from("L1")));
        assert_eq!((main.session.as_str(), main.local_root), ("P", true));
        assert_eq!(main.context, ctx("P", 1));
        let same = frame(&snapshot, "S").unwrap();
        assert_eq!(same.parent, Some(FrameId::from("M")));
        assert_eq!(same.document, Some(DocumentEpoch::from("L2")));
        assert_eq!((same.session.as_str(), same.local_root), ("P", false));
        assert_eq!(same.context, ctx("P", 2));
        let oopif = frame(&snapshot, "X").unwrap();
        assert_eq!(oopif.parent, Some(FrameId::from("M")));
        assert_eq!(oopif.document, Some(DocumentEpoch::from("L3")));
        assert_eq!((oopif.session.as_str(), oopif.local_root), ("C", true));
        assert_eq!(oopif.context, ctx("C", 1));

        // Destroying context 1 of the child leaves the page's context 1.
        graph.apply(&event(
            Some("C"),
            "Runtime.executionContextDestroyed",
            json!({ "executionContextId": 1 }),
        ));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(frame(&snapshot, "X").unwrap().context, None);
        assert_eq!(frame(&snapshot, "M").unwrap().context, ctx("P", 1));
        // Clearing the page session's contexts leaves the child's alone.
        graph.apply(&context("C", 4, "X", true));
        graph.apply(&event(
            Some("P"),
            "Runtime.executionContextsCleared",
            json!({}),
        ));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(frame(&snapshot, "M").unwrap().context, None);
        assert_eq!(frame(&snapshot, "S").unwrap().context, None);
        assert_eq!(frame(&snapshot, "X").unwrap().context, ctx("C", 4));
    }

    #[test]
    fn navigation_replaces_the_document_and_drops_descendants() {
        let mut graph = seeded();
        graph.apply(&attached_frame("P", "G", "S"));
        graph.apply(&navigated("P", "G", Some("S"), "L4"));
        graph.apply(&context("P", 2, "S", true));
        let before = graph.snapshot().unwrap();
        assert_eq!(ids(&before), ["M", "S", "G"]);

        graph.apply(&navigated("P", "S", Some("M"), "L5"));
        let after = graph.snapshot().unwrap();
        assert_eq!(ids(&after), ["M", "S"]);
        let same = frame(&after, "S").unwrap();
        assert_eq!(same.document, Some(DocumentEpoch::from("L5")));
        assert_eq!(same.context, None, "the old document's context is gone");
        assert!(after.version > before.version);

        // A frame attached but not yet navigated has no document.
        graph.apply(&attached_frame("P", "N", "M"));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(frame(&snapshot, "N").unwrap().document, None);

        // A main-frame navigation drops everything below it.
        graph.apply(&navigated("P", "M", None, "L6"));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M"]);
        assert_eq!(snapshot.frames[0].document, Some(DocumentEpoch::from("L6")));
        // Same-document navigation is not a new document.
        let version = snapshot.version;
        graph.apply(&event(
            Some("P"),
            "Page.navigatedWithinDocument",
            json!({ "frameId": "M", "url": "http://x/#a" }),
        ));
        assert_eq!(graph.snapshot().unwrap().version, version);
    }

    #[test]
    fn detach_removes_subtrees_and_their_sessions() {
        let mut graph = seeded();
        // OOPIF X under M in child session C; nested OOPIF Y under X in D.
        graph.apply(&attached_frame("P", "X", "M"));
        graph.apply(&attached_target("P", "C", "iframe", "X", true));
        graph.apply(&navigated("C", "X", Some("M"), "L3"));
        // The parent's swap notice for a frame now owned by C changes nothing.
        graph.apply(&detached_frame("P", "X", "swap"));
        graph.apply(&attached_frame("C", "Y", "X"));
        graph.apply(&attached_target("C", "D", "iframe", "Y", true));
        graph.apply(&navigated("D", "Y", Some("X"), "L4"));
        graph.apply(&detached_frame("C", "Y", "swap"));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M", "S", "X", "Y"]);
        assert_eq!(frame(&snapshot, "Y").unwrap().session.as_str(), "D");
        assert_eq!(
            frame(&snapshot, "X").unwrap().document,
            Some(DocumentEpoch::from("L3"))
        );

        // Only D's parent session reports D's detach.
        graph.apply(&detached_target("P", "D"));
        assert!(graph.owns_session(&session("D")));
        assert_eq!(ids(&graph.snapshot().unwrap()), ["M", "S", "X", "Y"]);

        // Detaching C ends D too and removes both frames.
        let ends = graph.session_ends();
        graph.apply(&detached_target("P", "C"));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M", "S"]);
        assert!(!graph.owns_session(&session("C")));
        assert!(!graph.owns_session(&session("D")));
        assert!(graph.session_ends() > ends);
        // Late events from an ended session are ignored.
        graph.apply(&navigated("D", "Y", Some("X"), "L9"));
        assert_eq!(ids(&graph.snapshot().unwrap()), ["M", "S"]);

        // Removing a same-process frame removes its subtree.
        graph.apply(&attached_frame("P", "G", "S"));
        graph.apply(&detached_frame("P", "S", "remove"));
        assert_eq!(ids(&graph.snapshot().unwrap()), ["M"]);
        assert_eq!(graph.frames.len(), 1, "no orphaned frames are kept");

        // A swap away from the session that still owns the frame leaves the
        // frame without a document until its new owner reports it.
        graph.apply(&attached_frame("P", "Z", "M"));
        graph.apply(&navigated("P", "Z", Some("M"), "L7"));
        graph.apply(&attached_frame("P", "W", "Z"));
        graph.apply(&detached_frame("P", "Z", "swap"));
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M", "Z"]);
        assert_eq!(frame(&snapshot, "Z").unwrap().document, None);
    }

    #[test]
    fn non_iframe_targets_are_rejected() {
        let mut graph = seeded();
        assert_eq!(
            graph.apply(&attached_target("P", "W", "worker", "T", true)),
            Some(Action::Reject {
                parent: session("P"),
                session: session("W"),
                waiting: true
            })
        );
        assert!(!graph.owns_session(&session("W")));
        // Children of sessions that are not ours are not configured.
        assert_eq!(
            graph.apply(&attached_target("Z", "C", "iframe", "X", true)),
            None
        );
        assert!(!graph.owns_session(&session("C")));
    }

    #[test]
    fn seed_fills_gaps_without_overwriting_events() {
        let mut graph = FrameGraph::new(session("P"));
        graph.apply(&navigated("P", "M", None, "L2"));
        graph.seed(
            &session("P"),
            &reply(tree(
                "M",
                None,
                "L1",
                vec![tree("S", Some("M"), "L3", vec![])],
            )),
        );
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M", "S"]);
        assert_eq!(
            frame(&snapshot, "M").unwrap().document,
            Some(DocumentEpoch::from("L2"))
        );
        assert_eq!(
            frame(&snapshot, "S").unwrap().document,
            Some(DocumentEpoch::from("L3"))
        );
        // A child session's tree is rooted at its OOPIF, under a known parent.
        graph.apply(&attached_target("P", "C", "iframe", "X", false));
        graph.seed(&session("C"), &reply(tree("X", Some("M"), "L4", vec![])));
        let snapshot = graph.snapshot().unwrap();
        let oopif = frame(&snapshot, "X").unwrap();
        assert_eq!((oopif.session.as_str(), oopif.local_root), ("C", true));
        assert_eq!(oopif.document, Some(DocumentEpoch::from("L4")));
        // Trees from sessions that are not ours are ignored.
        graph.seed(&session("Z"), &reply(tree("Q", Some("M"), "L5", vec![])));
        assert_eq!(ids(&graph.snapshot().unwrap()), ["M", "S", "X"]);
    }

    #[test]
    fn reset_makes_the_graph_unknown_until_rebuilt() {
        let mut graph = seeded();
        graph.apply(&attached_target("P", "C", "iframe", "X", false));
        let version = graph.snapshot().unwrap().version;
        let ends = graph.session_ends();
        graph.reset();
        assert_eq!(graph.snapshot(), None);
        assert!(!graph.owns_session(&session("C")));
        assert!(graph.owns_session(&session("P")));
        assert!(
            graph.session_ends() > ends,
            "calls on old child sessions end"
        );
        graph.seed(&session("P"), &reply(tree("M", None, "L1", vec![])));
        assert_eq!(graph.snapshot(), None, "still unknown while rebuilding");
        graph.finish_resync();
        let snapshot = graph.snapshot().unwrap();
        assert_eq!(ids(&snapshot), ["M"]);
        assert!(snapshot.version > version);
        assert_eq!(graph.resyncs(), 1);
    }
}
