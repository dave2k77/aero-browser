//! D01: merge frame trees into one observation, assign element handles once
//! and render bounded, escaped text for the planner.
//!
//! Text format (provisional DEC-013), one line per node, two spaces of
//! indentation per rendered ancestor:
//!
//! - control: `@e<n> <role> "<name>"` then ` value="<value>"` and state flags in
//!   fixed order (`disabled readonly required focused checked|unchecked|mixed
//!   selected expanded|collapsed truncated`);
//! - context: `heading <level> "<name>"`, `cell "<name>"`, `text "<text>"`, …;
//! - structure: `table`, `row`, `list`, `listitem`;
//! - a final `[omitted …]` line whenever anything was left out.
//!
//! Every page-supplied string is quoted and escaped (quotes, backslashes,
//! control, line-separator, bidirectional and zero-width characters), so page
//! text cannot start a line or forge a handle. Names and values are cut to
//! [`MAX_NAME_CHARS`] Unicode scalar values and flagged `truncated`. When the
//! text exceeds [`MAX_OBSERVATION_BYTES`], lines are kept by priority —
//! focused and dialog content, then controls, then context next to a control,
//! then everything else — and rendered in document order. Handles are
//! assigned after that selection, in document order, so every handle in the
//! text is in the map and vice versa.

use std::collections::{HashMap, HashSet};

use aero_cdp::page::Page;
use aero_cdp::runtime::RuntimeError;
use aero_core::ids::{DocumentEpoch, FrameId, ObservationId, RuntimeEpoch};
use aero_core::types::ElementHandle;
use serde_json::json;

use crate::axtree::{AxError, AxNode, AxTree, Checked};

/// Largest rendered observation, UTF-8 bytes (implementation plan §4).
pub const MAX_OBSERVATION_BYTES: usize = 16 * 1024;
/// Longest rendered name or value, Unicode scalar values.
pub const MAX_NAME_CHARS: usize = 256;
/// Most frames merged into one observation.
pub const MAX_FRAMES: usize = 64;
/// Most parsed nodes across all supplied frames.
pub const MAX_OBSERVED_NODES: usize = 200_000;
/// Bytes kept free for the `[omitted …]` line once truncation is needed.
const TRAILER_RESERVE: usize = 96;
/// Deepest indentation rendered (deeper nodes share it).
const MAX_INDENT: usize = 8;

/// Roles that receive a handle when they have a DOM node.
const ACTIONABLE: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "listbox",
    "checkbox",
    "radio",
    "switch",
    "slider",
    "spinbutton",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "option",
    "treeitem",
];
/// Context whose name already contains its text children.
const NAMED_CONTEXT: &[&str] = &["heading", "cell", "gridcell", "columnheader", "rowheader"];
const DIALOGS: &[&str] = &["dialog", "alertdialog", "alert"];
const STRUCTURE: &[&str] = &["table", "grid", "row", "list", "listitem"];

/// One frame's parsed tree and the document generation it was read from.
#[derive(Clone, Debug)]
pub struct FrameTree {
    pub frame: FrameId,
    pub document: DocumentEpoch,
    pub tree: AxTree,
}

/// The child frame shown by an `Iframe` node of a parent frame.
#[derive(Clone, Debug)]
pub struct FrameLink {
    pub parent: FrameId,
    pub iframe_backend_node_id: i64,
    pub child: FrameId,
}

/// Everything one observation is built from.
#[derive(Clone, Debug)]
pub struct ObservationInput {
    pub id: ObservationId,
    pub runtime: RuntimeEpoch,
    pub root: FrameId,
    pub frames: Vec<FrameTree>,
    pub links: Vec<FrameLink>,
}

/// Why an observation could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObserveError {
    UnknownRootFrame,
    DuplicateFrame(FrameId),
    TooManyFrames(usize),
    /// The supplied frames hold more than [`MAX_OBSERVED_NODES`] nodes.
    TooManyNodes(usize),
    /// A link names a frame that was not supplied.
    LinkToUnknownFrame(FrameId),
    /// A frame is linked as a child twice, or the root is linked as a child.
    FrameLinkedTwice(FrameId),
    /// One iframe node is linked to two child frames.
    IframeLinkedTwice(FrameId, i64),
}

impl std::fmt::Display for ObserveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRootFrame => f.write_str("the root frame was not supplied"),
            Self::DuplicateFrame(_) => f.write_str("a frame was supplied twice"),
            Self::TooManyFrames(count) => write!(f, "{count} frames exceed the limit"),
            Self::TooManyNodes(count) => write!(f, "{count} nodes exceed the limit"),
            Self::LinkToUnknownFrame(_) => f.write_str("a frame link names an unknown frame"),
            Self::FrameLinkedTwice(_) => f.write_str("a frame is linked as a child twice"),
            Self::IframeLinkedTwice(..) => f.write_str("an iframe is linked to two frames"),
        }
    }
}

impl std::error::Error for ObserveError {}

/// What a handle denotes, for the executor's freshness checks (D02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleTarget {
    pub frame: FrameId,
    pub document: DocumentEpoch,
    pub backend_node_id: i64,
    pub role: String,
    pub name: String,
}

