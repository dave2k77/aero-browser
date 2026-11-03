//! C06: bounded `Runtime.evaluate` with explicit failure.
//!
//! Results are returned by value and must be JSON (no `NaN`, `Infinity`,
//! `-0` or `BigInt`) of at most [`EvaluateOptions::max_result_bytes`]. A thrown
//! exception or rejected promise is an error, never a `null` result. A context
//! destroyed by navigation is retried only when the caller opts in (the
//! expression must be safe to run again) and only for the specific
//! context-lost protocol errors; every other error propagates.

use std::time::Duration;

use aero_core::ids::CdpSessionId;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::{CdpConnection, CdpError};

/// Longest exception description kept (it is untrusted page text).
pub const MAX_EXCEPTION_CHARS: usize = 1024;

/// Bounds for one evaluation.
#[derive(Clone, Debug)]
pub struct EvaluateOptions {
    /// Largest accepted JSON result (serialized bytes).
    pub max_result_bytes: usize,
    /// Overall deadline, across retries.
    pub deadline: Duration,
    /// Re-runs allowed after the context was destroyed. Zero unless the
    /// expression is idempotent.
    pub context_destroyed_retries: u8,
}

impl Default for EvaluateOptions {
    fn default() -> Self {
        Self {
            max_result_bytes: 1 << 20,
            deadline: Duration::from_secs(10),
            context_destroyed_retries: 0,
        }
    }
}

/// Why an evaluation produced no value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluateError {
    /// The expression threw or its promise rejected (untrusted, truncated).
    Exception(String),
    /// The result is not representable as JSON (e.g. `NaN`, `BigInt`).
    Unserializable(String),
    /// The JSON result exceeds the size limit.
    ResultTooLarge {
        bytes: usize,
    },
    /// The execution context went away (navigation) and no retry remained.
    ContextDestroyed,
    /// The page's session is no longer owned.
    Detached,
    Cdp(CdpError),
}

impl std::fmt::Display for EvaluateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exception(_) => f.write_str("the page script threw"),
            Self::Unserializable(kind) => write!(f, "result is not JSON ({kind})"),
            Self::ResultTooLarge { bytes } => {
                write!(f, "result of {bytes} bytes exceeds the limit")
            }
            Self::ContextDestroyed => f.write_str("the execution context was destroyed"),
            Self::Detached => f.write_str("the page session is detached"),
            Self::Cdp(error) => write!(f, "CDP error: {error}"),
        }
    }
}

impl std::error::Error for EvaluateError {}

/// Interpret a `Runtime.evaluate` reply made with `returnByValue: true`.
pub(crate) fn interpret(reply: &Value, max_result_bytes: usize) -> Result<Value, EvaluateError> {
    let malformed = |why: &str| EvaluateError::Cdp(CdpError::InvalidMessage(why.into()));
    if let Some(details) = reply.get("exceptionDetails") {
        let text = details["exception"]["description"]
            .as_str()
            .or_else(|| details["text"].as_str())
            .unwrap_or("exception without description");
        return Err(EvaluateError::Exception(
            text.chars().take(MAX_EXCEPTION_CHARS).collect(),
        ));
    }
    let result = reply
        .get("result")
        .filter(|r| r.is_object())
        .ok_or_else(|| malformed("evaluate reply without result"))?;
    if let Some(kind) = result.get("unserializableValue") {
        return Err(EvaluateError::Unserializable(
            kind.as_str().unwrap_or("unknown").to_owned(),
        ));
    }
    let value = match (result["type"].as_str(), result.get("value")) {
        (Some("undefined"), _) => Value::Null,
        (Some(_), Some(value)) => value.clone(),
        _ => return Err(malformed("evaluate result without a value")),
    };
    let bytes = serde_json::to_vec(&value)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if bytes > max_result_bytes {
        return Err(EvaluateError::ResultTooLarge { bytes });
    }
    Ok(value)
}

/// The protocol errors that mean "this context is gone or not ready yet".
pub(crate) fn is_context_lost(error: &CdpError) -> bool {
    const LOST: &[&str] = &[
        "Execution context was destroyed.",
        "Cannot find context with specified id",
        "Cannot find default execution context",
        "Inspected target navigated or closed",
    ];
    matches!(error, CdpError::Protocol { message, .. } if LOST.contains(&message.as_str()))
}

