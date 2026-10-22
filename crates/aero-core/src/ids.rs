//! Opaque string identifiers scoped by their Rust types, not interchangeable aliases.
//!
//! A frame identifier cannot be replaced by a target identifier:
//!
//! ```compile_fail,E0308
//! use aero_core::ids::{FrameId, TargetId};
//! fn needs_frame(_: FrameId) {}
//! needs_frame(TargetId::from("same-wire-value"));
//! ```
//!
//! Runtime and document generations remain different identities:
//!
//! ```compile_fail,E0308
//! use aero_core::ids::{DocumentEpoch, RuntimeEpoch};
//! let runtime = RuntimeEpoch::from("generation-1");
//! let document: DocumentEpoch = runtime;
//! ```
//!
//! An observation is not a CDP session:
//!
//! ```compile_fail,E0308
//! use aero_core::ids::{CdpSessionId, ObservationId};
//! fn needs_session(_: CdpSessionId) {}
//! needs_session(ObservationId::from("same-wire-value"));
//! ```

// These wrappers preserve protocol bytes without normalization or ID generation.
// A typed value alone does not establish issuer, ownership, freshness or authority;
// the runtime/controller must check those boundaries before using it.
macro_rules! string_id {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Borrow the exact opaque wire value.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(TargetId, "A browser target, not a frame or CDP session.");
string_id!(FrameId, "A document frame in the runtime's frame graph.");
string_id!(
    CdpSessionId,
    "An attached CDP session used to route protocol messages."
);
string_id!(
    RuntimeEpoch,
    "An opaque generation of an owned browser runtime."
);
string_id!(
    DocumentEpoch,
    "An opaque generation of a particular document."
);
string_id!(
    ObservationId,
    "An observation snapshot that scopes its transient handles."
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::TypeId;
    use std::collections::HashSet;

    #[test]
    fn ids_are_distinct() {
        let ids = [
            TypeId::of::<TargetId>(),
            TypeId::of::<FrameId>(),
            TypeId::of::<CdpSessionId>(),
            TypeId::of::<RuntimeEpoch>(),
            TypeId::of::<DocumentEpoch>(),
            TypeId::of::<ObservationId>(),
        ];
        assert_eq!(ids.into_iter().collect::<HashSet<_>>().len(), ids.len());
    }

    #[test]
    fn ids_roundtrip_as_exact_strings() {
        let raw = "  opaque:雪/target-01  ";
        macro_rules! check {
            ($id:ty) => {{
                let id = <$id>::from(raw.to_owned());
                assert_eq!(id.as_str(), raw);
                let value = serde_json::to_value(&id).unwrap();
                assert_eq!(value, serde_json::Value::String(raw.into()));
                assert_eq!(serde_json::from_value::<$id>(value).unwrap(), id);
                assert_eq!(<$id>::from(raw), id);
            }};
        }
        check!(TargetId);
        check!(FrameId);
        check!(CdpSessionId);
        check!(RuntimeEpoch);
        check!(DocumentEpoch);
        check!(ObservationId);
    }

    #[test]
    fn ids_reject_nonstring_wire_values() {
        macro_rules! check {
            ($id:ty) => {
                for invalid in ["null", "1", "true", "[]", "{}"] {
                    assert!(serde_json::from_str::<$id>(invalid).is_err(), "{invalid}");
                }
            };
        }
        check!(TargetId);
        check!(FrameId);
        check!(CdpSessionId);
        check!(RuntimeEpoch);
        check!(DocumentEpoch);
        check!(ObservationId);
    }

    #[test]
    fn ids_are_usable_as_owned_map_keys() {
        let mut values = std::collections::HashMap::new();
        let id = TargetId::from("target-1");
        values.insert(id.clone(), "owned-target");
        assert_eq!(values.get(&id), Some(&"owned-target"));
        assert_eq!(values.get(&TargetId::from("different")), None);
    }
}