/// Why a handle did not resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleError {
    /// Not of the form `@e<n>`.
    Malformed,
    /// Issued by, or presented for, another observation.
    OtherObservation,
    /// The browser runtime was replaced since the observation.
    OtherRuntime,
    /// Not issued by this observation.
    Unknown,
}

/// The handles one observation issued, bound to that observation and runtime.
#[derive(Clone, Debug)]
pub struct HandleMap {
    observation: ObservationId,
    runtime: RuntimeEpoch,
    targets: Vec<HandleTarget>,
}

impl HandleMap {
    pub fn observation(&self) -> &ObservationId {
        &self.observation
    }

    pub fn runtime(&self) -> &RuntimeEpoch {
        &self.runtime
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Issued handles in order.
    pub fn iter(&self) -> impl Iterator<Item = (ElementHandle, &HandleTarget)> {
        self.targets
            .iter()
            .enumerate()
            .map(|(index, target)| (ElementHandle::new(index as u32), target))
    }

    /// Resolve a handle the planner presented for `observation` while the
    /// current runtime is `runtime`. Document freshness is checked by D02.
    pub fn resolve(
        &self,
        observation: &ObservationId,
        runtime: &RuntimeEpoch,
        handle: &str,
    ) -> Result<&HandleTarget, HandleError> {
        let handle = ElementHandle::parse(handle).ok_or(HandleError::Malformed)?;
        if observation != &self.observation {
            return Err(HandleError::OtherObservation);
        }
        if runtime != &self.runtime {
            return Err(HandleError::OtherRuntime);
        }
        usize::try_from(handle.index())
            .ok()
            .and_then(|index| self.targets.get(index))
            .ok_or(HandleError::Unknown)
    }
}

/// What was left out of the rendered text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Truncation {
    /// Controls omitted for size (they have no handle).
    pub omitted_controls: usize,
    /// Other lines omitted for size.
    pub omitted_lines: usize,
    /// Names or values cut to [`MAX_NAME_CHARS`].
    pub cut_texts: usize,
}

impl Truncation {
    pub fn is_complete(&self) -> bool {
        self.omitted_controls == 0 && self.omitted_lines == 0 && self.cut_texts == 0
    }
}

/// A bounded semantic observation.
#[derive(Clone, Debug)]
pub struct Observation {
    id: ObservationId,
    frames: Vec<(FrameId, DocumentEpoch)>,
    text: String,
    handles: HandleMap,
    truncation: Truncation,
}

impl Observation {
    pub fn id(&self) -> &ObservationId {
        &self.id
    }

    /// Frames included, in the order they were entered.
    pub fn frames(&self) -> &[(FrameId, DocumentEpoch)] {
        &self.frames
    }

    /// The planner-facing text, at most [`MAX_OBSERVATION_BYTES`].
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn handles(&self) -> &HandleMap {
        &self.handles
    }

    pub fn truncation(&self) -> Truncation {
        self.truncation
    }
}

/// Why a live page could not be observed.
#[derive(Debug, PartialEq)]
pub enum PageObserveError {
    Runtime(RuntimeError),
    Tree(AxError),
    Observe(ObserveError),
    /// The main frame's document changed while the tree was being read.
    DocumentChanged,
}

impl std::fmt::Display for PageObserveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(error) => write!(f, "browser error: {error}"),
            Self::Tree(error) => write!(f, "{error}"),
            Self::Observe(error) => write!(f, "{error}"),
            Self::DocumentChanged => f.write_str("the document changed during observation"),
        }
    }
}

impl std::error::Error for PageObserveError {}

/// Observe the page's main frame (D02; iframe trees are D04). Handles are
/// bound to `id`, `runtime` and the main frame's current document epoch.
pub async fn observe_page(
    page: &Page,
    id: ObservationId,
    runtime: RuntimeEpoch,
) -> Result<Observation, PageObserveError> {
    let before = page.main_frame().await.map_err(PageObserveError::Runtime)?;
    let reply = page
        .call("Accessibility.getFullAXTree", json!({}))
        .await
        .map_err(PageObserveError::Runtime)?;
    let tree = crate::axtree::parse_tree(&reply).map_err(PageObserveError::Tree)?;
    // A navigation between the two reads would bind the new document's nodes
    // to the old epoch (or the reverse).
    let after = page.main_frame().await.map_err(PageObserveError::Runtime)?;
    if after != before {
        return Err(PageObserveError::DocumentChanged);
    }
    observe(ObservationInput {
        id,
        runtime,
        root: before.frame.clone(),
        frames: vec![FrameTree {
            frame: before.frame,
            document: before.document,
            tree,
        }],
        links: Vec::new(),
    })
    .map_err(PageObserveError::Observe)
}

