//! Dry run: "I play the model". A fake Gemini server answers over real HTTP the way the real
//! service would: it rejects any request that does not follow the documented shape, and the
//! answers are written by a model-like policy (a person would answer the same way).
//! The real `GeminiProvider`, pipeline, scanner, database, pacing and metrics run end to end,
//! so when the first real call is made, only the real model is new.
//!
//! Limits of this test: it knows Gemini's API only as documented (ADR-007). The first real call
//! (`gemini_smoke_live`) is what confirms the documentation was read right.

use super::gemini::GeminiProvider;
use super::settings::{self, AiSettings, ProviderKind, RateLimit};
use super::{build_provider, golden, metrics, AiProvider};
use crate::modules::projects::conversation::{conversation_view, retry, send_message, start_conversation, AnswerOutcome};
use crate::modules::projects::diagnosis::*;
use crate::modules::projects::domain::conversation::Phase;
use crate::test_support::project_in_diagnosis;
use crate::modules::projects::domain::priority::Scores;
use crate::core::profile::domain::ProfileInput;
use crate::scanner::guard::Decision;
use crate::storage::open_encrypted;
use crate::core::profile::storage as profile;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const DB_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
const API_KEY: &str = "clave-ficticia-de-prueba";

// ---------------------------------------------------------------- the fake service

struct Seen {
    model: String,
    task: &'static str,
    raw: String,
    body: Value,
}

#[derive(Default)]
struct FakeState {
    seen: Vec<Seen>,
    /// Requests the real service would have rejected with a 400.
    violations: Vec<String>,
    /// Answers served before the model plays: (HTTP status, body).
    inject: VecDeque<(u16, Value)>,
    /// How many times to answer with text that is not JSON.
    not_json: u32,
    calls_by_task: BTreeMap<&'static str, u32>,
}

#[derive(Clone, Default)]
struct FakeGemini(Arc<Mutex<FakeState>>);

fn google_error(status: u16, message: &str, grpc: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({"error": {"code": status, "message": message, "status": grpc}}))
}

fn tokens(s: &str) -> u64 {
    (s.chars().count() as u64).div_ceil(3)
}

/// What the real service checks in a request, as documented. Returns the problems found.
fn check_shape(b: &Value) -> Vec<String> {
    let mut p = Vec::new();
    let Some(top) = b.as_object() else { return vec!["the body is not a JSON object".into()] };
    for k in top.keys() {
        if !["systemInstruction", "contents", "generationConfig"].contains(&k.as_str()) {
            p.push(format!("Unknown name \"{k}\": Cannot find field."));
        }
    }
    if b["systemInstruction"]["parts"][0]["text"].as_str().map_or(true, str::is_empty) {
        p.push("systemInstruction.parts[0].text is missing".into());
    }
    match b["contents"].as_array() {
        Some(c) if c.len() == 1 && c[0]["role"] == "user" => {
            let parts = c[0]["parts"].as_array().cloned().unwrap_or_default();
            if parts.is_empty() {
                p.push("contents[0].parts is empty".into());
            }
            for part in parts {
                if part.as_object().map_or(true, |o| o.keys().any(|k| k != "text") || !part["text"].is_string()) {
                    p.push(format!("contents[0].parts has an invalid part: {part}"));
                }
            }
        }
        _ => p.push("contents must be one user turn".into()),
    }
    let g = &b["generationConfig"];
    match g.as_object() {
        None => p.push("generationConfig is missing".into()),
        Some(o) => {
            for k in o.keys() {
                if !["maxOutputTokens", "responseMimeType", "responseJsonSchema", "thinkingConfig", "temperature", "seed"].contains(&k.as_str()) {
                    p.push(format!("Unknown name \"{k}\" at 'generation_config'"));
                }
            }
            if o.get("temperature").is_some_and(|t| !t.as_f64().is_some_and(|t| (0.0..=2.0).contains(&t))) {
                p.push("temperature must be between 0 and 2".into());
            }
            if o.get("seed").is_some_and(|s| !s.is_i64()) {
                p.push("seed must be an integer".into());
            }
            if g["responseMimeType"] != "application/json" {
                p.push("responseMimeType must be application/json".into());
            }
            if !g["maxOutputTokens"].as_u64().map_or(false, |n| (1..=65_536).contains(&n)) {
                p.push("maxOutputTokens out of range".into());
            }
            if g["responseJsonSchema"]["type"] != "object" {
                p.push("responseJsonSchema must describe an object".into());
            }
            if g["responseJsonSchema"].to_string().contains("additionalProperties") {
                p.push("responseJsonSchema: additionalProperties is not part of what we rely on".into());
            }
            if let Some(t) = o.get("thinkingConfig") {
                let level = t["thinkingLevel"].as_str().unwrap_or("");
                if t.as_object().map_or(true, |t| t.len() != 1) || !["minimal", "low", "medium", "high"].contains(&level) {
                    p.push(format!("thinkingConfig is invalid: {t}"));
                }
            }
        }
    }
    p
}

