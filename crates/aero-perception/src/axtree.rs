//! D01: parse one frame's `Accessibility.getFullAXTree` reply into a pruned
//! tree in DOM order.
//!
//! The reply is browser output describing page-controlled content, so its
//! shape is checked strictly and its size is bounded before walking. Chromium
//! lists nodes breadth-first; DOM order comes from walking `childIds` from the
//! `RootWebArea`. Ignored nodes (layout wrappers) are skipped but their
//! children are kept; `InlineTextBox` nodes (no DOM node, text repeated from
//! their `StaticText` parent) are dropped.

use std::collections::HashMap;

use serde_json::Value;

/// Largest accepted node count for one frame's tree.
pub const MAX_AX_NODES: usize = 50_000;

/// Tri-state `checked` (checkboxes, switches, menu items).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Checked {
    True,
    False,
    Mixed,
}

/// The accessibility states D01 reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AxStates {
    pub disabled: bool,
    pub readonly: bool,
    pub required: bool,
    pub focused: bool,
    pub checked: Option<Checked>,
    pub selected: Option<bool>,
    pub expanded: Option<bool>,
    pub level: Option<u32>,
}

/// One kept node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxNode {
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub backend_node_id: Option<i64>,
    pub states: AxStates,
    /// Index of the nearest kept ancestor in [`AxTree::nodes`]; `None` only
    /// for the root.
    pub parent: Option<usize>,
}

/// A pruned tree in DOM (pre-)order; `nodes()[0]` is the `RootWebArea`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxTree {
    nodes: Vec<AxNode>,
}

impl AxTree {
    pub fn nodes(&self) -> &[AxNode] {
        &self.nodes
    }
}

/// Why a reply was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AxError {
    /// A field has the wrong shape.
    Malformed(String),
    TooManyNodes(usize),
    DuplicateNode(String),
    /// No parentless `RootWebArea`.
    MissingRoot,
    /// A `childIds` entry names no node.
    MissingChild(String),
    /// A node is reachable twice (a cycle or shared child).
    Revisited(String),
}

impl std::fmt::Display for AxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "malformed accessibility tree: {why}"),
            Self::TooManyNodes(count) => write!(f, "{count} accessibility nodes exceed the limit"),
            Self::DuplicateNode(_) => f.write_str("duplicate accessibility node ID"),
            Self::MissingRoot => f.write_str("accessibility tree without a root"),
            Self::MissingChild(_) => f.write_str("accessibility child ID names no node"),
            Self::Revisited(_) => f.write_str("accessibility node reachable twice"),
        }
    }
}

impl std::error::Error for AxError {}

/// Parse a `getFullAXTree` reply (`{"nodes": [...]}`).
pub fn parse_tree(reply: &Value) -> Result<AxTree, AxError> {
    let raw = reply
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("reply without a nodes array"))?;
    if raw.len() > MAX_AX_NODES {
        return Err(AxError::TooManyNodes(raw.len()));
    }
    let mut by_id: HashMap<&str, usize> = HashMap::with_capacity(raw.len());
    for (index, node) in raw.iter().enumerate() {
        let id = node
            .get("nodeId")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("node without a string nodeId"))?;
        if by_id.insert(id, index).is_some() {
            return Err(AxError::DuplicateNode(id.to_owned()));
        }
    }
    let root = raw
        .iter()
        .position(|node| {
            node.get("parentId").is_none_or(Value::is_null)
                && node["role"]["value"].as_str() == Some("RootWebArea")
        })
        .ok_or(AxError::MissingRoot)?;

    let mut visited = vec![false; raw.len()];
    let mut nodes = Vec::new();
    // (raw index, nearest kept ancestor); children pushed in reverse so the
    // walk is DOM pre-order without recursion.
    let mut stack = vec![(root, None::<usize>)];
    while let Some((index, parent)) = stack.pop() {
        let node = &raw[index];
        if std::mem::replace(&mut visited[index], true) {
            return Err(AxError::Revisited(
                node["nodeId"].as_str().unwrap_or_default().to_owned(),
            ));
        }
        let role = optional_text(&node["role"]["value"], "role")?.unwrap_or_default();
        let ignored = match node.get("ignored") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(ignored)) => *ignored,
            Some(_) => return Err(malformed("ignored is not a boolean")),
        };
        let keep = index == root || (!ignored && role != "InlineTextBox");
        let child_parent = if keep {
            nodes.push(AxNode {
                name: optional_text(&node["name"]["value"], "name")?.unwrap_or_default(),
                value: scalar_text(&node["value"]["value"])?,
                backend_node_id: match node.get("backendDOMNodeId") {
                    None | Some(Value::Null) => None,
                    Some(id) => Some(id.as_i64().ok_or_else(|| malformed("backendDOMNodeId"))?),
                },
                states: states(node.get("properties"))?,
                role,
                parent,
            });
            Some(nodes.len() - 1)
        } else {
            parent
        };
        let children = match node.get("childIds") {
            None | Some(Value::Null) => &[][..],
            Some(Value::Array(children)) => children.as_slice(),
            Some(_) => return Err(malformed("childIds is not an array")),
        };
        for child in children.iter().rev() {
            let id = child
                .as_str()
                .ok_or_else(|| malformed("child ID is not a string"))?;
            let child = *by_id
                .get(id)
                .ok_or_else(|| AxError::MissingChild(id.to_owned()))?;
            stack.push((child, child_parent));
        }
    }
    Ok(AxTree { nodes })
}