/// Build an observation from frame trees.
pub fn observe(input: ObservationInput) -> Result<Observation, ObserveError> {
    let graph = FrameGraph::new(&input)?;
    let (lines, entered) = collect_lines(&input, &graph);
    let keep = select(&lines);
    let mut text = String::new();
    let mut targets = Vec::new();
    let mut truncation = Truncation::default();
    for (line, kept) in lines.iter().zip(&keep) {
        if !kept {
            match line.target {
                Some(_) => truncation.omitted_controls += 1,
                None => truncation.omitted_lines += 1,
            }
            continue;
        }
        truncation.cut_texts += line.cut;
        text.push_str(&"  ".repeat(line.depth.min(MAX_INDENT)));
        if let Some(target) = &line.target {
            text.push_str(&format!("{} ", ElementHandle::new(targets.len() as u32)));
            targets.push(target.clone());
        }
        text.push_str(&line.body);
        text.push('\n');
    }
    if truncation.omitted_controls + truncation.omitted_lines > 0 {
        text.push_str(&format!(
            "[omitted {} controls and {} other lines]\n",
            truncation.omitted_controls, truncation.omitted_lines
        ));
    }
    debug_assert!(text.len() <= MAX_OBSERVATION_BYTES);
    Ok(Observation {
        frames: entered
            .into_iter()
            .map(|index| {
                let frame = &input.frames[index];
                (frame.frame.clone(), frame.document.clone())
            })
            .collect(),
        handles: HandleMap {
            observation: input.id.clone(),
            runtime: input.runtime.clone(),
            targets,
        },
        id: input.id,
        text,
        truncation,
    })
}

/// Validated frame indices and iframe links.
struct FrameGraph {
    root: usize,
    /// (parent frame index, iframe backend node) → child frame index.
    links: HashMap<(usize, i64), usize>,
}

impl FrameGraph {
    fn new(input: &ObservationInput) -> Result<Self, ObserveError> {
        if input.frames.len() > MAX_FRAMES {
            return Err(ObserveError::TooManyFrames(input.frames.len()));
        }
        let total: usize = input.frames.iter().map(|f| f.tree.nodes().len()).sum();
        if total > MAX_OBSERVED_NODES {
            return Err(ObserveError::TooManyNodes(total));
        }
        let mut index = HashMap::new();
        for (position, frame) in input.frames.iter().enumerate() {
            if index.insert(&frame.frame, position).is_some() {
                return Err(ObserveError::DuplicateFrame(frame.frame.clone()));
            }
        }
        let root = *index
            .get(&input.root)
            .ok_or(ObserveError::UnknownRootFrame)?;
        let find = |frame: &FrameId| {
            index
                .get(frame)
                .copied()
                .ok_or_else(|| ObserveError::LinkToUnknownFrame(frame.clone()))
        };
        let mut links = HashMap::new();
        let mut children = HashSet::new();
        for link in &input.links {
            let parent = find(&link.parent)?;
            let child = find(&link.child)?;
            if child == root || !children.insert(child) {
                return Err(ObserveError::FrameLinkedTwice(link.child.clone()));
            }
            if links
                .insert((parent, link.iframe_backend_node_id), child)
                .is_some()
            {
                return Err(ObserveError::IframeLinkedTwice(
                    link.parent.clone(),
                    link.iframe_backend_node_id,
                ));
            }
        }
        Ok(Self { root, links })
    }
}

/// One candidate line, before selection and handle assignment.
struct Line {
    depth: usize,
    /// Rendered text after the handle (escaped and cut).
    body: String,
    /// Present for controls.
    target: Option<HandleTarget>,
    /// Focused, or a dialog or inside one.
    urgent: bool,
    is_heading: bool,
    /// Names/values cut while rendering.
    cut: usize,
}

/// What the ancestors of a node were.
#[derive(Clone, Copy, Default)]
struct Ancestry {
    control: bool,
    combobox: bool,
    named: bool,
    label: bool,
    dialog: bool,
}

/// Walk the merged tree in document order and classify every node.
fn collect_lines(input: &ObservationInput, graph: &FrameGraph) -> (Vec<Line>, Vec<usize>) {
    let children: Vec<Vec<Vec<usize>>> = input
        .frames
        .iter()
        .map(|frame| {
            let nodes = frame.tree.nodes();
            let mut children = vec![Vec::new(); nodes.len()];
            for (index, node) in nodes.iter().enumerate() {
                if let Some(parent) = node.parent {
                    children[parent].push(index);
                }
            }
            children
        })
        .collect();
    let mut lines = Vec::new();
    let mut entered = vec![graph.root];
    let mut stack: Vec<(usize, usize, usize, Ancestry)> = Vec::new();
    let push_children =
        |stack: &mut Vec<_>, frame: usize, node: usize, depth: usize, ancestry: Ancestry| {
            for &child in children[frame][node].iter().rev() {
                stack.push((frame, child, depth, ancestry));
            }
        };
    // The frame's RootWebArea is not rendered.
    push_children(&mut stack, graph.root, 0, 0, Ancestry::default());
    while let Some((frame_index, node_index, depth, ancestry)) = stack.pop() {
        let frame = &input.frames[frame_index];
        let node = &frame.tree.nodes()[node_index];
        let line = classify(frame, node, depth, ancestry);
        let child_depth = depth + usize::from(line.is_some());
        lines.extend(line);
        let role = node.role.as_str();
        let below = Ancestry {
            control: ancestry.control || ACTIONABLE.contains(&role),
            combobox: ancestry.combobox || role == "combobox",
            named: ancestry.named || (NAMED_CONTEXT.contains(&role) && !node.name.is_empty()),
            label: ancestry.label || role == "LabelText",
            dialog: ancestry.dialog || DIALOGS.contains(&role),
        };
        push_children(&mut stack, frame_index, node_index, child_depth, below);
        if role == "Iframe"
            && let Some(backend) = node.backend_node_id
            && let Some(&child) = graph.links.get(&(frame_index, backend))
        {
            entered.push(child);
            push_children(&mut stack, child, 0, child_depth, below);
        }
    }
    (lines, entered)
}

