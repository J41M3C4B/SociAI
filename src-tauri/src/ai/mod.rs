//! AI provider abstraction, versioned prompts and usage tracking.
//!
//! The AI only asks diagnosis questions, reasons about needs and writes text.
//! It never calculates and never writes files: it returns JSON, code validates it.

pub mod agent;
pub mod anthropic;
pub mod canonical;
pub mod figures;
pub mod gemini;
#[cfg(test)]
pub mod gemini_native;
pub mod metrics;
#[cfg(test)]
pub mod mock;
#[cfg(test)]
pub(crate) mod paced;
pub mod pipeline;
pub mod prompts;
pub mod settings;
// the vault moved with the detector to the `privacy` crate (ADR-004)
pub use privacy::vault;

#[cfg(test)]
pub mod golden;
// ERP-4: the dry run of the provider drove it through the conversation and the summary of Projects, which no longer
// use the AI. The file stays as it was, switched off, for the AI rail to carry its tests over to a driver of the
// platform (the fake Gemini server, the harness and the checks of the pipeline are still good).
#[cfg(any())]
mod dry_run_tests;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use settings::{AiSettings, ProviderKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Light,
    Strong,
}

/// Known tasks (docs/07-ia-y-costos.md). The tier and size limits live here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTask {
    /// One turn of the guided conversation of the diagnosis (ADR-017).
    ConversationTurn,
    DiagnosisSummary,
    PrioritizationProposeNeeds,
    /// Writes one section of the project's guide from what the conversation and the person confirmed (ADR-018).
    DraftingSection,
    /// When the drafting begins: makes clear what each section of the proposal asks, proposes the budget lines (no
    /// prices) and the schedule, all at once (ADR-021).
    DraftingPlan,
    /// Writes every section of the proposal in one go, as a full draft or as a short guide (ADR-021).
    DraftingAll,
    /// Fills the canonical schema of a call (ADR-015), whole or one block at a time.
    CallCanonical,
    /// Writes the «en pocas palabras» of a call for the person (ADR-024), from what its reading understood.
    CallBrief,
    /// One step of an agent (ADR-034): it asks for a tool or gives its answer. The agent says its name (the task in
    /// `ai_usage`, `agent.<name>`) and its tier; its prompt and schema come with the call (`pipeline::Custom`).
    Agent { name: &'static str, tier: ModelTier },
    /// The vectors of the fragments of the documents of the institution, or of a question to look for in them (IA5,
    /// ADR-034 §3): no prompt and no answer to validate, only text that was already through the scanner.
    Embed,
}

#[cfg_attr(not(test), allow(dead_code))] // used by the measurement tests, not by the application
pub const ALL_TASKS: [AiTask; 8] = [
    AiTask::ConversationTurn,
    AiTask::DiagnosisSummary,
    AiTask::PrioritizationProposeNeeds,
    AiTask::DraftingSection,
    AiTask::DraftingPlan,
    AiTask::DraftingAll,
    AiTask::CallCanonical,
    AiTask::CallBrief,
];

