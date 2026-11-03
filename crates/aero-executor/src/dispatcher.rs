//! D02: exactly one mouse move/press/release sequence, never retried.
//!
//! D03: text, keys and form controls. Typing grounds the target (D02 checks),
//! focuses it once without any pointer input, verifies it is the active
//! element, optionally clears it with Ctrl+A and Backspace, inserts Unicode
//! with `Input.insertText` and reads the value back after two animation
//! frames; Enter is a separate key event, never newline text. Select and
//! checkbox changes are typed operations whose results are read back.
//! Values typed or read are never included in errors.

use aero_cdp::page::Page;
use aero_cdp::runtime::RuntimeError;
use aero_core::types::RootViewportPoint;
use serde_json::json;

use crate::TargetRef;
use crate::target::{GroundingError, call_on, ground_node, malformed};

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

/// Why a text, key, select or checkbox action failed. Grounding failures and
/// refusals send no input; `Dispatch` and the `*Mismatch`/`NotApplied`
/// variants mean input was sent but the result is not the requested one.
#[derive(Debug, PartialEq)]
pub enum InputError {
    Grounding(GroundingError),
    Dispatch(DispatchError),
    /// Not a text input, textarea or other supported editable element.
    NotEditable,
    Readonly,
    /// Newline text for a single-line input (Enter is a separate key event).
    NewlineInSingleLine,
    /// Focusing did not make the target the active element.
    FocusFailed,
    /// Ctrl+A and Backspace did not leave the field empty.
    ClearFailed,
    /// The field's value after typing is not the expected value.
    ValueMismatch,
    /// Not in the B02 named-key allowlist.
    UnsupportedKey,
    NotASelect,
    /// A value names no option, or an option that is disabled.
    UnknownOption,
    /// Several values for a single select, or none.
    WrongValueCount,
    NotACheckbox,
    /// A radio button cannot be unchecked by user input.
    CannotUncheckRadio,
    /// The control's state after the action is not the requested state.
    NotApplied,
    Runtime(RuntimeError),
}

/// The CDP description of one allow-listed key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyDefinition {
    pub key: &'static str,
    pub code: &'static str,
    pub virtual_key: u32,
    /// Text the key produces, which makes it a character key (keypress).
    pub text: Option<&'static str>,
}

/// The definition of a B02 named key, or `None` for anything else.
pub(crate) fn key_definition(name: &str) -> Option<KeyDefinition> {
    let (key, code, virtual_key, text) = match name {
        "Enter" => ("Enter", "Enter", 13, Some("\r")),
        "Tab" => ("Tab", "Tab", 9, None),
        "Escape" => ("Escape", "Escape", 27, None),
        "Backspace" => ("Backspace", "Backspace", 8, None),
        "Delete" => ("Delete", "Delete", 46, None),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38, None),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40, None),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37, None),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39, None),
        "Home" => ("Home", "Home", 36, None),
        "End" => ("End", "End", 35, None),
        "PageUp" => ("PageUp", "PageUp", 33, None),
        "PageDown" => ("PageDown", "PageDown", 34, None),
        "Space" => (" ", "Space", 32, Some(" ")),
        _ => return None,
    };
    Some(KeyDefinition {
        key,
        code,
        virtual_key,
        text,
    })
}

/// Ctrl+A for clearing; internal only, not a model-callable key.
const SELECT_ALL: KeyDefinition = KeyDefinition {
    key: "a",
    code: "KeyA",
    virtual_key: 65,
    text: None,
};
const CONTROL: u8 = 2;

/// Sends input events in order and classifies a failure: before anything was
/// delivered nothing happened; after that the effect is uncertain.
struct Sender<'a> {
    page: &'a Page,
    delivered: u8,
}

impl<'a> Sender<'a> {
    fn new(page: &'a Page) -> Self {
        Self { page, delivered: 0 }
    }

    async fn send(&mut self, method: &str, params: serde_json::Value) -> Result<(), InputError> {
        match self.page.call(method, params).await {
            Ok(_) => {
                self.delivered = self.delivered.saturating_add(1);
                Ok(())
            }
            Err(error) if self.delivered == 0 => {
                Err(InputError::Dispatch(DispatchError::NotStarted(error)))
            }
            Err(error) => Err(InputError::Dispatch(DispatchError::Uncertain {
                delivered: self.delivered,
                error,
            })),
        }
    }

