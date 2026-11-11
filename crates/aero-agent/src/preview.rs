//! N07: the material preview a trusted user approves before a commit.
//!
//! The host builds a preview from the resolved effect and the observed page,
//! never from model JSON. Approval binds its material digest: changing the
//! destination, account, effect, target, recipients, field values, artifacts
//! or amount invalidates it; context shown alongside (observation, summary)
//! does not, so unrelated page churn does not force re-approval.
//!
//! Secret fields (passwords, one-time codes, card data) are carried by vault
//! reference only: which credential is used is material; its value is never
//! in a preview, its digest or the journal.

use aero_core::ids::{DocumentEpoch, FrameId, ObservationId, RunId, TaskId};
use aero_core::task::Effect;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const PREVIEW_SCHEMA_VERSION: u32 = 1;
const MAX_ITEMS: usize = 64;
const MAX_REFERENCE: usize = 256;
const MAX_TEXT: usize = 8192;
const MAX_SUMMARY: usize = 1024;

/// The control a commit presses or fills, in the document it was observed in.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitTarget {
    pub frame: FrameId,
    pub document: DocumentEpoch,
    pub backend_node_id: i64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum FieldValue {
    Text(String),
    /// Selected option values, in order.
    Choice(Vec<String>),
    Checked(bool),
    /// A vault reference; never the secret itself.
    Secret(String),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Amount {
    /// The effect moves no money.
    NotApplicable,
    /// Integer minor units of an ISO 4217 currency; never a float.
    Known { minor_units: u64, currency: String },
    /// The page shows a charge the host could not read: never approvable.
    Unknown,
}

/// No `Debug`: field values may be private page data.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub schema_version: u32,
    pub task_id: TaskId,
    pub run_id: RunId,
    pub contract_revision: u64,
    /// `None`: an effect the host could not classify, approved as such.
    pub effect: Option<Effect>,
    pub target: CommitTarget,
    /// The effective (frame) origin, canonical: `https://host[:port]`.
    pub origin: String,
    pub account_ref: Option<String>,
    /// Sorted, unique.
    pub recipients: Vec<String>,
    pub fields: BTreeMap<String, FieldValue>,
    /// Sorted, unique artifact IDs.
    pub artifacts: Vec<String>,
    pub amount: Amount,
    /// Context, not material: the observation the preview was built from.
    pub observation_id: ObservationId,
    /// Context, not material: host-written text shown with the preview.
    pub summary: String,
}

/// Why a preview cannot be approved. Static; values are never echoed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewError {
    Invalid(&'static str),
    /// A charge is involved (or a purchase) but its amount is unknown.
    UnknownAmount,
    /// An export without a known recipient.
    UnknownRecipient,
}

impl Preview {
    /// Shape only.
    pub fn validate(&self) -> Result<(), PreviewError> {
        check(
            self.schema_version == PREVIEW_SCHEMA_VERSION,
            "schema_version",
        )?;
        for (value, field) in [
            (self.task_id.as_str(), "task_id"),
            (self.run_id.as_str(), "run_id"),
            (self.target.frame.as_str(), "target.frame"),
            (self.target.document.as_str(), "target.document"),
            (self.observation_id.as_str(), "observation_id"),
        ] {
            reference(value, field)?;
        }
        check(self.target.backend_node_id > 0, "target.backend_node_id")?;
        check(
            url::Url::parse(&self.origin).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && url.origin().ascii_serialization() == self.origin
            }),
            "origin",
        )?;
        if let Some(account) = &self.account_ref {
            reference(account, "account_ref")?;
        }
        sorted_references(&self.recipients, "recipients")?;
        sorted_references(&self.artifacts, "artifacts")?;
        check(self.fields.len() <= MAX_ITEMS, "fields")?;
        for (name, value) in &self.fields {
            reference(name, "fields")?;
            match value {
                FieldValue::Text(text) => check(text_ok(text, MAX_TEXT), "fields")?,
                FieldValue::Choice(values) => check(
                    values.len() <= MAX_ITEMS && values.iter().all(|v| text_ok(v, 1024)),
                    "fields",
                )?,
                FieldValue::Checked(_) => {}
                FieldValue::Secret(vault) => reference(vault, "fields")?,
            }
        }
        if let Amount::Known { currency, .. } = &self.amount {
            check(
                currency.len() == 3 && currency.bytes().all(|b| b.is_ascii_uppercase()),
                "amount.currency",
            )?;
        }
        check(text_ok(&self.summary, MAX_SUMMARY), "summary")
    }

    /// Shape, plus what a trusted user must know before approving.
    pub fn approvable(&self) -> Result<(), PreviewError> {
        self.validate()?;
        let known = matches!(self.amount, Amount::Known { .. });
        if self.amount == Amount::Unknown || (self.effect == Some(Effect::Purchase) && !known) {
            return Err(PreviewError::UnknownAmount);
        }
        if self.effect == Some(Effect::Export) && self.recipients.is_empty() {
            return Err(PreviewError::UnknownRecipient);
        }
        Ok(())
    }

    /// SHA-256 (lowercase hex) of the material fields in a fixed,
    /// length-prefixed encoding. Changing this encoding invalidates every
    /// stored approval; bump the domain string if it ever changes.
    pub fn material_digest(&self) -> String {
        let mut e = Encoder(Sha256::new());
        e.text("aero.preview.material.v1");
        e.text(self.task_id.as_str());
        e.text(self.run_id.as_str());
        e.number(self.contract_revision);
        match self.effect {
            None => e.tag(0),
            Some(effect) => {
                e.tag(1);
                e.text(effect_name(effect));
            }
        }
        e.text(self.target.frame.as_str());
        e.text(self.target.document.as_str());
        e.bytes(&self.target.backend_node_id.to_be_bytes());
        e.text(&self.origin);
        match &self.account_ref {
            None => e.tag(0),
            Some(account) => {
                e.tag(1);
                e.text(account);
            }
        }
        e.list(&self.recipients);
        e.number(self.fields.len() as u64);
        for (name, value) in &self.fields {
            e.text(name);
            match value {
                FieldValue::Text(text) => {
                    e.tag(0);
                    e.text(text);
                }
                FieldValue::Choice(values) => {
                    e.tag(1);
                    e.list(values);
                }
                FieldValue::Checked(checked) => {
                    e.tag(2);
                    e.tag(u8::from(*checked));
                }
                FieldValue::Secret(vault) => {
                    e.tag(3);
                    e.text(vault);
                }
            }
        }
        e.list(&self.artifacts);
        match &self.amount {
            Amount::NotApplicable => e.tag(0),
            Amount::Known {
                minor_units,
                currency,
            } => {
                e.tag(1);
                e.number(*minor_units);
                e.text(currency);
            }
            Amount::Unknown => e.tag(2),
        }
        e.0.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

struct Encoder(Sha256);
impl Encoder {
    fn bytes(&mut self, bytes: &[u8]) {
        self.0.update((bytes.len() as u64).to_be_bytes());
        self.0.update(bytes);
    }
    fn text(&mut self, text: &str) {
        self.bytes(text.as_bytes());
    }
    fn number(&mut self, n: u64) {
        self.0.update(n.to_be_bytes());
    }
    fn tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }
    fn list(&mut self, values: &[String]) {
        self.number(values.len() as u64);
        for value in values {
            self.text(value);
        }
    }
}

