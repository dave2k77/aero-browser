//! Pre-encryption form data, never a public event or sanitized logging payload.
//!
//! Form data deliberately has no raw Debug representation:
//! ```compile_fail,E0277
//! use aero_core::form::FormSnapshot;
//! let snapshot = FormSnapshot { fields: vec![] };
//! format!("{snapshot:?}");
//! ```

use crate::wire::{ValidationError, nonempty, require, strict_object, strict_tagged_object};

pub const MAX_FORM_ENTRIES: usize = 2000;

strict_object! {
    #[derive(Hash)]
    pub struct FieldLocator {
        pub frame_path: Vec<String>,
        pub origin: String,
        pub document_url: String,
        pub shadow_hosts: Vec<String>,
        pub form_selector: Option<String>,
        pub field_selector: String,
        pub tag: String,
        pub input_type: String,
        pub name: Option<String>,
    }
}

strict_tagged_object! {
    #[derive(Eq)]
    pub enum FieldValue {
        Text {
            value: String,
        },
        Checkbox {
            checked: bool,
        },
        RadioGroup {
            #[serde(deserialize_with = "Option::<String>::deserialize")]
            selected_value: Option<String>,
        },
        Select {
            values: Vec<String>,
        },
    }
    tag "kind";
}

impl FieldValue {
    fn compatible_with(&self, locator: &FieldLocator) -> bool {
        let input = locator.tag.eq_ignore_ascii_case("input");
        let kind = |name: &str| locator.input_type.eq_ignore_ascii_case(name);
        match self {
            Self::Text { .. } => {
                (locator.tag.eq_ignore_ascii_case("textarea") && kind("textarea"))
                    || (input
                        && ![
                            "checkbox", "radio", "file", "password", "hidden", "button", "submit",
                            "reset", "image",
                        ]
                        .iter()
                        .any(|name| kind(name)))
            }
            Self::Checkbox { .. } => input && kind("checkbox"),
            Self::RadioGroup { .. } => input && kind("radio"),
            Self::Select { values } => {
                locator.tag.eq_ignore_ascii_case("select")
                    && (kind("select-multiple") || (kind("select-one") && values.len() <= 1))
            }
        }
    }
}

strict_object! {
    pub struct FormFieldEntry {
        pub locator: FieldLocator,
        pub value: FieldValue,
    }
}

strict_object! {
    pub struct FormSnapshot {
        pub fields: Vec<FormFieldEntry>,
    }
}

