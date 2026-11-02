//! Private building blocks for strict core contracts; no JSON runtime dependency.

/// A static field/reason pair. Rejected values are never embedded in this error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationError {
    field: &'static str,
    message: &'static str,
}

impl ValidationError {
    pub fn field(&self) -> &'static str {
        self.field
    }
    pub fn message(&self) -> &'static str {
        self.message
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}
impl std::error::Error for ValidationError {}

pub(crate) fn require(
    valid: bool,
    field: &'static str,
    message: &'static str,
) -> Result<(), ValidationError> {
    if valid {
        Ok(())
    } else {
        Err(ValidationError { field, message })
    }
}

pub(crate) fn nonempty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require(!value.is_empty(), field, "must not be empty")
}

pub(crate) fn unique_nonempty<'a>(
    values: impl Iterator<Item = &'a str>,
    field: &'static str,
) -> Result<(), ValidationError> {
    let mut seen = std::collections::HashSet::new();
    for value in values {
        nonempty(value, field)?;
        require(seen.insert(value), field, "duplicate reference")?;
    }
    Ok(())
}

pub(crate) fn read_once<'de, A, T>(
    map: &mut A,
    slot: &mut Option<T>,
    field: &'static str,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
    T: serde::Deserialize<'de>,
{
    if slot.is_some() {
        return Err(serde::de::Error::duplicate_field(field));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

// Declare each schema once. Map-only decoding rejects positional arrays; every
// field is required, including Option fields (explicit null), and no raw Debug
// is derived for objects that may hold pre-encryption browser state.
macro_rules! strict_object {
    ($(#[$meta:meta])* $vis:vis struct $name:ident {
        $($(#[$field_meta:meta])* pub $field:ident: $ty:ty),* $(,)?
    } $(version $version_field:ident = $version:expr;)?) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, serde::Serialize)]
        $vis struct $name {
            $($(#[$field_meta])* pub $field: $ty),*
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[allow(non_camel_case_types)]
                #[derive(serde::Deserialize)]
                #[serde(field_identifier)]
                enum Field { $($field),* }
                struct ObjectVisitor;
                impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str(concat!(stringify!($name), " object"))
                    }
                    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<$name, A::Error> {
                        use serde::de::Error;
                        $(let mut $field: Option<$ty> = None;)*
                        while let Some(field) = map.next_key::<Field>()? {
                            match field {
                                $(Field::$field => crate::wire::read_once(&mut map, &mut $field, stringify!($field))?,)*
                            }
                        }
                        let value = $name {
                            $($field: $field.ok_or_else(|| A::Error::missing_field(stringify!($field)))?),*
                        };
                        $(if value.$version_field != $version {
                            return Err(A::Error::custom("unsupported schema version"));
                        })?
                        Ok(value)
                    }
                }
                deserializer.deserialize_map(ObjectVisitor)
            }
        }
    };
}
pub(crate) use strict_object;

