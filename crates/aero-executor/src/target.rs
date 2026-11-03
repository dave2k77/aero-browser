//! D02: ground an observation handle in a fresh, visible, unobstructed point.
//!
//! Order of checks (each failure sends no input): the handle was issued by
//! this observation and runtime; the main frame still shows the observed
//! document; the node still exists and is connected; it is not disabled; it is
//! scrolled into view and measured afterwards; its box is axis-aligned and the
//! viewport unscaled; a visible interior point exists; and a hit test at that
//! point, run in an isolated world the page cannot tamper with, lands on the
//! node or its descendant. Main frame only (frames are D04/D05).

use aero_cdp::CdpError;
use aero_cdp::page::Page;
use aero_cdp::runtime::RuntimeError;
use aero_core::ids::{ObservationId, RuntimeEpoch};
use aero_core::types::{CssRect, RootViewportPoint};
use aero_perception::observation::{HandleError, HandleMap};
use serde_json::{Value, json};

/// How far quad corners may deviate and still count as axis-aligned (CSS px).
const AXIS_TOLERANCE: f64 = 0.01;

/// A grounded target: the node and the root-viewport point to press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundedTarget {
    pub backend_node_id: i64,
    pub point: RootViewportPoint,
}

/// Why a handle could not be grounded. No input was sent.
#[derive(Debug, PartialEq)]
pub enum GroundingError {
    Handle(HandleError),
    /// The main frame navigated since the observation (or the handle belongs
    /// to another frame).
    StaleDocument,
    /// The node no longer exists or is no longer in the document.
    NodeGone,
    Disabled,
    /// The node has no layout box (hidden, `display: none`, zero-sized).
    NotRendered,
    /// Rotated, skewed or perspective box: no trustworthy input point (SPEC-19).
    UnsupportedTransform,
    /// No visible interior point inside the viewport after scrolling.
    OffViewport,
    /// A scaled or pinch-zoomed visual viewport.
    ViewportScaled,
    /// Another element receives input at the chosen point.
    Occluded,
    Runtime(RuntimeError),
}

/// Ground `handle` for a click.
pub async fn ground(
    page: &Page,
    handles: &HandleMap,
    observation: &ObservationId,
    runtime: &RuntimeEpoch,
    handle: &str,
) -> Result<GroundedTarget, GroundingError> {
    let target = handles
        .resolve(observation, runtime, handle)
        .map_err(GroundingError::Handle)?;
    let main = page.main_frame().await.map_err(GroundingError::Runtime)?;
    if main.frame != target.frame || main.document != target.document {
        return Err(GroundingError::StaleDocument);
    }
    let backend = target.backend_node_id;
    // A fresh isolated world: page script cannot replace its primitives.
    let world = call(
        page,
        "Page.createIsolatedWorld",
        json!({ "frameId": main.frame.as_str(), "worldName": "aero-grounding" }),
    )
    .await?;
    let context = world["executionContextId"]
        .as_i64()
        .ok_or_else(|| malformed("isolated world without a context"))?;
    let node = match page
        .call(
            "DOM.resolveNode",
            json!({ "backendNodeId": backend, "executionContextId": context }),
        )
        .await
    {
        Ok(node) => node,
        // "Node with given id does not belong to the document", "No node with given id found".
        Err(RuntimeError::Cdp(CdpError::Protocol { .. })) => return Err(GroundingError::NodeGone),
        Err(error) => return Err(GroundingError::Runtime(error)),
    };
    let object = node["object"]["objectId"]
        .as_str()
        .ok_or_else(|| malformed("resolved node without an object"))?;
    if call_on(
        page,
        object,
        "function () { return this.isConnected === true; }",
        json!([]),
    )
    .await?
        != Value::Bool(true)
    {
        return Err(GroundingError::NodeGone);
    }
    if is_disabled(page, backend).await? {
        return Err(GroundingError::Disabled);
    }
    rendered(
        page.call(
            "DOM.scrollIntoViewIfNeeded",
            json!({ "backendNodeId": backend }),
        )
        .await,
    )?;
    // Measured after scrolling: earlier geometry is stale by definition.
    let quads = rendered(
        page.call("DOM.getContentQuads", json!({ "backendNodeId": backend }))
            .await,
    )?;
    let metrics = call(page, "Page.getLayoutMetrics", json!({})).await?;
    let visual = &metrics["cssVisualViewport"];
    let unscaled = |key: &str| {
        visual
            .get(key)
            .is_none_or(|value| value.as_f64() == Some(1.0))
    };
    if !unscaled("scale") || !unscaled("zoom") {
        return Err(GroundingError::ViewportScaled);
    }
    let side = |key: &str| {
        visual[key]
            .as_f64()
            .ok_or_else(|| malformed("layout metrics without a visual viewport"))
    };
    let (x, y) = visible_point(
        &quads["quads"],
        (side("clientWidth")?, side("clientHeight")?),
    )?;
    let point = RootViewportPoint::new(x, y).ok_or(GroundingError::OffViewport)?;
    if call_on(
        page,
        object,
        HIT_TEST,
        json!([{ "value": x }, { "value": y }]),
    )
    .await?
        != Value::Bool(true)
    {
        return Err(GroundingError::Occluded);
    }
    Ok(GroundedTarget {
        backend_node_id: backend,
        point,
    })
}