    /// One key press: a character key sends `keyDown` with its text (so it
    /// produces keypress/input or implicit submission); others `rawKeyDown`.
    async fn key(
        &mut self,
        key: KeyDefinition,
        modifiers: u8,
        commands: &[&str],
    ) -> Result<(), InputError> {
        let mut down = json!({
            "type": if key.text.is_some() && modifiers == 0 { "keyDown" } else { "rawKeyDown" },
            "key": key.key,
            "code": key.code,
            "windowsVirtualKeyCode": key.virtual_key,
            "nativeVirtualKeyCode": key.virtual_key,
            "modifiers": modifiers,
        });
        if modifiers == 0
            && let Some(text) = key.text
        {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        if !commands.is_empty() {
            down["commands"] = json!(commands);
        }
        self.send("Input.dispatchKeyEvent", down).await?;
        self.send(
            "Input.dispatchKeyEvent",
            json!({
                "type": "keyUp",
                "key": key.key,
                "code": key.code,
                "windowsVirtualKeyCode": key.virtual_key,
                "nativeVirtualKeyCode": key.virtual_key,
                "modifiers": modifiers,
            }),
        )
        .await
    }
}

/// Press and release one allow-listed key in the focused element.
pub async fn press_key(page: &Page, key: &str) -> Result<(), InputError> {
    let key = key_definition(key).ok_or(InputError::UnsupportedKey)?;
    Sender::new(page).key(key, 0, &[]).await
}

/// What kind of text control the target is (run in the isolated world).
const TEXT_PROBE: &str = "function () {
  const singleLine = ['text', 'search', 'email', 'url', 'tel', 'password'];
  if (this instanceof HTMLTextAreaElement) {
    return { kind: 'multi', readonly: this.readOnly, value: this.value };
  }
  if (this instanceof HTMLInputElement && singleLine.includes(this.type)) {
    return { kind: 'single', readonly: this.readOnly, value: this.value };
  }
  return { kind: 'other' };
}";

/// Whether the target is the active element, through open shadow roots.
const IS_ACTIVE: &str = "function () {
  let active = document.activeElement;
  while (active && active.shadowRoot && active.shadowRoot.activeElement) {
    active = active.shadowRoot.activeElement;
  }
  return active === this;
}";

/// Put the caret at the end (types such as `email` have no selection API).
const CARET_TO_END: &str = "function () {
  try { const end = this.value.length; this.setSelectionRange(end, end); } catch (error) {}
  return true;
}";

/// `property` of the target after two animation frames, so that application
/// re-renders triggered by the input have run.
fn settled(property: &str) -> String {
    format!(
        "function () {{ return new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve({property})))); }}"
    )
}

async fn ground(
    page: &Page,
    target: TargetRef<'_>,
) -> Result<(crate::target::GroundedTarget, String), InputError> {
    ground_node(
        page,
        target.handles,
        target.observation,
        target.runtime,
        target.handle,
    )
    .await
    .map_err(InputError::Grounding)
}

async fn run(
    page: &Page,
    object: &str,
    function: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, InputError> {
    call_on(page, object, function, arguments)
        .await
        .map_err(InputError::Grounding)
}

/// Type `text` into the target (after clearing it if `clear`), then press
/// Enter once if `press_enter`.
pub async fn type_text(
    page: &Page,
    target: TargetRef<'_>,
    text: &str,
    clear: bool,
    press_enter: bool,
) -> Result<(), InputError> {
    let (grounded, object) = ground(page, target).await?;
    let probe = run(page, &object, TEXT_PROBE, json!([])).await?;
    let multi_line = match probe["kind"].as_str() {
        Some("single") => false,
        Some("multi") => true,
        _ => return Err(InputError::NotEditable),
    };
    if probe["readonly"] == true {
        return Err(InputError::Readonly);
    }
    if !multi_line && text.contains(['\n', '\r']) {
        return Err(InputError::NewlineInSingleLine);
    }
    let previous = probe["value"]
        .as_str()
        .ok_or_else(|| InputError::Grounding(malformed("text control without a value")))?;
    page.call(
        "DOM.focus",
        json!({ "backendNodeId": grounded.backend_node_id }),
    )
    .await
    .map_err(InputError::Runtime)?;
    if run(page, &object, IS_ACTIVE, json!([])).await? != true {
        return Err(InputError::FocusFailed);
    }
    let mut sender = Sender::new(page);
    let expected = if clear {
        sender.key(SELECT_ALL, CONTROL, &["selectAll"]).await?;
        sender
            .key(key_definition("Backspace").expect("allow-listed"), 0, &[])
            .await?;
        if run(page, &object, &settled("this.value"), json!([])).await? != "" {
            return Err(InputError::ClearFailed);
        }
        String::new()
    } else {
        run(page, &object, CARET_TO_END, json!([])).await?;
        previous.to_owned()
    };
    // A textarea stores line breaks as LF.
    let inserted = if multi_line {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_owned()
    };
    if !text.is_empty() {
        sender
            .send("Input.insertText", json!({ "text": text }))
            .await?;
    }
    if run(page, &object, &settled("this.value"), json!([])).await? != expected + &inserted {
        return Err(InputError::ValueMismatch);
    }
    if press_enter {
        sender
            .key(key_definition("Enter").expect("allow-listed"), 0, &[])
            .await?;
    }
    Ok(())
}

/// Select exactly the given option values (run in the isolated world). The
/// change is announced with `input` and `change` events.
const SELECT: &str = "function (values) {
  if (!(this instanceof HTMLSelectElement)) return { error: 'not_select' };
  const wanted = Array.from(new Set(values));
  if (!this.multiple && wanted.length !== 1) return { error: 'count' };
  const options = Array.from(this.options);
  for (const value of wanted) {
    const option = options.find((candidate) => candidate.value === value);
    if (!option || option.disabled) return { error: 'unknown' };
  }
  for (const option of options) option.selected = wanted.includes(option.value);
  this.dispatchEvent(new Event('input', { bubbles: true }));
  this.dispatchEvent(new Event('change', { bubbles: true }));
  return { error: null };
}";