fn task_of(b: &Value) -> &'static str {
    let props = &b["generationConfig"]["responseJsonSchema"]["properties"];
    if props.get("root_hypothesis").is_some() {
        "conversation.turn"
    } else if props.get("open_points").is_some() {
        "drafting.section"
    } else if props.get("problem_statement").is_some() {
        "diagnosis.summary"
    } else if props.get("needs").is_some() {
        "prioritization.propose_needs"
    } else {
        "unknown"
    }
}

fn texts(b: &Value) -> Vec<String> {
    b["contents"][0]["parts"].as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).map(String::from).collect()
}

/// The part I play: a model that reads the request and answers in the requested format.
fn play(task: &str, texts: &[String]) -> (Value, u64) {
    match task {
        "conversation.turn" => {
            let user = texts.last().cloned().unwrap_or_default();
            let convo = texts.iter().find(|t| t.starts_with("Conversación hasta ahora:")).cloned().unwrap_or_default();
            // what the person just wrote: the cause is that, quoted literally
            let last_person = convo.lines().rev().find(|l| l.starts_with("Persona: ")).map(|l| l.trim_start_matches("Persona: ").to_string()).unwrap_or_default();
            let quote: String = last_person.split_whitespace().take(5).collect::<Vec<_>>().join(" ");
            let cause = json!({"text": format!("Lo que dijo: {quote}"), "quote": quote});
            let level: u8 = user.split("porqué número ").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok()).unwrap_or(0);
            let root = "No hay un plan ni un fondo para adecuar la casa a las personas mayores";
            let turn = |message: &str, cause: Value, hypothesis: Option<&str>, fit: Option<&str>| {
                json!({"message": message, "cause": cause, "root_hypothesis": hypothesis, "options": [], "fit": fit, "fit_note": ""})
            };
            let value = if user.contains("Paso: apertura") {
                turn("Hola, para la convocatoria Apoyos de ejemplo: ¿qué proyecto tienen en mente, por qué no se ha podido y cómo cambiaría la vida de las personas?", Value::Null, None, None)
            } else if user.contains("Paso: primer porqué") {
                turn("Entiendo. ¿Por qué no se ha podido hacer?", Value::Null, None, Some("fits"))
            } else if user.contains("Paso: proponer de nuevo") {
                turn("Entiendo.", cause, Some(root), None)
            } else if level >= 3 && user.contains("root_hypothesis") && !user.contains("Todavía es pronto") {
                turn("Entonces la causa parece ser la falta de un sistema.", cause, Some(root), None)
            } else {
                turn("Entiendo. ¿Y por qué cree que pasa eso?", cause, None, None)
            };
            (value, 0)
        }
        "drafting.section" => (
            json!({
                "content": "La institución cambiará las instalaciones de la casa y dejará un responsable de revisarlas cada mes, para que el agua y la cocina estén seguras.",
                "open_points": ["Quién hará la obra"]
            }),
            1200,
        ),
        "diagnosis.summary" => (
            json!({
                "problem_statement": "Las personas adultas mayores con movilidad reducida no pueden bañarse con seguridad: el piso resbala, la regadera tiene un escalón y este año hubo 3 caídas, una con fractura de cadera.",
                "affected": {
                    "group": "Adultos mayores con movilidad reducida",
                    "count": 14,
                    "description": "Son 14 personas, 6 de ellas en silla de ruedas, y los otros baños están en la planta alta."
                },
                "current_consequences": [
                    "3 caídas en el año, una con fractura de cadera.",
                    "Bañar a una persona en silla de ruedas requiere a 2 personas y tarda casi 40 minutos.",
                    "En la noche solo hay 1 enfermera, así que el personal no alcanza a atender con seguridad."
                ],
                "root_causes": [
                    "El edificio es de los años 70 y no se pensó para personas mayores.",
                    "No hay barras de apoyo ni regadera a ras de piso.",
                    "Los tapetes antiderrapantes se mueven y falta dinero para hacer algo más."
                ],
                "reframed_need": "Seguridad y accesibilidad en la higiene de adultos mayores con movilidad reducida, con más autonomía y menos carga para el personal.",
                "alternatives": [
                    {"title": "Regadera a ras de piso con barras de apoyo", "pros": ["Quita el escalón", "Reduce el riesgo de caídas"], "cons": ["Requiere obra y cotización"]},
                    {"title": "Piso antiderrapante, silla de baño y puerta amplia", "pros": ["Mejora el acceso de las sillas de ruedas"], "cons": ["Se necesitan las medidas del baño"]},
                    {"title": "Capacitación del personal en movilización segura", "pros": ["Menos esfuerzo y más seguridad al bañar"], "cons": ["No sustituye la adecuación del espacio"]}
                ],
                "suggested_indicators": [
                    "Número de caídas en el baño por mes.",
                    "Tiempo promedio por baño asistido, en minutos.",
                    "Número de residentes que se bañan con menos ayuda."
                ],
                "open_questions": ["Cotizaciones de la obra.", "Medidas del baño.", "Costo anual de mantenimiento."]
            }),
            1800,
        ),
        "prioritization.propose_needs" => (
            json!({"needs": [
                {"title": "Higiene segura y accesible", "description": "Un espacio donde las personas con movilidad reducida puedan bañarse sin riesgo."},
                {"title": "Capacitación del personal en movilización segura", "description": "Que el personal pueda ayudar a bañar con menos esfuerzo y más seguridad."}
            ]}),
            900,
        ),
        _ => (json!({}), 0),
    }
}

