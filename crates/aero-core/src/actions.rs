//! Strict planner proposals. Parsing and validation never grant execution authority.
//!
//! Deserialization checks shape and version; callers must also call `validate()`.
//! These are untrusted, mutable proposals, not executable capabilities. Runtime
//! ownership, handle freshness, origin/effect policy and verification are separate.
//! Ordinary serde errors and proposal Debug output may contain input data; neither
//! is a sanitized audit record. Only `ValidationError` uses payload-free messages.

use crate::ids::ObservationId;
use crate::wire::strict_tagged_object;
use serde::{Deserialize, Serialize};

/// The only supported decision wire version. Missing/other versions are rejected.
pub const DECISION_SCHEMA_VERSION: u32 = 1;

strict_tagged_object! {
    /// A proposed operation, not evidence that the operation is supported or authorized.
    #[derive(Debug)]
    pub enum Action {
        Click {
            target: String,
        },
        Type {
            target: String,
            text: String,
            clear: bool,
            press_enter: bool,
        },
        Select {
            target: String,
            values: Vec<String>,
        },
        SetChecked {
            target: String,
            checked: bool,
        },
        PressKey {
            key: String,
        },
        Scroll {
            target: Option<String>,
            delta_x: f64,
            delta_y: f64,
        },
        Navigate {
            url: String,
        },
        NewTab {
            url: String,
        },
        SwitchTab {
            tab: String,
        },
        CloseTab {
            tab: String,
        },
        FillCredentials {
            username_target: String,
            password_target: String,
        },
        FillTotp {
            target: String,
        },
        ExtractData {
            target: String,
            columns: Vec<String>,
            limit: u32,
            format: String,
        },
        DownloadFile {
            target: String,
        },
        BatchMetadata {
            urls: Vec<String>,
        },
        Wait {
            milliseconds: u32,
        },
        AskUser {
            question: String,
        },
        Complete {
            summary: String,
        },
    }
    tag "action";
}

/// One proposal tied to an observation; trusted task/run/role/grants live elsewhere.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decision {
    pub schema_version: u32,
    pub observation_id: ObservationId,
    /// A bounded public explanation, not hidden model reasoning.
    pub rationale: String,
    pub action: Action,
}

// A derived struct deserializer also accepts positional JSON arrays. The wire
// contract is an object, so implement only map visitation, keeping the same
// duplicate/unknown/missing-field strictness as the action deserializer.
impl<'de> Deserialize<'de> for Decision {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            SchemaVersion,
            ObservationId,
            Rationale,
            Action,
        }

        struct DecisionVisitor;

        impl<'de> serde::de::Visitor<'de> for DecisionVisitor {
            type Value = Decision;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a versioned decision object")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Decision, A::Error> {
                use serde::de::Error;

                let mut schema_version = None;
                let mut observation_id = None;
                let mut rationale = None;
                let mut action = None;
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::SchemaVersion => {
                            read_once(&mut map, &mut schema_version, "schema_version")?
                        }
                        Field::ObservationId => {
                            read_once(&mut map, &mut observation_id, "observation_id")?
                        }
                        Field::Rationale => read_once(&mut map, &mut rationale, "rationale")?,
                        Field::Action => read_once(&mut map, &mut action, "action")?,
                    }
                }
                let schema_version =
                    schema_version.ok_or_else(|| A::Error::missing_field("schema_version"))?;
                if schema_version != DECISION_SCHEMA_VERSION {
                    return Err(A::Error::custom("unsupported decision schema version"));
                }
                Ok(Decision {
                    schema_version,
                    observation_id: observation_id
                        .ok_or_else(|| A::Error::missing_field("observation_id"))?,
                    rationale: rationale.ok_or_else(|| A::Error::missing_field("rationale"))?,
                    action: action.ok_or_else(|| A::Error::missing_field("action"))?,
                })
            }
        }

        deserializer.deserialize_map(DecisionVisitor)
    }
}