fn classify(frame: &FrameTree, node: &AxNode, depth: usize, ancestry: Ancestry) -> Option<Line> {
    let role = node.role.as_str();
    let mut body = String::new();
    let mut cut = 0;
    let mut target = None;
    let mut named = |body: &mut String, label: &str| {
        body.push_str(label);
        body.push(' ');
        cut += usize::from(quote(&node.name, body));
    };
    if ACTIONABLE.contains(&role)
        && !(role == "option" && ancestry.combobox)
        && let Some(backend_node_id) = node.backend_node_id
    {
        named(&mut body, role);
        target = Some(HandleTarget {
            frame: frame.frame.clone(),
            document: frame.document.clone(),
            backend_node_id,
            role: node.role.clone(),
            name: node.name.clone(),
        });
    } else if role == "StaticText" {
        if ancestry.control || ancestry.named || ancestry.label || node.name.trim().is_empty() {
            return None;
        }
        named(&mut body, "text");
    } else if role == "heading" {
        match node.states.level {
            Some(level) => named(&mut body, &format!("heading {level}")),
            None => named(&mut body, "heading"),
        }
    } else if NAMED_CONTEXT.contains(&role) || ACTIONABLE.contains(&role) || DIALOGS.contains(&role)
    {
        // Includes options of a native select and controls without a DOM node.
        named(&mut body, role);
    } else if role == "Iframe" {
        named(&mut body, "iframe");
    } else if matches!(role, "img" | "image") && !node.name.is_empty() {
        named(&mut body, "img");
    } else if STRUCTURE.contains(&role) {
        body.push_str(role);
    } else {
        return None;
    }
    if ACTIONABLE.contains(&role) {
        if let Some(value) = node.value.as_deref().filter(|value| !value.is_empty()) {
            body.push_str(" value=");
            cut += usize::from(quote(value, &mut body));
        }
        push_flags(&mut body, node);
    }
    if cut > 0 {
        body.push_str(" truncated");
    }
    Some(Line {
        depth,
        body,
        target,
        urgent: node.states.focused || ancestry.dialog || DIALOGS.contains(&role),
        is_heading: role == "heading",
        cut,
    })
}

fn push_flags(body: &mut String, node: &AxNode) {
    let states = &node.states;
    for (on, flag) in [
        (states.disabled, "disabled"),
        (states.readonly, "readonly"),
        (states.required, "required"),
        (states.focused, "focused"),
    ] {
        if on {
            body.push(' ');
            body.push_str(flag);
        }
    }
    if let Some(checked) = states.checked {
        body.push_str(match checked {
            Checked::True => " checked",
            Checked::False => " unchecked",
            Checked::Mixed => " mixed",
        });
    }
    if states.selected == Some(true) {
        body.push_str(" selected");
    }
    match states.expanded {
        Some(true) => body.push_str(" expanded"),
        Some(false) => body.push_str(" collapsed"),
        None => {}
    }
}

/// Quote and escape `text`, cut to [`MAX_NAME_CHARS`]; returns whether it was cut.
fn quote(text: &str, out: &mut String) -> bool {
    out.push('"');
    let mut cut = false;
    for (count, c) in text.chars().enumerate() {
        if count == MAX_NAME_CHARS {
            cut = true;
            break;
        }
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if needs_escape(c) => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    cut
}

/// Characters that could break or disguise line structure.
fn needs_escape(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}'
        )
}

