//! Every AI call goes through here, in this order (docs/agents/principios-de-ingenieria.md):
//! budget check -> scanner over the whole prompt -> pace -> provider -> JSON schema
//! validation -> usage log. The model only ever returns JSON; code decides the rest.

use super::metrics;
use super::prompts;
use super::settings::{self, AiSettings};
use super::vault::Vault;
use super::{AiError, AiProvider, AiRequest, AiResponse, AiTask, ModelTier};
use crate::audit::{self, AuditKind};
use crate::scanner::SensitiveScanner;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ulid::Ulid;

/// Calls and text a model handled in the last seconds (for pacing and for the metrics screen).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Window {
    pub calls: u32,
    pub tokens: u64,
    /// Age of the oldest call inside the window; the wait ends when it leaves the window.
    pub oldest_age_secs: u32,
}

/// Where the pipeline reads settings and writes the usage log and audit events.
pub trait Ledger: Send + Sync {
    fn settings(&self) -> Result<AiSettings, AiError>;
    fn month_spend_mxn(&self) -> Result<f64, AiError>;
    fn record_usage(&self, rec: &UsageRecord) -> Result<(), AiError>;
    fn record_leak_prevented(&self, task: AiTask, counts: &BTreeMap<&'static str, usize>) -> Result<(), AiError>;
    /// Calls that reached the provider for `model` in the last `seconds`.
    fn window(&self, model: &str, seconds: u32) -> Result<Window, AiError>;
    /// The end of an errand of an agent (`ai.run`): who, how many steps and tools, how it ended. Never content.
    fn record_run(&self, run: &RunRecord) -> Result<(), AiError>;
}

/// What the audit log keeps of an errand of an agent (ADR-034 §5): counts and codes, never content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub agent: &'static str,
    pub model_calls: u32,
    pub tool_calls: u32,
    /// How many times each tool ran, by name.
    pub tools: BTreeMap<&'static str, u32>,
    /// Requests of a tool the agent may not use, that does not exist or with bad arguments.
    pub rejected: u32,
    /// `answered`, `step_limit`, `token_limit` or the kind of the AI error.
    pub result: String,
}

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub task: AiTask,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub thought_tokens: u64,
    pub cost_mxn: f64,
    pub project_id: Option<String>,
    pub success: bool,
    /// Milliseconds the provider took; `None` when there was no answer to time.
    pub latency_ms: Option<u64>,
    /// Why the attempt failed; `None` when the answer was valid.
    pub error_kind: Option<String>,
}

pub struct AiCall {
    pub task: AiTask,
    /// Minimum context blocks (never whole documents).
    pub context: Vec<String>,
    pub user: String,
    pub project_id: Option<String>,
}

fn db_err(e: impl std::fmt::Display) -> AiError {
    AiError::Internal(e.to_string())
}

fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    let validator = jsonschema::validator_for(schema).map_err(|e| format!("bad schema: {e}"))?;
    validator.validate(value).map_err(|e| e.to_string())
}

