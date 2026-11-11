//! Durable intent data, not a grant, executable plan, verified outcome or mutable store.
//! Call validate after decoding and at trusted consumer boundaries. N02 owns immutable
//! accepted revisions; N03 owns cumulative reservation, N04/N05 verification, N13/N14
//! data-flow enforcement. Opaque references do not establish authority or provenance.
//!
//! Task text/profile/account references can be private; no aggregate raw Debug:
//! ```compile_fail,E0277
//! fn log(contract: &aero_core::task::TaskContract) { println!("{contract:?}"); }
//! ```

use crate::actions::Action;
use crate::checkpoint::{MAX_COUNTER, bounded_counter};
use crate::ids::TaskId;
use crate::wire::{ValidationError, require, strict_object, strict_tagged_object};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const TASK_SCHEMA_VERSION: u32 = 1;
pub const MAX_ITEMS: usize = 64;
pub const MAX_TEXT_BYTES: usize = 8192;
pub const MAX_REFERENCE_BYTES: usize = 256;
pub const MAX_RETENTION_DAYS: u64 = 365;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CoverageFraction(f64);
// Eq is sound only when construction/decoding exclude NaN.
impl Eq for CoverageFraction {}
impl CoverageFraction {
    pub fn new(value: f64) -> Result<Self, ValidationError> {
        require(
            value.is_finite() && value > 0.0 && value <= 1.0,
            "criteria.required_fraction",
            "must be finite, positive and at most one",
        )?;
        Ok(Self(value))
    }
    pub fn get(self) -> f64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for CoverageFraction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(f64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Navigate,
    Extract,
    StageArtifact,
    AccountCreate,
    Submit,
    Purchase,
    Upload,
    Download,
    Export,
    CredentialFill,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingMode {
    LocalOnly,
    ApprovedCloud,
    Sensitive,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Csv,
    Markdown,
    Json,
    Parquet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    Pause,
    Background,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Public,
    Private,
    Sensitive,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCondition {
    BudgetExhausted,
    PermissionRequired,
    UncertainEffect,
}

/// A fixed registry selector, never checker code or a model's pass/fail report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerifierKind {
    ArtifactSchema,
    RowCount,
    EvidenceCoverage,
}

strict_object! {
    pub struct Budget {
        pub max_action_attempts: u64,
        pub max_active_ms: u64,
        pub max_input_tokens: u64,
        pub max_output_tokens: u64,
        pub max_purchase_minor_units: u64,
        pub budget_currency: String,
        pub max_inference_minor_units: Option<u64>,
    }
}
strict_object! {
    pub struct OutputRequirement {
        pub name: String,
        pub format: OutputFormat,
        pub required: bool,
    }
}
strict_object! {
    /// Requested segment-bounded prefix. "/" is explicit whole-origin scope;
    /// empty route lists permit no routes. Actual normalized URL matching is N06.
    pub struct RouteScope { pub origin: String, pub path_prefix: String }
}
strict_object! {
    pub struct SourceClassification { pub origin: String, pub classification: Classification }
}
strict_object! {
    pub struct Question { pub id: String, pub prompt: String }
}
strict_object! {
    pub struct Retention { pub snapshot_days: u64, pub artifact_days: u64, pub memory_opt_in: bool }
}
strict_tagged_object! {
    #[derive(Eq)]
    pub enum Criterion {
        ArtifactSchema { id: String, required: bool, artifact: String, columns: Vec<String> },
        RowCount { id: String, required: bool, artifact: String, expected: u64 },
        EvidenceCoverage { id: String, required: bool, artifact: String, required_fraction: CoverageFraction },
    }
    tag "kind";
}
impl Criterion {
    pub fn id(&self) -> &str {
        match self {
            Self::ArtifactSchema { id, .. }
            | Self::RowCount { id, .. }
            | Self::EvidenceCoverage { id, .. } => id,
        }
    }
    pub fn artifact(&self) -> &str {
        match self {
            Self::ArtifactSchema { artifact, .. }
            | Self::RowCount { artifact, .. }
            | Self::EvidenceCoverage { artifact, .. } => artifact,
        }
    }
    pub fn required(&self) -> bool {
        match self {
            Self::ArtifactSchema { required, .. }
            | Self::RowCount { required, .. }
            | Self::EvidenceCoverage { required, .. } => *required,
        }
    }
    pub fn verifier(&self) -> VerifierKind {
        match self {
            Self::ArtifactSchema { .. } => VerifierKind::ArtifactSchema,
            Self::RowCount { .. } => VerifierKind::RowCount,
            Self::EvidenceCoverage { .. } => VerifierKind::EvidenceCoverage,
        }
    }
}
strict_object! {
    pub struct TaskContract {
        pub schema_version: u32,
        pub task_id: TaskId,
        pub revision: u64,
        pub title: String,
        pub objective: String,
        pub profile_ref: String,
        pub account_refs: Vec<String>,
        pub allowed_origins: Vec<String>,
        pub allowed_routes: Vec<RouteScope>,
        pub allowed_effects: Vec<Effect>,
        pub forbidden_effects: Vec<Effect>,
        pub processing_mode: ProcessingMode,
        pub processor_grants: Vec<String>,
        pub source_classifications: Vec<SourceClassification>,
        pub export_grants: Vec<String>,
        pub screenshot_transfer_allowed: bool,
        pub budgets: Budget,
        pub criteria: Vec<Criterion>,
        pub outputs: Vec<OutputRequirement>,
        pub dependencies: Vec<TaskId>,
        pub stop_conditions: Vec<StopCondition>,
        pub unresolved_questions: Vec<Question>,
        pub close_behavior: CloseBehavior,
        pub retention: Retention,
    }
    version schema_version = TASK_SCHEMA_VERSION;
}

/// Trusted host observations, NOT deserializable authority supplied by the planner.
/// Success only checks these prerequisites; grants/prices/records still need validation.
#[derive(Clone, Copy, Debug)]
pub struct StartPrerequisites {
    pub local_processor_available: bool,
    pub background_service_available: bool,
}

impl TaskContract {
    /// Internal consistency, not issuer, grant validity, evidence, history or permission.
    pub fn validate(&self) -> Result<(), ValidationError> {
        require(
            self.schema_version == TASK_SCHEMA_VERSION,
            "task.schema_version",
            "unsupported task schema version",
        )?;
        reference(self.task_id.as_str(), "task.task_id")?;
        bounded_counter(self.revision, "task.revision")?;
        require(self.revision > 0, "task.revision", "must be positive")?;
        text(&self.title, MAX_REFERENCE_BYTES, "task.title")?;
        text(&self.objective, MAX_TEXT_BYTES, "task.objective")?;
        reference(&self.profile_ref, "task.profile_ref")?;
        references(
            self.account_refs.iter().map(String::as_str),
            "task.account_refs",
        )?;
        references(
            self.processor_grants.iter().map(String::as_str),
            "task.processor_grants",
        )?;
        references(
            self.export_grants.iter().map(String::as_str),
            "task.export_grants",
        )?;
        references(
            self.dependencies.iter().map(TaskId::as_str),
            "task.dependencies",
        )?;
        require(
            !self.dependencies.contains(&self.task_id),
            "task.dependencies",
            "task must not depend on itself",
        )?;
        bounded_list(&self.allowed_origins, "task.allowed_origins")?;
        let mut origins = HashSet::new();
        for origin in &self.allowed_origins {
            require(
                origin_shape(origin),
                "task.allowed_origins",
                "must be a supported canonical HTTP(S) origin",
            )?;
            require(
                origins.insert(origin.as_str()),
                "task.allowed_origins",
                "duplicate origin",
            )?;
        }
        bounded_list(&self.allowed_routes, "task.allowed_routes")?;
        let mut routes = HashSet::new();
        for route in &self.allowed_routes {
            require(
                origins.contains(route.origin.as_str()),
                "task.allowed_routes",
                "route origin must be in allowed origins",
            )?;
            require(
                route_shape(&route.path_prefix),
                "task.allowed_routes",
                "unsupported route prefix syntax",
            )?;
            require(
                routes.insert((&route.origin, &route.path_prefix)),
                "task.allowed_routes",
                "duplicate route",
            )?;
        }
        bounded_list(&self.source_classifications, "task.source_classifications")?;
        let mut classified = HashSet::new();
        for source in &self.source_classifications {
            require(
                origins.contains(source.origin.as_str()),
                "task.source_classifications",
                "source must be in allowed origins",
            )?;
            require(
                classified.insert(&source.origin),
                "task.source_classifications",
                "duplicate source classification",
            )?;
        }
        unique(&self.allowed_effects, "task.allowed_effects")?;
        unique(&self.forbidden_effects, "task.forbidden_effects")?;
        require(
            !self
                .allowed_effects
                .iter()
                .any(|effect| self.forbidden_effects.contains(effect)),
            "task.effects",
            "allowed and forbidden effects must not overlap",
        )?;
        unique(&self.stop_conditions, "task.stop_conditions")?;
        require(
            [
                StopCondition::BudgetExhausted,
                StopCondition::PermissionRequired,
                StopCondition::UncertainEffect,
            ]
            .iter()
            .all(|condition| self.stop_conditions.contains(condition)),
            "task.stop_conditions",
            "all mandatory safety stops must be explicit",
        )?;
        if self.processing_mode == ProcessingMode::LocalOnly {
            require(
                self.processor_grants.is_empty() && !self.screenshot_transfer_allowed,
                "task.processing_mode",
                "local-only processing forbids remote processor/screenshot transfers",
            )?;
        }
        if self.processing_mode == ProcessingMode::ApprovedCloud || self.screenshot_transfer_allowed
        {
            require(
                !self.processor_grants.is_empty(),
                "task.processor_grants",
                "explicit processor references required",
            )?;
        }
        if self.processing_mode == ProcessingMode::Sensitive && self.retention.memory_opt_in {
            require(
                !self.processor_grants.is_empty(),
                "task.retention.memory_opt_in",
                "restricted memory requires an explicit exception reference",
            )?;
        }
        if self.allowed_effects.contains(&Effect::Export) {
            require(
                !self.export_grants.is_empty(),
                "task.export_grants",
                "export requires explicit references",
            )?;
        }
        self.budgets.validate()?;
        for days in [self.retention.snapshot_days, self.retention.artifact_days] {
            require(
                days <= MAX_RETENTION_DAYS,
                "task.retention",
                "retention must not exceed 365 days",
            )?;
        }
        bounded_list(&self.unresolved_questions, "task.unresolved_questions")?;
        let mut questions = HashSet::new();
        for question in &self.unresolved_questions {
            token(&question.id, "task.unresolved_questions.id")?;
            text(
                &question.prompt,
                MAX_TEXT_BYTES,
                "task.unresolved_questions.prompt",
            )?;
            require(
                questions.insert(&question.id),
                "task.unresolved_questions",
                "duplicate question",
            )?;
        }
        bounded_list(&self.outputs, "task.outputs")?;
        let mut outputs = std::collections::HashMap::new();
        for output in &self.outputs {
            token(&output.name, "task.outputs.name")?;
            require(
                outputs.insert(output.name.as_str(), output).is_none(),
                "task.outputs",
                "duplicate output name",
            )?;
        }
        require(
            self.outputs.iter().any(|output| output.required),
            "task.outputs",
            "at least one required output needed",
        )?;
        bounded_list(&self.criteria, "task.criteria")?;
        require(
            self.criteria.iter().any(Criterion::required),
            "task.criteria",
            "at least one required criterion needed",
        )?;
        let mut criteria = HashSet::new();
        let mut required_row_counts = std::collections::HashMap::new();
        for criterion in &self.criteria {
            token(criterion.id(), "task.criteria.id")?;
            token(criterion.artifact(), "task.criteria.artifact")?;
            require(
                criteria.insert(criterion.id()),
                "task.criteria",
                "duplicate criterion ID",
            )?;
            let output = outputs.get(criterion.artifact());
            require(
                output.is_some(),
                "task.criteria.artifact",
                "criterion references an unknown output",
            )?;
            if let Some(output) = output {
                require(
                    !criterion.required() || output.required,
                    "task.criteria.artifact",
                    "required criterion needs required output",
                )?;
                match criterion {
                    Criterion::ArtifactSchema { columns, .. } => {
                        require(
                            output.format != OutputFormat::Markdown,
                            "task.criteria",
                            "schema requires structured output",
                        )?;
                        require(
                            !columns.is_empty(),
                            "task.criteria.columns",
                            "schema needs columns",
                        )?;
                        references(columns.iter().map(String::as_str), "task.criteria.columns")?;
                    }
                    Criterion::RowCount { expected, .. } => {
                        require(
                            output.format != OutputFormat::Markdown,
                            "task.criteria",
                            "row count requires structured output",
                        )?;
                        bounded_counter(*expected, "task.criteria.expected")?;
                        if criterion.required() {
                            require(
                                required_row_counts
                                    .insert(criterion.artifact(), *expected)
                                    .is_none_or(|previous| previous == *expected),
                                "task.criteria.expected",
                                "required row counts for one output must agree",
                            )?;
                        }
                    }
                    Criterion::EvidenceCoverage {
                        required_fraction, ..
                    } => {
                        CoverageFraction::new(required_fraction.get())?;
                    }
                }
            }
        }
        for output in self.outputs.iter().filter(|output| output.required) {
            require(
                self.criteria
                    .iter()
                    .any(|criterion| criterion.required() && criterion.artifact() == output.name),
                "task.outputs",
                "required output needs a required criterion",
            )?;
        }
        Ok(())
    }

    /// Success is NOT permission to launch: trusted grants/prices/dependency
    /// outcomes and actual provider configuration still need their runtime owners.
    pub fn validate_for_start(
        &self,
        prerequisites: &StartPrerequisites,
    ) -> Result<(), ValidationError> {
        self.validate()?;
        if self.processing_mode != ProcessingMode::ApprovedCloud {
            require(
                prerequisites.local_processor_available,
                "task.processing_mode",
                "required local processor is unavailable; no remote fallback",
            )?;
        }
        if self.close_behavior == CloseBehavior::Background {
            require(
                prerequisites.background_service_available,
                "task.close_behavior",
                "background service is unavailable",
            )?;
        }
        require(
            self.unresolved_questions.is_empty(),
            "task.unresolved_questions",
            "resolve questions before starting",
        )
    }
}

impl Budget {
    /// Cumulative limits, not reservations. Explicit null inference money means
    /// unknown, not free. Both monetary ceilings use budget_currency; syntax
    /// validation does not establish supported currency, FX rates or actual prices.
    pub fn validate(&self) -> Result<(), ValidationError> {
        for value in [
            self.max_action_attempts,
            self.max_active_ms,
            self.max_input_tokens,
            self.max_output_tokens,
            self.max_purchase_minor_units,
        ] {
            bounded_counter(value, "task.budgets")?;
        }
        if let Some(value) = self.max_inference_minor_units {
            bounded_counter(value, "task.budgets.max_inference_minor_units")?;
        }
        require(
            self.max_action_attempts > 0 && self.max_active_ms > 0,
            "task.budgets",
            "attempt and active-time limits must be positive",
        )?;
        require(
            self.max_input_tokens
                .checked_add(self.max_output_tokens)
                .is_some_and(|sum| sum <= MAX_COUNTER),
            "task.budgets.tokens",
            "combined token limits exceed counter range",
        )?;
        require(
            self.budget_currency.len() == 3
                && self.budget_currency.bytes().all(|b| b.is_ascii_uppercase()),
            "task.budgets.budget_currency",
            "must be a three-letter uppercase currency code",
        )
    }
}

fn bounded_list<T>(values: &[T], field: &'static str) -> Result<(), ValidationError> {
    require(
        values.len() <= MAX_ITEMS,
        field,
        "must not exceed 64 entries",
    )
}
fn unique<T: Eq + std::hash::Hash>(
    values: &[T],
    field: &'static str,
) -> Result<(), ValidationError> {
    bounded_list(values, field)?;
    let mut seen = HashSet::new();
    require(
        values.iter().all(|value| seen.insert(value)),
        field,
        "duplicate entry",
    )
}
fn text(value: &str, limit: usize, field: &'static str) -> Result<(), ValidationError> {
    require(
        !value.trim().is_empty() && value.len() <= limit && !value.contains('\0'),
        field,
        "empty, oversized or NUL-bearing text",
    )
}
fn reference(value: &str, field: &'static str) -> Result<(), ValidationError> {
    text(value, MAX_REFERENCE_BYTES, field)
}
fn references<'a>(
    values: impl Iterator<Item = &'a str>,
    field: &'static str,
) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for value in values {
        reference(value, field)?;
        require(seen.insert(value), field, "duplicate reference")?;
        require(seen.len() <= MAX_ITEMS, field, "must not exceed 64 entries")?;
    }
    Ok(())
}
fn token(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require(
        !value.is_empty()
            && value.len() <= MAX_REFERENCE_BYTES
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        field,
        "must be a bounded alphanumeric, hyphen or underscore token, not a path",
    )
}