impl FormSnapshot {
    /// Validate structural locator identity/counts, not live DOM eligibility or privacy.
    pub fn validate(&self) -> Result<(), ValidationError> {
        require(
            self.fields.len() <= MAX_FORM_ENTRIES,
            "form.fields",
            "must not exceed 2000 entries",
        )?;
        let mut seen = std::collections::HashSet::new();
        for entry in &self.fields {
            let locator = &entry.locator;
            nonempty(&locator.origin, "form.locator.origin")?;
            nonempty(&locator.document_url, "form.locator.document_url")?;
            nonempty(&locator.field_selector, "form.locator.field_selector")?;
            nonempty(&locator.tag, "form.locator.tag")?;
            nonempty(&locator.input_type, "form.locator.input_type")?;
            require(
                locator
                    .frame_path
                    .iter()
                    .chain(&locator.shadow_hosts)
                    .all(|part| !part.is_empty()),
                "form.locator.path",
                "path segments must not be empty",
            )?;
            require(
                locator
                    .form_selector
                    .as_ref()
                    .is_none_or(|selector| !selector.is_empty()),
                "form.locator.form_selector",
                "a present selector must not be empty",
            )?;
            require(
                entry.value.compatible_with(locator),
                "form.value",
                "value kind does not match control metadata",
            )?;
            require(
                seen.insert(locator),
                "form.fields",
                "duplicate locator is ambiguous",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn locator(selector: &str) -> FieldLocator {
        FieldLocator {
            frame_path: vec!["frame-route".into()],
            origin: "https://merchant.test".into(),
            document_url: "https://merchant.test/form".into(),
            shadow_hosts: vec!["#profile-widget".into()],
            form_selector: None,
            field_selector: selector.into(),
            tag: "input".into(),
            input_type: "text".into(),
            name: Some(String::new()),
        }
    }

    fn snapshot() -> FormSnapshot {
        let values = [
            FieldValue::Text {
                value: String::new(),
            },
            FieldValue::Text {
                value: "雪 and café".into(),
            },
            FieldValue::Checkbox { checked: false },
            FieldValue::Checkbox { checked: true },
            FieldValue::RadioGroup {
                selected_value: None,
            },
            FieldValue::RadioGroup {
                selected_value: Some(String::new()),
            },
            FieldValue::Select { values: vec![] },
            FieldValue::Select {
                values: vec!["".into(), "雪".into()],
            },
        ];
        FormSnapshot {
            fields: values
                .into_iter()
                .enumerate()
                .map(|(i, value)| {
                    let mut locator = locator(&format!("#field-{i}"));
                    match &value {
                        FieldValue::Checkbox { .. } => locator.input_type = "checkbox".into(),
                        FieldValue::RadioGroup { .. } => locator.input_type = "radio".into(),
                        FieldValue::Select { .. } => {
                            locator.tag = "select".into();
                            locator.input_type = "select-multiple".into();
                        }
                        FieldValue::Text { .. } => {}
                    }
                    FormFieldEntry { locator, value }
                })
                .collect(),
        }
    }

    #[test]
    fn form_json_preserves_empty_and_false() {
        let original = snapshot();
        let encoded = serde_json::to_string(&original).unwrap();
        let value: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        assert!(value["fields"].is_array());
        assert_eq!(
            value["fields"][0]["value"],
            json!({"kind":"text","value":""})
        );
        assert_eq!(
            value["fields"][2]["value"],
            json!({"kind":"checkbox","checked":false})
        );
        assert_eq!(
            value["fields"][4]["value"],
            json!({"kind":"radio_group","selected_value":null})
        );
        assert!(serde_json::from_str::<FormSnapshot>(&encoded).unwrap() == original);
        assert_eq!(original.validate(), Ok(()));
        let empty = FormSnapshot { fields: vec![] };
        assert!(
            serde_json::from_str::<FormSnapshot>(&serde_json::to_string(&empty).unwrap()).unwrap()
                == empty
        );
    }

    #[test]
    fn form_value_requires_compatible_control_metadata() {
        for (index, tag, input_type) in [
            (0, "input", "checkbox"),
            (0, "input", "radio"),
            (0, "input", "file"),
            (0, "input", "password"),
            (0, "input", "hidden"),
            (0, "input", "button"),
            (0, "select", "select-one"),
            (2, "input", "text"),
            (2, "textarea", "checkbox"),
            (4, "input", "checkbox"),
            (6, "input", "text"),
            (6, "select", "text"),
        ] {
            let mut value = snapshot();
            value.fields[index].locator.tag = tag.into();
            value.fields[index].locator.input_type = input_type.into();
            assert_eq!(value.validate().unwrap_err().field(), "form.value");
        }
        let mut value = snapshot();
        value.fields[0].locator.tag = "TEXTAREA".into();
        value.fields[0].locator.input_type = "textarea".into();
        value.fields[2].locator.tag = "INPUT".into();
        value.fields[2].locator.input_type = "CHECKBOX".into();
        assert_eq!(value.validate(), Ok(()));
        value.fields[7].locator.input_type = "select-one".into();
        assert_eq!(value.validate().unwrap_err().field(), "form.value");
        value.fields[7].value = FieldValue::Select {
            values: vec!["雪".into()],
        };
        assert_eq!(value.validate(), Ok(()));
    }

    #[test]
    fn form_objects_are_strict_at_every_level() {
        let value = serde_json::to_value(snapshot()).unwrap();
        for pointer in ["", "/fields/0", "/fields/0/locator", "/fields/0/value"] {
            let mut bad = value.clone();
            bad.pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
            assert!(serde_json::from_value::<FormSnapshot>(bad).is_err());
        }
        let mut bad = value.clone();
        bad["fields"][0]["value"] = json!({"kind":"password","value":"SYNTHETIC"});
        assert!(serde_json::from_value::<FormSnapshot>(bad).is_err());
        for raw in ["null", "[]", "[[]]", r#"{"fields":[],"fields":[]}"#, "{}"] {
            assert!(serde_json::from_str::<FormSnapshot>(raw).is_err());
        }
        let entry = value["fields"][0].clone();
        let entry_array = json!([entry["locator"], entry["value"]]);
        assert!(serde_json::from_value::<FormFieldEntry>(entry_array).is_err());
        let loc = entry["locator"].as_object().unwrap();
        let loc_array = json!([
            loc["frame_path"],
            loc["origin"],
            loc["document_url"],
            loc["shadow_hosts"],
            loc["form_selector"],
            loc["field_selector"],
            loc["tag"],
            loc["input_type"],
            loc["name"],
        ]);
        assert!(serde_json::from_value::<FieldLocator>(loc_array).is_err());
        // Nullable fields are explicitly present, so absence cannot erase state.
        let mut missing = entry["locator"].clone();
        missing.as_object_mut().unwrap().remove("name");
        assert!(serde_json::from_value::<FieldLocator>(missing).is_err());
        // Feed original duplicate bytes: a null first value is still present.
        for field in ["form_selector", "name"] {
            for (first, second) in [(json!(null), json!("other")), (json!("first"), json!(null))] {
                let mut locator = entry["locator"].clone();
                locator[field] = second;
                let encoded = serde_json::to_string(&locator).unwrap();
                let duplicate = format!("{{\"{field}\":{first},{}", &encoded[1..]);
                assert!(serde_json::from_str::<FieldLocator>(&duplicate).is_err());
            }
        }
        for raw in [
            r#"["text","SYNTHETIC"]"#,
            r#"["checkbox",false]"#,
            r#"["radio_group",null]"#,
            r#"["select",[]]"#,
        ] {
            assert!(
                serde_json::from_str::<FieldValue>(raw).is_err(),
                "accepted FieldValue array: {raw}"
            );
            let mut nested = value.clone();
            nested["fields"][0]["value"] = serde_json::from_str(raw).unwrap();
            assert!(serde_json::from_value::<FormSnapshot>(nested).is_err());
        }
        for raw in [
            r#"{"kind":"checkbox","checked":false,"checked":true}"#,
            r#"{"kind":"radio_group","selected_value":null,"selected_value":"x"}"#,
            r#"{"kind":"radio_group"}"#,
            r#"{"kind":"text","value":"","value":"x"}"#,
        ] {
            assert!(serde_json::from_str::<FieldValue>(raw).is_err());
        }
    }

    #[test]
    fn form_validation_rejects_ambiguous_or_empty_locators_and_limits() {
        let mut value = snapshot();
        value.fields.push(value.fields[0].clone());
        assert_eq!(value.validate().unwrap_err().field(), "form.fields");
        for mutate in [
            |l: &mut FieldLocator| l.origin.clear(),
            |l: &mut FieldLocator| l.document_url.clear(),
            |l: &mut FieldLocator| l.field_selector.clear(),
            |l: &mut FieldLocator| l.tag.clear(),
            |l: &mut FieldLocator| l.input_type.clear(),
            |l: &mut FieldLocator| l.frame_path.push(String::new()),
            |l: &mut FieldLocator| l.shadow_hosts.push(String::new()),
            |l: &mut FieldLocator| l.form_selector = Some(String::new()),
        ] {
            let mut value = snapshot();
            mutate(&mut value.fields[0].locator);
            assert!(value.validate().is_err());
        }
        let fields = (0..MAX_FORM_ENTRIES)
            .map(|i| FormFieldEntry {
                locator: locator(&format!("#field-{i}")),
                value: FieldValue::Text {
                    value: String::new(),
                },
            })
            .collect::<Vec<_>>();
        let mut value = FormSnapshot { fields };
        assert_eq!(value.validate(), Ok(()));
        value.fields.push(FormFieldEntry {
            locator: locator("#one-too-many"),
            value: FieldValue::Text {
                value: String::new(),
            },
        });
        assert_eq!(value.validate().unwrap_err().field(), "form.fields");
        let error = value.validate().unwrap_err();
        assert!(!format!("{error:?} {error}").contains("one-too-many"));
    }
}