impl Respond for FakeGemini {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let mut st = self.0.lock().unwrap();
        let raw = String::from_utf8_lossy(&req.body).to_string();
        let model = req.url.path().strip_prefix("/v1beta/models/").and_then(|p| p.strip_suffix(":generateContent")).map(String::from);
        let Some(model) = model else {
            st.violations.push(format!("wrong path {}", req.url.path()));
            return google_error(404, "not found", "NOT_FOUND");
        };
        if req.headers.get("x-goog-api-key").and_then(|v| v.to_str().ok()) != Some(API_KEY) {
            return google_error(400, "API key not valid. Please pass a valid API key.", "INVALID_ARGUMENT");
        }
        let Ok(body) = serde_json::from_slice::<Value>(&req.body) else {
            st.violations.push("the body is not JSON".into());
            return google_error(400, "Invalid JSON payload received.", "INVALID_ARGUMENT");
        };
        let problems = check_shape(&body);
        if !problems.is_empty() {
            st.violations.extend(problems.clone());
            return google_error(400, &format!("Invalid JSON payload received. {}", problems.join(" ")), "INVALID_ARGUMENT");
        }
        let task = task_of(&body);
        *st.calls_by_task.entry(task).or_insert(0) += 1;
        let earlier_same_task = st.calls_by_task[task] > 1;
        st.seen.push(Seen { model: model.clone(), task, raw, body: body.clone() });

        if let Some((status, b)) = st.inject.pop_front() {
            return ResponseTemplate::new(status).set_body_json(b);
        }
        let strong = task != "conversation.turn";
        let delay = Duration::from_millis(if strong { 100 } else { 30 });
        if st.not_json > 0 {
            st.not_json -= 1;
            let text = "Claro, con gusto: la repregunta sería sobre las caídas.";
            return ResponseTemplate::new(200).set_delay(delay).set_body_json(envelope(&model, &body, text, 0, 0));
        }
        let (answer, thoughts) = play(task, &texts(&body));
        // implicit caching: an identical system prompt seen before is billed at the cached price
        let cached = if strong && earlier_same_task { tokens(body["systemInstruction"]["parts"][0]["text"].as_str().unwrap_or("")) } else { 0 };
        ResponseTemplate::new(200).set_delay(delay).set_body_json(envelope(&model, &body, &answer.to_string(), thoughts, cached))
    }
}

/// A `generateContent` answer. Like the real service, zero counters are left out.
fn envelope(model: &str, request: &Value, text: &str, thoughts: u64, cached: u64) -> Value {
    let prompt = tokens(&request["systemInstruction"].to_string()) + tokens(&request["contents"].to_string());
    let answer = tokens(text);
    let mut usage = json!({"promptTokenCount": prompt, "candidatesTokenCount": answer, "totalTokenCount": prompt + answer + thoughts});
    if thoughts > 0 {
        usage["thoughtsTokenCount"] = json!(thoughts);
    }
    if cached > 0 {
        usage["cachedContentTokenCount"] = json!(cached);
    }
    let mut parts = Vec::new();
    if thoughts > 0 {
        parts.push(json!({"text": "Resumen de mi razonamiento sobre el caso.", "thought": true}));
    }
    parts.push(json!({"text": text}));
    json!({
        "candidates": [{"content": {"role": "model", "parts": parts}, "finishReason": "STOP", "index": 0}],
        "usageMetadata": usage,
        "modelVersion": format!("{model}-001"),
        "responseId": "dry-run"
    })
}

// ---------------------------------------------------------------- harness

struct Harness {
    _dir: tempfile::TempDir,
    _server: MockServer,
    db: SharedDb,
    fake: FakeGemini,
    provider: GeminiProvider,
    settings: AiSettings,
    pid: String,
}

async fn harness(tune: impl FnOnce(&mut AiSettings)) -> Harness {
    let fake = FakeGemini::default();
    let server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(fake.clone()).mount(&server).await;

    let dir = tempfile::tempdir().unwrap();
    let mut conn = open_encrypted(&dir.path().join("t.db"), DB_KEY).unwrap();
    let input: ProfileInput = serde_json::from_str(include_str!("../../../fixtures/institucion-asilo.json")).unwrap();
    profile::save_finished(&mut conn, &input);
    let mut settings = AiSettings::default();
    // The chain tests below are about how the chain behaves, not about which model is first by
    // default (that is checked in `settings`), so they run on a fixed chain.
    settings.gemini_model_strong = "gemini-3.8-flash".into();
    settings.gemini_strong_fallbacks = vec!["gemini-3.7-flash".into(), "gemini-3.6-flash".into()];
    settings.disabled_models.clear();
    // one light model, so the tests about retries and allowances talk to a single model
    settings.gemini_light_fallbacks.clear();
    tune(&mut settings);
    settings::save(&conn, &settings).unwrap();
    let db: SharedDb = Arc::new(Mutex::new(conn));
    let pid = project_in_diagnosis(&db);
    let provider = GeminiProvider::with_base_url(API_KEY.into(), settings.clone(), server.uri());
    Harness { _dir: dir, _server: server, db, fake, provider, settings, pid }
}