fn malformed(why: &str) -> AxError {
    AxError::Malformed(why.to_owned())
}

/// A string field that may be absent or null.
fn optional_text(value: &Value, field: &str) -> Result<Option<String>, AxError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(malformed(&format!("{field} is not a string"))),
    }
}

/// An AX value: strings, numbers and booleans are rendered as text.
fn scalar_text(value: &Value) -> Result<Option<String>, AxError> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Number(number) => Ok(Some(number.to_string())),
        Value::Bool(flag) => Ok(Some(flag.to_string())),
        _ => Err(malformed("value is not a scalar")),
    }
}

fn states(properties: Option<&Value>) -> Result<AxStates, AxError> {
    let mut states = AxStates::default();
    let properties = match properties {
        None | Some(Value::Null) => return Ok(states),
        Some(Value::Array(properties)) => properties,
        Some(_) => return Err(malformed("properties is not an array")),
    };
    for property in properties {
        let name = property["name"]
            .as_str()
            .ok_or_else(|| malformed("property without a name"))?;
        let value = &property["value"]["value"];
        let flag = || {
            value
                .as_bool()
                .ok_or_else(|| malformed(&format!("{name} is not a boolean")))
        };
        match name {
            "disabled" => states.disabled = flag()?,
            "readonly" => states.readonly = flag()?,
            "required" => states.required = flag()?,
            "focused" => states.focused = flag()?,
            "selected" => states.selected = Some(flag()?),
            "expanded" => states.expanded = Some(flag()?),
            "checked" => {
                states.checked = Some(match value {
                    Value::Bool(true) => Checked::True,
                    Value::Bool(false) => Checked::False,
                    Value::String(text) if text == "true" => Checked::True,
                    Value::String(text) if text == "false" => Checked::False,
                    Value::String(text) if text == "mixed" => Checked::Mixed,
                    _ => return Err(malformed("checked is not a tri-state")),
                })
            }
            "level" => {
                states.level = Some(
                    value
                        .as_u64()
                        .and_then(|level| u32::try_from(level).ok())
                        .ok_or_else(|| malformed("level is not a small integer"))?,
                )
            }
            _ => {}
        }
    }
    Ok(states)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A Chromium-shaped node; numeric IDs double as backend DOM node IDs.
    pub(crate) fn node(
        id: &str,
        parent: Option<&str>,
        role: &str,
        name: &str,
        children: &[&str],
    ) -> Value {
        let mut value = json!({
            "nodeId": id,
            "ignored": false,
            "role": { "type": "role", "value": role },
            "name": { "type": "computedString", "value": name },
            "childIds": children,
        });
        if let Some(parent) = parent {
            value["parentId"] = json!(parent);
        }
        if let Ok(backend) = id.parse::<i64>() {
            value["backendDOMNodeId"] = json!(backend);
        }
        value
    }

    pub(crate) fn prop(mut node: Value, name: &str, value: Value) -> Value {
        if node.get("properties").is_none() {
            node["properties"] = json!([]);
        }
        node["properties"].as_array_mut().unwrap().push(
            json!({ "name": name, "value": { "type": "booleanOrUndefined", "value": value } }),
        );
        node
    }

    pub(crate) fn ignored(mut node: Value) -> Value {
        node["ignored"] = json!(true);
        node["role"]["value"] = json!("none");
        node["name"] = Value::Null;
        node
    }

    pub(crate) fn tree(nodes: Vec<Value>) -> AxTree {
        parse_tree(&json!({ "nodes": nodes })).expect("valid tree")
    }

    fn roles(tree: &AxTree) -> Vec<(String, String, Option<usize>)> {
        tree.nodes()
            .iter()
            .map(|n| (n.role.clone(), n.name.clone(), n.parent))
            .collect()
    }

    #[test]
    fn axtree_walks_dom_order_through_ignored_wrappers() {
        // Listed breadth-first, as Chromium does; DOM order is 1, 3, 4, 5, 6.
        let tree = tree(vec![
            node("1", None, "RootWebArea", "Page", &["2"]),
            ignored(node("2", Some("1"), "none", "", &["3", "5"])),
            node("3", Some("2"), "heading", "Title", &["4"]),
            node("5", Some("2"), "button", "Go", &["6"]),
            node("4", Some("3"), "StaticText", "Title", &["-100"]),
            node("6", Some("5"), "StaticText", "Go", &[]),
            node("-100", Some("4"), "InlineTextBox", "Title", &[]),
        ]);
        assert_eq!(
            roles(&tree),
            [
                ("RootWebArea".into(), "Page".into(), None),
                ("heading".into(), "Title".into(), Some(0)),
                ("StaticText".into(), "Title".into(), Some(1)),
                ("button".into(), "Go".into(), Some(0)),
                ("StaticText".into(), "Go".into(), Some(3)),
            ]
        );
        assert_eq!(tree.nodes()[3].backend_node_id, Some(5));
    }

    #[test]
    fn axtree_parses_states_and_values() {
        let mut slider = node("5", Some("1"), "slider", "Volume", &[]);
        slider["value"] = json!({ "type": "number", "value": 7 });
        let mut textbox = prop(
            prop(
                prop(
                    node("3", Some("1"), "textbox", "Name", &[]),
                    "readonly",
                    json!(true),
                ),
                "required",
                json!(true),
            ),
            "focused",
            json!(true),
        );
        textbox["value"] = json!({ "type": "string", "value": "v" });
        let tree = tree(vec![
            node(
                "1",
                None,
                "RootWebArea",
                "Page",
                &["2", "3", "4", "5", "6", "7"],
            ),
            prop(
                node("2", Some("1"), "heading", "Results", &[]),
                "level",
                json!(2),
            ),
            textbox,
            prop(
                node("4", Some("1"), "checkbox", "Box", &[]),
                "checked",
                json!("mixed"),
            ),
            slider,
            prop(
                prop(
                    node("6", Some("1"), "button", "Off", &[]),
                    "disabled",
                    json!(true),
                ),
                "expanded",
                json!(false),
            ),
            prop(
                node("7", Some("1"), "option", "B", &[]),
                "selected",
                json!(true),
            ),
        ]);
        let n = tree.nodes();
        assert_eq!(n[1].states.level, Some(2));
        assert_eq!(n[2].value.as_deref(), Some("v"));
        assert!(n[2].states.readonly && n[2].states.required && n[2].states.focused);
        assert!(!n[2].states.disabled);
        assert_eq!(n[3].states.checked, Some(Checked::Mixed));
        assert_eq!(n[4].value.as_deref(), Some("7"));
        assert!(n[5].states.disabled);
        assert_eq!(n[5].states.expanded, Some(false));
        assert_eq!(n[6].states.selected, Some(true));
        assert_eq!(n[0].states, AxStates::default());
    }

    #[test]
    fn axtree_rejects_malformed_duplicate_cyclic_and_oversized() {
        let parse = |nodes: Vec<Value>| parse_tree(&json!({ "nodes": nodes }));
        assert!(matches!(parse_tree(&json!({})), Err(AxError::Malformed(_))));
        assert!(matches!(
            parse_tree(&json!({ "nodes": {} })),
            Err(AxError::Malformed(_))
        ));
        assert!(matches!(
            parse(vec![json!({ "role": { "value": "button" } })]),
            Err(AxError::Malformed(_))
        ));
        assert_eq!(
            parse(vec![
                node("1", None, "RootWebArea", "", &[]),
                node("1", None, "button", "", &[])
            ]),
            Err(AxError::DuplicateNode("1".into()))
        );
        assert_eq!(
            parse(vec![node("1", None, "button", "", &[])]),
            Err(AxError::MissingRoot)
        );
        assert_eq!(
            parse(vec![node("1", None, "RootWebArea", "", &["9"])]),
            Err(AxError::MissingChild("9".into()))
        );
        assert_eq!(
            parse(vec![
                node("1", None, "RootWebArea", "", &["2"]),
                node("2", Some("1"), "group", "", &["3"]),
                node("3", Some("2"), "group", "", &["2"]),
            ]),
            Err(AxError::Revisited("2".into()))
        );
        assert!(matches!(
            parse(vec![
                node("1", None, "RootWebArea", "", &["2"]),
                prop(
                    node("2", Some("1"), "heading", "", &[]),
                    "level",
                    json!("two")
                ),
            ]),
            Err(AxError::Malformed(_))
        ));
        let mut bad_name = node("1", None, "RootWebArea", "", &[]);
        bad_name["name"]["value"] = json!(["not", "text"]);
        assert!(matches!(parse(vec![bad_name]), Err(AxError::Malformed(_))));
        let many: Vec<Value> = (0..=MAX_AX_NODES)
            .map(|i| json!({ "nodeId": i.to_string() }))
            .collect();
        assert_eq!(parse(many), Err(AxError::TooManyNodes(MAX_AX_NODES + 1)));
    }
}
