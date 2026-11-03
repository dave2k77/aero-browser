//! D02: exactly one mouse move/press/release sequence, never retried.

use aero_cdp::page::Page;
use aero_cdp::runtime::RuntimeError;
use aero_core::types::RootViewportPoint;
use serde_json::json;

/// Why a click did not complete.
#[derive(Debug, PartialEq)]
pub enum DispatchError {
    /// Nothing that could activate the target was sent (the pointer move failed).
    NotStarted(RuntimeError),
    /// The press was sent but the sequence did not complete: the effect is
    /// uncertain and must not be retried automatically.
    Uncertain { delivered: u8, error: RuntimeError },
}

/// Move to `point`, press and release the left button once.
pub async fn click(page: &Page, point: RootViewportPoint) -> Result<(), DispatchError> {
    let (x, y) = point.coordinates();
    let events = [
        json!({ "type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": 0 }),
        json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1 }),
        json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": 1 }),
    ];
    for (delivered, event) in events.into_iter().enumerate() {
        if let Err(error) = page.call("Input.dispatchMouseEvent", event).await {
            return Err(if delivered == 0 {
                DispatchError::NotStarted(error)
            } else {
                DispatchError::Uncertain {
                    delivered: delivered as u8,
                    error,
                }
            });
        }
    }
    Ok(())
}