/// Runs in the isolated world: does the point hit this node or a descendant?
/// Inside a shadow tree the shadow root's own hit test is used, since the
/// document's returns the host.
const HIT_TEST: &str = "function (x, y) {
  const root = this.getRootNode();
  const scope = root instanceof ShadowRoot ? root : document;
  const hit = scope.elementFromPoint(x, y);
  return hit !== null && (hit === this || this.contains(hit));
}";

async fn call(page: &Page, method: &str, params: Value) -> Result<Value, GroundingError> {
    page.call(method, params)
        .await
        .map_err(GroundingError::Runtime)
}

/// A protocol refusal here means the node has no layout box.
fn rendered(reply: Result<Value, RuntimeError>) -> Result<Value, GroundingError> {
    match reply {
        Ok(value) => Ok(value),
        Err(RuntimeError::Cdp(CdpError::Protocol { .. })) => Err(GroundingError::NotRendered),
        Err(error) => Err(GroundingError::Runtime(error)),
    }
}

/// Call `function` on `object` by value; a thrown exception is an error.
async fn call_on(
    page: &Page,
    object: &str,
    function: &str,
    arguments: Value,
) -> Result<Value, GroundingError> {
    let reply = call(
        page,
        "Runtime.callFunctionOn",
        json!({
            "objectId": object,
            "functionDeclaration": function,
            "arguments": arguments,
            "returnByValue": true,
        }),
    )
    .await?;
    if reply.get("exceptionDetails").is_some() {
        return Err(malformed("grounding script threw"));
    }
    Ok(reply["result"]["value"].clone())
}

/// The accessibility `disabled` state, which covers `disabled`, disabled
/// fieldsets and `aria-disabled`.
async fn is_disabled(page: &Page, backend: i64) -> Result<bool, GroundingError> {
    let tree = call(
        page,
        "Accessibility.getPartialAXTree",
        json!({ "backendNodeId": backend, "fetchRelatives": false }),
    )
    .await?;
    let nodes = tree["nodes"]
        .as_array()
        .ok_or_else(|| malformed("partial accessibility tree without nodes"))?;
    Ok(nodes
        .iter()
        .filter(|node| node["backendDOMNodeId"].as_i64() == Some(backend))
        .flat_map(|node| node["properties"].as_array().into_iter().flatten())
        .any(|property| property["name"] == "disabled" && property["value"]["value"] == true))
}

/// Choose a point from a `DOM.getContentQuads` result: every quad must be
/// axis-aligned; the first quad with a visible interior inside `viewport`
/// (width, height) supplies its clipped centre.
pub(crate) fn visible_point(
    quads: &Value,
    viewport: (f64, f64),
) -> Result<(f64, f64), GroundingError> {
    let quads = quads
        .as_array()
        .ok_or_else(|| malformed("content quads are not an array"))?;
    let mut rects = Vec::with_capacity(quads.len());
    for quad in quads {
        let points = quad
            .as_array()
            .filter(|points| points.len() == 8)
            .ok_or_else(|| malformed("quad without eight coordinates"))?;
        let mut corners = [0.0; 8];
        for (slot, value) in corners.iter_mut().zip(points) {
            *slot = value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| malformed("quad coordinate is not a number"))?;
        }
        rects.push(axis_aligned(&corners).ok_or(GroundingError::UnsupportedTransform)?);
    }
    let boxes: Vec<CssRect> = rects
        .into_iter()
        .filter(|rect| rect.width > 0.0 && rect.height > 0.0)
        .collect();
    if boxes.is_empty() {
        return Err(GroundingError::NotRendered);
    }
    boxes
        .iter()
        .find_map(|rect| rect.clipped_center(viewport))
        .ok_or(GroundingError::OffViewport)
}