impl Harness {
    fn p(&self) -> Option<&dyn AiProvider> {
        Some(&self.provider)
    }

    /// Has the whole conversation like the golden case does, until the root cause is confirmed. Returns what the
    /// AI part of each step did.
    async fn diagnose(&self) -> Vec<AiStatus> {
        let mut statuses = Vec::new();
        let mut whys = golden::WHY_ANSWERS.iter();
        let mut out = start_conversation(&self.db, self.p(), &self.pid).await.unwrap();
        for _ in 0..30 {
            let AnswerOutcome::Saved { view, ai } = out else { panic!("quarantine on a golden answer") };
            statuses.push(ai);
            out = match view.phase {
                Phase::Closed => return statuses,
                Phase::AwaitingAnswer => {
                    let text = if view.turns.len() == 1 { golden::OPENING_ANSWER } else { whys.next().expect("the conversation did not reach a root cause") };
                    send_message(&self.db, self.p(), &self.pid, text, false, None).await.unwrap()
                }
                Phase::RootProposed => send_message(&self.db, self.p(), &self.pid, "", true, None).await.unwrap(),
                Phase::AwaitingAi => retry(&self.db, self.p(), &self.pid).await.unwrap(),
                Phase::NeedsOpening => start_conversation(&self.db, self.p(), &self.pid).await.unwrap(),
            };
        }
        panic!("too many turns")
    }

    fn requests(&self) -> Vec<(String, &'static str)> {
        self.fake.0.lock().unwrap().seen.iter().map(|s| (s.model.clone(), s.task)).collect()
    }

    fn count(&self, task: &str) -> usize {
        self.requests().iter().filter(|(_, t)| *t == task).count()
    }

    fn violations(&self) -> Vec<String> {
        self.fake.0.lock().unwrap().violations.clone()
    }

    fn inject(&self, status: u16, body: Value) {
        self.fake.0.lock().unwrap().inject.push_back((status, body));
    }

    fn q<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.db.lock().unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    async fn summary_confirmed(&self) -> SummaryOutcome {
        self.diagnose().await;
        let out = generate_summary(&self.db, self.p(), &self.pid).await.unwrap();
        confirm_summary(&self.db, &self.pid).unwrap();
        advance(&self.db, &self.pid).unwrap();
        out
    }
}

fn per_minute_error() -> Value {
    json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED",
        "message": "Quota exceeded for metric: GenerateRequestsPerMinutePerProjectPerModel-FreeTier"}})
}

