//! D02: ground an observation handle in a fresh, visible, unobstructed point.
//!
//! Order of checks (each failure sends no input): the handle was issued by
//! this observation and runtime; the main frame still shows the observed
//! document; the node still exists and is connected; it is not disabled; it is
//! scrolled into view and measured afterwards; its box is axis-aligned and the
//! viewport unscaled; a visible interior point exists; and a hit test at that
//! point, run in an isolated world the page cannot tamper with, lands on the
//! node or its descendant.
//!
//! D05: a target inside a frame is grounded through every frame on its path
//! to the main frame: the target frame still shows the observed document;
//! each level is scrolled into view, then measured; every owner `<iframe>` is
//! axis-aligned; the point is clipped to every frame viewport; and the hit
//! test runs at every level in that frame's isolated world (each parent must
//! hit exactly the owner of the frame below). The resulting root-viewport
//! point is pressed on the page session, so the browser's own hit testing
//! routes it (`aero_cdp::geometry` holds the arithmetic).

use aero_cdp::CdpError;
use aero_cdp::frame_manager::{FrameInfo, FrameSnapshot};
use aero_cdp::geometry::{self, FrameGeometryError, FrameLevel, FrameLink, FramePoint};
use aero_cdp::page::{MainFrame, Page};
use aero_cdp::runtime::RuntimeError;
use aero_core::ids::{DocumentEpoch, FrameId, ObservationId, RuntimeEpoch};
use aero_core::types::{CssRect, RootViewportPoint};
use aero_perception::observation::{HandleError, HandleMap, HandleTarget};
use serde_json::{Value, json};

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
    /// The main frame navigated since the observation.
    StaleDocument,
    /// The handle belongs to a child frame; acting inside frames is D05.
    UnsupportedFrame,
    /// The node no longer exists or is no longer in the document.
    NodeGone,
    Disabled,
    /// The node has no layout box (hidden, `display: none`, zero-sized).
    NotRendered,
    /// Rotated, skewed or perspective box: no trustworthy input point (SPEC-19).
    UnsupportedTransform,
    /// An `<iframe>` on the path to the target is rotated, skewed or in
    /// perspective: its content cannot be mapped to root coordinates.
    UnsupportedFrameTransform,
    /// The page's frames changed while grounding (a frame attached,
    /// navigated or detached).
    FramesChanged,
    /// No visible interior point inside the viewport after scrolling.
    OffViewport,
    /// A scaled or pinch-zoomed visual viewport.
    ViewportScaled,
    /// Another element receives input at the chosen point.
    Occluded,
    Runtime(RuntimeError),
}

/// Ground `handle` for a click, in the main frame or any frame below it
/// (D05).
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
    ground_target(page, target).await.map(|(target, _)| target)
}

/// [`ground`], also returning the node's object in the isolated world the
/// checks ran in (for follow-up reads and writes that page script cannot
/// intercept). Main frame only: input other than clicks inside frames is D06.
pub(crate) async fn ground_node(
    page: &Page,
    handles: &HandleMap,
    observation: &ObservationId,
    runtime: &RuntimeEpoch,
    handle: &str,
) -> Result<(GroundedTarget, String), GroundingError> {
    let target = handles
        .resolve(observation, runtime, handle)
        .map_err(GroundingError::Handle)?;
    let main = page.main_frame().await.map_err(GroundingError::Runtime)?;
    if main.frame != target.frame {
        return Err(GroundingError::UnsupportedFrame);
    }
    ground_target(page, target).await
}

/// One frame on the path from the target's frame up to the main frame.
struct Level {
    info: FrameInfo,
    /// This frame's isolated-world execution context.
    world: i64,
}