// Conservative canonical origin subset, not resolution, permission or SSRF.
fn origin_shape(origin: &str) -> bool {
    if origin.len() > 2048
        || !origin.is_ascii()
        || origin != origin.to_ascii_lowercase()
        || (Action::Navigate { url: origin.into() })
            .validate()
            .is_err()
    {
        return false;
    }
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") || authority.contains(['/', '?', '#']) {
        return false;
    }
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let Some((address, suffix)) = rest.split_once(']') else {
            return false;
        };
        if !address
            .parse::<std::net::Ipv6Addr>()
            // Rust uses dotted decimal for mapped IPv6; browsers serialize it
            // in hexadecimal. Exclude the family rather than accept aliases.
            .is_ok_and(|ip| ip.to_ipv4_mapped().is_none() && ip.to_string() == address)
        {
            return false;
        }
        (address, suffix.strip_prefix(':'))
    } else {
        authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)))
    };
    if host.ends_with('.') {
        return false;
    }
    port.is_none_or(|port| {
        port.parse::<u16>().is_ok_and(|n| {
            n > 0
                && n.to_string() == port
                && !(scheme == "http" && n == 80 || scheme == "https" && n == 443)
        })
    })
}

pub(crate) fn route_shape(path: &str) -> bool {
    path.len() <= 2048
        && path.starts_with('/')
        && !path.contains("//")
        && !path
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '\\' | '%' | '?' | '#'))
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
}