/// Select exactly `values` (by option value) in a `<select>`.
pub async fn select_values(
    page: &Page,
    target: TargetRef<'_>,
    values: &[String],
) -> Result<(), InputError> {
    let (_, object) = ground(page, target).await?;
    let outcome = run(page, &object, SELECT, json!([{ "value": values }])).await?;
    match outcome["error"].as_str() {
        None => {}
        Some("not_select") => return Err(InputError::NotASelect),
        Some("count") => return Err(InputError::WrongValueCount),
        Some("unknown") => return Err(InputError::UnknownOption),
        Some(_) => {
            return Err(InputError::Grounding(malformed(
                "unexpected select outcome",
            )));
        }
    }
    let selected = run(
        page,
        &object,
        &settled("Array.from(this.selectedOptions, (option) => option.value)"),
        json!([]),
    )
    .await?;
    let mut actual: Vec<&str> = selected
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    let mut wanted: Vec<&str> = values.iter().map(String::as_str).collect();
    actual.sort_unstable();
    actual.dedup();
    wanted.sort_unstable();
    wanted.dedup();
    if actual != wanted {
        return Err(InputError::NotApplied);
    }
    Ok(())
}

/// Checkbox or radio state (run in the isolated world).
const CHECK_PROBE: &str = "function () {
  if (!(this instanceof HTMLInputElement) || (this.type !== 'checkbox' && this.type !== 'radio')) {
    return { kind: 'other' };
  }
  return { kind: this.type, checked: this.checked };
}";

/// Make a checkbox or radio button `checked`, clicking it at most once.
pub async fn set_checked(
    page: &Page,
    target: TargetRef<'_>,
    checked: bool,
) -> Result<(), InputError> {
    let (grounded, object) = ground(page, target).await?;
    let probe = run(page, &object, CHECK_PROBE, json!([])).await?;
    let radio = match probe["kind"].as_str() {
        Some("checkbox") => false,
        Some("radio") => true,
        _ => return Err(InputError::NotACheckbox),
    };
    if probe["checked"] == checked {
        return Ok(());
    }
    if radio && !checked {
        return Err(InputError::CannotUncheckRadio);
    }
    click(page, grounded.point)
        .await
        .map_err(InputError::Dispatch)?;
    if run(page, &object, &settled("this.checked"), json!([])).await? != checked {
        return Err(InputError::NotApplied);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_have_definitions_and_nothing_else_does() {
        for name in [
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Space",
        ] {
            assert!(key_definition(name).is_some(), "{name}");
        }
        assert_eq!(
            key_definition("Enter"),
            Some(KeyDefinition {
                key: "Enter",
                code: "Enter",
                virtual_key: 13,
                text: Some("\r")
            })
        );
        assert_eq!(
            key_definition("Space"),
            Some(KeyDefinition {
                key: " ",
                code: "Space",
                virtual_key: 32,
                text: Some(" ")
            })
        );
        assert_eq!(
            key_definition("Tab"),
            Some(KeyDefinition {
                key: "Tab",
                code: "Tab",
                virtual_key: 9,
                text: None
            })
        );
        assert_eq!(key_definition("ArrowLeft").map(|k| k.virtual_key), Some(37));
        for refused in ["", "enter", "F5", "a", "Control", "Meta", " ", "Enter "] {
            assert_eq!(key_definition(refused), None, "{refused:?}");
        }
    }
}