// Internally tagged serde enums also accept positional sequences by default.
// Keep one variant definition, but gate decoding through map visitation before
// delegating to serde's generated tag/field/duplicate/required-field checks.
macro_rules! strict_tagged_object {
    ($(#[$meta:meta])* $vis:vis enum $name:ident {
        $($variant:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty),* $(,)? }),* $(,)?
    } tag $tag:literal;) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, serde::Serialize)]
        #[serde(tag = $tag, rename_all = "snake_case", deny_unknown_fields)]
        $vis enum $name {
            $($variant { $($(#[$field_meta])* $field: $ty),* }),*
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                type Model = $name;
                #[derive(serde::Deserialize)]
                #[serde(remote = "Model", tag = $tag, rename_all = "snake_case", deny_unknown_fields)]
                enum Wire {
                    $($variant { $($(#[$field_meta])* $field: $ty),* }),*
                }
                struct ObjectVisitor;
                impl<'de> serde::de::Visitor<'de> for ObjectVisitor {
                    type Value = $name;
                    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        f.write_str(concat!(stringify!($name), " object"))
                    }
                    fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<$name, A::Error> {
                        Wire::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    }
                }
                deserializer.deserialize_map(ObjectVisitor)
            }
        }
    };
}
pub(crate) use strict_tagged_object;

#[cfg(test)]
mod tests {
    use crate::checkpoint::RunStatus;
    use crate::checkpoint::tests::{context, fixture, usage};
    use crate::events::{
        DispatchState, EVENT_SCHEMA_VERSION, EventKind, PublicEvent, VerificationStatus,
    };
    use crate::form::FieldValue;
    use serde::Serialize;
    use serde::de::{DeserializeOwned, MapAccess, Visitor};
    use serde_json::Value;

    struct OrderedFields;

    impl<'de> Visitor<'de> for OrderedFields {
        type Value = Vec<(String, Value)>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("serialized object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut fields = Vec::new();
            while let Some(field) = map.next_entry()? {
                fields.push(field);
            }
            Ok(fields)
        }
    }

    fn rejects<T: DeserializeOwned>(raw: &str) {
        assert!(
            serde_json::from_str::<T>(raw).is_err(),
            "accepted malformed {}",
            std::any::type_name::<T>()
        );
    }

    fn strict<T: Serialize + DeserializeOwned>(original: &T) {
        let encoded = serde_json::to_string(original).unwrap();
        let object: Value = serde_json::from_str(&encoded).unwrap();
        let decoded: T = serde_json::from_str(&encoded).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), object);
        let fields = serde::Deserializer::deserialize_map(
            &mut serde_json::Deserializer::from_str(&encoded),
            OrderedFields,
        )
        .unwrap();
        // Preserve declaration/tag order: sorted maps can accidentally make a
        // positional-array regression fail on field types instead of root shape.
        let positional = Value::Array(fields.iter().map(|(_, v)| v.clone()).collect());
        rejects::<T>(&serde_json::to_string(&positional).unwrap());
        for raw in ["[]", "null", "false", "1", r#""object""#] {
            rejects::<T>(raw);
        }
        rejects::<T>(&format!("{encoded} {{}}"));
        rejects::<T>(&format!(
            "{{\"unexpected_audit_field\":true,{}",
            &encoded[1..]
        ));
        for (key, value) in fields {
            let mut missing = object.clone();
            missing.as_object_mut().unwrap().remove(&key);
            rejects::<T>(&missing.to_string());
            if !value.is_null()
                && !["form_selector", "name", "selected_value"].contains(&key.as_str())
            {
                let mut null = object.clone();
                null[&key] = Value::Null;
                rejects::<T>(&null.to_string());
            }
            let quoted = serde_json::to_string(&key).unwrap();
            for first in [value, Value::Null] {
                // Duplicate original bytes, never a map that erases duplicates.
                rejects::<T>(&format!("{{{quoted}:{first},{}", &encoded[1..]));
            }
        }
    }

    #[test]
    fn recovery_and_event_objects_reject_field_corruption_systematically() {
        let checkpoint = fixture();
        strict(&checkpoint);
        strict(&checkpoint.context);
        strict(&checkpoint.usage);
        let tab = &checkpoint.tabs[0];
        strict(tab);
        let document = &tab.documents[0];
        strict(document);
        strict(&document.form);
        let entry = &document.form.fields[0];
        strict(entry);
        strict(&entry.locator);
        for value in [
            FieldValue::Text {
                value: String::new(),
            },
            FieldValue::Checkbox { checked: false },
            FieldValue::RadioGroup {
                selected_value: None,
            },
            FieldValue::RadioGroup {
                selected_value: Some("雪".into()),
            },
            FieldValue::Select { values: vec![] },
        ] {
            strict(&value);
        }
        for kind in [
            EventKind::StatusChanged {
                status: RunStatus::Running,
            },
            EventKind::UsageUpdated { usage: usage() },
            EventKind::StepOutcome {
                step: 1,
                dispatch_state: DispatchState::Dispatched,
                verification_status: VerificationStatus::NotChecked,
            },
        ] {
            strict(&kind);
            strict(&PublicEvent {
                schema_version: EVENT_SCHEMA_VERSION,
                context: context(),
                sequence: 1,
                kind,
                evidence_ids: vec![],
                artifact_ids: vec![],
            });
        }
    }
}