impl AiTask {
    pub fn as_str(self) -> &'static str {
        match self {
            AiTask::ConversationTurn => "conversation.turn",
            AiTask::DiagnosisSummary => "diagnosis.summary",
            AiTask::PrioritizationProposeNeeds => "prioritization.propose_needs",
            AiTask::DraftingSection => "drafting.section",
            AiTask::DraftingPlan => "drafting.plan",
            AiTask::DraftingAll => "drafting.all",
            AiTask::CallCanonical => "call.canonical",
            AiTask::CallBrief => "call.brief",
            AiTask::Agent { name, .. } => name,
            AiTask::Embed => "embed.records",
        }
    }

    pub fn tier(self) -> ModelTier {
        match self {
            AiTask::ConversationTurn | AiTask::CallBrief | AiTask::Embed => ModelTier::Light,
            AiTask::DiagnosisSummary
            | AiTask::PrioritizationProposeNeeds
            | AiTask::DraftingSection
            | AiTask::DraftingPlan
            | AiTask::DraftingAll
            | AiTask::CallCanonical => ModelTier::Strong,
            AiTask::Agent { tier, .. } => tier,
        }
    }

    /// How long one call may take before it is given up, in seconds. A question must answer
    /// quickly; the reading of a call runs once, in the background, over a lot of text.
    pub fn timeout_secs(self) -> u64 {
        match self {
            AiTask::Embed => 30,
            AiTask::ConversationTurn | AiTask::CallBrief | AiTask::Agent { .. } => 60,
            AiTask::DiagnosisSummary | AiTask::PrioritizationProposeNeeds | AiTask::DraftingSection | AiTask::DraftingPlan => 120,
            AiTask::DraftingAll => 180,
            AiTask::CallCanonical => 300,
        }
    }

    /// The light model goes first and the strong ones are its backup. Only the reading of a call
    /// (ADR-015): it copies quotes the code then verifies, so it does not need the model that reasons
    /// the most, and the light one has a daily allowance 25 times larger. A diagnosis question or
    /// summary writes for a person and keeps the strong model first.
    pub fn light_model_first(self) -> bool {
        matches!(self, AiTask::CallCanonical)
    }

    /// Temperature and seed for the tasks that classify or copy, where randomness only adds noise
    /// (measured on `gemini-3.5-flash-lite`: the same call gave the same answer every time). The
    /// tasks that write for a person keep the provider's defaults.
    pub fn sampling(self) -> Option<(f32, i64)> {
        match self {
            AiTask::CallCanonical => Some((0.0, 7)),
            _ => None,
        }
    }

    /// Approximate answer size from the docs; the summary gets more room than the
    /// doc's 1,200 because a truncated JSON is useless and cost follows real usage.
    pub fn max_output_tokens(self) -> u32 {
        match self {
            // an agent sets it per call (`pipeline::Custom`)
            AiTask::Embed => 0,
            AiTask::ConversationTurn | AiTask::CallBrief | AiTask::Agent { .. } => 500,
            AiTask::DiagnosisSummary => 2000,
            AiTask::PrioritizationProposeNeeds => 1000,
            AiTask::DraftingSection => 1500,
            AiTask::DraftingPlan => 4000,
            // every section of the proposal at once: a truncated JSON is useless, so it gets room
            AiTask::DraftingAll => 9000,
            // the caller sets it per call (`pipeline::Custom`): a whole schema needs far more than one block
            AiTask::CallCanonical => 12000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AiRequest {
    pub task: AiTask,
    pub tier: ModelTier,
    /// Fixed, versioned prompt (cacheable).
    pub system: String,
    /// Minimum context blocks.
    pub context: Vec<String>,
    pub user: String,
    pub output_schema: Value,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Fresh input, without what was served from the provider's cache.
    pub input_tokens: u64,
    /// Everything billed as output, thinking included.
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    /// The part of `output_tokens` the model spent thinking (0 when the provider does not say).
    pub thought_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub value: Value,
    /// Model that actually answered (may be a fallback).
    pub model: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum AiError {
    #[error("no API key configured")]
    NoApiKey,
    #[error("could not reach the AI service")]
    Offline,
    /// Connected, but the service took longer than the task allows. Not the same as having no
    /// internet: it counts against the allowance, and retrying the same model only repeats the wait.
    #[error("the AI service took too long to answer")]
    Timeout,
    #[error("rate limited")]
    RateLimited,
    /// The daily allowance of a model is used up; trying again today only wastes calls.
    #[error("the daily allowance of this model is used up")]
    QuotaReached,
    #[error("the API key was rejected")]
    Auth,
    #[error("the request was declined")]
    Refused,
    #[error("the answer was cut off")]
    Truncated,
    #[error("the answer did not match the expected format: {0}")]
    BadOutput(String),
    #[error("AI service error {status}: {message}")]
    Http { status: u16, message: String },
    #[error("the monthly AI budget is used up")]
    BudgetExhausted,
    #[error("internal error: {0}")]
    Internal(String),
}

impl AiError {
    /// Worth trying once more.
    pub fn is_retryable(&self) -> bool {
        matches!(self, AiError::Offline | AiError::RateLimited)
            || matches!(self, AiError::Http { status, .. } if *status >= 500)
    }

    /// Short label stored in `ai_usage.error_kind`.
    pub fn kind(&self) -> String {
        match self {
            AiError::NoApiKey => "no_key".into(),
            AiError::Offline => "offline".into(),
            AiError::Timeout => "timeout".into(),
            AiError::RateLimited => "rate_limited".into(),
            AiError::QuotaReached => "quota_reached".into(),
            AiError::Auth => "auth".into(),
            AiError::Refused => "refused".into(),
            AiError::Truncated => "truncated".into(),
            AiError::BadOutput(_) => "bad_output".into(),
            AiError::Http { status, .. } => format!("http_{status}"),
            AiError::BudgetExhausted => "budget".into(),
            AiError::Internal(_) => "internal".into(),
        }
    }
}

/// Result of checking one configured model without generating anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelCheck {
    pub model: String,
    /// The provider knows this model name.
    pub exists: bool,
    /// The model can answer the kind of request the app makes.
    pub can_generate: bool,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn complete(&self, req: &AiRequest) -> Result<AiResponse, AiError>;
    fn name(&self) -> &str;
    /// The model this provider will use for a tier (the pipeline paces calls per model).
    fn model_name(&self, tier: ModelTier) -> String;
    /// The models to try for a tier, best first: the main one, then its backups (ADR-008).
    fn model_chain(&self, tier: ModelTier) -> Vec<String> {
        vec![self.model_name(tier)]
    }
    /// Like `complete`, on a specific model of the chain. Providers without backups ignore `model`.
    async fn complete_with_model(&self, req: &AiRequest, _model: &str) -> Result<AiResponse, AiError> {
        self.complete(req).await
    }
    /// Checks the key and the configured models without spending a generation call.
    async fn check(&self) -> Result<Vec<ModelCheck>, AiError> {
        Err(AiError::Internal("this provider cannot be checked without a call".into()))
    }
}

/// The provider chosen in the settings, if its key exists.
pub fn build_provider(settings: &AiSettings, key: String) -> Box<dyn AiProvider> {
    match settings.provider {
        ProviderKind::Gemini => Box::new(gemini::GeminiProvider::new(key, settings.clone())),
        ProviderKind::Anthropic => Box::new(anthropic::AnthropicProvider::new(key, settings.clone())),
    }
}