async fn ground_target(
    page: &Page,
    target: &HandleTarget,
) -> Result<(GroundedTarget, String), GroundingError> {
    let main = page.main_frame().await.map_err(GroundingError::Runtime)?;
    if main.frame == target.frame && main.document != target.document {
        return Err(GroundingError::StaleDocument);
    }
    let snapshot = page.frames().map_err(|_| GroundingError::FramesChanged)?;
    let path = frame_path(&snapshot, &target.frame, &main)?;
    if target.frame != main.frame
        && current_document(page, &path[0]).await?.as_ref() != Some(&target.document)
    {
        return Err(GroundingError::StaleDocument);
    }
    // A fresh isolated world per frame: page script cannot replace its
    // primitives.
    let mut levels = Vec::with_capacity(path.len());
    for info in path {
        let world = frame_call(
            page,
            &info.frame,
            "Page.createIsolatedWorld",
            json!({ "frameId": info.frame.as_str(), "worldName": "aero-grounding" }),
        )
        .await?;
        let world = world["executionContextId"]
            .as_i64()
            .ok_or_else(|| malformed("isolated world without a context"))?;
        levels.push(Level { info, world });
    }
    let frame = &levels[0].info.frame;
    let backend = target.backend_node_id;
    let object = match page
        .frame_call(
            frame,
            "DOM.resolveNode",
            json!({ "backendNodeId": backend, "executionContextId": levels[0].world }),
        )
        .await
    {
        Ok(node) => node["object"]["objectId"]
            .as_str()
            .ok_or_else(|| malformed("resolved node without an object"))?
            .to_owned(),
        // "Node with given id does not belong to the document", "No node with given id found".
        Err(RuntimeError::Cdp(CdpError::Protocol { .. })) => return Err(GroundingError::NodeGone),
        Err(error) => return Err(frame_error(error)),
    };
    if call_on_in(
        page,
        frame,
        &object,
        "function () { return this.isConnected === true; }",
        json!([]),
    )
    .await?
        != Value::Bool(true)
    {
        return Err(GroundingError::NodeGone);
    }
    if is_disabled(page, frame, backend).await? {
        return Err(GroundingError::Disabled);
    }
    // Scroll the node into view in its frame, then each frame's owner into
    // view in its parent.
    rendered(
        page.frame_call(
            frame,
            "DOM.scrollIntoViewIfNeeded",
            json!({ "backendNodeId": backend }),
        )
        .await,
    )?;
    let mut owners = Vec::with_capacity(levels.len() - 1);
    for pair in levels.windows(2) {
        let owner = page
            .frame_owner(&pair[0].info.frame)
            .await
            .map_err(frame_error)?;
        rendered(
            page.frame_call(
                &pair[1].info.frame,
                "DOM.scrollIntoViewIfNeeded",
                json!({ "backendNodeId": owner }),
            )
            .await,
        )?;
        owners.push(owner);
    }
    // The browser routes input by the compositor's hit-test data, not the
    // DOM: after a scroll, a press can race it and land where the old layout
    // was (D05: a nested out-of-process frame's click went to its parent).
    // Let every compositor on the path present two frames first.
    for level in levels.iter().filter(|level| level.info.local_root) {
        let reply = frame_call(
            page,
            &level.info.frame,
            "Runtime.evaluate",
            json!({
                "expression": PRESENTED,
                "contextId": level.world,
                "awaitPromise": true,
                "returnByValue": true,
            }),
        )
        .await?;
        call_value(reply)?;
    }
    // Measured after scrolling: earlier geometry is stale by definition.
    let quads = rendered(
        page.frame_call(
            frame,
            "DOM.getContentQuads",
            json!({ "backendNodeId": backend }),
        )
        .await,
    )?;
    let root = root_viewport(page).await?;
    let mut path = Vec::with_capacity(levels.len());
    for (index, level) in levels.iter().enumerate().take(owners.len()) {
        let parent = &levels[index + 1].info.frame;
        let model = rendered(
            page.frame_call(
                parent,
                "DOM.getBoxModel",
                json!({ "backendNodeId": owners[index] }),
            )
            .await,
        )?;
        let owner_content = geometry::axis_aligned(&quad(&model["model"]["content"])?)
            .ok_or(GroundingError::UnsupportedFrameTransform)?;
        let inner = frame_viewport(page, level).await?;
        path.push(FrameLevel {
            viewport: inner.visible,
            link: Some(FrameLink {
                owner_content,
                inner: inner.layout,
                local_root: level.info.local_root,
            }),
        });
    }
    path.push(FrameLevel {
        viewport: root,
        link: None,
    });
    let point = target_point(&quads["quads"], &path)?;
    let root_point =
        RootViewportPoint::new(point.root.0, point.root.1).ok_or(GroundingError::OffViewport)?;
    // Hit test at every level, each in its own frame's coordinates: the
    // target frame must hit the node, every frame above it exactly the owner
    // of the frame below. A covering element anywhere on the path occludes.
    let (x, y) = point.local[0];
    if call_on_in(
        page,
        frame,
        &object,
        HIT_TEST,
        json!([{ "value": x }, { "value": y }]),
    )
    .await?
        != Value::Bool(true)
    {
        return Err(GroundingError::Occluded);
    }
    for (index, owner) in owners.iter().enumerate() {
        let level = &levels[index + 1];
        let resolved = frame_call(
            page,
            &level.info.frame,
            "DOM.resolveNode",
            json!({ "backendNodeId": owner, "executionContextId": level.world }),
        )
        .await?;
        let owner_object = resolved["object"]["objectId"]
            .as_str()
            .ok_or_else(|| malformed("resolved owner without an object"))?;
        let (x, y) = point.local[index + 1];
        if call_on_in(
            page,
            &level.info.frame,
            owner_object,
            OWNER_HIT_TEST,
            json!([{ "value": x }, { "value": y }]),
        )
        .await?
            != Value::Bool(true)
        {
            return Err(GroundingError::Occluded);
        }
    }
    // Nothing on the path attached, navigated or detached meanwhile.
    if page.frames().map(|now| now.version).ok() != Some(snapshot.version) {
        return Err(GroundingError::FramesChanged);
    }
    Ok((
        GroundedTarget {
            backend_node_id: backend,
            point: root_point,
        },
        object,
    ))
}