/// Which lines to keep: all if they fit, otherwise by priority within
/// `MAX_OBSERVATION_BYTES - TRAILER_RESERVE`. Costs assume the widest handle.
fn select(lines: &[Line]) -> Vec<bool> {
    let controls = lines.iter().filter(|line| line.target.is_some()).count();
    let handle_width = format!("{} ", ElementHandle::new(controls.saturating_sub(1) as u32)).len();
    let cost = |line: &Line| {
        2 * line.depth.min(MAX_INDENT)
            + if line.target.is_some() {
                handle_width
            } else {
                0
            }
            + line.body.len()
            + 1
    };
    if lines.iter().map(cost).sum::<usize>() <= MAX_OBSERVATION_BYTES {
        return vec![true; lines.len()];
    }
    // Context next to a control: its neighbours and the nearest heading above.
    let mut nearby = vec![false; lines.len()];
    let mut heading = None;
    for (index, line) in lines.iter().enumerate() {
        if line.is_heading {
            heading = Some(index);
        }
        if line.target.is_some() {
            if let Some(heading) = heading {
                nearby[heading] = true;
            }
            for neighbour in [index.wrapping_sub(1), index + 1] {
                if lines.get(neighbour).is_some_and(|l| l.target.is_none()) {
                    nearby[neighbour] = true;
                }
            }
        }
    }
    let class = |index: usize| {
        let line = &lines[index];
        if line.urgent {
            0
        } else if line.target.is_some() {
            1
        } else if nearby[index] {
            2
        } else {
            3
        }
    };
    let mut keep = vec![false; lines.len()];
    let mut remaining = MAX_OBSERVATION_BYTES - TRAILER_RESERVE;
    for priority in 0..4 {
        for index in 0..lines.len() {
            if class(index) == priority && !keep[index] {
                let cost = cost(&lines[index]);
                if cost <= remaining {
                    remaining -= cost;
                    keep[index] = true;
                }
            }
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axtree::tests::{ignored, node, prop, tree};
    use serde_json::{Value, json};

    fn ids() -> (ObservationId, RuntimeEpoch) {
        (
            ObservationId::from("obs-1"),
            RuntimeEpoch::from("runtime-1"),
        )
    }

    fn single(nodes: Vec<Value>) -> ObservationInput {
        let (id, runtime) = ids();
        ObservationInput {
            id,
            runtime,
            root: FrameId::from("main"),
            frames: vec![FrameTree {
                frame: FrameId::from("main"),
                document: DocumentEpoch::from("doc-1"),
                tree: tree(nodes),
            }],
            links: Vec::new(),
        }
    }

    fn valued(mut node: Value, value: &str) -> Value {
        node["value"] = json!({ "type": "string", "value": value });
        node
    }

    /// Handles at the start of lines, in order.
    fn line_handles(text: &str) -> Vec<String> {
        text.lines()
            .filter_map(|line| {
                line.trim_start()
                    .strip_prefix("@e")
                    .map(|rest| rest.split(' ').next().unwrap())
            })
            .map(|digits| format!("@e{digits}"))
            .collect()
    }

    fn assert_handles_match_map(observation: &Observation) {
        let in_text = line_handles(observation.text());
        let in_map: Vec<String> = observation
            .handles()
            .iter()
            .map(|(h, _)| h.to_string())
            .collect();
        assert_eq!(in_text, in_map);
    }

    fn page() -> Vec<Value> {
        // Shuffled relative to DOM order on purpose.
        vec![
            node("1", None, "RootWebArea", "Shop", &["2"]),
            ignored(node(
                "2",
                Some("1"),
                "none",
                "",
                &["3", "4", "6", "8", "9", "10", "11", "13"],
            )),
            prop(
                node("3", Some("2"), "heading", "Results", &["20"]),
                "level",
                json!(2),
            ),
            node("4", Some("2"), "paragraph", "", &["22", "5", "23"]),
            node("6", Some("2"), "LabelText", "", &["25", "7"]),
            prop(
                node("8", Some("2"), "textbox", "Off", &[]),
                "disabled",
                json!(true),
            ),
            node("9", Some("2"), "button", "Go", &["28"]),
            prop(
                node("10", Some("2"), "checkbox", "Box", &[]),
                "checked",
                json!("mixed"),
            ),
            prop(
                valued(node("11", Some("2"), "combobox", "Pick", &["12"]), "B"),
                "expanded",
                json!(false),
            ),
            node("13", Some("2"), "table", "", &["14"]),
            node("14", Some("13"), "row", "", &["15", "16"]),
            node("15", Some("14"), "columnheader", "Item", &["32"]),
            node("16", Some("14"), "cell", "Tea", &["33"]),
            node("20", Some("3"), "StaticText", "Results", &["-1"]),
            node("-1", Some("20"), "InlineTextBox", "Results", &[]),
            node("22", Some("4"), "StaticText", "Plain ", &[]),
            node("5", Some("4"), "link", "more", &["24"]),
            node("23", Some("4"), "StaticText", " after", &[]),
            node("24", Some("5"), "StaticText", "more", &[]),
            node("25", Some("6"), "StaticText", "Name ", &[]),
            prop(
                valued(node("7", Some("6"), "textbox", "Name", &["26"]), "v"),
                "focused",
                json!(true),
            ),
            node("26", Some("7"), "generic", "", &["27"]),
            node("27", Some("26"), "StaticText", "v", &[]),
            node("28", Some("9"), "StaticText", "Go", &[]),
            node("12", Some("11"), "MenuListPopup", "", &["30", "31"]),
            prop(
                node("30", Some("12"), "option", "A", &[]),
                "selected",
                json!(false),
            ),
            prop(
                node("31", Some("12"), "option", "B", &[]),
                "selected",
                json!(true),
            ),
            node("32", Some("15"), "StaticText", "Item", &[]),
            node("33", Some("16"), "StaticText", "Tea", &[]),
        ]
    }

    #[test]
    fn axtree_controls_get_handles_and_context_is_kept_once() {
        let observation = observe(single(page())).expect("observation");
        assert_eq!(
            observation.text(),
            concat!(
                "heading 2 \"Results\"\n",
                "text \"Plain \"\n",
                "@e0 link \"more\"\n",
                "text \" after\"\n",
                "@e1 textbox \"Name\" value=\"v\" focused\n",
                "@e2 textbox \"Off\" disabled\n",
                "@e3 button \"Go\"\n",
                "@e4 checkbox \"Box\" mixed\n",
                "@e5 combobox \"Pick\" value=\"B\" collapsed\n",
                "  option \"A\"\n",
                "  option \"B\" selected\n",
                "table\n",
                "  row\n",
                "    columnheader \"Item\"\n",
                "    cell \"Tea\"\n",
            )
        );
        assert!(observation.truncation().is_complete());
        assert_eq!(
            observation.frames(),
            [(FrameId::from("main"), DocumentEpoch::from("doc-1"))]
        );
        assert_handles_match_map(&observation);
    }

    #[test]
    fn axtree_handles_match_map_and_reject_other_generations() {
        let observation = observe(single(page())).expect("observation");
        let (id, runtime) = ids();
        let map = observation.handles();
        assert_eq!(map.len(), 6);
        assert_eq!(map.observation(), &id);
        assert_eq!(map.runtime(), &runtime);
        let button = map.resolve(&id, &runtime, "@e3").expect("issued");
        assert_eq!(
            button,
            &HandleTarget {
                frame: FrameId::from("main"),
                document: DocumentEpoch::from("doc-1"),
                backend_node_id: 9,
                role: "button".into(),
                name: "Go".into(),
            }
        );
        assert_eq!(
            map.resolve(&ObservationId::from("obs-0"), &runtime, "@e3"),
            Err(HandleError::OtherObservation)
        );
        assert_eq!(
            map.resolve(&id, &RuntimeEpoch::from("runtime-2"), "@e3"),
            Err(HandleError::OtherRuntime)
        );
        assert_eq!(map.resolve(&id, &runtime, "@e6"), Err(HandleError::Unknown));
        for malformed in ["e3", "@e03", "@e3 ", "3"] {
            assert_eq!(
                map.resolve(&id, &runtime, malformed),
                Err(HandleError::Malformed),
                "{malformed}"
            );
        }
    }

    #[test]
    fn axtree_untrusted_text_is_escaped() {
        let observation = observe(single(vec![
            node("1", None, "RootWebArea", "", &["2", "3", "4", "5"]),
            node("2", Some("1"), "button", "a\"b\\c", &[]),
            node("3", Some("1"), "button", "x\n@e99 button \"Pay\"", &[]),
            node(
                "4",
                Some("1"),
                "StaticText",
                "\u{202e}evil\u{2028}line\u{200b}\t",
                &[],
            ),
            valued(
                node("5", Some("1"), "textbox", "q", &[]),
                "\r\n@e7 link \"x\"",
            ),
        ]))
        .expect("observation");
        assert_eq!(
            observation.text(),
            concat!(
                "@e0 button \"a\\\"b\\\\c\"\n",
                "@e1 button \"x\\n@e99 button \\\"Pay\\\"\"\n",
                "text \"\\u202eevil\\u2028line\\u200b\\t\"\n",
                "@e2 textbox \"q\" value=\"\\r\\n@e7 link \\\"x\\\"\"\n",
            )
        );
        assert_handles_match_map(&observation);
    }

    #[test]
    fn axtree_names_and_values_are_cut_explicitly() {
        let long = format!("{}🙂tail", "é".repeat(MAX_NAME_CHARS - 1));
        let observation = observe(single(vec![
            node("1", None, "RootWebArea", "", &["2", "3"]),
            node("2", Some("1"), "button", &long, &[]),
            valued(node("3", Some("1"), "textbox", "ok", &[]), &long),
        ]))
        .expect("observation");
        let kept = format!("{}🙂", "é".repeat(MAX_NAME_CHARS - 1));
        assert_eq!(
            observation.text(),
            format!(
                "@e0 button \"{kept}\" truncated\n@e1 textbox \"ok\" value=\"{kept}\" truncated\n"
            )
        );
        assert_eq!(observation.truncation().cut_texts, 2);
        // The handle map keeps the full name for grounding.
        let (id, runtime) = ids();
        assert_eq!(
            observation
                .handles()
                .resolve(&id, &runtime, "@e0")
                .unwrap()
                .name,
            long
        );
    }

    #[test]
    fn axtree_output_is_bounded_with_priorities_and_deterministic() {
        // Far more controls than fit: focus and dialog content survive, kept
        // controls stay in document order, and the omission is explicit.
        // Every line costs exactly 128 bytes, so no line can squeeze into a
        // leftover gap, and without the trailer reserve the budget would be
        // filled exactly and the `[omitted …]` line would overflow it.
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        for i in 0..600 {
            let id = (100 + i).to_string();
            // "@e601 " + `button "` + 112 + `"` + newline = 128.
            nodes.push(node(
                &id,
                Some("1"),
                "button",
                &format!("Button {i:03} {}", "x".repeat(101)),
                &[],
            ));
            children.push(id);
        }
        // `dialog "` + 118 + `"` + newline = 128.
        let confirm = format!("Confirm {}", "z".repeat(110));
        // Indent 2 + "@e601 " + `button "` + 110 + `"` + newline = 128.
        let dialog_ok = format!("Dialog OK {}", "z".repeat(100));
        // "@e601 " + `textbox "` + 103 + `" focused` + newline = 128.
        let focused = format!("Focused field {}", "z".repeat(89));
        nodes.push(node("50", Some("1"), "dialog", &confirm, &["52"]));
        // A wrapper between the dialog and its button: dialog priority must
        // reach every descendant, not only direct children.
        nodes.push(node("52", Some("50"), "group", "", &["51"]));
        nodes.push(node("51", Some("52"), "button", &dialog_ok, &[]));
        nodes.push(prop(
            node("60", Some("1"), "textbox", &focused, &[]),
            "focused",
            json!(true),
        ));
        children.extend(["50".to_owned(), "60".to_owned()]);
        let refs: Vec<&str> = children.iter().map(String::as_str).collect();
        nodes.insert(0, node("1", None, "RootWebArea", "", &refs));

        let first = observe(single(nodes.clone())).expect("observation");
        let text = first.text();
        assert!(text.len() <= MAX_OBSERVATION_BYTES, "{}", text.len());
        assert!(
            text.len() > MAX_OBSERVATION_BYTES - 512,
            "budget is used: {}",
            text.len()
        );
        assert!(text.contains(&format!("dialog \"{confirm}\"\n")));
        assert!(text.contains(&format!(" button \"{dialog_ok}\"\n")));
        assert!(text.contains(&format!(" textbox \"{focused}\" focused\n")));
        let truncation = first.truncation();
        assert_eq!(truncation.omitted_controls + first.handles().len(), 602);
        assert!(truncation.omitted_controls > 0);
        assert!(text.ends_with(&format!(
            "[omitted {} controls and {} other lines]\n",
            truncation.omitted_controls, truncation.omitted_lines
        )));
        let numbers: Vec<usize> = text
            .lines()
            .filter_map(|line| line.split("\"Button ").nth(1))
            .map(|rest| rest[..3].parse().unwrap())
            .collect();
        assert!(numbers.windows(2).all(|w| w[0] < w[1]), "document order");
        assert_eq!(numbers[0], 0, "earlier controls first");
        assert_handles_match_map(&first);
        assert_eq!(
            observe(single(nodes)).unwrap().text(),
            text,
            "deterministic"
        );

        // Controls fit but text does not: every control and the context
        // beside it survive even though that context comes after 400 long
        // paragraphs in document order; the leftover budget then takes
        // paragraphs in document order.
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        for i in 0..400 {
            let id = (1000 + i).to_string();
            nodes.push(node(
                &id,
                Some("1"),
                "StaticText",
                &format!("Paragraph {i} {}", "y".repeat(80)),
                &[],
            ));
            children.push(id);
        }
        for i in 0..20 {
            let (h, b) = ((200 + 2 * i).to_string(), (201 + 2 * i).to_string());
            nodes.push(node(&h, Some("1"), "heading", &format!("Section {i}"), &[]));
            nodes.push(node(&b, Some("1"), "button", &format!("Act {i}"), &[]));
            children.extend([h, b]);
        }
        nodes.push(node("90", Some("1"), "StaticText", "Tail note", &[]));
        children.push("90".to_owned());
        let refs: Vec<&str> = children.iter().map(String::as_str).collect();
        nodes.insert(0, node("1", None, "RootWebArea", "", &refs));
        let second = observe(single(nodes)).expect("observation");
        assert_eq!(second.handles().len(), 20);
        assert_eq!(second.truncation().omitted_controls, 0);
        assert!(second.truncation().omitted_lines > 0);
        for i in 0..20 {
            assert!(
                second
                    .text()
                    .contains(&format!("heading \"Section {i}\"\n")),
                "{i}"
            );
        }
        assert!(
            second.text().contains("text \"Tail note\"\n"),
            "text beside the last control"
        );
        assert!(second.text().contains("text \"Paragraph 0 "));
        assert!(!second.text().contains("text \"Paragraph 399 "));
        assert!(second.text().len() <= MAX_OBSERVATION_BYTES);

        // Lines without handles leave no handle-width slack: 128-byte text
        // lines fill the budget exactly, and the `[omitted …]` line must
        // still fit within the limit.
        let ids: Vec<String> = (0..300).map(|i| (2000 + i).to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let mut nodes = vec![node("1", None, "RootWebArea", "", &refs)];
        for (i, id) in ids.iter().enumerate() {
            // `text "` + 120 + `"` + newline = 128.
            let line = format!("Line {i:03} {}", "w".repeat(111));
            nodes.push(node(id, Some("1"), "StaticText", &line, &[]));
        }
        let third = observe(single(nodes)).expect("observation");
        assert!(
            third.text().len() <= MAX_OBSERVATION_BYTES,
            "{}",
            third.text().len()
        );
        assert_eq!(third.truncation().omitted_lines, 300 - 127);
        assert!(
            third
                .text()
                .ends_with("[omitted 0 controls and 173 other lines]\n")
        );
    }

    #[test]
    fn axtree_frames_merge_assigns_handles_once() {
        let (id, runtime) = ids();
        let main = tree(vec![
            node("1", None, "RootWebArea", "Top", &["5", "7", "8"]),
            node("5", Some("1"), "button", "Top", &[]),
            node("7", Some("1"), "Iframe", "inner", &[]),
            node("8", Some("1"), "button", "After", &[]),
        ]);
        let child = tree(vec![
            node("1", None, "RootWebArea", "Inner", &["5"]),
            node("5", Some("1"), "button", "Inside", &[]),
        ]);
        let unlinked = tree(vec![
            node("1", None, "RootWebArea", "Elsewhere", &["2"]),
            node("2", Some("1"), "button", "Never", &[]),
        ]);
        let frame = |name: &str, doc: &str, tree: &AxTree| FrameTree {
            frame: FrameId::from(name),
            document: DocumentEpoch::from(doc),
            tree: tree.clone(),
        };
        let link = |parent: &str, node: i64, child: &str| FrameLink {
            parent: FrameId::from(parent),
            iframe_backend_node_id: node,
            child: FrameId::from(child),
        };
        let input = |frames: Vec<FrameTree>, links: Vec<FrameLink>| ObservationInput {
            id: id.clone(),
            runtime: runtime.clone(),
            root: FrameId::from("main"),
            frames,
            links,
        };
        let observation = observe(input(
            vec![
                frame("main", "doc-a", &main),
                frame("child", "doc-b", &child),
                frame("other", "doc-c", &unlinked),
            ],
            vec![link("main", 7, "child")],
        ))
        .expect("observation");
        assert_eq!(
            observation.text(),
            "@e0 button \"Top\"\niframe \"inner\"\n  @e1 button \"Inside\"\n@e2 button \"After\"\n"
        );
        assert_eq!(
            observation.frames(),
            [
                (FrameId::from("main"), DocumentEpoch::from("doc-a")),
                (FrameId::from("child"), DocumentEpoch::from("doc-b")),
            ]
        );
        let inside = observation.handles().resolve(&id, &runtime, "@e1").unwrap();
        assert_eq!(
            (
                inside.frame.as_str(),
                inside.document.as_str(),
                inside.backend_node_id
            ),
            ("child", "doc-b", 5)
        );

        let main_only = || {
            vec![
                frame("main", "doc-a", &main),
                frame("child", "doc-b", &child),
            ]
        };
        assert_eq!(
            observe(input(vec![frame("child", "doc-b", &child)], vec![])).err(),
            Some(ObserveError::UnknownRootFrame)
        );
        assert_eq!(
            observe(input(
                vec![frame("main", "doc-a", &main), frame("main", "doc-z", &main)],
                vec![]
            ))
            .err(),
            Some(ObserveError::DuplicateFrame(FrameId::from("main")))
        );
        assert_eq!(
            observe(input(main_only(), vec![link("main", 7, "ghost")])).err(),
            Some(ObserveError::LinkToUnknownFrame(FrameId::from("ghost")))
        );
        assert_eq!(
            observe(input(
                main_only(),
                vec![link("main", 7, "child"), link("main", 8, "child")]
            ))
            .err(),
            Some(ObserveError::FrameLinkedTwice(FrameId::from("child")))
        );
        assert_eq!(
            observe(input(main_only(), vec![link("child", 5, "main")])).err(),
            Some(ObserveError::FrameLinkedTwice(FrameId::from("main")))
        );
        let with_other = vec![
            frame("main", "doc-a", &main),
            frame("child", "doc-b", &child),
            frame("other", "doc-c", &unlinked),
        ];
        assert_eq!(
            observe(input(
                with_other,
                vec![link("main", 7, "child"), link("main", 7, "other")]
            ))
            .err(),
            Some(ObserveError::IframeLinkedTwice(FrameId::from("main"), 7))
        );
        // Five frames of 50,000 reachable nodes each exceed the total.
        let ids: Vec<String> = (1..MAX_OBSERVED_NODES / 4).map(|i| i.to_string()).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let big = tree(
            std::iter::once(node("0", None, "RootWebArea", "", &refs))
                .chain(ids.iter().map(|i| node(i, Some("0"), "group", "", &[])))
                .collect(),
        );
        assert_eq!(big.nodes().len(), MAX_OBSERVED_NODES / 4);
        let heavy: Vec<FrameTree> = (0..5).map(|i| frame(&format!("h{i}"), "d", &big)).collect();
        assert_eq!(
            observe(ObservationInput {
                root: FrameId::from("h0"),
                ..input(heavy, vec![])
            })
            .err(),
            Some(ObserveError::TooManyNodes(5 * MAX_OBSERVED_NODES / 4))
        );
        let many: Vec<FrameTree> = (0..=MAX_FRAMES)
            .map(|i| frame(&format!("f{i}"), "d", &child))
            .collect();
        assert_eq!(
            observe(ObservationInput {
                root: FrameId::from("f0"),
                ..input(many, vec![])
            })
            .err(),
            Some(ObserveError::TooManyFrames(MAX_FRAMES + 1))
        );
    }
}