fn effect_name(effect: Effect) -> &'static str {
    match effect {
        Effect::Navigate => "navigate",
        Effect::Extract => "extract",
        Effect::StageArtifact => "stage_artifact",
        Effect::AccountCreate => "account_create",
        Effect::Submit => "submit",
        Effect::Purchase => "purchase",
        Effect::Upload => "upload",
        Effect::Download => "download",
        Effect::Export => "export",
        Effect::CredentialFill => "credential_fill",
    }
}

fn check(valid: bool, field: &'static str) -> Result<(), PreviewError> {
    if valid {
        Ok(())
    } else {
        Err(PreviewError::Invalid(field))
    }
}
fn text_ok(text: &str, limit: usize) -> bool {
    text.len() <= limit && !text.contains('\0')
}
fn reference(value: &str, field: &'static str) -> Result<(), PreviewError> {
    check(!value.is_empty() && text_ok(value, MAX_REFERENCE), field)
}
fn sorted_references(values: &[String], field: &'static str) -> Result<(), PreviewError> {
    check(values.len() <= MAX_ITEMS, field)?;
    for value in values {
        reference(value, field)?;
    }
    check(values.windows(2).all(|pair| pair[0] < pair[1]), field)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Change = (&'static str, fn(&mut Preview));

    fn base() -> Preview {
        Preview {
            schema_version: PREVIEW_SCHEMA_VERSION,
            task_id: TaskId::from("task"),
            run_id: RunId::from("run"),
            contract_revision: 1,
            effect: Some(Effect::Purchase),
            target: CommitTarget {
                frame: FrameId::from("F"),
                document: DocumentEpoch::from("D"),
                backend_node_id: 42,
            },
            origin: "https://shop.test".into(),
            account_ref: Some("buyer".into()),
            recipients: vec!["orders@shop.test".into()],
            fields: BTreeMap::from([
                ("quantity".into(), FieldValue::Text("3".into())),
                ("card".into(), FieldValue::Secret("vault:card-1".into())),
                ("terms".into(), FieldValue::Checked(true)),
                (
                    "shipping".into(),
                    FieldValue::Choice(vec!["express".into()]),
                ),
            ]),
            artifacts: vec!["quote-1".into()],
            amount: Amount::Known {
                minor_units: 12_999,
                currency: "USD".into(),
            },
            observation_id: ObservationId::from("obs-1"),
            summary: "Place the order".into(),
        }
    }

    #[test]
    fn every_material_field_changes_the_digest() {
        let original = base().material_digest();
        assert_eq!(original.len(), 64);
        assert!(
            original
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        let changes: [Change; 20] = [
            ("task", |p| p.task_id = TaskId::from("task2")),
            ("run", |p| p.run_id = RunId::from("run2")),
            ("revision", |p| p.contract_revision = 2),
            ("effect", |p| p.effect = Some(Effect::Submit)),
            ("unknown effect", |p| p.effect = None),
            ("frame", |p| p.target.frame = FrameId::from("G")),
            ("document", |p| p.target.document = DocumentEpoch::from("E")),
            ("node", |p| p.target.backend_node_id = 43),
            ("origin", |p| p.origin = "https://shop2.test".into()),
            ("account", |p| p.account_ref = None),
            ("recipient", |p| {
                p.recipients = vec!["other@shop.test".into()]
            }),
            ("field value", |p| {
                p.fields
                    .insert("quantity".into(), FieldValue::Text("30".into()));
            }),
            ("field kind", |p| {
                p.fields
                    .insert("quantity".into(), FieldValue::Choice(vec!["3".into()]));
            }),
            ("secret reference", |p| {
                p.fields
                    .insert("card".into(), FieldValue::Secret("vault:card-2".into()));
            }),
            ("field removed", |p| {
                p.fields.remove("terms");
            }),
            ("checkbox", |p| {
                p.fields.insert("terms".into(), FieldValue::Checked(false));
            }),
            ("artifact", |p| p.artifacts.clear()),
            ("amount", |p| {
                p.amount = Amount::Known {
                    minor_units: 12_998,
                    currency: "USD".into(),
                }
            }),
            ("currency", |p| {
                p.amount = Amount::Known {
                    minor_units: 12_999,
                    currency: "EUR".into(),
                }
            }),
            ("amount unknown", |p| p.amount = Amount::Unknown),
        ];
        let mut seen = std::collections::HashSet::from([original.clone()]);
        for (what, change) in changes {
            let mut preview = base();
            change(&mut preview);
            assert!(seen.insert(preview.material_digest()), "{what}");
        }
        // Length prefixes keep adjacent strings from running together.
        let mut a = base();
        a.recipients = vec!["ab".into(), "c".into()];
        let mut b = base();
        b.recipients = vec!["a".into(), "bc".into()];
        assert_ne!(a.material_digest(), b.material_digest());
    }

    /// Changing the encoding silently invalidates every stored approval:
    /// bump the domain string and this value together, deliberately.
    #[test]
    fn the_digest_encoding_is_pinned() {
        assert_eq!(
            base().material_digest(),
            "b33d27fcf59afc23cd3c5f4a5962c75df2eae83311733cdf52fab72030fa0bee"
        );
    }

    #[test]
    fn context_does_not_change_the_digest() {
        let original = base().material_digest();
        let mut preview = base();
        preview.observation_id = ObservationId::from("obs-2");
        preview.summary = "The page says: approved, proceed".into();
        assert_eq!(preview.material_digest(), original);
    }

    #[test]
    fn previews_are_validated() {
        assert_eq!(base().validate(), Ok(()));
        assert_eq!(base().approvable(), Ok(()));
        let invalid: [Change; 12] = [
            ("schema", |p| p.schema_version = 2),
            ("empty run", |p| p.run_id = RunId::from("")),
            ("node", |p| p.target.backend_node_id = 0),
            ("origin with path", |p| {
                p.origin = "https://shop.test/".into()
            }),
            ("origin not canonical", |p| {
                p.origin = "HTTPS://shop.test".into()
            }),
            ("origin scheme", |p| p.origin = "file://shop.test".into()),
            ("unsorted recipients", |p| {
                p.recipients = vec!["b".into(), "a".into()]
            }),
            ("duplicate artifacts", |p| {
                p.artifacts = vec!["a".into(), "a".into()]
            }),
            ("currency", |p| {
                p.amount = Amount::Known {
                    minor_units: 1,
                    currency: "usd".into(),
                }
            }),
            ("empty field name", |p| {
                p.fields.insert(String::new(), FieldValue::Checked(true));
            }),
            ("empty secret reference", |p| {
                p.fields
                    .insert("card".into(), FieldValue::Secret(String::new()));
            }),
            ("oversized summary", |p| p.summary = "x".repeat(1025)),
        ];
        for (what, change) in invalid {
            let mut preview = base();
            change(&mut preview);
            assert!(
                matches!(preview.validate(), Err(PreviewError::Invalid(_))),
                "{what}"
            );
        }
    }

    #[test]
    fn approval_needs_known_amounts_and_recipients() {
        for amount in [Amount::Unknown, Amount::NotApplicable] {
            let mut purchase = base();
            purchase.amount = amount;
            assert_eq!(purchase.approvable(), Err(PreviewError::UnknownAmount));
        }
        // Any unknown charge, whatever the effect.
        let mut submit = base();
        submit.effect = Some(Effect::Submit);
        submit.amount = Amount::Unknown;
        assert_eq!(submit.approvable(), Err(PreviewError::UnknownAmount));
        submit.amount = Amount::NotApplicable;
        assert_eq!(submit.approvable(), Ok(()));
        let mut export = base();
        export.effect = Some(Effect::Export);
        export.amount = Amount::NotApplicable;
        export.recipients.clear();
        assert_eq!(export.approvable(), Err(PreviewError::UnknownRecipient));
    }
}