/// The frames from `frame` up to the main frame, target first.
fn frame_path(
    snapshot: &FrameSnapshot,
    frame: &FrameId,
    main: &MainFrame,
) -> Result<Vec<FrameInfo>, GroundingError> {
    let find = |id: &FrameId| snapshot.frames.iter().find(|info| &info.frame == id);
    // The target's frame is gone: its document is no longer shown.
    let mut current = find(frame).ok_or(GroundingError::StaleDocument)?;
    let mut path = vec![current.clone()];
    while let Some(parent) = &current.parent {
        current = find(parent).ok_or(GroundingError::FramesChanged)?;
        path.push(current.clone());
        if path.len() > snapshot.frames.len() {
            return Err(malformed("frame graph has a cycle"));
        }
    }
    if current.frame != main.frame {
        return Err(GroundingError::FramesChanged);
    }
    Ok(path)
}

/// The document `info`'s frame currently shows, read from its own session's
/// frame tree rather than from events that may lag.
async fn current_document(
    page: &Page,
    info: &FrameInfo,
) -> Result<Option<DocumentEpoch>, GroundingError> {
    let tree = frame_call(page, &info.frame, "Page.getFrameTree", json!({})).await?;
    let mut pending = vec![&tree["frameTree"]];
    while let Some(node) = pending.pop() {
        if node["frame"]["id"].as_str() == Some(info.frame.as_str()) {
            return Ok(node["frame"]["loaderId"].as_str().map(DocumentEpoch::from));
        }
        pending.extend(node["childFrames"].as_array().into_iter().flatten());
    }
    Ok(None)
}

/// The root (main-frame) visual viewport's size, refusing a scaled one.
async fn root_viewport(page: &Page) -> Result<(f64, f64), GroundingError> {
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
    Ok((side("clientWidth")?, side("clientHeight")?))
}

/// A child frame's viewport, read in its isolated world.
struct FrameViewport {
    /// `innerWidth`/`innerHeight`: what the owner's content box displays.
    layout: (f64, f64),
    /// The visible part, without scrollbars.
    visible: (f64, f64),
}

async fn frame_viewport(page: &Page, level: &Level) -> Result<FrameViewport, GroundingError> {
    let reply = frame_call(
        page,
        &level.info.frame,
        "Runtime.evaluate",
        json!({
            "expression": "[innerWidth, innerHeight, visualViewport.width, visualViewport.height, visualViewport.scale]",
            "contextId": level.world,
            "returnByValue": true,
        }),
    )
    .await?;
    let values: Vec<f64> = reply["result"]["value"]
        .as_array()
        .map(|values| values.iter().filter_map(Value::as_f64).collect())
        .filter(|values: &Vec<f64>| values.len() == 5)
        .ok_or_else(|| malformed("frame viewport without five numbers"))?;
    if values[4] != 1.0 {
        return Err(GroundingError::ViewportScaled);
    }
    Ok(FrameViewport {
        layout: (values[0], values[1]),
        visible: (values[2], values[3]),
    })
}