fn read_once<'de, A, T>(
    map: &mut A,
    slot: &mut Option<T>,
    field: &'static str,
) -> Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
    T: Deserialize<'de>,
{
    if slot.is_some() {
        return Err(serde::de::Error::duplicate_field(field));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

impl Decision {
    /// Check version and parameter bounds, including proposals constructed in Rust.
    /// Success is neither authorization nor a freshness/completion guarantee.
    pub fn validate(&self) -> Result<(), ValidationError> {
        require(
            self.schema_version == DECISION_SCHEMA_VERSION,
            "schema_version",
            "unsupported decision schema version",
        )?;
        nonempty(self.observation_id.as_str(), "observation_id")?;
        require(
            self.rationale.chars().count() <= 1024,
            "rationale",
            "must not exceed 1024 Unicode scalar values",
        )?;
        self.action.validate()
    }
}

impl Action {
    /// Check only this action's parameter contract; no effects are performed.
    /// Validate the enclosing Decision too before passing it to trusted policy.
    pub fn validate(&self) -> Result<(), ValidationError> {
        // Handles remain opaque and unnormalized; nonempty does not prove issuance.
        match self {
            Self::Click { target }
            | Self::Type { target, .. }
            | Self::Select { target, .. }
            | Self::SetChecked { target, .. }
            | Self::FillTotp { target }
            | Self::ExtractData { target, .. }
            | Self::DownloadFile { target }
            | Self::Scroll {
                target: Some(target),
                ..
            } => nonempty(target, "action.target")?,
            Self::SwitchTab { tab } | Self::CloseTab { tab } => nonempty(tab, "action.tab")?,
            Self::FillCredentials {
                username_target,
                password_target,
            } => {
                nonempty(username_target, "action.username_target")?;
                nonempty(password_target, "action.password_target")?;
            }
            _ => {}
        }
        match self {
            Self::Type { text, .. } => require(
                text.len() <= 16 * 1024,
                "action.text",
                "must not exceed 16 KiB of UTF-8",
            ),
            Self::PressKey { key } => require(
                NAMED_KEYS.contains(&key.as_str()),
                "action.key",
                "must be an allowed named key",
            ),
            Self::Scroll {
                delta_x, delta_y, ..
            } => {
                scroll_delta(*delta_x, "action.delta_x")?;
                scroll_delta(*delta_y, "action.delta_y")
            }
            Self::Navigate { url } | Self::NewTab { url } => require(
                url == "about:blank" || has_http_url_shape(url),
                "action.url",
                "must be an absolute HTTP(S) URL or exactly about:blank",
            ),
            Self::ExtractData { limit, format, .. } => {
                require(
                    (1..=10000).contains(limit),
                    "action.limit",
                    "must be between 1 and 10000",
                )?;
                require(
                    matches!(format.as_str(), "csv" | "parquet"),
                    "action.format",
                    "must be csv or parquet; exporter availability is checked separately",
                )
            }
            Self::BatchMetadata { urls } => {
                require(
                    (1..=64).contains(&urls.len()),
                    "action.urls",
                    "must contain between 1 and 64 URLs",
                )?;
                require(
                    urls.iter().all(|url| has_http_url_shape(url)),
                    "action.urls",
                    "every entry must be an absolute HTTP(S) URL",
                )
            }
            Self::Wait { milliseconds } => require(
                *milliseconds <= 5000,
                "action.milliseconds",
                "must not exceed 5000 ms",
            ),
            Self::Complete { summary } => require(
                summary.chars().count() <= 8192,
                "action.summary",
                "must not exceed 8192 Unicode scalar values",
            ),
            _ => Ok(()),
        }
    }
}

const NAMED_KEYS: &[&str] = &[
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
];

fn require(valid: bool, field: &'static str, message: &'static str) -> Result<(), ValidationError> {
    if valid {
        Ok(())
    } else {
        Err(ValidationError { field, message })
    }
}

fn nonempty(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require(!value.is_empty(), field, "must not be empty")
}

fn scroll_delta(value: f64, field: &'static str) -> Result<(), ValidationError> {
    require(
        value.is_finite() && (-4000.0..=4000.0).contains(&value),
        field,
        "must be finite and between -4000 and 4000",
    )
}

// Conservative syntax only, not WHATWG parsing, IDNA conversion, DNS, origin,
// redirect, SSRF or network policy. Accept ASCII DNS/canonical IPv4/bracketed IPv6
// hosts, decimal u16 ports and well-formed percent escapes; reject userinfo and
// browser-repair forms. Unicode paths are preserved; Unicode hosts need an
// already encoded ASCII IDNA form. The future adapter must still parse/resolve
// and policy-check the effective destination. Never normalize these input bytes.
fn has_http_url_shape(url: &str) -> bool {
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return false;
    }
    let mut bytes = url.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%'
            && !(bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                && bytes.next().is_some_and(|b| b.is_ascii_hexdigit()))
        {
            return false;
        }
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return false;
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((address, suffix)) = bracketed.split_once(']') else {
            return false;
        };
        address.parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port))
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)));
        valid_ascii_host(host) && port.is_none_or(valid_port)
    }
}

