//! The battery of the chat (IA-4): the cases in `chat_battery.json`, by category, with the outcomes each may end in.
//! They run on the capabilities the app links (Mi institución and the sample), with a profile taken from the fictitious
//! data of `fixtures/`.
//!
//! - With the simulated model, in every `cargo test`: each case says what the model answers, and the battery checks
//!   that the platform does the right thing with it, the controls included (a model that names what was not offered,
//!   a text that may not go to the provider). No network and no spending.
//! - With a real model (`chat_battery_live`, ignored by default): the same cases, the answers of the model measured
//!   against what each case expects, with the tokens and the cost of every call, and the words (path A of E1) next to
//!   the data (path B). At most 12 calls per minute; the whole output goes to a file.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use capability::Registry;
use kernel::Modules;
use platform::models::{Failure, Models, Reply};
use platform::operators::Operator;
use platform::outcome::{Ask, Body};
use platform::policy::{Conditions, Provider, ToModel};
use platform::request::{Channel, Content};
use platform::task::TaskState;
use platform::{Accounts, Answer, Clock, Deliver, Outcome, Request, Runtime, Who};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};

const ANA: &str = "usr_01JABCDEFGHJKMNPQRSTVWXYZ0";
const WHO: Who = Who { account: ANA, organization: "org" };

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    category: String,
    turns: Vec<Turn>,
}

#[derive(Debug, Deserialize)]
struct Turn {
    text: String,
    /// What the simulated router answers; none when no model is called.
    router: Option<Value>,
    /// What the simulated `ia.responder` writes.
    responder: Option<String>,
    /// Whether the turn goes on from the one before, as the router should say.
    continues: Option<bool>,
    /// The outcomes this turn may end in: any of them is right.
    expect: Vec<Expected>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Expected {
    outcome: String,
    capability: Option<String>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("chat_battery.json")).expect("the battery is JSON")
}

/// What a turn ended in: a kind and, when there is one, the capability.
fn got(o: &Outcome) -> Expected {
    let (outcome, capability) = match &o.body {
        Body::Answer { evidence, .. } => ("answer", evidence.first().and_then(|e| e.reference.as_deref()).map(|r| r.split('#').next().unwrap_or(r).to_string())),
        Body::Data { capability, .. } => ("answer", Some(capability.clone())),
        Body::Clarification { ask: Ask::Choose { .. }, .. } => ("choose", None),
        Body::Clarification { capability, .. } => ("clarify", Some(capability.clone())),
        Body::Error { code, capability, .. } => (code.as_str(), capability.clone()),
        Body::Proposal { capability, .. } => ("proposal", Some(capability.clone())),
        Body::State { reason } => (reason.as_str(), None),
    };
    Expected { outcome: outcome.into(), capability }
}

fn right(expect: &[Expected], got: &Expected) -> bool {
    expect.iter().any(|e| e.outcome == got.outcome && (e.capability.is_none() || e.capability == got.capability))
}

struct Ana;

impl Accounts for Ana {
    fn now(&self, _: &Connection, who: &Who) -> Option<kernel::Account> {
        (who.account == ANA).then(|| kernel::Account { id: ANA.into(), organization: "org".into(), role: "manager".into(), active: true, must_change_password: false })
    }
}

struct Ticks(Cell<i64>);

impl Clock for Ticks {
    fn now_ms(&self) -> i64 {
        self.0.set(self.0.get() + 1);
        self.0.get()
    }
}

struct Nowhere;

impl Deliver for Nowhere {
    fn deliver(&self, _: &Outcome) {}
}

const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

/// The database as the app opens it, in a folder that goes with it, with a profile of Mi institución from the
/// fictitious data: its name and its mission, saved from its screen like a person would.
fn opened(registry: &Registry, modules: &Modules) -> (tempfile::TempDir, Mutex<Connection>) {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = crate::storage::open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
    platform::migrate(&mut conn).unwrap();
    let fixture: Value = serde_json::from_str(include_str!("../../../fixtures/institucion-casa-hogar.json")).unwrap();
    let institution = &fixture["institution"];
    let runtime = Runtime { registry, modules, accounts: &Ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: None, answer: Answer::Data };
    let screen = Channel::Screen { place: "institution".into() };
    let input = json!({ "window": "identity", "name": institution["name"], "mission": institution["mission"] });
    let asked = runtime
        .submit(&mut conn, &WHO, Request { channel: screen.clone(), task: None, content: Content::Invoke { capability: "mi_institucion.guardar_ventana".into(), input } })
        .unwrap();
    let Body::Proposal { fingerprint, .. } = &asked.body else { panic!("the profile could not be prepared: {asked:?}") };
    let content = Content::Decision { fingerprint: fingerprint.clone(), approve: true };
    let saved = runtime.submit(&mut conn, &WHO, Request { channel: screen, task: Some(asked.task.clone()), content }).unwrap();
    assert_eq!(saved.state, TaskState::Done, "{saved:?}");
    (dir, Mutex::new(conn))
}

