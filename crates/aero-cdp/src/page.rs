//! C05: an attached page. Every call carries the page's own flattened
//! session; there is no way to send a page method to the browser endpoint
//! through this type. Dropping a `Page` does not detach it: the registry keeps
//! owning the session (explicit detach and tab lifecycle are G05).

use std::sync::Arc;
use std::time::Duration;

use aero_core::ids::{CdpSessionId, DocumentEpoch, FrameId, TargetId};
use serde_json::{Value, json};

use tokio::time::Instant;

use crate::evaluate::{EvaluateError, EvaluateOptions};
use crate::runtime::RuntimeError;
use crate::target_registry::{CALL, TargetRegistryHandle};
use crate::{CdpConnection, CdpError, CdpEvent, EventStream};

/// Bounds for [`Page::navigate_and_wait`] (implementation plan: navigation
/// 30 s; stability 5 s cap).
#[derive(Clone, Debug)]
pub struct NavigationLimits {
    /// From the navigation request until the new document's DOMContentLoaded.
    pub navigation: Duration,
    /// Longest stability wait; reaching it is reported, not an error.
    pub stability_cap: Duration,
}

impl Default for NavigationLimits {
    fn default() -> Self {
        Self {
            navigation: Duration::from_secs(30),
            stability_cap: Duration::from_secs(5),
        }
    }
}

/// What a completed navigation produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationOutcome {
    /// A fragment/history change within the current document.
    pub same_document: bool,
    /// The committed URL from the browser's navigation history.
    pub url: String,
    /// HTTP status of the new document's response, when one was observed.
    pub status: Option<u16>,
    /// DOMContentLoaded, 200 ms without DOM mutations and two animation
    /// frames happened before the stability cap.
    pub stable: bool,
}

/// The page's main frame and its current document generation (the frame's
/// `loaderId`, which changes on every cross-document navigation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainFrame {
    pub frame: FrameId,
    pub document: DocumentEpoch,
}

/// One attached tab.
pub struct Page {
    connection: Arc<CdpConnection>,
    registry: TargetRegistryHandle,
    target: TargetId,
    session: CdpSessionId,
}

impl Page {
    pub(crate) fn new(
        connection: Arc<CdpConnection>,
        registry: TargetRegistryHandle,
        target: TargetId,
        session: CdpSessionId,
    ) -> Self {
        Self {
            connection,
            registry,
            target,
            session,
        }
    }

    pub fn target_id(&self) -> &TargetId {
        &self.target
    }

    pub fn session(&self) -> &CdpSessionId {
        &self.session
    }