/// A protocol call in the session that owns `frame`. The frame having gone
/// meanwhile is a changed frame graph, not a runtime fault.
async fn frame_call(
    page: &Page,
    frame: &FrameId,
    method: &str,
    params: Value,
) -> Result<Value, GroundingError> {
    page.frame_call(frame, method, params)
        .await
        .map_err(frame_error)
}

fn frame_error(error: RuntimeError) -> GroundingError {
    match error {
        RuntimeError::FrameGone | RuntimeError::Detached | RuntimeError::FramesUnknown => {
            GroundingError::FramesChanged
        }
        error => GroundingError::Runtime(error),
    }
}

/// Resolves once the frame's compositor has produced the frame after the
/// current one.
const PRESENTED: &str = "new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))";

/// Runs in the isolated world on an `<iframe>`: is it exactly what the point
/// hits in its document?
const OWNER_HIT_TEST: &str = "function (x, y) {
  const root = this.getRootNode();
  const scope = root instanceof ShadowRoot ? root : document;
  return scope.elementFromPoint(x, y) === this;
}";

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
        Err(error) => Err(frame_error(error)),
    }
}

/// Call `function` on `object` (in the main frame) by value, awaiting a
/// returned promise; a thrown exception or rejection is an error.
pub(crate) async fn call_on(
    page: &Page,
    object: &str,
    function: &str,
    arguments: Value,
) -> Result<Value, GroundingError> {
    let reply = call(
        page,
        "Runtime.callFunctionOn",
        call_params(object, function, arguments),
    )
    .await?;
    call_value(reply)
}

/// [`call_on`] for an object in `frame`, in the session that owns it.
async fn call_on_in(
    page: &Page,
    frame: &FrameId,
    object: &str,
    function: &str,
    arguments: Value,
) -> Result<Value, GroundingError> {
    let reply = frame_call(
        page,
        frame,
        "Runtime.callFunctionOn",
        call_params(object, function, arguments),
    )
    .await?;
    call_value(reply)
}

fn call_params(object: &str, function: &str, arguments: Value) -> Value {
    json!({
        "objectId": object,
        "functionDeclaration": function,
        "arguments": arguments,
        "returnByValue": true,
        "awaitPromise": true,
    })
}

fn call_value(reply: Value) -> Result<Value, GroundingError> {
    if reply.get("exceptionDetails").is_some() {
        return Err(malformed("grounding script threw"));
    }
    Ok(reply["result"]["value"].clone())
}

/// The accessibility `disabled` state, which covers `disabled`, disabled
/// fieldsets and `aria-disabled`; read in the session that owns `frame`.
async fn is_disabled(page: &Page, frame: &FrameId, backend: i64) -> Result<bool, GroundingError> {
    let tree = frame_call(
        page,
        frame,
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

/// One quad's eight coordinates.
fn quad(value: &Value) -> Result<[f64; 8], GroundingError> {
    let points = value
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
    Ok(corners)
}

/// The rectangles of a `DOM.getContentQuads` result. Every quad must be
/// axis-aligned: one transformed fragment taints the whole target.
fn quad_rects(quads: &Value) -> Result<Vec<CssRect>, GroundingError> {
    quads
        .as_array()
        .ok_or_else(|| malformed("content quads are not an array"))?
        .iter()
        .map(|value| {
            geometry::axis_aligned(&quad(value)?).ok_or(GroundingError::UnsupportedTransform)
        })
        .collect()
}

/// The point to press for a target's content quads seen through `path`.
fn target_point(quads: &Value, path: &[FrameLevel]) -> Result<FramePoint, GroundingError> {
    geometry::frame_point(&quad_rects(quads)?, path).map_err(|error| match error {
        FrameGeometryError::NotRendered => GroundingError::NotRendered,
        FrameGeometryError::OffViewport => GroundingError::OffViewport,
        FrameGeometryError::Invalid => malformed("frame geometry out of range"),
    })
}

pub(crate) fn malformed(why: &str) -> GroundingError {
    GroundingError::Runtime(RuntimeError::Cdp(CdpError::InvalidMessage(why.to_owned())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: (f64, f64) = (1265.0, 800.0);

    /// The main-frame case: the root point for `quads` in `viewport`.
    fn visible_point(quads: &Value, viewport: (f64, f64)) -> Result<(f64, f64), GroundingError> {
        target_point(
            quads,
            &[FrameLevel {
                viewport,
                link: None,
            }],
        )
        .map(|point| point.root)
    }

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