fn per_day_error() -> Value {
    json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED", "message": "You exceeded your current quota.",
        "details": [{"violations": [{"quotaId": "GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]}]}})
}

// ---------------------------------------------------------------- the dry run

#[tokio::test]
async fn the_fake_service_really_rejects_requests_that_do_not_follow_the_documented_shape() {
    // If this passes vacuously, the full run below would prove nothing.
    let h = harness(|_| {}).await;
    let url = format!("{}/v1beta/models/gemini-3.5-flash-lite:generateContent", h._server.uri());
    let http = reqwest::Client::new();
    let bad = json!({"contents": [{"role": "user", "parts": [{"text": "x"}]}], "generationConfig": {"topK": 20}});
    let r = http.post(&url).header("x-goog-api-key", API_KEY).json(&bad).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert!(h.violations().iter().any(|v| v.contains("topK")), "{:?}", h.violations());
    let r = http.post(&url).header("x-goog-api-key", "otra").json(&bad).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 400);
}

#[tokio::test]
async fn full_golden_case_through_the_real_gemini_provider_and_pipeline() {
    let h = harness(|_| {}).await;
    let started = std::time::Instant::now();

    // ---- the conversation: opening, three whys, the root cause proposed and confirmed by the person
    let statuses = h.diagnose().await;
    assert_eq!(statuses, vec![AiStatus::Used, AiStatus::Used, AiStatus::Used, AiStatus::Used, AiStatus::Used, AiStatus::Skipped], "the confirmation needs no AI");
    let view = conversation_view(&h.db.lock().unwrap(), &h.pid).unwrap();
    assert_eq!(view.phase, Phase::Closed);
    assert_eq!(view.root.as_ref().map(|r| r.text.as_str()), Some("No hay un plan ni un fondo para adecuar la casa a las personas mayores"));
    assert_eq!(view.fit.as_ref().map(|f| f.fit.as_str()), Some("fits"));

    // ---- summary written by the model, checked by code
    let out = generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(out.ai, AiStatus::Used);
    assert!(out.view.unsupported_figures.is_empty(), "{:?}", out.view.unsupported_figures);
    let summary = out.view.summary.clone().unwrap();
    assert_eq!(summary.origin, "ai_assumption");
    let criteria = golden::criteria(&summary.summary, &out.view.unsupported_figures);
    golden::print(&criteria);
    let failed: Vec<_> = criteria.iter().filter(|c| !c.ok).map(|c| c.name).collect();
    assert!(failed.is_empty(), "criteria machinery rejects an ideal summary: {failed:?}");
    confirm_summary(&h.db, &h.pid).unwrap();
    advance(&h.db, &h.pid).unwrap();

    // ---- prioritization: AI proposes, code scores
    let (needs, ai) = propose_needs(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!((ai, needs.needs.len()), (AiStatus::Used, 2));
    let id = needs.needs[0].id.clone();
    let view = rate_need(&h.db, &h.pid, &id, Scores { beneficiaries: 5, severity: 5, mission: 5, feasibility: 3, sustainability: 4 }).unwrap();
    assert_eq!(view.needs[0].total_score, Some(91.0));
    select_need(&h.db, &h.pid, &id).unwrap();
    println!("\nRecorrido completo (HTTP real, servicio simulado): {:?}", started.elapsed());

    // ---- nothing the real service would reject, and the right models for the right tiers
    assert!(h.violations().is_empty(), "{:?}", h.violations());
    assert_eq!((h.count("conversation.turn"), h.count("diagnosis.summary"), h.count("prioritization.propose_needs")), (5, 1, 1));
    for (model, task) in h.requests() {
        let expect = if task == "conversation.turn" { "gemini-3.5-flash-lite" } else { "gemini-3.8-flash" };
        assert_eq!(model, expect, "{task}");
    }

    // ---- every call left a metrics row: timed, priced, with thinking kept apart
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage"), 7);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE success=1 AND provider='gemini' AND latency_ms IS NOT NULL AND latency_ms > 0"), 7);
    assert_eq!(h.q::<i64>("SELECT COALESCE(SUM(thought_tokens),0) FROM ai_usage WHERE task='conversation.turn'"), 0);
    assert_eq!(h.q::<i64>("SELECT COALESCE(SUM(thought_tokens),0) FROM ai_usage WHERE task!='conversation.turn'"), 2700);
    // the stored cost is exactly what the price table says for what was used
    let rows: Vec<(String, i64, i64, i64, f64)> = {
        let c = h.db.lock().unwrap();
        let mut st = c.prepare("SELECT model,input_tokens,output_tokens,cached_tokens,estimated_cost_mxn FROM ai_usage").unwrap();
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap().collect::<Result<_, _>>().unwrap();
        v
    };
    let today = settings::today_utc();
    for (model, i, o, cached, cost) in rows {
        let usage = super::Usage { input_tokens: i as u64, output_tokens: o as u64, cache_read_tokens: cached as u64, ..Default::default() };
        assert!((h.settings.cost_mxn_on(&model, &usage, &today) - cost).abs() < 1e-9, "{model}");
        assert!(cost > 0.0);
    }

    // ---- the metrics a person (or this test) reads afterwards
    let report = metrics::usage_report(&h.db.lock().unwrap(), &h.settings).unwrap();
    println!("\n{}", metrics::format_report(&report));
    let lite = report.models.iter().find(|m| m.model == "gemini-3.5-flash-lite").unwrap();
    let strong = report.models.iter().find(|m| m.model == "gemini-3.8-flash").unwrap();
    assert_eq!((lite.calls_total, strong.calls_total), (5, 2));
    assert_eq!((lite.calls_last_day, strong.calls_last_day), (5, 2));
    assert!(lite.avg_latency_ms.unwrap() >= 30 && strong.avg_latency_ms.unwrap() >= 100);
    assert!(strong.thought_tokens > 0 && lite.thought_tokens == 0);
    assert_eq!(strong.limit.unwrap().per_day, 20, "the allowance bar knows the limit");
    assert!(report.month_spend_mxn > 0.0 && report.month_spend_mxn < report.cap_mxn);
    assert!((report.month_spend_mxn - report.models.iter().map(|m| m.cost_mxn).sum::<f64>()).abs() < 1e-9);
    assert_eq!(report.tasks.len(), 3);
}

#[tokio::test]
async fn personal_data_never_reaches_the_service() {
    let h = harness(|_| {}).await;
    start_conversation(&h.db, h.p(), &h.pid).await.unwrap(); // the opening
    let before = h.requests().len();
    let risky = "La señora con CURP LOPM800101MDFRZN09 se cayó y su teléfono es 55 1234 5678";
    // first barrier: quarantine, and nothing is sent
    let AnswerOutcome::Quarantine { .. } = send_message(&h.db, h.p(), &h.pid, risky, false, None).await.unwrap() else { panic!() };
    assert_eq!(h.requests().len(), before, "nothing is sent while the text is in quarantine");
    // the person chooses to cover it: only the covered text travels
    let AnswerOutcome::Saved { ai, .. } = send_message(&h.db, h.p(), &h.pid, risky, false, Some(Decision::Redact)).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Used);
    let sent: String = h.fake.0.lock().unwrap().seen.iter().map(|s| s.raw.clone()).collect();
    assert!(!sent.contains("LOPM8") && !sent.contains("1234 5678"), "{sent}");
    assert!(sent.contains("CURP OCULTA"));
}

#[tokio::test]
async fn the_system_prompt_is_identical_between_calls_so_it_can_be_cached() {
    let h = harness(|_| {}).await;
    h.diagnose().await;
    let seen = h.fake.0.lock().unwrap();
    let systems: Vec<&Value> = seen.seen.iter().filter(|s| s.task == "conversation.turn").map(|s| &s.body["systemInstruction"]).collect();
    assert!(systems.len() > 1 && systems.windows(2).all(|w| w[0] == w[1]));
    // and the question that changes is last, after the stable context
    let parts = seen.seen[0].body["contents"][0]["parts"].as_array().unwrap().clone();
    assert!(parts.last().unwrap()["text"].as_str().unwrap().contains("Paso:"));
}

#[tokio::test]
async fn a_repeated_strong_prompt_is_billed_as_cached_and_costs_less() {
    let h = harness(|_| {}).await;
    h.diagnose().await;
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap(); // the person asks for the summary again
    let rows: Vec<(i64, i64, f64)> = {
        let c = h.db.lock().unwrap();
        let mut st = c.prepare("SELECT input_tokens, cached_tokens, estimated_cost_mxn FROM ai_usage WHERE task='diagnosis.summary' ORDER BY rowid").unwrap();
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<_, _>>().unwrap();
        v
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, 0, "the first call has nothing cached");
    assert!(rows[1].1 > 0, "the second call reuses the fixed prompt");
    // the cached part is not billed again as fresh input: input fell by what was cached
    assert_eq!(rows[0].0 - rows[1].0, rows[1].1);
    assert!(rows[1].2 < rows[0].2);
}

fn overloaded() -> Value {
    json!({"error": {"code": 503, "status": "UNAVAILABLE",
        "message": "This model is currently experiencing high demand. Spikes in demand are usually temporary. Please try again later."}})
}

fn summary_models(h: &Harness) -> Vec<String> {
    h.requests().into_iter().filter(|(_, t)| *t == "diagnosis.summary").map(|(m, _)| m).collect()
}

#[tokio::test]
async fn an_overloaded_main_model_hands_the_summary_to_the_next_model_without_retrying_it() {
    // the exact failure of the first live run: 503 "high demand" on gemini-3.8-flash
    let h = harness(|_| {}).await;
    h.diagnose().await;
    h.inject(503, overloaded());
    let out = generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(out.ai, AiStatus::Used, "the person never notices");
    assert_eq!(summary_models(&h), vec!["gemini-3.8-flash", "gemini-3.7-flash"], "no second try on the overloaded one");
    assert!(out.view.unsupported_figures.is_empty());
    // the metrics tell the story: the failure under the main model, the answer under the backup
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE model='gemini-3.8-flash' AND error_kind='http_503'"), 1);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE model='gemini-3.7-flash' AND success=1 AND task='diagnosis.summary'"), 1);
    let report = metrics::usage_report(&h.db.lock().unwrap(), &h.settings).unwrap();
    let backup = report.models.iter().find(|m| m.model == "gemini-3.7-flash").unwrap();
    assert!(backup.is_fallback && backup.calls_total == 1);
    // the cost is the price of the model that answered
    let cost: f64 = h.q("SELECT estimated_cost_mxn FROM ai_usage WHERE model='gemini-3.7-flash'");
    assert!(cost > 0.0);
}