/// The rectangle a quad describes, if its four points are the corners of an
/// axis-aligned rectangle (in any order).
fn axis_aligned(corners: &[f64; 8]) -> Option<CssRect> {
    let xs = [corners[0], corners[2], corners[4], corners[6]];
    let ys = [corners[1], corners[3], corners[5], corners[7]];
    let min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let near = |a: f64, b: f64| (a - b).abs() <= AXIS_TOLERANCE;
    let on_corner = (0..4).all(|i| {
        (near(xs[i], min_x) || near(xs[i], max_x)) && (near(ys[i], min_y) || near(ys[i], max_y))
    });
    let has = |x: f64, y: f64| (0..4).any(|i| near(xs[i], x) && near(ys[i], y));
    let complete = has(min_x, min_y) && has(max_x, min_y) && has(max_x, max_y) && has(min_x, max_y);
    (on_corner && complete).then_some(CssRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

fn malformed(why: &str) -> GroundingError {
    GroundingError::Runtime(RuntimeError::Cdp(CdpError::InvalidMessage(why.to_owned())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: (f64, f64) = (1265.0, 800.0);

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Value {
        json!([x, y, x + w, y, x + w, y + h, x, y + h])
    }

    #[test]
    fn grounding_uses_the_centre_of_an_axis_aligned_box() {
        assert_eq!(
            visible_point(&json!([rect(8.0, 588.0, 79.0, 21.0)]), VIEWPORT),
            Ok((47.5, 598.5))
        );
        // Corner order does not matter, only that the corners form a rectangle.
        let reordered = json!([[87.0, 609.0, 8.0, 609.0, 8.0, 588.0, 87.0, 588.0]]);
        assert_eq!(visible_point(&reordered, VIEWPORT), Ok((47.5, 598.5)));
    }

    #[test]
    fn grounding_clips_to_the_viewport_and_skips_hidden_parts() {
        // Partly above the viewport: the centre of the visible part.
        assert_eq!(
            visible_point(&json!([rect(0.0, -10.0, 20.0, 30.0)]), VIEWPORT),
            Ok((10.0, 10.0))
        );
        // A wrapped inline element: the first line is off screen, the second is not.
        let wrapped = json!([rect(10.0, -40.0, 100.0, 20.0), rect(10.0, 5.0, 50.0, 20.0)]);
        assert_eq!(visible_point(&wrapped, VIEWPORT), Ok((35.0, 15.0)));
        assert_eq!(
            visible_point(&json!([rect(10.0, 900.0, 100.0, 20.0)]), VIEWPORT),
            Err(GroundingError::OffViewport)
        );
    }

    #[test]
    fn grounding_rejects_transforms_missing_boxes_and_bad_replies() {
        let rotated = json!([[
            140.43, 573.73, 194.72, 605.08, 184.22, 623.27, 129.93, 591.92
        ]]);
        assert_eq!(
            visible_point(&rotated, VIEWPORT),
            Err(GroundingError::UnsupportedTransform)
        );
        // One transformed fragment taints the whole target.
        let mixed = json!([
            rect(10.0, 10.0, 50.0, 20.0),
            [0.0, 0.0, 10.0, 1.0, 10.0, 11.0, 0.0, 10.0]
        ]);
        assert_eq!(
            visible_point(&mixed, VIEWPORT),
            Err(GroundingError::UnsupportedTransform)
        );
        assert_eq!(
            visible_point(&json!([]), VIEWPORT),
            Err(GroundingError::NotRendered)
        );
        assert_eq!(
            visible_point(&json!([rect(10.0, 10.0, 0.0, 20.0)]), VIEWPORT),
            Err(GroundingError::NotRendered)
        );
        for bad in [
            json!({}),
            json!([[1, 2, 3]]),
            json!([["a", 0, 1, 0, 1, 1, 0, 1]]),
        ] {
            assert!(
                matches!(
                    visible_point(&bad, VIEWPORT),
                    Err(GroundingError::Runtime(_))
                ),
                "{bad}"
            );
        }
    }
}