/// A turn of the chat as the app runs it: the request answers at once, and the rest goes on in `think`.
fn turn(runtime: &Runtime, db: &Mutex<Connection>, task: Option<&str>, text: &str) -> Outcome {
    let request = Request { channel: Channel::Chat, task: task.map(String::from), content: Content::Text { text: text.into() } };
    let first = runtime.submit(&mut db.lock().unwrap(), &WHO, request).unwrap();
    if first.state != TaskState::Running {
        return first;
    }
    runtime.think(db, &WHO, &first.task).unwrap().expect("a running task goes on")
}

fn follows(db: &Mutex<Connection>, task: &str) -> bool {
    db.lock().unwrap().query_row("SELECT follows IS NOT NULL FROM ai_task WHERE id = ?1", [task], |r| r.get(0)).unwrap()
}

/// The simulated model: what each turn of a case says it answers, in order.
struct Simulated {
    provider: Provider,
    replies: RefCell<VecDeque<(&'static str, Value)>>,
}

impl Models for Simulated {
    fn provider(&self) -> &Provider {
        &self.provider
    }

    fn call(&self, operator: &Operator, _: &ToModel) -> Result<Reply, Failure> {
        let (expected, value) = self.replies.borrow_mut().pop_front().unwrap_or_else(|| panic!("nothing more to say to {}", operator.id));
        assert_eq!(operator.id, expected);
        Ok(Reply { value, model: "simulado".into(), input_tokens: 0, output_tokens: 0, cost_mxn: 0.0 })
    }
}

/// Every case of the battery, with the simulated model: the platform does with each answer what it has to, and each
/// turn ends in one of the outcomes its case allows.
#[test]
fn chat_battery() {
    let registry = Registry::discover().unwrap();
    let modules = Modules::discover().unwrap();
    let mut by_category: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut wrong = Vec::new();
    for case in cases() {
        let (_dir, db) = opened(&registry, &modules);
        let model = Simulated { provider: Provider { id: "simulado".into(), conditions: Conditions::Processor }, replies: RefCell::default() };
        for t in &case.turns {
            if let Some(r) = &t.router {
                model.replies.borrow_mut().push_back(("ia.enrutar", r.clone()));
            }
            if let Some(r) = &t.responder {
                model.replies.borrow_mut().push_back(("ia.responder", json!({ "text": r })));
            }
        }
        let runtime = Runtime { registry: &registry, modules: &modules, accounts: &Ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: Some(&model), answer: Answer::Words };
        let mut task: Option<String> = None;
        for t in &case.turns {
            let o = turn(&runtime, &db, task.as_deref(), &t.text);
            let g = got(&o);
            let ok = right(&t.expect, &g) && t.continues.is_none_or(|c| follows(&db, &o.task) == c);
            let tally = by_category.entry(case.category.clone()).or_default();
            tally.1 += 1;
            if ok {
                tally.0 += 1;
            } else {
                wrong.push(format!("{} «{}»: {g:?}", case.name, t.text));
            }
            task = Some(o.task);
        }
        assert!(model.replies.borrow().is_empty(), "{}: the model was not asked everything the case expected", case.name);
    }
    for (category, (ok, all)) in &by_category {
        println!("{category}: {ok} de {all}");
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The same battery with a real model of the provider saved in the app (Gemini's free plan, in development, with the
/// fictitious data: E3). It spends calls: run it on purpose, with its output to a file.
///
/// ```text
/// $env:SOCIAI_CHAT_LIVE_OUT = "C:\...\chat-live.txt"
/// cargo test chat_battery_live -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn chat_battery_live() {
    use std::fmt::Write as _;
    use std::sync::Arc;

    use crate::ai::platform_models::ProviderModels;
    use crate::ai::settings::{self, ProviderKind};

    let out = std::env::var("SOCIAI_CHAT_LIVE_OUT").expect("set SOCIAI_CHAT_LIVE_OUT to the file for the output");
    let key = crate::storage::get_api_key("gemini").unwrap().expect("no Gemini key saved: save it in the app first");
    let registry = Registry::discover().unwrap();
    let modules = Modules::discover().unwrap();
    let mut report = String::new();
    let mut by_category: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let (mut calls, mut cost, mut tokens, mut questions) = (0u32, 0.0f64, 0u64, 0u32);
    for case in cases() {
        let (_dir, db) = opened(&registry, &modules);
        let db = Arc::new(db);
        let mut s = settings::load(&db.lock().unwrap()).unwrap();
        s.provider = ProviderKind::Gemini;
        settings::save(&db.lock().unwrap(), &s).unwrap();
        let inner = crate::ai::build_provider(&s, key.clone());
        let models = Spaced { models: ProviderModels::new(ProviderKind::Gemini, inner, db.clone()), routed_input: RefCell::new(json!({})) };
        let runtime = Runtime { registry: &registry, modules: &modules, accounts: &Ana, clock: &platform::SystemClock, deliver: &Nowhere, models: Some(&models), answer: Answer::Words };
        let mut task: Option<String> = None;
        for t in &case.turns {
            let o = turn(&runtime, &db, task.as_deref(), &t.text);
            let g = got(&o);
            let continues = follows(&db, &o.task);
            let ok = right(&t.expect, &g) && t.continues.is_none_or(|c| continues == c);
            let tally = by_category.entry(case.category.clone()).or_default();
            tally.1 += 1;
            tally.0 += u32::from(ok);
            questions += 1;
            let _ = writeln!(report, "==== {} [{}] {}\n«{}»\nresultado: {g:?}{}", case.name, case.category, if ok { "BIEN" } else { "MAL" }, t.text, if t.continues.is_some() { format!(", sigue al anterior: {continues}") } else { String::new() });
            let conn = db.lock().unwrap();
            let mut statement = conn.prepare("SELECT operator, model, input_tokens, output_tokens, cost_mxn, result FROM ai_model_call WHERE task_id = ?1 ORDER BY n").unwrap();
            let rows = statement
                .query_map([&o.task], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, f64>(4)?, r.get::<_, String>(5)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            for (operator, model, input, output, mxn, result) in rows {
                calls += 1;
                cost += mxn;
                tokens += (input + output) as u64;
                let _ = writeln!(report, "  {operator} · {} · {input} + {output} tokens · ${mxn:.5} MXN · {result}", model.unwrap_or_default());
            }
            drop(statement);
            drop(conn);
            match &o.body {
                Body::Answer { text, .. } => {
                    let _ = writeln!(report, "A (palabras): {text}");
                    // path B: the same capability from its screen, with no model
                    if let Some(capability) = &g.capability {
                        let input = models.routed_input.borrow().clone();
                        let screen = Request { channel: Channel::Screen { place: "chat".into() }, task: None, content: Content::Invoke { capability: capability.clone(), input } };
                        let data = runtime.submit(&mut db.lock().unwrap(), &WHO, screen).unwrap();
                        if let Body::Data { value, withheld, .. } = &data.body {
                            let _ = writeln!(report, "B (datos): {value}{}", if withheld.is_empty() { String::new() } else { format!(" · sin mostrar: {withheld:?}") });
                        }
                    }
                }
                other => {
                    let _ = writeln!(report, "{other:?}");
                }
            }
            report.push('\n');
            task = Some(o.task);
        }
    }
    let mut summary = String::from("==== RESUMEN\n");
    let (ok, all) = by_category.values().fold((0, 0), |(a, b), (o, n)| (a + o, b + n));
    let _ = writeln!(summary, "aciertos: {ok} de {all}");
    for (category, (o, n)) in &by_category {
        let _ = writeln!(summary, "  {category}: {o} de {n}");
    }
    let _ = writeln!(summary, "llamadas: {calls} · tokens: {tokens} · costo: ${cost:.4} MXN · por pregunta: ${:.5} MXN", cost / f64::from(questions.max(1)));
    let _ = writeln!(summary, "(el costo sale de los tokens reales con los precios de pago del modelo, aunque la llave sea del plan gratuito)\n");
    std::fs::write(&out, format!("{summary}{report}")).unwrap();
    println!("{summary}");
}

/// At most 12 calls per minute: one every five seconds, whatever the provider allows. It keeps the input the router
/// chose last, to run path B with the same one.
struct Spaced {
    models: crate::ai::platform_models::ProviderModels,
    routed_input: RefCell<Value>,
}

impl Models for Spaced {
    fn provider(&self) -> &Provider {
        self.models.provider()
    }

    fn call(&self, operator: &Operator, to_model: &ToModel) -> Result<Reply, Failure> {
        std::thread::sleep(std::time::Duration::from_secs(5));
        let reply = self.models.call(operator, to_model);
        if let (Ok(r), true) = (&reply, operator.id == platform::operators::ROUTE.id) {
            let input = r.value.get("input").and_then(Value::as_str).and_then(|i| serde_json::from_str(i).ok()).unwrap_or(json!({}));
            *self.routed_input.borrow_mut() = input;
        }
        reply
    }
}