#[tokio::test]
async fn with_the_default_chain_the_summary_goes_to_3_flash_preview_because_3_5_flash_is_switched_off() {
    // the product's own default (ADR-009), not the fixed test chain
    let h = harness(|s| {
        let d = AiSettings::default();
        s.gemini_model_strong = d.gemini_model_strong;
        s.gemini_strong_fallbacks = d.gemini_strong_fallbacks;
        s.disabled_models = d.disabled_models;
    })
    .await;
    h.diagnose().await;
    let out = generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(out.ai, AiStatus::Used);
    assert_eq!(summary_models(&h), vec!["gemini-3-flash-preview"], "the model that worked in the live runs goes first");
    h.inject(503, overloaded());
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(summary_models(&h), vec!["gemini-3-flash-preview"; 3], "3.5 Flash is never asked while it is switched off; the last model gets one retry");
}

#[tokio::test]
async fn a_daily_quota_answer_for_the_main_model_moves_on_to_the_next_one() {
    let h = harness(|_| {}).await;
    h.diagnose().await;
    h.inject(429, per_day_error());
    let out = generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(out.ai, AiStatus::Used);
    assert_eq!(summary_models(&h), vec!["gemini-3.8-flash", "gemini-3.7-flash"]);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE model='gemini-3.8-flash' AND error_kind='quota_reached'"), 1);
}

#[tokio::test]
async fn a_main_model_with_no_allowance_left_is_skipped_locally_and_costs_no_call() {
    let h = harness(|s| {
        s.rate_limits.insert("gemini-3.8-flash".into(), RateLimit { per_minute: 5, tokens_per_minute: 250_000, per_day: 1 });
    })
    .await;
    h.diagnose().await;
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap(); // uses the only call of the 3.8 today
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap(); // the person asks again
    assert_eq!(summary_models(&h), vec!["gemini-3.8-flash", "gemini-3.7-flash"], "the second went straight to the backup");
}