    /// Send `method` to this page's session. Fails with
    /// [`RuntimeError::Detached`] once the registry no longer owns the session.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        if !self.registry.owns(&self.target, &self.session) {
            return Err(RuntimeError::Detached);
        }
        let reply = self
            .unless_detached(
                self.connection
                    .call(method, params, Some(&self.session), CALL),
            )
            .await
            .ok_or(RuntimeError::Detached)?;
        Ok(reply?)
    }

    /// Run `work` unless the session ends first. A browser does not answer
    /// requests to a closed session, so waiting would last the whole deadline.
    async fn unless_detached<T>(&self, work: impl std::future::Future<Output = T>) -> Option<T> {
        tokio::select! {
            result = work => Some(result),
            () = self.registry.until_detached(&self.target, &self.session) => None,
        }
    }

    /// The main frame and its current document generation.
    pub async fn main_frame(&self) -> Result<MainFrame, RuntimeError> {
        let tree = self.call("Page.getFrameTree", json!({})).await?;
        let frame = &tree["frameTree"]["frame"];
        let text = |key: &str| {
            frame[key]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    RuntimeError::Cdp(CdpError::InvalidMessage(format!(
                        "main frame without {key}"
                    )))
                })
        };
        Ok(MainFrame {
            frame: FrameId::from(text("id")?),
            document: DocumentEpoch::from(text("loaderId")?),
        })
    }

    /// Evaluate `expression` (by value, awaiting promises) with default bounds
    /// and no retry.
    pub async fn evaluate(&self, expression: &str) -> Result<Value, EvaluateError> {
        self.evaluate_with(expression, &EvaluateOptions::default())
            .await
    }

    /// Evaluate with explicit bounds and retry policy.
    pub async fn evaluate_with(
        &self,
        expression: &str,
        options: &EvaluateOptions,
    ) -> Result<Value, EvaluateError> {
        if !self.registry.owns(&self.target, &self.session) {
            return Err(EvaluateError::Detached);
        }
        self.unless_detached(crate::evaluate::evaluate(
            &self.connection,
            &self.session,
            expression,
            None,
            options,
        ))
        .await
        .unwrap_or(Err(EvaluateError::Detached))
    }

    /// Navigate and wait for the new document (or same-document change), its
    /// HTTP status and bounded stability. 4xx/5xx documents, rejected
    /// navigations and missed deadlines are errors.
    pub async fn navigate_and_wait(
        &self,
        url: &str,
        limits: &NavigationLimits,
    ) -> Result<NavigationOutcome, RuntimeError> {
        let deadline = Instant::now() + limits.navigation;
        // Subscribe before enabling the domains and before navigating.
        let mut events = self.connection.subscribe();
        for (method, params) in [
            ("Page.enable", json!({})),
            ("Page.setLifecycleEventsEnabled", json!({ "enabled": true })),
            ("Network.enable", json!({})),
        ] {
            self.call(method, params).await?;
        }
        let reply = match self
            .call_until(deadline, "Page.navigate", json!({ "url": url }))
            .await
        {
            Err(RuntimeError::Cdp(CdpError::Timeout)) => return Err(self.abandon().await),
            other => other?,
        };
        if let Some(text) = reply
            .get("errorText")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            return Err(RuntimeError::NavigationFailed(text.to_owned()));
        }
        let frame = reply["frameId"]
            .as_str()
            .ok_or_else(|| CdpError::InvalidMessage("navigate reply without frameId".into()))?
            .to_owned();
        let mut status = None;
        // CDP omits `loaderId` for a same-document navigation.
        let same_document = match reply.get("loaderId").and_then(Value::as_str) {
            None => true,
            Some(loader) => {
                loop {
                    let event = match next_session_event(&mut events, &self.session, deadline).await
                    {
                        Err(RuntimeError::NavigationTimeout) => return Err(self.abandon().await),
                        other => other?,
                    };
                    let params = &event.params;
                    let ours = params["loaderId"] == loader && params["frameId"] == frame.as_str();
                    match event.method.as_str() {
                        "Network.responseReceived" if ours && params["type"] == "Document" => {
                            status = params["response"]["status"]
                                .as_u64()
                                .and_then(|status| u16::try_from(status).ok());
                        }
                        "Page.lifecycleEvent" if ours && params["name"] == "DOMContentLoaded" => {
                            break;
                        }
                        _ => {}
                    }
                }
                false
            }
        };
        if let Some(status) = status.filter(|status| *status >= 400) {
            return Err(RuntimeError::HttpStatus(status));
        }
        let stable = self.wait_until_stable(&frame, limits.stability_cap).await?;
        Ok(NavigationOutcome {
            same_document,
            url: self.current_url().await?,
            status,
            stable,
        })
    }

    /// `call` with an absolute deadline instead of the default one.
    async fn call_until(
        &self,
        deadline: Instant,
        method: &str,
        params: Value,
    ) -> Result<Value, RuntimeError> {
        if !self.registry.owns(&self.target, &self.session) {
            return Err(RuntimeError::Detached);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(RuntimeError::Cdp(CdpError::Timeout));
        }
        let reply = self
            .unless_detached(
                self.connection
                    .call(method, params, Some(&self.session), remaining),
            )
            .await
            .ok_or(RuntimeError::Detached)?;
        Ok(reply?)
    }

    /// Stop a navigation that missed its deadline (best effort).
    async fn abandon(&self) -> RuntimeError {
        let stop = Instant::now() + Duration::from_secs(2);
        let _ = self.call_until(stop, "Page.stopLoading", json!({})).await;
        RuntimeError::NavigationTimeout
    }

    /// The committed URL according to the browser's navigation history.
    async fn current_url(&self) -> Result<String, RuntimeError> {
        let history = self.call("Page.getNavigationHistory", json!({})).await?;
        let index = history["currentIndex"].as_u64().unwrap_or(u64::MAX);
        usize::try_from(index)
            .ok()
            .and_then(|index| history["entries"].get(index))
            .and_then(|entry| entry["url"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                RuntimeError::Cdp(CdpError::InvalidMessage(
                    "navigation history without current entry".into(),
                ))
            })
    }

    /// Bounded stability in an isolated world (page scripts cannot replace
    /// its timers or observers): DOMContentLoaded, 200 ms without DOM
    /// mutations, then two animation frames, or `false` at the cap. A context
    /// destroyed by a further navigation gets one fresh attempt.
    async fn wait_until_stable(&self, frame: &str, cap: Duration) -> Result<bool, RuntimeError> {
        let cap_ms = cap.as_millis().min(u128::from(u32::MAX));
        let script = STABILITY_SCRIPT.replace("CAP_MS", &cap_ms.to_string());
        let options = EvaluateOptions {
            max_result_bytes: 1024,
            deadline: cap + Duration::from_secs(2),
            context_destroyed_retries: 0,
        };
        for attempt in 0..2 {
            let world = self
                .call(
                    "Page.createIsolatedWorld",
                    json!({ "frameId": frame, "worldName": "aero-stability" }),
                )
                .await?;
            let context = world["executionContextId"]
                .as_i64()
                .ok_or_else(|| CdpError::InvalidMessage("isolated world without context".into()))?;
            let evaluated = self
                .unless_detached(crate::evaluate::evaluate(
                    &self.connection,
                    &self.session,
                    &script,
                    Some(context),
                    &options,
                ))
                .await
                .ok_or(RuntimeError::Detached)?;
            match evaluated {
                Ok(result) => return Ok(result["stable"] == true),
                Err(EvaluateError::ContextDestroyed) if attempt == 0 => {}
                Err(EvaluateError::ContextDestroyed) => return Ok(false),
                Err(error) => return Err(RuntimeError::Evaluate(error)),
            }
        }
        Ok(false)
    }

    /// Start navigating to `url`. A navigation the browser rejects
    /// immediately (`errorText`, e.g. a refused connection) is an error.
    /// Waiting for load and lifecycle is C06.
    pub async fn navigate(&self, url: &str) -> Result<(), RuntimeError> {
        let reply = self.call("Page.navigate", json!({ "url": url })).await?;
        match reply.get("errorText").and_then(Value::as_str) {
            Some(text) if !text.is_empty() => Err(RuntimeError::NavigationFailed(text.to_owned())),
            _ => Ok(()),
        }
    }
}