/// Evaluate `expression` in the page session (optionally in `context`).
pub(crate) async fn evaluate(
    connection: &CdpConnection,
    session: &CdpSessionId,
    expression: &str,
    context: Option<i64>,
    options: &EvaluateOptions,
) -> Result<Value, EvaluateError> {
    let deadline = Instant::now() + options.deadline;
    let mut retries = options.context_destroyed_retries;
    loop {
        let mut params = json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true,
        });
        if let Some(context) = context {
            params["contextId"] = context.into();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(EvaluateError::Cdp(CdpError::Timeout));
        }
        match connection
            .call("Runtime.evaluate", params, Some(session), remaining)
            .await
        {
            Ok(reply) => return interpret(&reply, options.max_result_bytes),
            Err(error) if is_context_lost(&error) => {
                if retries == 0 {
                    return Err(EvaluateError::ContextDestroyed);
                }
                retries -= 1;
                // Give the replacement document a moment to create its context.
                tokio::time::sleep(Duration::from_millis(50).min(remaining)).await;
            }
            Err(error) => return Err(EvaluateError::Cdp(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: usize = 64;

    #[test]
    fn values_and_undefined_are_returned() {
        let number = json!({ "result": { "type": "number", "value": 2, "description": "2" } });
        assert_eq!(interpret(&number, LIMIT), Ok(json!(2)));
        let object = json!({ "result": { "type": "object", "value": { "a": [1, "x"] } } });
        assert_eq!(interpret(&object, LIMIT), Ok(json!({ "a": [1, "x"] })));
        let undefined = json!({ "result": { "type": "undefined" } });
        assert_eq!(interpret(&undefined, LIMIT), Ok(Value::Null));
        let null = json!({ "result": { "type": "object", "subtype": "null", "value": null } });
        assert_eq!(interpret(&null, LIMIT), Ok(Value::Null));
    }

    #[test]
    fn exceptions_are_errors_not_null() {
        let thrown = json!({
            "result": { "type": "object", "subtype": "error", "description": "Error: boom\n    at <anonymous>:1:7" },
            "exceptionDetails": {
                "text": "Uncaught",
                "exception": { "type": "object", "subtype": "error", "description": "Error: boom\n    at <anonymous>:1:7" }
            }
        });
        match interpret(&thrown, LIMIT) {
            Err(EvaluateError::Exception(text)) => {
                assert!(text.starts_with("Error: boom"), "{text}")
            }
            other => panic!("{other:?}"),
        }
        // A thrown non-Error value has no description; the text is used.
        let primitive = json!({
            "result": { "type": "string", "value": "nope" },
            "exceptionDetails": { "text": "Uncaught nope" }
        });
        assert_eq!(
            interpret(&primitive, LIMIT),
            Err(EvaluateError::Exception("Uncaught nope".into()))
        );
        // Untrusted text is truncated.
        let long = json!({
            "result": { "type": "undefined" },
            "exceptionDetails": { "text": "x".repeat(5000) }
        });
        match interpret(&long, LIMIT) {
            Err(EvaluateError::Exception(text)) => {
                assert_eq!(text.chars().count(), MAX_EXCEPTION_CHARS)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn non_json_large_and_malformed_results_are_errors() {
        let nan = json!({ "result": { "type": "number", "unserializableValue": "NaN" } });
        assert_eq!(
            interpret(&nan, LIMIT),
            Err(EvaluateError::Unserializable("NaN".into()))
        );
        let big = json!({ "result": { "type": "string", "value": "y".repeat(100) } });
        assert_eq!(
            interpret(&big, LIMIT),
            Err(EvaluateError::ResultTooLarge { bytes: 102 })
        );
        for malformed in [
            json!({}),
            json!({ "result": { "type": "object" } }),
            json!([1]),
        ] {
            assert!(
                matches!(
                    interpret(&malformed, LIMIT),
                    Err(EvaluateError::Cdp(CdpError::InvalidMessage(_)))
                ),
                "{malformed}"
            );
        }
    }

    #[test]
    fn only_context_loss_is_retryable() {
        let protocol = |message: &str| CdpError::Protocol {
            code: -32000,
            message: message.into(),
        };
        for lost in [
            "Execution context was destroyed.",
            "Cannot find context with specified id",
            "Cannot find default execution context",
            "Inspected target navigated or closed",
        ] {
            assert!(is_context_lost(&protocol(lost)), "{lost}");
        }
        for other in [
            protocol("Object reference chain is too long"),
            protocol("Execution context was destroyed but this is not the start"),
            CdpError::Timeout,
            CdpError::Disconnected,
            CdpError::InvalidMessage("Execution context was destroyed.".into()),
        ] {
            assert!(!is_context_lost(&other), "{other:?}");
        }
    }
}