#[tokio::test]
async fn when_every_model_of_the_chain_is_down_the_summary_waits_and_nothing_is_made_up() {
    let h = harness(|_| {}).await;
    h.diagnose().await;
    // 3.8, 3.7, 3.6 and the single retry of the last one
    for _ in 0..4 {
        h.inject(503, overloaded());
    }
    let out = generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(out.ai, AiStatus::Unavailable);
    assert_eq!(summary_models(&h), vec!["gemini-3.8-flash", "gemini-3.7-flash", "gemini-3.6-flash", "gemini-3.6-flash"]);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE error_kind='http_503'"), 4);
    // there is no summary made without the AI: the person tries again when the service is back
    assert!(out.view.summary.is_none());
    assert_eq!(generate_summary(&h.db, h.p(), &h.pid).await.unwrap().ai, AiStatus::Used);
}

#[tokio::test]
async fn the_light_model_has_no_backup_so_a_busy_service_just_means_one_retry() {
    let h = harness(|_| {}).await;
    h.inject(503, overloaded());
    let AnswerOutcome::Saved { ai, .. } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Used);
    assert_eq!(h.requests().iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(), vec!["gemini-3.5-flash-lite"; 2]);
}

#[tokio::test]
async fn the_local_daily_allowance_stops_calls_before_they_leave_the_computer() {
    let h = harness(|s| {
        s.rate_limits.insert("gemini-3.8-flash".into(), RateLimit { per_minute: 5, tokens_per_minute: 250_000, per_day: 1 });
        s.gemini_strong_fallbacks.clear(); // no backup: the allowance is the end of the line
    })
    .await;
    h.summary_confirmed().await; // uses the only strong call of the day
    assert_eq!(h.count("diagnosis.summary"), 1);
    let (needs, ai) = propose_needs(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(ai, AiStatus::QuotaReached);
    assert!(needs.needs.is_empty(), "the person adds needs by hand, nothing breaks");
    assert_eq!(h.count("prioritization.propose_needs"), 0, "the second strong call never reached the service");
    // the light model has its own, larger allowance and keeps working
    assert_eq!(h.count("conversation.turn"), 5);
}

#[tokio::test]
async fn a_daily_quota_answer_from_the_service_is_final_and_not_retried() {
    let h = harness(|_| {}).await;
    h.inject(429, per_day_error());
    let AnswerOutcome::Saved { view, ai } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::QuotaReached);
    assert_eq!(h.requests().len(), 1, "no second attempt against an empty daily allowance");
    assert_eq!((view.phase, view.turns.len()), (Phase::NeedsOpening, 0), "nothing is made up in its place");
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE error_kind='quota_reached' AND success=0"), 1);
}

#[tokio::test]
async fn a_busy_minute_is_retried_once_and_both_attempts_are_in_the_metrics() {
    let h = harness(|_| {}).await;
    h.inject(429, per_minute_error());
    let AnswerOutcome::Saved { ai, .. } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Used);
    assert_eq!(h.requests().len(), 2);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE error_kind='rate_limited'"), 1);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE success=1"), 1);
    // both attempts reached the service, so both count toward its allowance
    let m = metrics::window(&h.db.lock().unwrap(), "gemini-3.5-flash-lite", 60).unwrap();
    assert_eq!(m.calls, 2);
}

#[tokio::test]
async fn a_rejected_key_is_reported_in_plain_words_and_not_retried() {
    let h = harness(|_| {}).await;
    h.inject(400, json!({"error": {"code": 400, "message": "API key not valid. Please pass a valid API key.", "status": "INVALID_ARGUMENT"}}));
    let AnswerOutcome::Saved { ai, .. } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::KeyRejected);
    assert_eq!(h.requests().len(), 1);
}

#[tokio::test]
async fn a_cut_off_or_blocked_answer_never_blocks_the_person() {
    let h = harness(|_| {}).await;
    // thinking ate the budget: finishReason MAX_TOKENS
    h.inject(200, json!({"candidates": [{"content": {"role": "model"}, "finishReason": "MAX_TOKENS"}],
                         "usageMetadata": {"promptTokenCount": 900, "thoughtsTokenCount": 350}}));
    let AnswerOutcome::Saved { ai, view } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Unavailable);
    assert_eq!(view.phase, Phase::NeedsOpening, "the person can try again");
    // the service declines the prompt
    h.inject(200, json!({"promptFeedback": {"blockReason": "PROHIBITED_CONTENT"}}));
    let AnswerOutcome::Saved { ai, .. } = retry(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Unavailable);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE error_kind IN ('truncated','refused')"), 2);
}

#[tokio::test]
async fn an_answer_that_is_not_json_is_corrected_once_with_the_error() {
    let h = harness(|_| {}).await;
    h.fake.0.lock().unwrap().not_json = 1;
    let AnswerOutcome::Saved { ai, .. } = start_conversation(&h.db, h.p(), &h.pid).await.unwrap() else { panic!() };
    assert_eq!(ai, AiStatus::Used);
    let seen = h.fake.0.lock().unwrap();
    assert_eq!(seen.seen.len(), 2);
    let second = texts(&seen.seen[1].body).last().cloned().unwrap();
    assert!(second.contains("no cumplió el formato"), "{second}");
    drop(seen);
    assert_eq!(h.q::<i64>("SELECT count(*) FROM ai_usage WHERE error_kind='bad_output'"), 1);
}

