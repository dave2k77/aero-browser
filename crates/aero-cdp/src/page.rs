//! C05: an attached page. Every call carries the page's own flattened
//! session; there is no way to send a page method to the browser endpoint
//! through this type. Dropping a `Page` does not detach it: the registry keeps
//! owning the session (explicit detach and tab lifecycle are G05).

use std::sync::Arc;

use aero_core::ids::{CdpSessionId, TargetId};
use serde_json::{Value, json};

use crate::CdpConnection;
use crate::runtime::RuntimeError;
use crate::target_registry::{CALL, TargetRegistryHandle};

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
        Ok(self
            .connection
            .call(method, params, Some(&self.session), CALL)
            .await?)
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