/// Before each attempt: stop at the daily allowance (trying again today only wastes calls)
/// and wait out the per-minute one instead of provoking an error. The count looks at the
/// last 24 hours, which can only be stricter than the provider's own day.
pub(crate) async fn pace(ledger: &dyn Ledger, settings: &AiSettings, model: &str) -> Result<(), AiError> {
    let Some(limit) = settings.rate_limits.get(model) else { return Ok(()) };
    if limit.per_day > 0 && ledger.window(model, 86_400)?.calls >= limit.per_day {
        return Err(AiError::QuotaReached);
    }
    let minute = ledger.window(model, 60)?;
    let calls_full = limit.per_minute > 0 && minute.calls >= limit.per_minute;
    let text_full = limit.tokens_per_minute > 0 && minute.tokens >= limit.tokens_per_minute;
    if calls_full || text_full {
        let wait = 61u64.saturating_sub(u64::from(minute.oldest_age_secs)).clamp(1, 61);
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
    Ok(())
}

struct Answered {
    resp: AiResponse,
    latency_ms: u64,
}

/// The failure is about this model right now, not about the request or the key, so another
/// model of the chain may well answer: overloaded or erroring (5xx), over its allowance, or
/// not found, or that filled its whole output without finishing (a model that loops: the next one
/// may not). An offline computer is not it (every model is equally out of reach), and a bad
/// key, a refusal or a bad answer would repeat on the next model.
fn another_model_may_answer(e: &AiError) -> bool {
    matches!(e, AiError::RateLimited | AiError::QuotaReached | AiError::Timeout | AiError::Truncated)
        || matches!(e, AiError::Http { status, .. } if *status >= 500 || *status == 404)
}

/// One provider call, paced, timed and logged when it fails. The chain is walked in order: a
/// model that is overloaded, out of allowance or missing hands over to the next one at once
/// (ADR-008). When there is nobody to hand over to, a temporary failure is tried once more.
/// Every attempt that fails leaves its own row so the metrics tell the whole story.
async fn call_provider(
    provider: &dyn AiProvider,
    ledger: &dyn Ledger,
    settings: &AiSettings,
    call: &AiCall,
    req: &AiRequest,
) -> Result<Answered, AiError> {
    let mut chain = provider.model_chain(req.tier);
    if call.task.light_model_first() {
        // the light models first, then the strong ones as backup, without repeating a model
        let mut first = provider.model_chain(ModelTier::Light);
        for m in chain {
            if !first.contains(&m) {
                first.push(m);
            }
        }
        chain = first;
    }
    // A timeout costs the whole time limit, and a service that is slow for one model is usually
    // slow for the next: after two, stop instead of spending the limit on every model of the chain.
    let mut timeouts = 0;
    for (position, model) in chain.iter().enumerate() {
        let has_next = position + 1 < chain.len();
        let mut tries = 0;
        loop {
            tries += 1;
            // the daily allowance of this model is gone: no call is wasted, the next one gets its turn
            match pace(ledger, settings, model).await {
                Err(e) if has_next && another_model_may_answer(&e) => break,
                other => other?,
            }
            let started = Instant::now();
            let result = provider.complete_with_model(req, model).await;
            let latency_ms = started.elapsed().as_millis() as u64;
            match result {
                Ok(resp) => return Ok(Answered { resp, latency_ms }),
                Err(e) => {
                    ledger.record_usage(&UsageRecord {
                        task: call.task,
                        provider: provider.name().to_string(),
                        model: model.clone(),
                        input_tokens: 0,
                        output_tokens: 0,
                        cached_tokens: 0,
                        thought_tokens: 0,
                        cost_mxn: 0.0,
                        project_id: call.project_id.clone(),
                        success: false,
                        latency_ms: Some(latency_ms),
                        error_kind: Some(e.kind()),
                    })?;
                    if matches!(e, AiError::Timeout) {
                        timeouts += 1;
                        if timeouts >= 2 {
                            return Err(e);
                        }
                    }
                    if has_next && another_model_may_answer(&e) {
                        break;
                    }
                    if tries == 1 && e.is_retryable() {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
    Err(AiError::Internal("the model chain is empty".into()))
}

/// What a caller may decide about one call besides its task: the schema of the answer (the canonical
/// reading asks for the whole schema or for one block of it), how much it may write and, for an agent, its
/// own fixed prompt.
pub struct Custom {
    pub schema: Value,
    pub max_output_tokens: u32,
    pub system: Option<String>,
}

/// A valid answer and the text it cost (input and output of every attempt, failed ones included).
pub struct Measured {
    pub value: Value,
    pub tokens: u64,
    /// The model that gave the valid answer: it may be a backup of the one asked.
    pub model: String,
    /// The text that went in and came out, and what it cost, in every attempt that got an answer.
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_mxn: f64,
}

pub async fn run(
    provider: &dyn AiProvider,
    scanner: &dyn SensitiveScanner,
    ledger: &dyn Ledger,
    call: AiCall,
) -> Result<Value, AiError> {
    run_with(provider, scanner, ledger, call, None).await
}

pub async fn run_with(
    provider: &dyn AiProvider,
    scanner: &dyn SensitiveScanner,
    ledger: &dyn Ledger,
    call: AiCall,
    custom: Option<Custom>,
) -> Result<Value, AiError> {
    run_measured(provider, scanner, ledger, call, custom).await.map(|m| m.value)
}

/// Like `run_with`, and says how much text the call cost (an agent keeps a cap per errand).
pub async fn run_measured(
    provider: &dyn AiProvider,
    scanner: &dyn SensitiveScanner,
    ledger: &dyn Ledger,
    call: AiCall,
    custom: Option<Custom>,
) -> Result<Measured, AiError> {
    run_marked(provider, scanner, ledger, call, custom, None).await
}

/// Only in a development build (`pnpm tauri dev`) and with `SOCIAI_AI_TRACE` naming a file: each request as it went
/// to the provider and what came back, to see what the model read in a real session of the app. It is written after
/// the scanner and the vault, so it holds what the provider got, never a personal value; the fixed prompt is left out.
/// The file stays on this computer, never in the repository.
#[cfg(debug_assertions)]
fn trace(req: &AiRequest, answer: &Value) {
    use std::io::Write;
    let Ok(path) = std::env::var("SOCIAI_AI_TRACE") else { return };
    let entry = format!("==== {}\n{}\n---- pide\n{}\n---- respondió\n{answer}\n\n", req.task.as_str(), req.context.join("\n\n"), req.user);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(entry.as_bytes());
    }
}

#[cfg(not(debug_assertions))]
fn trace(_: &AiRequest, _: &Value) {}

/// Like `run_measured`; with the vault of an errand (ADR-044), what the scanner finds goes as its marks instead of its
/// label, and the legend of the marks goes last in the context.
pub async fn run_marked(
    provider: &dyn AiProvider,
    scanner: &dyn SensitiveScanner,
    ledger: &dyn Ledger,
    call: AiCall,
    custom: Option<Custom>,
    vault: Option<&Vault>,
) -> Result<Measured, AiError> {
    let settings = ledger.settings()?;

    // 1. Budget: at the cap, AI is paused and everything else keeps working.
    if ledger.month_spend_mxn()? >= settings.monthly_cap_mxn {
        return Err(AiError::BudgetExhausted);
    }

    // 2. Second barrier of the scanner over the whole prompt: cover anything found, with its label or with its mark.
    let mut leaks: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut clean = |text: &str| -> String {
        let report = scanner.scan(text);
        for (k, v) in report.counts() {
            *leaks.entry(k).or_insert(0) += v;
        }
        match vault {
            // a value marked earlier in the errand is marked again even where the scanner does not see it now
            Some(v) => v.mark(text, &report),
            None if report.is_clean() => text.to_string(),
            None => scanner.redact(text, &report),
        }
    };
    let mut context: Vec<String> = call.context.iter().map(|c| clean(c)).collect();
    let user = clean(&call.user);
    if let Some(legend) = vault.and_then(Vault::legend) {
        context.push(legend);
    }
    if !leaks.is_empty() {
        ledger.record_leak_prevented(call.task, &leaks)?;
    }

    // 3-5. Pace and provider, schema validation (one retry with the error), usage log.
    let (schema, max_output_tokens, system) = match custom {
        Some(c) => (c.schema, c.max_output_tokens, c.system.unwrap_or_else(|| prompts::system_prompt(call.task))),
        None => (prompts::schema(call.task), call.task.max_output_tokens(), prompts::system_prompt(call.task)),
    };
    let mut tokens = 0u64;
    let (mut input_tokens, mut output_tokens, mut cost_mxn) = (0u64, 0u64, 0.0f64);
    let mut attempt_user = user.clone();
    for attempt in 0..2 {
        let req = AiRequest {
            task: call.task,
            tier: call.task.tier(),
            system: system.clone(),
            context: context.clone(),
            user: attempt_user.clone(),
            output_schema: schema.clone(),
            max_output_tokens,
        };
        let problem = match call_provider(provider, ledger, &settings, &call, &req).await {
            Ok(Answered { resp, latency_ms }) => {
                trace(&req, &resp.value);
                let model = if resp.model.is_empty() { provider.model_name(req.tier) } else { resp.model.clone() };
                let check = validate(&schema, &resp.value);
                tokens += resp.usage.input_tokens + resp.usage.cache_read_tokens + resp.usage.output_tokens;
                let cost = settings.cost_mxn(&model, &resp.usage);
                input_tokens += resp.usage.input_tokens + resp.usage.cache_read_tokens;
                output_tokens += resp.usage.output_tokens;
                cost_mxn += cost;
                ledger.record_usage(&UsageRecord {
                    task: call.task,
                    provider: provider.name().to_string(),
                    cost_mxn: cost,
                    model: model.clone(),
                    input_tokens: resp.usage.input_tokens,
                    output_tokens: resp.usage.output_tokens,
                    cached_tokens: resp.usage.cache_read_tokens,
                    thought_tokens: resp.usage.thought_tokens,
                    project_id: call.project_id.clone(),
                    success: check.is_ok(),
                    latency_ms: Some(latency_ms),
                    error_kind: check.as_ref().err().map(|_| "schema".to_string()),
                })?;
                match check {
                    Ok(()) => return Ok(Measured { value: resp.value, tokens, model, input_tokens, output_tokens, cost_mxn }),
                    Err(msg) => msg,
                }
            }
            Err(AiError::BadOutput(msg)) => msg,
            Err(e) => return Err(e),
        };
        if attempt == 1 {
            return Err(AiError::BadOutput(problem));
        }
        attempt_user = format!(
            "{user}\n\nTu respuesta anterior no cumplió el formato ({problem}). Responde de nuevo solo con el JSON válido."
        );
    }
    unreachable!("the loop returns on the second attempt")
}

/// Ledger backed by the encrypted database.
pub struct SqliteLedger(pub Arc<Mutex<Connection>>);

impl SqliteLedger {
    fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AiError> {
        self.0.lock().map_err(|_| AiError::Internal("database lock poisoned".into()))
    }
}

impl Ledger for SqliteLedger {
    fn settings(&self) -> Result<AiSettings, AiError> {
        settings::load(&*self.conn()?).map_err(db_err)
    }

    fn month_spend_mxn(&self) -> Result<f64, AiError> {
        settings::month_spend_mxn(&*self.conn()?).map_err(db_err)
    }

    fn record_usage(&self, r: &UsageRecord) -> Result<(), AiError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,cached_tokens,estimated_cost_mxn,project_id,success,latency_ms,thought_tokens,error_kind)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%SZ','now'), ?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                format!("use_{}", Ulid::generate()),
                r.task.as_str(),
                r.provider,
                r.model,
                r.input_tokens as i64,
                r.output_tokens as i64,
                r.cached_tokens as i64,
                r.cost_mxn,
                r.project_id,
                r.success as i64,
                r.latency_ms.map(|v| v as i64),
                r.thought_tokens as i64,
                r.error_kind
            ],
        )
        .map_err(db_err)?;
        audit::record(
            &conn,
            AuditKind::AiCall,
            Some("ai_usage"),
            None,
            json!({ "task": r.task.as_str(), "ok": r.success }),
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn record_leak_prevented(&self, task: AiTask, counts: &BTreeMap<&'static str, usize>) -> Result<(), AiError> {
        let conn = self.conn()?;
        audit::record(
            &conn,
            AuditKind::ScannerLeakPrevented,
            Some("ai_call"),
            None,
            json!({ "task": task.as_str(), "findings": counts }),
        )
        .map_err(db_err)
    }

    fn window(&self, model: &str, seconds: u32) -> Result<Window, AiError> {
        metrics::window(&*self.conn()?, model, seconds).map_err(db_err)
    }

    fn record_run(&self, r: &RunRecord) -> Result<(), AiError> {
        let conn = self.conn()?;
        audit::record(
            &conn,
            AuditKind::AiRun,
            Some("ai_run"),
            None,
            json!({ "agent": r.agent, "model_calls": r.model_calls, "tool_calls": r.tool_calls, "tools": r.tools, "rejected": r.rejected, "result": r.result }),
        )
        .map_err(db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::mock::MockProvider;
    use crate::ai::settings::RateLimit;
    use crate::ai::Usage;
    use crate::scanner::{RegexScanner, ScannerConfig};
    use crate::storage::open_encrypted;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn ledger() -> (tempfile::TempDir, SqliteLedger, Arc<Mutex<Connection>>) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Arc::new(Mutex::new(open_encrypted(&dir.path().join("t.db"), KEY).unwrap()));
        (dir, SqliteLedger(conn.clone()), conn)
    }

    fn scanner() -> RegexScanner {
        RegexScanner::new(ScannerConfig::default())
    }

    fn call() -> AiCall {
        AiCall {
            task: AiTask::ConversationTurn,
            context: vec!["Perfil: 18 adultos mayores".into()],
            user: "Respuesta: el baño resbala".into(),
            project_id: Some("proj_1".into()),
        }
    }

    fn good() -> AiResponse {
        AiResponse {
            value: json!({"message": "¿Cuántas caídas hubo?", "cause": null, "root_hypothesis": null, "options": [], "fit": null, "fit_note": ""}),
            model: "claude-haiku-4-5".into(),
            usage: Usage { input_tokens: 1000, output_tokens: 100, cache_read_tokens: 0, cache_write_tokens: 0, thought_tokens: 0 },
        }
    }

    fn rows(conn: &Arc<Mutex<Connection>>, sql: &str) -> i64 {
        conn.lock().unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Limits for the model name the mock reports ("mock").
    fn limit_mock(conn: &Arc<Mutex<Connection>>, per_minute: u32, per_day: u32) {
        let c = conn.lock().unwrap();
        let mut s = AiSettings::default();
        s.rate_limits.insert("mock".into(), RateLimit { per_minute, tokens_per_minute: 0, per_day });
        settings::save(&c, &s).unwrap();
    }

    fn seed_calls(conn: &Arc<Mutex<Connection>>, n: usize, error_kind: Option<&str>) {
        let c = conn.lock().unwrap();
        for i in 0..n {
            c.execute(
                "INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,estimated_cost_mxn,success,error_kind)
                 VALUES (?1,strftime('%Y-%m-%dT%H:%M:%SZ','now'),'t','mock','mock',10,10,0,1,?2)",
                params![format!("seed{i}"), error_kind],
            )
            .unwrap();
        }
    }

    #[tokio::test]
    async fn happy_path_validates_and_logs_usage_with_cost() {
        let (_d, l, conn) = ledger();
        let p = MockProvider::new(vec![Ok(good())]);
        let v = run(&p, &scanner(), &l, call()).await.unwrap();
        assert_eq!(v["message"], "¿Cuántas caídas hubo?");
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE success=1 AND project_id='proj_1'"), 1);
        let cost: f64 = conn.lock().unwrap().query_row("SELECT estimated_cost_mxn FROM ai_usage", [], |r| r.get(0)).unwrap();
        // (1000 * $1 + 100 * $5) / 1M * 18.5
        assert!((cost - 0.02775).abs() < 1e-9, "{cost}");
        // audit has the call, without content
        let d: String = conn.lock().unwrap().query_row("SELECT details_json FROM audit_log WHERE event='ai.call'", [], |r| r.get(0)).unwrap();
        assert!(!d.contains("baño"));
    }

    #[tokio::test]
    async fn every_answer_is_timed_and_keeps_its_thinking_tokens() {
        let (_d, l, conn) = ledger();
        let mut r = good();
        r.usage.thought_tokens = 40;
        let p = MockProvider::new(vec![Ok(r)]);
        run(&p, &scanner(), &l, call()).await.unwrap();
        let (ms, thought, kind): (Option<i64>, i64, Option<String>) = conn
            .lock()
            .unwrap()
            .query_row("SELECT latency_ms, thought_tokens, error_kind FROM ai_usage", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert!(ms.is_some(), "latency is recorded");
        assert_eq!(thought, 40);
        assert_eq!(kind, None);
    }

    #[tokio::test]
    async fn prompt_is_scanned_and_personal_data_never_reaches_the_provider() {
        let (_d, l, conn) = ledger();
        let p = MockProvider::new(vec![Ok(good())]);
        let mut c = call();
        c.user = "La CURP es LOPM800101MDFRZN09 y vive aquí".into();
        c.context = vec!["Llamar al 55 1234 5678".into()];
        run(&p, &scanner(), &l, c).await.unwrap();
        let seen = p.requests();
        let sent = format!("{} {}", seen[0].user, seen[0].context.join(" "));
        assert!(!sent.contains("LOPM8") && !sent.contains("1234 5678"), "{sent}");
        assert!(sent.contains("[CURP OCULTA]"));
        assert_eq!(rows(&conn, "SELECT count(*) FROM audit_log WHERE event='scanner.leak_prevented'"), 1);
        let d: String = conn.lock().unwrap().query_row("SELECT details_json FROM audit_log WHERE event='scanner.leak_prevented'", [], |r| r.get(0)).unwrap();
        assert!(d.contains("\"curp\":1") && !d.contains("LOPM"));
    }

    #[tokio::test]
    async fn invalid_json_gets_one_retry_with_the_error() {
        let (_d, l, conn) = ledger();
        let bad = AiResponse { value: json!({"message": 5}), ..good() };
        let p = MockProvider::new(vec![Ok(bad), Ok(good())]);
        let v = run(&p, &scanner(), &l, call()).await.unwrap();
        assert!(v["root_hypothesis"].is_null());
        let reqs = p.requests();
        assert_eq!(reqs.len(), 2);
        assert!(reqs[1].user.contains("no cumplió el formato"));
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE success=0 AND error_kind='schema'"), 1);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE success=1"), 1);
    }

    #[tokio::test]
    async fn two_invalid_answers_end_in_an_error() {
        let (_d, l, _c) = ledger();
        let bad = || AiResponse { value: json!({"nope": 1}), ..good() };
        let p = MockProvider::new(vec![Ok(bad()), Ok(bad())]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::BadOutput(_))));
        assert_eq!(p.requests().len(), 2);
    }

    #[tokio::test]
    async fn at_the_monthly_cap_ai_is_paused_without_calling_the_provider() {
        let (_d, l, conn) = ledger();
        {
            let c = conn.lock().unwrap();
            let mut s = AiSettings::default();
            s.monthly_cap_mxn = 1.0;
            settings::save(&c, &s).unwrap();
            c.execute("INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,estimated_cost_mxn,success) VALUES ('x',strftime('%Y-%m-%dT%H:%M:%SZ','now'),'t','p','m',1,1,1.0,1)", []).unwrap();
        }
        let p = MockProvider::new(vec![Ok(good())]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::BudgetExhausted)));
        assert!(p.requests().is_empty());
    }

    #[tokio::test]
    async fn temporary_errors_are_retried_once_and_every_failed_attempt_is_logged() {
        let (_d, l, conn) = ledger();
        let p = MockProvider::new(vec![Err(AiError::RateLimited), Ok(good())]);
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.requests().len(), 2);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE error_kind='rate_limited' AND success=0"), 1);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE success=1"), 1);

        let p = MockProvider::new(vec![Err(AiError::Auth)]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Auth)));
        assert_eq!(p.requests().len(), 1);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE error_kind='auth'"), 1);
    }

    #[tokio::test]
    async fn the_system_prompt_is_fixed_and_the_task_sets_the_tier() {
        let (_d, l, _c) = ledger();
        let p = MockProvider::new(vec![Ok(good()), Ok(good())]);
        run(&p, &scanner(), &l, call()).await.unwrap();
        run(&p, &scanner(), &l, call()).await.unwrap();
        let r = p.requests();
        assert_eq!(r[0].system, r[1].system); // identical -> cacheable
        assert_eq!(r[0].tier, crate::ai::ModelTier::Light);
        assert_eq!(r[0].max_output_tokens, AiTask::ConversationTurn.max_output_tokens());
    }

    #[tokio::test]
    async fn at_the_daily_allowance_the_provider_is_not_called_at_all() {
        let (_d, l, conn) = ledger();
        limit_mock(&conn, 100, 3);
        seed_calls(&conn, 3, None);
        let p = MockProvider::new(vec![Ok(good())]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::QuotaReached)));
        assert!(p.requests().is_empty(), "no call is wasted once the day is used up");
    }

    #[tokio::test]
    async fn calls_that_never_left_the_computer_do_not_use_the_allowance() {
        let (_d, l, conn) = ledger();
        limit_mock(&conn, 100, 3);
        seed_calls(&conn, 5, Some("offline"));
        let p = MockProvider::new(vec![Ok(good())]);
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_minute_waits_instead_of_calling() {
        let (_d, l, conn) = ledger();
        limit_mock(&conn, 2, 100);
        seed_calls(&conn, 2, None);
        let p = MockProvider::new(vec![Ok(good())]);
        let t0 = tokio::time::Instant::now();
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert!(t0.elapsed() >= Duration::from_secs(55), "waited for the window: {:?}", t0.elapsed());
        assert_eq!(p.requests().len(), 1);
    }

    fn unavailable() -> Result<AiResponse, AiError> {
        Err(AiError::Http { status: 503, message: "This model is currently experiencing high demand".into() })
    }

    #[tokio::test]
    async fn an_overloaded_model_hands_over_to_the_next_one_at_once() {
        let (_d, l, conn) = ledger();
        let p = MockProvider::with_chain(vec![unavailable(), Ok(good())], &["m1", "m2"]);
        let t0 = Instant::now();
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.models_used(), vec!["m1", "m2"], "the overloaded model is not retried");
        assert!(t0.elapsed() < Duration::from_millis(1000), "no waiting before handing over");
        // the failure is filed under the model that failed, the answer under the one that answered
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE model='m1' AND error_kind='http_503' AND success=0"), 1);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE model='claude-haiku-4-5' AND success=1"), 1);
    }

    #[tokio::test]
    async fn a_timeout_moves_to_the_next_model_but_two_timeouts_stop_the_chain() {
        // found on a live run: a call that hung for 120 s was retried on the same model, and the
        // chain then spent its time on four more models
        let (_d, l, conn) = ledger();
        let p = MockProvider::with_chain(vec![Err(AiError::Timeout), Ok(good())], &["m1", "m2"]);
        let t0 = Instant::now();
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.models_used(), vec!["m1", "m2"], "the slow model is not retried");
        assert!(t0.elapsed() < Duration::from_millis(1000), "no waiting before handing over");
        // a timeout is a call that reached the service: it is logged as such and counts for its allowance
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE model='m1' AND error_kind='timeout'"), 1);
        assert_eq!(super::super::metrics::window(&conn.lock().unwrap(), "m1", 60).unwrap().calls, 1);

        let p = MockProvider::with_chain(vec![Err(AiError::Timeout), Err(AiError::Timeout), Ok(good())], &["m1", "m2", "m3"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Timeout)));
        assert_eq!(p.models_used(), vec!["m1", "m2"], "the third model is never asked");

        // with a single model there is nobody to hand over to, and no retry of a slow call
        let p = MockProvider::with_chain(vec![Err(AiError::Timeout), Ok(good())], &["solo"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Timeout)));
        assert_eq!(p.models_used(), vec!["solo"]);
    }

    fn request_for(task: AiTask, tier: ModelTier) -> AiRequest {
        AiRequest { task, tier, system: String::new(), context: vec![], user: "x".into(), output_schema: json!({}), max_output_tokens: 100 }
    }

    #[tokio::test]
    async fn the_call_reading_goes_to_the_light_model_first_and_the_strong_ones_are_its_backup() {
        let (_d, l, _conn) = ledger();
        let settings = l.settings().unwrap();
        let overloaded = || Err(AiError::Http { status: 503, message: "high demand".into() });
        let review = AiCall { task: AiTask::CallCanonical, ..call() };

        // the light model first; the strong ones answer when it cannot
        let p = MockProvider::with_chain(vec![overloaded(), overloaded(), Ok(good())], &["s1", "s2"]).with_light(&["lite"]);
        let r = call_provider(&p, &l, &settings, &review, &request_for(AiTask::CallCanonical, ModelTier::Strong)).await;
        assert!(r.is_ok());
        assert_eq!(p.models_used(), vec!["lite", "s1", "s2"]);
        // when the light one answers, nobody else is asked
        let p = MockProvider::with_chain(vec![Ok(good())], &["s1", "s2"]).with_light(&["lite"]);
        assert!(call_provider(&p, &l, &settings, &review, &request_for(AiTask::CallCanonical, ModelTier::Strong)).await.is_ok());
        assert_eq!(p.models_used(), vec!["lite"]);

        // a question or a summary does not lose quality unseen: it stays on the strong models
        let p = MockProvider::with_chain(vec![overloaded(), overloaded(), overloaded(), Ok(good())], &["s1", "s2"]).with_light(&["lite"]);
        let r = call_provider(&p, &l, &settings, &call(), &request_for(AiTask::DiagnosisSummary, ModelTier::Strong)).await;
        assert!(r.is_err());
        assert!(!p.models_used().contains(&"lite".to_string()));
    }

    #[test]
    fn the_background_reading_may_take_longer_than_a_question() {
        assert!(AiTask::CallCanonical.timeout_secs() > AiTask::DiagnosisSummary.timeout_secs());
        assert!(AiTask::DiagnosisSummary.timeout_secs() > AiTask::ConversationTurn.timeout_secs());
        assert!(crate::ai::ALL_TASKS.iter().all(|t| t.timeout_secs() >= 30));
    }

    #[tokio::test]
    async fn a_rejected_key_or_a_bad_answer_does_not_wander_through_the_chain() {
        let (_d, l, _c) = ledger();
        let p = MockProvider::with_chain(vec![Err(AiError::Auth), Ok(good())], &["m1", "m2"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Auth)));
        assert_eq!(p.models_used(), vec!["m1"]);
        let p = MockProvider::with_chain(vec![Err(AiError::Refused), Ok(good())], &["m1", "m2"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Refused)));
        assert_eq!(p.models_used(), vec!["m1"]);
    }

    #[tokio::test]
    async fn a_model_that_fills_its_whole_output_without_finishing_hands_over_to_the_next() {
        // found live: gemini-3-flash-preview with temperature 0 looped until it was cut, three runs in three
        let (_d, l, conn) = ledger();
        let p = MockProvider::with_chain(vec![Err(AiError::Truncated), Ok(good())], &["m1", "m2"]);
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.models_used(), vec!["m1", "m2"]);
        assert_eq!(rows(&conn, "SELECT count(*) FROM ai_usage WHERE model='m1' AND error_kind='truncated'"), 1);
    }

    #[tokio::test]
    async fn the_last_model_still_gets_one_retry_and_then_the_error_is_final() {
        let (_d, l, _c) = ledger();
        let p = MockProvider::with_chain(vec![unavailable(), unavailable(), Ok(good())], &["m1", "m2"]);
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.models_used(), vec!["m1", "m2", "m2"]);

        let p = MockProvider::with_chain(vec![unavailable(), unavailable(), unavailable()], &["m1", "m2"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::Http { status: 503, .. })));
        assert_eq!(p.models_used(), vec!["m1", "m2", "m2"]);
    }

    #[tokio::test]
    async fn a_model_out_of_daily_allowance_is_skipped_without_a_call() {
        let (_d, l, conn) = ledger();
        {
            let c = conn.lock().unwrap();
            let mut s = AiSettings::default();
            s.rate_limits.insert("m1".into(), RateLimit { per_minute: 100, tokens_per_minute: 0, per_day: 1 });
            settings::save(&c, &s).unwrap();
            c.execute(
                "INSERT INTO ai_usage (id,at,task,provider,model,input_tokens,output_tokens,estimated_cost_mxn,success)
                 VALUES ('s',strftime('%Y-%m-%dT%H:%M:%SZ','now'),'t','mock','m1',1,1,0,1)",
                [],
            )
            .unwrap();
        }
        let p = MockProvider::with_chain(vec![Ok(good())], &["m1", "m2"]);
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert_eq!(p.models_used(), vec!["m2"], "m1 was never called");
        // with nobody to hand over to, the same situation is a final answer
        let p = MockProvider::with_chain(vec![Ok(good())], &["m1"]);
        assert!(matches!(run(&p, &scanner(), &l, call()).await, Err(AiError::QuotaReached)));
        assert!(p.models_used().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn under_the_minute_limit_there_is_no_wait() {
        let (_d, l, conn) = ledger();
        limit_mock(&conn, 5, 100);
        seed_calls(&conn, 2, None);
        let p = MockProvider::new(vec![Ok(good())]);
        let t0 = tokio::time::Instant::now();
        assert!(run(&p, &scanner(), &l, call()).await.is_ok());
        assert!(t0.elapsed() < Duration::from_secs(1));
    }
}