fn valid_port(port: &str) -> bool {
    !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

fn valid_ascii_host(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return false;
    }
    let last = host.rsplit('.').next().unwrap_or_default();
    // Reject browser IPv4 shortcuts, octal/hex forms and malformed numeric hosts
    // instead of interpreting them as innocent DNS names for later policy.
    let ends_in_number = last.bytes().all(|b| b.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .or_else(|| last.strip_prefix("0X"))
            .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()));
    if ends_in_number {
        return host.parse::<std::net::Ipv4Addr>().is_ok();
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// A field and static reason only: never embeds proposal text, URLs or secrets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationError {
    field: &'static str,
    message: &'static str,
}

impl ValidationError {
    /// Contract field path, not a model-supplied string.
    pub fn field(&self) -> &'static str {
        self.field
    }

    /// Static reason suitable for reporting without echoing the rejected value.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ObservationId;
    use serde_json::{Value, json};

    fn decision(action: Action) -> Decision {
        Decision {
            schema_version: DECISION_SCHEMA_VERSION,
            observation_id: ObservationId::from("obs-1"),
            rationale: "Public explanation".into(),
            action,
        }
    }

    fn click_json() -> Value {
        json!({
            "schema_version": 1,
            "observation_id": "obs-1",
            "rationale": "Open result",
            "action": {"action": "click", "target": "@e0"}
        })
    }

    fn assert_invalid(action: Action, field: &str) {
        let error = decision(action).validate().expect_err("invalid proposal");
        assert_eq!(error.field(), field);
    }

    #[test]
    fn decision_is_strict_and_round_trips() {
        let raw = r#"{"schema_version":1,"observation_id":"obs-1","rationale":"Open result","action":{"action":"click","target":"@e0"}}"#;
        let parsed: Decision = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.schema_version, DECISION_SCHEMA_VERSION);
        assert_eq!(parsed.observation_id, ObservationId::from("obs-1"));
        assert_eq!(
            parsed.action,
            Action::Click {
                target: "@e0".into()
            }
        );
        assert_eq!(parsed.validate(), Ok(()));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), click_json());
        let encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(serde_json::from_str::<Decision>(&encoded).unwrap(), parsed);

        for extra in [
            "role",
            "task_id",
            "run_id",
            "contract_revision",
            "grant",
            "reasoning",
        ] {
            let mut value = click_json();
            value[extra] = json!("untrusted");
            assert!(
                serde_json::from_value::<Decision>(value).is_err(),
                "{extra}"
            );
        }
        let mut value = click_json();
        value["action"]["javascript"] = json!("alert(1)");
        assert!(serde_json::from_value::<Decision>(value).is_err());
    }

    #[test]
    fn decision_all_actions_round_trip_without_defaults() {
        let actions = [
            json!({"action":"click","target":"@e0"}),
            json!({"action":"type","target":"@e0","text":"雪","clear":false,"press_enter":false}),
            json!({"action":"select","target":"@e0","values":["", "雪"]}),
            json!({"action":"set_checked","target":"@e0","checked":false}),
            json!({"action":"press_key","key":"Enter"}),
            json!({"action":"scroll","target":null,"delta_x":-0.5,"delta_y":1.25}),
            json!({"action":"navigate","url":"https://example.test/path?q=1#part"}),
            json!({"action":"new_tab","url":"about:blank"}),
            json!({"action":"switch_tab","tab":"tab-1"}),
            json!({"action":"close_tab","tab":"tab-1"}),
            json!({"action":"fill_credentials","username_target":"@e0","password_target":"@e1"}),
            json!({"action":"fill_totp","target":"@e0"}),
            json!({"action":"extract_data","target":"@e0","columns":["name","price"],"limit":1,"format":"csv"}),
            json!({"action":"download_file","target":"@e0"}),
            json!({"action":"batch_metadata","urls":["http://example.test/"]}),
            json!({"action":"wait","milliseconds":0}),
            json!({"action":"ask_user","question":"Which supplier?"}),
            json!({"action":"complete","summary":"Proposed; not verified."}),
        ];
        for action in actions {
            let mut value = click_json();
            value["action"] = action;
            let parsed: Decision = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(parsed.validate(), Ok(()));
            assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
            value["action"]["output_path"] = json!("/untrusted/path");
            assert!(serde_json::from_value::<Decision>(value).is_err());
        }
        for action in ["fill_credentials", "fill_totp"] {
            let mut value = click_json();
            value["action"] = if action == "fill_credentials" {
                json!({"action":action,"username_target":"@e0","password_target":"@e1","auto_submit":true})
            } else {
                json!({"action":action,"target":"@e0","auto_submit":true})
            };
            assert!(serde_json::from_value::<Decision>(value).is_err());
        }
    }

    #[test]
    fn decision_rejects_unknown_action() {
        for action in [
            "execute_javascript",
            "raw_cdp",
            "shell",
            "approve",
            "CLICK",
            "",
        ] {
            let mut value = click_json();
            value["action"] = json!({"action": action});
            assert!(serde_json::from_value::<Decision>(value).is_err());
        }
    }

    #[test]
    fn decision_rejects_missing_wrong_and_duplicate_fields() {
        for field in ["schema_version", "observation_id", "rationale", "action"] {
            let mut value = click_json();
            value.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Decision>(value).is_err(),
                "{field}"
            );
        }
        for (field, wrong) in [
            ("schema_version", json!("1")),
            ("schema_version", json!(1.0)),
            ("schema_version", json!(-1)),
            ("schema_version", json!(null)),
            ("observation_id", json!(1)),
            ("rationale", json!(null)),
            ("action", json!([])),
        ] {
            let mut value = click_json();
            value[field] = wrong;
            assert!(
                serde_json::from_value::<Decision>(value).is_err(),
                "{field}"
            );
        }
        for action in [
            json!({"action":"click"}),
            json!({"action":"click","target":null}),
            json!({"action":"type","target":"@e0","text":"x","clear":false}),
            json!({"action":"type","target":"@e0","text":"x","press_enter":false}),
            json!({"action":"type","target":"@e0","text":"x","clear":"false","press_enter":false}),
            json!({"action":"wait","milliseconds":-1}),
            json!({"action":"scroll","delta_x":null,"delta_y":1}),
        ] {
            let mut value = click_json();
            value["action"] = action;
            assert!(serde_json::from_value::<Decision>(value).is_err());
        }
        for raw in [
            r#"{"schema_version":1,"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"click","target":"@e0"}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","observation_id":"obs-2","rationale":"x","action":{"action":"click","target":"@e0"}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","rationale":"y","action":{"action":"click","target":"@e0"}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"click","target":"@e0"},"action":{"action":"wait","milliseconds":0}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"click","target":"@e0","target":"@e1"}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"click","action":"click","target":"@e0"}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"scroll","target":null,"target":"@e0","delta_x":0,"delta_y":1}}"#,
            r#"{"schema_version":1,"observation_id":"obs-1","rationale":"x","action":{"action":"click","target":"@e0"}} {}"#,
        ] {
            assert!(serde_json::from_str::<Decision>(raw).is_err());
        }
    }

    #[test]
    fn decision_requires_an_object_envelope() {
        for raw in [
            "null",
            "true",
            "1",
            r#""decision""#,
            "[]",
            r#"[1,"obs-1","Public explanation",{"action":"click","target":"@e0"}]"#,
        ] {
            assert!(serde_json::from_str::<Decision>(raw).is_err());
        }
    }

    #[test]
    fn action_requires_an_object_for_every_variant() {
        for action in positional_actions() {
            let raw = serde_json::to_string(&action).unwrap();
            assert!(
                serde_json::from_str::<Action>(&raw).is_err(),
                "accepted positional Action: {raw}"
            );
        }
        for raw in ["null", "true", "1", r#""click""#, "[]"] {
            assert!(serde_json::from_str::<Action>(raw).is_err());
        }
    }

    #[test]
    fn decision_rejects_positional_action_arrays() {
        for action in positional_actions() {
            let mut value = click_json();
            value["action"] = action;
            let raw = serde_json::to_string(&value).unwrap();
            assert!(
                serde_json::from_str::<Decision>(&raw).is_err(),
                "accepted nested positional Action: {raw}"
            );
        }
    }

    fn positional_actions() -> [serde_json::Value; 18] {
        [
            json!(["click", "@e0"]),
            json!(["type", "@e0", "雪", false, false]),
            json!(["select", "@e0", ["", "雪"]]),
            json!(["set_checked", "@e0", false]),
            json!(["press_key", "Enter"]),
            json!(["scroll", null, -0.5, 1.25]),
            json!(["navigate", "https://example.test/"]),
            json!(["new_tab", "about:blank"]),
            json!(["switch_tab", "tab-1"]),
            json!(["close_tab", "tab-1"]),
            json!(["fill_credentials", "@e0", "@e1"]),
            json!(["fill_totp", "@e0"]),
            json!(["extract_data", "@e0", ["name", "price"], 1, "csv"]),
            json!(["download_file", "@e0"]),
            json!(["batch_metadata", ["http://example.test/"]]),
            json!(["wait", 0]),
            json!(["ask_user", "Which supplier?"]),
            json!(["complete", "Proposed; not verified."]),
        ]
    }

    #[test]
    fn decision_scroll_target_is_optional() {
        let mut value = click_json();
        value["action"] = json!({"action":"scroll","delta_x":0,"delta_y":1});
        let parsed: Decision = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.validate(), Ok(()));
        assert_eq!(
            parsed.action,
            Action::Scroll {
                target: None,
                delta_x: 0.0,
                delta_y: 1.0
            }
        );
    }

    #[test]
    fn decision_rejects_unsupported_versions() {
        for version in [0, 2, u32::MAX] {
            let mut value = click_json();
            value["schema_version"] = json!(version);
            assert!(serde_json::from_value::<Decision>(value).is_err());
            let mut proposal = decision(Action::Wait { milliseconds: 0 });
            proposal.schema_version = version;
            assert_eq!(proposal.validate().unwrap_err().field(), "schema_version");
        }
    }

    #[test]
    fn decision_rejects_limits() {
        assert_invalid(
            Action::Type {
                target: "@e0".into(),
                text: "x".repeat(16 * 1024 + 1),
                clear: false,
                press_enter: false,
            },
            "action.text",
        );
        // Text limits count UTF-8 bytes, not Unicode scalar values.
        assert_invalid(
            Action::Type {
                target: "@e0".into(),
                text: "雪".repeat(16 * 1024 / 3 + 1),
                clear: true,
                press_enter: false,
            },
            "action.text",
        );
        assert_invalid(Action::Wait { milliseconds: 5001 }, "action.milliseconds");
        for limit in [0, 10001, u32::MAX] {
            assert_invalid(
                Action::ExtractData {
                    target: "@e0".into(),
                    columns: vec!["name".into()],
                    limit,
                    format: "csv".into(),
                },
                "action.limit",
            );
        }
        for urls in [vec![], vec!["https://example.test/".into(); 65]] {
            assert_invalid(Action::BatchMetadata { urls }, "action.urls");
        }
        let mut proposal = decision(Action::Wait { milliseconds: 0 });
        proposal.rationale = "雪".repeat(1025);
        assert_eq!(proposal.validate().unwrap_err().field(), "rationale");
        assert_invalid(
            Action::Complete {
                summary: "雪".repeat(8193),
            },
            "action.summary",
        );
        for format in ["", "CSV", "json", "/tmp/output.csv"] {
            assert_invalid(
                Action::ExtractData {
                    target: "@e0".into(),
                    columns: vec![],
                    limit: 1,
                    format: format.into(),
                },
                "action.format",
            );
        }
    }

    #[test]
    fn decision_accepts_exact_limits() {
        for text in [
            String::new(),
            "x".repeat(16 * 1024),
            "雪".repeat(16 * 1024 / 3),
        ] {
            assert_eq!(
                decision(Action::Type {
                    target: "@e0".into(),
                    text,
                    clear: false,
                    press_enter: true,
                })
                .validate(),
                Ok(())
            );
        }
        assert_eq!(
            decision(Action::Wait { milliseconds: 5000 }).validate(),
            Ok(())
        );
        for limit in [1, 10000] {
            for format in ["csv", "parquet"] {
                assert_eq!(
                    decision(Action::ExtractData {
                        target: "@e0".into(),
                        columns: vec![],
                        limit,
                        format: format.into(),
                    })
                    .validate(),
                    Ok(())
                );
            }
        }
        for count in [1, 64] {
            assert_eq!(
                decision(Action::BatchMetadata {
                    urls: vec!["https://example.test/".into(); count],
                })
                .validate(),
                Ok(())
            );
        }
        let mut proposal = decision(Action::Complete {
            summary: "雪".repeat(8192),
        });
        proposal.rationale = "雪".repeat(1024);
        assert_eq!(proposal.validate(), Ok(()));
    }

    #[test]
    fn decision_rejects_nonfinite_and_out_of_range_scroll() {
        for delta in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -4000.001,
            4000.001,
        ] {
            assert_invalid(
                Action::Scroll {
                    target: None,
                    delta_x: delta,
                    delta_y: 0.0,
                },
                "action.delta_x",
            );
            assert_invalid(
                Action::Scroll {
                    target: Some("@e0".into()),
                    delta_x: 0.0,
                    delta_y: delta,
                },
                "action.delta_y",
            );
        }
        for delta in [-4000.0, -0.5, 0.0, 0.5, 4000.0] {
            assert_eq!(
                decision(Action::Scroll {
                    target: None,
                    delta_x: delta,
                    delta_y: -delta
                })
                .validate(),
                Ok(())
            );
        }
    }

    #[test]
    fn decision_checks_urls_in_every_url_action() {
        for url in [
            "http://example.test",
            "https://example.test/path?q=1#part",
            "HTTPS://example.test/",
            "https://localhost:8443/",
            "http://127.0.0.1/",
            "http://[::1]:8080/",
            "https://example.test/雪",
            "https://example.test./escaped%20path?q=%E9%9B%AA",
            "http://[2001:db8::1]:65535/",
            "http://localhost:0/",
        ] {
            for action in [
                Action::Navigate { url: url.into() },
                Action::NewTab { url: url.into() },
                Action::BatchMetadata {
                    urls: vec![url.into()],
                },
            ] {
                assert_eq!(decision(action).validate(), Ok(()));
            }
        }
        for action in [
            Action::Navigate {
                url: "about:blank".into(),
            },
            Action::NewTab {
                url: "about:blank".into(),
            },
        ] {
            assert_eq!(decision(action).validate(), Ok(()));
        }
        for url in [
            "",
            "/relative",
            "//example.test",
            "https:example.test",
            "https://",
            "https:///path",
            "https://?query",
            "https://#fragment",
            "https://user@/",
            "https://:443/",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "ftp://example.test/",
            "about:config",
            "about:blank#fragment",
            " about:blank",
            "https://example.test/\n",
            "https://example.test/a b",
            "https://example.test/\u{007f}",
            "https://example.test\\@other.test/",
        ] {
            assert_invalid(Action::Navigate { url: url.into() }, "action.url");
            assert_invalid(Action::NewTab { url: url.into() }, "action.url");
            assert_invalid(
                Action::BatchMetadata {
                    urls: vec!["https://example.test/".into(), url.into()],
                },
                "action.urls",
            );
        }
        assert_invalid(
            Action::BatchMetadata {
                urls: vec!["about:blank".into()],
            },
            "action.urls",
        );
    }

    #[test]
    fn decision_rejects_malformed_url_syntax() {
        let mut invalid_urls = vec![
            "https://user@example.test/".to_owned(),
            "https://user:synthetic@example.test/".to_owned(),
            "https://example.test:65536/".to_owned(),
            "https://example.test:/".to_owned(),
            "https://example.test:-1/".to_owned(),
            "https://example.test:+443/".to_owned(),
            "https://example.test:port/".to_owned(),
            "https://[not-ipv6]/".to_owned(),
            "https://[::1/".to_owned(),
            "https://[::1]junk/".to_owned(),
            "https://[::1]:65536/".to_owned(),
            "https://[::1]:/".to_owned(),
            "https://::1/".to_owned(),
            "https://[]/".to_owned(),
            "https://[fe80::1%25eth0]/".to_owned(),
            "https://example..test/".to_owned(),
            "https://-example.test/".to_owned(),
            "https://example-.test/".to_owned(),
            "https://example_test/".to_owned(),
            "https://例え.test/".to_owned(),
            "https://%65xample.test/".to_owned(),
            "https://127.0.0.999/".to_owned(),
            "https://127.1/".to_owned(),
            "https://0x7f000001/".to_owned(),
            "https://example.test/%".to_owned(),
            "https://example.test/%0".to_owned(),
            "https://example.test/%xz".to_owned(),
            "https://example.test/?q=%GG".to_owned(),
            "https://example.test/#%".to_owned(),
        ];
        invalid_urls.push(format!("https://{}.test/", "a".repeat(64)));
        invalid_urls.push(format!("https://{}/", vec!["a".repeat(63); 5].join(".")));
        let mut accepted = Vec::new();
        for (index, url) in invalid_urls.iter().enumerate() {
            for action in [
                Action::Navigate { url: url.clone() },
                Action::NewTab { url: url.clone() },
                Action::BatchMetadata {
                    urls: vec![url.clone()],
                },
            ] {
                if decision(action).validate().is_ok() {
                    accepted.push(index);
                }
            }
        }
        assert!(
            accepted.is_empty(),
            "accepted malformed URL case indexes: {accepted:?}"
        );
    }

    #[test]
    fn decision_allows_only_named_keys() {
        for key in [
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
            assert_eq!(
                decision(Action::PressKey { key: key.into() }).validate(),
                Ok(())
            );
        }
        for key in [
            "",
            "enter",
            "Return",
            "a",
            "\n",
            "Control+A",
            "Alt+F4",
            "F12",
            "Enter ",
        ] {
            assert_invalid(Action::PressKey { key: key.into() }, "action.key");
        }
    }

    #[test]
    fn decision_requires_nonempty_opaque_references() {
        let mut proposal = decision(Action::Wait { milliseconds: 0 });
        proposal.observation_id = ObservationId::from("");
        assert_eq!(proposal.validate().unwrap_err().field(), "observation_id");
        for action in [
            Action::Click { target: "".into() },
            Action::Type {
                target: "".into(),
                text: "".into(),
                clear: false,
                press_enter: false,
            },
            Action::Select {
                target: "".into(),
                values: vec![],
            },
            Action::SetChecked {
                target: "".into(),
                checked: false,
            },
            Action::Scroll {
                target: Some("".into()),
                delta_x: 0.0,
                delta_y: 0.0,
            },
            Action::FillTotp { target: "".into() },
            Action::ExtractData {
                target: "".into(),
                columns: vec![],
                limit: 1,
                format: "csv".into(),
            },
            Action::DownloadFile { target: "".into() },
        ] {
            assert_invalid(action, "action.target");
        }
        assert_invalid(
            Action::FillCredentials {
                username_target: "".into(),
                password_target: "@e1".into(),
            },
            "action.username_target",
        );
        assert_invalid(
            Action::FillCredentials {
                username_target: "@e0".into(),
                password_target: "".into(),
            },
            "action.password_target",
        );
        assert_invalid(Action::SwitchTab { tab: "".into() }, "action.tab");
        assert_invalid(Action::CloseTab { tab: "".into() }, "action.tab");
        let mut proposal = decision(Action::Click {
            target: " opaque:雪/@e0 ".into(),
        });
        proposal.observation_id = ObservationId::from(" opaque:雪/obs ");
        assert_eq!(proposal.validate(), Ok(()));
        let encoded = serde_json::to_string(&proposal).unwrap();
        assert_eq!(
            serde_json::from_str::<Decision>(&encoded).unwrap(),
            proposal
        );
    }

    #[test]
    fn decision_validation_errors_do_not_echo_payloads() {
        let sentinel = "SYNTHETIC_PRIVATE_SENTINEL";
        let proposal = decision(Action::Navigate {
            url: format!("file:///{sentinel}"),
        });
        let error = proposal.validate().unwrap_err();
        assert_eq!(error.field(), "action.url");
        assert!(!error.message().is_empty());
        assert!(!error.to_string().contains(sentinel));
        assert!(!format!("{error:?}").contains(sentinel));
        let _: &dyn std::error::Error = &error;
    }
}
