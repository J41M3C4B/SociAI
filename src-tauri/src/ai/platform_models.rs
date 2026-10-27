//! The door to the models for the platform (P11, IA-4): its operators reach the providers of today through here, with
//! the pace, the backups, the prices, the monthly cap and the usage log of the pipeline. The platform hands over what
//! the policy built (`ToModel`), and this sends that and nothing else: its parts as the question, the legend of its
//! marks as context, and the operator's own prompt and schema. The scanner of the pipeline stays as a second barrier.

use platform::models::{Failure, Models, Reply};
use platform::operators::{Operator, Tier};
use platform::policy::{Conditions, Provider, ToModel};

use super::pipeline::{self, AiCall, Custom, SqliteLedger};
use super::settings::{self, ProviderKind};
use super::{AiError, AiProvider, AiTask, ModelTier};
use crate::scanner::{RegexScanner, ScannerConfig};
use crate::storage::{self, SharedDb};

/// What each provider does with what it receives (ADR-004 §2). Anthropic and Gemini are processors: in production only
/// their paid APIs exist, run by SociAI, and the free plan of Gemini is only for development, with the fictitious data
/// of `fixtures/`. A new provider has to say its conditions here.
pub fn conditions(kind: ProviderKind) -> Conditions {
    match kind {
        ProviderKind::Gemini | ProviderKind::Anthropic => Conditions::Processor,
    }
}

pub struct ProviderModels {
    provider: Provider,
    inner: Box<dyn AiProvider>,
    ledger: SqliteLedger,
}

impl ProviderModels {
    pub fn new(kind: ProviderKind, inner: Box<dyn AiProvider>, db: SharedDb) -> ProviderModels {
        ProviderModels { provider: Provider { id: kind.as_str().into(), conditions: conditions(kind) }, inner, ledger: SqliteLedger(db) }
    }

    /// The models of the provider chosen in the settings, if its key is saved in the keychain.
    pub fn from_settings(db: &SharedDb) -> Option<ProviderModels> {
        let settings = settings::load(&*db.lock().ok()?).ok()?;
        let key = storage::get_api_key(settings.provider.as_str()).ok().flatten()?;
        Some(ProviderModels::new(settings.provider, super::build_provider(&settings, key), db.clone()))
    }
}

impl Models for ProviderModels {
    fn provider(&self) -> &Provider {
        &self.provider
    }

    fn call(&self, operator: &Operator, to_model: &ToModel) -> Result<Reply, Failure> {
        let tier = match operator.tier {
            Tier::Light => ModelTier::Light,
            Tier::Strong => ModelTier::Strong,
        };
        let call = AiCall { task: AiTask::Operator { id: operator.id, tier }, context: to_model.legend().into_iter().collect(), user: to_model.render(), project_id: None };
        let custom = Custom { schema: (operator.schema)(), max_output_tokens: operator.max_output_tokens, system: Some(operator.prompt.to_string()) };
        let scanner = RegexScanner::new(ScannerConfig::default());
        let run = pipeline::run_measured(self.inner.as_ref(), &scanner, &self.ledger, call, Some(custom));
        // the platform calls from its own thread, apart from the window: waiting here blocks nobody
        let measured = tauri::async_runtime::block_on(run).map_err(|e| failure(&e))?;
        Ok(Reply {
            value: measured.value,
            model: measured.model,
            input_tokens: measured.input_tokens,
            output_tokens: measured.output_tokens,
            cost_mxn: measured.cost_mxn,
        })
    }
}

fn failure(e: &AiError) -> Failure {
    match e {
        AiError::NoApiKey | AiError::Auth => Failure::Key,
        AiError::BudgetExhausted => Failure::Budget,
        AiError::Refused => Failure::Refused,
        AiError::BadOutput(_) | AiError::Truncated => Failure::BadOutput,
        AiError::Offline | AiError::Timeout | AiError::RateLimited | AiError::QuotaReached | AiError::Http { .. } | AiError::Internal(_) => {
            Failure::Unavailable
        }
    }
}