#[tokio::test]
async fn the_provider_follows_the_setting_and_each_one_reads_its_own_models() {
    let mut s = AiSettings::default();
    assert_eq!(build_provider(&s, "k".into()).name(), "gemini");
    s.provider = ProviderKind::Anthropic;
    let p = build_provider(&s, "k".into());
    assert_eq!(p.name(), "anthropic");
    assert_eq!(p.model_name(super::ModelTier::Strong), "claude-sonnet-5-5");
    s.provider = ProviderKind::Gemini;
    assert_eq!(build_provider(&s, "k".into()).model_name(super::ModelTier::Strong), "gemini-3-flash-preview");
}

#[tokio::test]
async fn a_model_switched_off_is_never_asked_and_switching_it_on_puts_it_first() {
    let h = harness(|s| {
        let d = AiSettings::default();
        s.gemini_model_strong = d.gemini_model_strong;
        s.gemini_strong_fallbacks = d.gemini_strong_fallbacks;
        s.disabled_models = vec!["gemini-3.5-flash".into()];
    })
    .await;
    h.diagnose().await;
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert!(!summary_models(&h).contains(&"gemini-3.5-flash".to_string()));

    let h = harness(|s| {
        let d = AiSettings::default();
        s.gemini_model_strong = d.gemini_model_strong;
        s.gemini_strong_fallbacks = d.gemini_strong_fallbacks;
        s.disabled_models.clear();
    })
    .await;
    h.diagnose().await;
    generate_summary(&h.db, h.p(), &h.pid).await.unwrap();
    assert_eq!(summary_models(&h), vec!["gemini-3.5-flash"]);
}

#[tokio::test]
async fn the_whole_path_from_the_conversation_to_the_guide_in_word_through_the_real_provider() {
    use crate::modules::projects::diagnosis::{add_need, select_need, AddNeedOutcome};
    use crate::modules::projects::domain::budget::Funder;
    use crate::modules::projects::domain::sections::SectionKind;
    use crate::modules::projects::drafting::{confirm_budget, confirm_schedule, confirm_text, draft_section, drafting_view, save_activity, save_budget_item, save_text, BudgetItemInput};
    let h = harness(|_| {}).await;
    h.summary_confirmed().await; // the conversation, the summary and the step to the goal
    let AddNeedOutcome::Saved { view } = add_need(&h.db, &h.pid, "Un sistema de mantenimiento preventivo", "Con responsable y fondo propio", None).unwrap() else { panic!() };
    select_need(&h.db, &h.pid, &view.needs[0].id).unwrap();
    assert_eq!(advance(&h.db, &h.pid).unwrap().stage, crate::modules::projects::domain::stage::Stage::Drafting);

    // the AI drafts one section; the person writes the others
    let out = draft_section(&h.db, h.p(), &h.pid, "what").await.unwrap();
    assert_eq!(out.ai, AiStatus::Used);
    assert!(out.view.sections.iter().find(|s| s.spec.key == "what").unwrap().content.contains("responsable de revisarlas"));
    save_budget_item(&h.db, &h.pid, BudgetItemInput { id: None, category: "material".into(), description: "Tubería".into(), quantity: 2.0, unit: None, unit_price_mxn: 1000.0, vat_included: false, funded_by: Funder::Requested, administrative: false }, None).unwrap();
    save_activity(&h.db, &h.pid, None, "Cambio de tuberías", 1, 3, None).unwrap();
    let required: Vec<String> = drafting_view(&h.db.lock().unwrap(), &h.pid).unwrap().sections.into_iter().filter(|s| s.spec.required && s.spec.kind == SectionKind::Text).map(|s| s.spec.key).collect();
    for key in &required {
        if key != "what" {
            save_text(&h.db, &h.pid, key, "Texto escrito por la persona.", None).unwrap();
        }
        confirm_text(&h.db, &h.pid, key).unwrap();
    }
    confirm_budget(&h.db, &h.pid).unwrap();
    confirm_schedule(&h.db, &h.pid).unwrap();
    assert_eq!(advance(&h.db, &h.pid).unwrap().stage, crate::modules::projects::domain::stage::Stage::Review);
    assert_eq!(advance(&h.db, &h.pid).unwrap().stage, crate::modules::projects::domain::stage::Stage::Ready);

    // the guide is written by the code; what the AI said reaches it only because the person confirmed it
    let dir = tempfile::tempdir().unwrap();
    let out = crate::modules::projects::guide::export_guide(&h.db, &h.pid, dir.path()).unwrap();
    let text = crate::documents::canonical::package::read_pieces(std::path::Path::new(&out.path)).unwrap().join("\n");
    assert!(text.contains("responsable de revisarlas") && text.contains("Tubería") && text.contains("$2,320.00"), "{text}");

    // nothing the real service would reject, and every call in its place
    assert!(h.violations().is_empty(), "{:?}", h.violations());
    assert_eq!((h.count("conversation.turn"), h.count("diagnosis.summary"), h.count("drafting.section")), (5, 1, 1));
    for (model, task) in h.requests() {
        let expect = if task == "conversation.turn" { "gemini-3.5-flash-lite" } else { "gemini-3.8-flash" };
        assert_eq!(model, expect, "{task}");
    }
}