/// Resolves `{stable}`; `CAP_MS` is replaced with the cap in milliseconds.
const STABILITY_SCRIPT: &str = r#"new Promise(resolve => {
  let finished = false, timer = 0, generation = 0, observer = null;
  const finish = stable => {
    if (finished) return;
    finished = true;
    clearTimeout(timer);
    if (observer) observer.disconnect();
    resolve({ stable });
  };
  setTimeout(() => finish(false), CAP_MS);
  const frames = () => {
    const seen = generation;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (seen === generation) finish(true);
    }));
  };
  const watch = () => {
    observer = new MutationObserver(() => {
      generation++;
      clearTimeout(timer);
      timer = setTimeout(frames, 200);
    });
    observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    timer = setTimeout(frames, 200);
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", watch, { once: true });
  } else {
    watch();
  }
})"#;

/// The next event from `session`, or `NavigationTimeout` at `deadline`.
/// Missed events (lag) and a closed stream are errors.
async fn next_session_event(
    events: &mut EventStream,
    session: &CdpSessionId,
    deadline: Instant,
) -> Result<CdpEvent, RuntimeError> {
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .map_err(|_| RuntimeError::NavigationTimeout)??;
        if event.session_id.as_ref() == Some(session) {
            return Ok(event);
        }
    }
}
