//! The executor (P7) and the path of a request through it: open or resume the task, run each step under its controls,
//! pause, close, and hand the outcome over. Before every invocation: the task may go on, the same step was not run
//! already, the permission and the classes of data the account sees, and the input fits its contract. After it: the
//! output fits its contract, the permission once more, and the policy hands it over field by field (ADR-004). What
//! the person writes in the chat meets the policy before any model. Every pause and every close leaves its code in the
//! trail.
//!
//! A modification is prepared and applied on the kernel (ERP-9): its `prepare` gives the kernel's proposal, the person
//! approves that proposal by its fingerprint, and the `apply` applies only that fingerprint.
//!
//! A text of the chat answers «thinking» at once, and the rest of its turn goes on in `think`, apart: the router
//! chooses among what the person may use, the capability runs like any other, and an operator puts its output in
//! words. Each call to a model is a step of the task, and the database is let go while the model thinks.

use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use capability::{Approval, Call, Capability, CapabilityError, Effect, Run};
use kernel::{Proposal, TaskEvent, TaskMoment};

use super::cite;
use super::context;
use super::dictionary;
use super::figures;
use super::models::Reply;
use super::operators::{self, Operator};
use super::outcome::{Again, Ask, Body, Choice, Cited, Evidence, Next, Outcome, Shown, Source};
use super::policy::{self, Datum, Destination, Part, Purpose, ToModel};
use super::request::{Channel, Content, Request};
use super::store;
use super::task::{Held, Pause, Phase, Reached, Task, TaskState, Waiting, ANSWERING, RESERVE_PER_CALL_MXN, THINKING};
use super::{Runtime, RuntimeError, Who};

type Done = Result<Outcome, RuntimeError>;

/// What the router answers (`operators::ROUTE`).
#[derive(serde::Deserialize)]
struct Routed {
    intent: String,
    capability: Option<String>,
    #[serde(default)]
    options: Vec<String>,
    input: Option<String>,
    continues: bool,
}

/// The fingerprint of a capability and its input: how a repeated step and an input that changed are known. The keys
/// of the input are written in order, so the same input always gives the same fingerprint. The approval of a
/// modification is tied to the kernel's fingerprint instead (`proposal.fingerprint`), which also takes the version of
/// the datum.
pub fn fingerprint(capability: &str, input: &Value) -> String {
    let mut text = String::from(capability);
    text.push('\n');
    canonical(input, &mut text);
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(k.clone()).to_string());
                out.push(':');
                canonical(&map[k], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Whether a value fits a schema. If not, the field to correct: a name from the schema or the path, never a value.
fn fits(schema: &Value, value: &Value) -> Result<(), String> {
    let validator = jsonschema::validator_for(schema).map_err(|_| "input".to_string())?;
    let Some(error) = validator.iter_errors(value).next() else { return Ok(()) };
    if let jsonschema::error::ValidationErrorKind::Required { property } = error.kind() {
        if let Some(name) = property.as_str() {
            return Err(name.to_string());
        }
    }
    let path = error.instance_path().to_string();
    Err(match path.trim_start_matches('/') {
        "" => "input".to_string(),
        p => p.to_string(),
    })
}

fn error(code: &'static str, next: Next) -> Body {
    Body::Error { code: code.into(), next, capability: None }
}

impl Runtime<'_> {
    /// Every request goes through here: a screen that names a capability, the chat, a fact of the calendar. It gets
    /// the connection exclusive, because the `apply` of a change hands it to the kernel for its transaction.
    pub fn submit(&self, conn: &mut Connection, who: &Who, request: Request) -> Done {
        let now = self.clock.now_ms();
        let Request { channel, task, content } = request;
        let text = matches!(content, Content::Text { .. });
        let mut task = match task {
            None => self.open(conn, who, channel, None, now)?,
            Some(id) => match store::load_task(conn, &id)? {
                // a new text after a task of the chat that ended or waits: a new task, that goes on from it (E4)
                Some(t) if t.account == who.account && t.organization == who.organization && text && t.channel == Channel::Chat && t.state != TaskState::Running => {
                    self.open(conn, who, Channel::Chat, Some(t.id), now)?
                }
                Some(t) if t.account == who.account && t.organization == who.organization => t,
                // a task of another account is not touched: a new one says no
                _ => {
                    let mut t = self.open(conn, who, channel, None, now)?;
                    return self.close(conn, &mut t, TaskState::Denied, "not_your_task", 0, error("not_your_task", Next::StartAgain));
                }
            },
        };
        // a turn under way finishes first: nothing else enters the task meanwhile, unless it ran out of its time (the
        // app closed while a model thought)
        if task.state == TaskState::Running && matches!(task.reason.as_deref(), Some(THINKING | ANSWERING)) {
            if now - task.turn_ms > task.limits().turn_ms {
                return self.limit(conn, &mut task, Reached::Time);
            }
            return Ok(self.notice(&task, Body::State { reason: "busy".into() }));
        }
        if task.state.is_closed() {
            // a closed task does not move: it says again how it ended
            return match store::last_outcome(conn, &task.id)? {
                Some(last) => Ok(last),
                None => self.respond(conn, &task, 0, Body::State { reason: task.reason.clone().unwrap_or_default() }),
            };
        }
        // an apply that started and never said how it ended is never run again (the AI blueprint §5)
        if let Some(unfinished) = store::unfinished_apply(conn, &task.id)? {
            return self.settle(conn, &mut task, unfinished);
        }
        if task.pause_expired(now) {
            return self.limit(conn, &mut task, Reached::Pause);
        }
        task.turn_ms = now;
        match content {
            Content::Text { text } => self.chat(conn, &mut task, text),
            Content::Invoke { capability, input } => self.invoke(conn, who, &mut task, &capability, input),
            Content::Answer { values } => self.answer(conn, who, &mut task, values),
            Content::Decision { fingerprint, approve } => self.decide(conn, who, &mut task, &fingerprint, approve),
        }
    }

    /// What the person wrote, before any model: the first point of the policy (ADR-004 §6). Without models, or with
    /// something in it that may not go to their provider, it ends here. Otherwise the turn says «thinking», and goes
    /// on in `think`.
    fn chat(&self, conn: &Connection, task: &mut Task, text: String) -> Done {
        let Some(models) = self.models else {
            return self.close(conn, task, TaskState::Error, "without_ai", 0, error("without_ai", Next::UseScreen));
        };
        let to_router = policy::to_model(models.provider(), Purpose::Route, vec![Datum::Text(text.clone())]);
        if !to_router.left_out().is_empty() {
            return self.not_for_this_provider(conn, task);
        }
        store::add_turn(conn, &task.id, &text, self.clock.now_ms())?;
        self.running(conn, task, THINKING, 0)
    }

    /// The rest of a turn of the chat, apart from the request that started it: whatever the task waits for now, with
    /// a model. The database is taken for each step and let go while a model thinks, so the app goes on meanwhile.
    /// Nothing, if the task is not of this account or has nothing under way.
    pub fn think(&self, db: &Mutex<Connection>, who: &Who, id: &str) -> Result<Option<Outcome>, RuntimeError> {
        let conn = db.lock().map_err(|_| RuntimeError::Lock)?;
        let mut task = match store::load_task(&conn, id)? {
            Some(t) if t.account == who.account && t.organization == who.organization && t.state == TaskState::Running => t,
            _ => return Ok(None),
        };
        let Some(models) = self.models else {
            return self.close(&conn, &mut task, TaskState::Error, "without_ai", 0, error("without_ai", Next::UseScreen)).map(Some);
        };
        let done = match task.reason.as_deref() {
            Some(THINKING) => self.route(db, conn, who, &mut task, models),
            Some(ANSWERING) => self.words(db, conn, &mut task, models),
            _ => return Ok(None),
        };
        done.map(Some)
    }

    /// The router (P4) chooses among what the person may use now: by code, before any model, the capabilities of the
    /// modules that are on and that this account may run (K2). Knowing what they want gives no permission: anything
    /// outside the list ends the task.
    fn route<'d>(&self, db: &'d Mutex<Connection>, mut conn: MutexGuard<'d, Connection>, who: &Who, task: &mut Task, models: &dyn super::Models) -> Done {
        let text = store::last_turn(&conn, &task.id)?.unwrap_or_default();
        let earlier = match &task.follows {
            Some(from) => store::last_turn(&conn, from)?,
            None => None,
        };
        let offered = self.offered(&conn, who);
        let candidates: Vec<Value> = offered
            .iter()
            .map(|c| match c.effect.changes() {
                true => json!({ "id": c.id, "description": c.description, "effect": "change" }),
                false => json!({ "id": c.id, "description": c.description, "effect": "query", "input": (c.input)() }),
            })
            .collect();
        // what the person writes now goes first; the turn before, only as earlier (E4)
        let mut data = vec![Datum::Text(text)];
        data.extend(earlier.map(Datum::Earlier));
        data.push(Datum::Program { what: "candidates", value: Value::Array(candidates) });
        let to_router = policy::to_model(models.provider(), Purpose::Route, data);
        if to_router.left_out().iter().any(|l| l == "text") {
            return self.not_for_this_provider(&conn, task);
        }
        let (guard, reply) = self.call_model(db, conn, task, models, &operators::ROUTE, &to_router)?;
        conn = guard;
        let reply = match reply {
            Ok(reply) => reply,
            Err(closed) => return Ok(closed),
        };
        let Ok(routed) = serde_json::from_value::<Routed>(reply.value) else {
            return self.close(&conn, task, TaskState::Error, "model_bad_output", 0, error("model_bad_output", Next::TryLater));
        };
        // the turn before goes no further when the text does not go on from it
        if !routed.continues {
            task.follows = None;
        }
        let find = |id: &str, changes: bool| offered.iter().copied().find(|c| c.id == id && c.effect.changes() == changes);
        match routed.intent.as_str() {
            "invoke" => {
                let Some(cap) = routed.capability.as_deref().and_then(|id| find(id, false)) else {
                    return self.not_offered(&conn, task);
                };
                let input = routed.input.as_deref().and_then(|i| serde_json::from_str::<Value>(i).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
                let outcome = self.step(&mut conn, who, task, cap, Phase::Read, input, None, None)?;
                // with words, the output waits for the operator that writes them
                if task.reason.as_deref() == Some(ANSWERING) {
                    return self.words(db, conn, task, models);
                }
                Ok(outcome)
            }
            "choose" => {
                let options: Option<Vec<Choice>> = routed
                    .options
                    .iter()
                    .map(|id| find(id, false).map(|c| Choice { capability: c.id.into(), description: c.description.into() }))
                    .collect();
                match options {
                    Some(options) if !options.is_empty() => {
                        let choices = options.iter().map(|o| o.capability.clone()).collect();
                        task.waiting = Some(Waiting { capability: operators::ROUTE.id.into(), phase: Phase::Read, input: json!({}), correction: false, asked: vec![], choices });
                        let body = Body::Clarification { capability: operators::ROUTE.id.into(), ask: Ask::Choose { options } };
                        self.pause(&conn, task, Pause::Person, "needs_person", 0, body)
                    }
                    _ => self.not_offered(&conn, task),
                }
            }
            // a change from the chat waits for IA-9: the person is sent to the screen of the change, if they may make it
            "change" => match routed.capability.as_deref().and_then(|id| find(id, true)) {
                Some(cap) => {
                    let body = Body::Error { code: "change_from_screen".into(), next: Next::UseScreen, capability: Some(cap.id.into()) };
                    self.close(&conn, task, TaskState::Error, "change_from_screen", 0, body)
                }
                None => self.close(&conn, task, TaskState::Error, "out_of_scope", 0, error("out_of_scope", Next::UseScreen)),
            },
            "out_of_scope" => self.close(&conn, task, TaskState::Error, "out_of_scope", 0, error("out_of_scope", Next::UseScreen)),
            _ => self.close(&conn, task, TaskState::Error, "model_bad_output", 0, error("model_bad_output", Next::TryLater)),
        }
    }

    /// The words and the data together (E1): the operator `ia.responder` gives the context and the order of what the
    /// capability gave, and cites each datum as `[[path]]` instead of writing it. It reads it as the policy lets it go
    /// to its provider (ADR-004 §6, the context). The words hold only if every citation is of something it read, no
    /// value is written instead of cited, and every figure is in what it read; the evidence is the platform's, never
    /// the model's. When the words do not hold, the person gets the data.
    fn words<'d>(&self, db: &'d Mutex<Connection>, conn: MutexGuard<'d, Connection>, task: &mut Task, models: &dyn super::Models) -> Done {
        let Some(held) = task.held.clone() else {
            return self.close(&conn, task, TaskState::Error, "bad_output", 0, error("bad_output", Next::TryLater));
        };
        let Some(cap) = self.registry.get(&held.capability) else {
            return self.close(&conn, task, TaskState::Error, "unknown_capability", 0, error("unknown_capability", Next::TryLater));
        };
        let Ok(fields) = dictionary::output_fields(cap, self.modules) else {
            return self.close(&conn, task, TaskState::Error, "bad_output", held.step, error("bad_output", Next::TryLater));
        };
        let question = store::last_turn(&conn, &task.id)?.unwrap_or_default();
        let earlier = match &task.follows {
            Some(from) => store::last_turn(&conn, from)?,
            None => None,
        };
        let provider = models.provider();
        let mut data = vec![Datum::Text(question)];
        data.extend(earlier.map(Datum::Earlier));
        data.extend(context::data_of(cap, &fields, &held.value, Destination::Model(provider.conditions), Purpose::Answer));
        let to_responder = policy::to_model(provider, Purpose::Answer, data);
        let (conn, reply) = self.call_model(db, conn, task, models, &operators::RESPOND, &to_responder)?;
        let reply = match reply {
            Ok(reply) => reply,
            Err(closed) => return Ok(closed),
        };
        let data = |task: &mut Task, conn: &Connection, reason: &'static str| {
            self.trail(conn, task, TaskMoment::Step, Some(operators::RESPOND.id), Some(reason), None)?;
            task.held = None;
            self.close(conn, task, TaskState::Done, "done", held.step, Body::Data { capability: cap.id.into(), value: held.value.clone(), withheld: held.withheld.clone() })
        };
        let Some(text) = reply.value.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) else {
            return data(task, &conn, "model_bad_output");
        };
        // what it read, by the path of each datum
        let read: Vec<(&str, &Value)> = to_responder
            .parts()
            .iter()
            .filter_map(|p| match p {
                Part::Field { id, value } => Some((id.as_str(), value)),
                _ => None,
            })
            .collect();
        let mut cited = Vec::new();
        for path in cite::paths(text) {
            // a datum it read, or a whole item of a list whose data it read (`missing[0]`)
            let read_here = |id: &str| id == path || id.strip_prefix(path.as_str()).is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['));
            let value = match read.iter().find(|(id, _)| *id == path) {
                Some(&(_, value)) => Some(value.clone()),
                None if read.iter().any(|(id, _)| read_here(id)) => context::at(&held.value, &path).cloned(),
                None => None,
            };
            let Some(value) = value else {
                return data(task, &conn, "unknown_citation");
            };
            let pattern = context::pattern_of(&path);
            let field = fields.iter().find(|f| f.path == pattern).map(|f| f.field.clone());
            cited.push(Cited { path, value, field });
        }
        // the data are always exact: a value is cited, never written by the model
        if cite::copies(text, read.iter().filter_map(|(_, v)| v.as_str())) {
            return data(task, &conn, "copied_value");
        }
        // the AI does not invent figures: each one it writes is in what it read
        if !figures::unsupported_figures(&[&cite::without_citations(text)], &[&to_responder.render()]).is_empty() {
            return data(task, &conn, "invented_figures");
        }
        let text = to_responder.restore(text);
        let evidence = vec![Evidence { source: Source::Calculation, label: cap.description.into(), reference: Some(format!("{}#{}", cap.id, held.step)) }];
        let shown = Shown { capability: cap.id.into(), value: held.value.clone(), withheld: held.withheld.clone(), cited };
        task.held = None;
        self.close(&conn, task, TaskState::Done, "done", held.step, Body::Answer { text, evidence, data: Some(shown) })
    }

    /// One call to a model, as one step of its task (the AI blueprint §5): within the limits of the task, with its
    /// spending reserved before and settled after, and written down with what it cost. The database is let go while
    /// the model thinks. A failure closes the task with its code and a way to go on.
    fn call_model<'d>(
        &self,
        db: &'d Mutex<Connection>,
        conn: MutexGuard<'d, Connection>,
        task: &mut Task,
        models: &dyn super::Models,
        operator: &'static Operator,
        to_model: &ToModel,
    ) -> Result<(MutexGuard<'d, Connection>, Result<Reply, Outcome>), RuntimeError> {
        let now = self.clock.now_ms();
        if let Err(reached) = task.may_step(now).and_then(|()| task.reserve(RESERVE_PER_CALL_MXN)) {
            let closed = self.limit(&conn, task, reached)?;
            return Ok((conn, Err(closed)));
        }
        task.steps += 1;
        let n = task.steps;
        store::start_model_call(&conn, &task.id, n, operator.id, &models.provider().id, now)?;
        store::save_progress(&conn, task)?;
        drop(conn);
        let result = models.call(operator, to_model);
        let conn = db.lock().map_err(|_| RuntimeError::Lock)?;
        let now = self.clock.now_ms();
        let (end, result) = match result {
            Ok(reply) if fits(&(operator.schema)(), &reply.value).is_ok() => {
                let end = store::ModelCallEnd { model: Some(&reply.model), input_tokens: reply.input_tokens, output_tokens: reply.output_tokens, cost_mxn: reply.cost_mxn, result: "ok" };
                store::end_model_call(&conn, &task.id, n, &end, now)?;
                task.settle(RESERVE_PER_CALL_MXN, reply.cost_mxn);
                (Ok(()), Ok(reply))
            }
            Ok(reply) => {
                let end = store::ModelCallEnd { model: Some(&reply.model), input_tokens: reply.input_tokens, output_tokens: reply.output_tokens, cost_mxn: reply.cost_mxn, result: "model_bad_output" };
                store::end_model_call(&conn, &task.id, n, &end, now)?;
                task.settle(RESERVE_PER_CALL_MXN, reply.cost_mxn);
                (Err("model_bad_output"), Err(super::Failure::BadOutput))
            }
            Err(failure) => {
                let end = store::ModelCallEnd { model: None, input_tokens: 0, output_tokens: 0, cost_mxn: 0.0, result: failure.code() };
                store::end_model_call(&conn, &task.id, n, &end, now)?;
                task.settle(RESERVE_PER_CALL_MXN, 0.0);
                (Err(failure.code()), Err(failure))
            }
        };
        self.trail(&conn, task, TaskMoment::Step, Some(operator.id), Some(end.err().unwrap_or("ok")), None)?;
        match result {
            Ok(reply) => Ok((conn, Ok(reply))),
            Err(failure) => {
                let next = match failure {
                    super::Failure::Key => Next::AskAdministrator,
                    super::Failure::Budget => Next::UseScreen,
                    _ => Next::TryLater,
                };
                let closed = self.close(&conn, task, TaskState::Error, failure.code(), n, error(failure.code(), next))?;
                Ok((conn, Err(closed)))
            }
        }
    }

    /// What the router may choose from: the capabilities of the modules that are on, that this account may run now
    /// (K2), in the order of their ids and no more than `OFFERED_AT_MOST`.
    fn offered(&self, conn: &Connection, who: &Who) -> Vec<&'static Capability> {
        let mut offered: Vec<&'static Capability> = self
            .registry
            .all()
            .filter(|c| self.modules.is_on(c.module()))
            .filter(|c| self.authorize(conn, who, c).is_ok())
            .collect();
        offered.sort_by_key(|c| c.id);
        offered.truncate(operators::OFFERED_AT_MOST);
        offered
    }

    fn not_for_this_provider(&self, conn: &Connection, task: &mut Task) -> Done {
        self.trail(conn, task, TaskMoment::Denied, None, None, Some("not_for_this_provider"))?;
        self.close(conn, task, TaskState::Denied, "not_for_this_provider", 0, error("not_for_this_provider", Next::UseScreen))
    }

    /// The model named something the person was not offered: the task ends, whatever it was.
    fn not_offered(&self, conn: &Connection, task: &mut Task) -> Done {
        self.trail(conn, task, TaskMoment::Denied, Some(operators::ROUTE.id), None, Some("not_offered"))?;
        self.close(conn, task, TaskState::Denied, "not_offered", 0, error("not_offered", Next::UseScreen))
    }

    /// The turn goes on apart: the task says why it has not answered yet.
    fn running(&self, conn: &Connection, task: &mut Task, reason: &'static str, n: u32) -> Done {
        task.state = TaskState::Running;
        task.pause = None;
        task.pause_until_ms = None;
        task.reason = Some(reason.into());
        self.respond(conn, task, n, Body::State { reason: reason.into() })
    }

    fn open(&self, conn: &Connection, who: &Who, channel: Channel, follows: Option<String>, now: i64) -> Result<Task, RuntimeError> {
        let mut task = Task::open(format!("task_{}", ulid::Ulid::generate()), who.organization, who.account, channel, now);
        task.follows = follows;
        store::insert_task(conn, &task)?;
        self.trail(conn, &task, TaskMoment::Opened, None, Some(task.channel.as_str()), None)?;
        Ok(task)
    }

    /// What became of an apply the app did not see end. The kernel says whether its fingerprint was applied, and its
    /// transaction never leaves a change half made: applied, the task ends well; not applied, it says so and applies
    /// nothing on its own.
    fn settle(&self, conn: &mut Connection, task: &mut Task, step: store::Unfinished) -> Done {
        let now = self.clock.now_ms();
        let capability = self.registry.get(&step.capability).map(|c| c.id);
        let Some(fp) = step.fingerprint else {
            // not a change of the kernel's: nobody can say whether it was made
            store::end_step(conn, &task.id, step.n, "uncertain", now)?;
            self.trail(conn, task, TaskMoment::Step, capability, Some("uncertain"), None)?;
            return self.close(conn, task, TaskState::Error, "uncertain_change", step.n, error("uncertain_change", Next::CheckHistory));
        };
        match kernel::applied(conn, &fp)? {
            Some(done) => {
                store::end_step(conn, &task.id, step.n, "ok", now)?;
                self.trail(conn, task, TaskMoment::Step, capability, Some("ok"), None)?;
                // what the kernel applied: ids and the version, nothing of the datum
                let value = serde_json::to_value(done).unwrap_or_default();
                self.close(conn, task, TaskState::Done, "done", step.n, Body::Data { capability: step.capability, value, withheld: vec![] })
            }
            None => {
                store::end_step(conn, &task.id, step.n, "not_applied", now)?;
                self.trail(conn, task, TaskMoment::Step, capability, Some("not_applied"), None)?;
                self.close(conn, task, TaskState::Error, "not_applied", step.n, error("not_applied", Next::StartAgain))
            }
        }
    }

    fn invoke(&self, conn: &mut Connection, who: &Who, task: &mut Task, capability: &str, input: Value) -> Done {
        let Some(cap) = self.registry.get(capability) else {
            return self.close(conn, task, TaskState::Error, "unknown_capability", 0, error("unknown_capability", Next::TryLater));
        };
        let mut again = None;
        if task.pause == Some(Pause::Approval) {
            if let Some(pending) = store::pending_approval(conn, &task.id)? {
                // the same proposal asked for again: it is shown again, nothing runs
                if pending.capability == cap.id && pending.input_hash == fingerprint(cap.id, &input) {
                    let body = Body::Proposal { capability: pending.capability, proposal: pending.proposal, fingerprint: pending.fingerprint, again: None };
                    return self.respond(conn, task, 0, body);
                }
                // other parameters: the earlier approval no longer counts
                again = Some(Again::Changed);
            }
        }
        let phase = if cap.effect.changes() { Phase::Prepare } else { Phase::Read };
        self.step(conn, who, task, cap, phase, input, again, None)
    }

    fn answer(&self, conn: &mut Connection, who: &Who, task: &mut Task, values: Value) -> Done {
        let (Some(Pause::Person), Some(waiting)) = (task.pause, task.waiting.clone()) else {
            return Ok(self.notice(task, error("not_waiting", Next::StartAgain)));
        };
        if !waiting.choices.is_empty() {
            // one of the options the person was offered, and nothing else; the step asks the kernel again
            let chosen = values.get("choice").and_then(Value::as_str).filter(|c| waiting.choices.iter().any(|w| w == c));
            let Some(cap) = chosen.and_then(|c| self.registry.get(c)) else {
                return Ok(self.notice(task, error("not_offered", Next::StartAgain)));
            };
            task.waiting = None;
            return self.step(conn, who, task, cap, Phase::Read, json!({}), None, None);
        }
        let Some(cap) = self.registry.get(&waiting.capability) else {
            return self.close(conn, task, TaskState::Error, "unknown_capability", 0, error("unknown_capability", Next::TryLater));
        };
        let mut input = waiting.input;
        let from = if waiting.asked.is_empty() {
            match values {
                Value::Object(from) => from,
                _ => Default::default(),
            }
        } else {
            // fields of the dictionary: the kernel checks the answer before anything runs
            match dictionary::answered(self.modules, &waiting.asked, &input, &values) {
                Ok(answer) => answer,
                Err(problems) => {
                    task.corrections += 1;
                    if task.corrections > task.limits().corrections {
                        return self.limit(conn, task, Reached::Corrections);
                    }
                    let Some(fields) = dictionary::asked(cap, self.modules, &waiting.asked, &problems) else {
                        return self.close(conn, task, TaskState::Error, "bad_output", 0, error("bad_output", Next::TryLater));
                    };
                    // the same fields are asked again, each with what did not pass
                    return self.pause_for_person(conn, task, cap, waiting.phase, input, 0, Ask::Missing { fields }, false);
                }
            }
        };
        if let Some(into) = input.as_object_mut() {
            into.extend(from);
        }
        if waiting.correction {
            task.corrections += 1;
            if task.corrections > task.limits().corrections {
                return self.limit(conn, task, Reached::Corrections);
            }
        }
        self.step(conn, who, task, cap, waiting.phase, input, None, None)
    }

    fn decide(&self, conn: &mut Connection, who: &Who, task: &mut Task, fp: &str, approve: bool) -> Done {
        let pending = match (task.pause, store::pending_approval(conn, &task.id)?) {
            (Some(Pause::Approval), Some(p)) => p,
            _ => return Ok(self.notice(task, error("not_waiting", Next::StartAgain))),
        };
        // a decision for another proposal: the one waiting is shown again
        if pending.fingerprint != fp {
            let body = Body::Proposal { capability: pending.capability, proposal: pending.proposal, fingerprint: pending.fingerprint, again: Some(Again::Outdated) };
            return self.respond(conn, task, 0, body);
        }
        let Some(cap) = self.registry.get(&pending.capability) else {
            return self.close(conn, task, TaskState::Error, "unknown_capability", 0, error("unknown_capability", Next::TryLater));
        };
        let now = self.clock.now_ms();
        if !approve {
            store::decide_approval(conn, &task.id, fp, false, who.account, now)?;
            // the close says it: done, because the person rejected it
            return self.close(conn, task, TaskState::Done, "rejected", 0, Body::State { reason: "rejected".into() });
        }
        // the approval is of a person with the permission, never of an agent
        if let Err(code) = self.authorize(conn, who, cap) {
            return self.deny(conn, task, cap.id, code);
        }
        // the input approved is the input applied
        if fingerprint(cap.id, &pending.input) != pending.input_hash {
            return self.close(conn, task, TaskState::Error, "approval_mismatch", 0, error("approval_mismatch", Next::StartAgain));
        }
        store::decide_approval(conn, &task.id, fp, true, who.account, now)?;
        self.trail(conn, task, TaskMoment::Approved, Some(cap.id), None, None)?;
        // the apply goes with the fingerprint the person approved: the kernel applies only that
        self.step(conn, who, task, cap, Phase::Apply, pending.input, None, Some(&pending.fingerprint))
    }

    /// One invocation of a capability, with its controls before and after. `approved` is the fingerprint an apply
    /// applies, and only an apply has one.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &self,
        conn: &mut Connection,
        who: &Who,
        task: &mut Task,
        cap: &'static Capability,
        phase: Phase,
        input: Value,
        again: Option<Again>,
        approved: Option<&str>,
    ) -> Done {
        let now = self.clock.now_ms();
        // the task may go on: its limits before every invocation (the blueprints §3.3)
        if let Err(reached) = task.may_step(now) {
            return self.limit(conn, task, reached);
        }
        let hash = fingerprint(cap.id, &input);
        // a prepare asked again because the datum changed is not a repeat: the person sees it and approves it again
        if again != Some(Again::DataChanged) && store::seen(conn, &task.id, cap.id, phase, &hash, approved)? {
            return self.limit(conn, task, Reached::Repeated);
        }
        // the permission and the classes it touches, right before the operation
        if let Err(code) = self.authorize(conn, who, cap) {
            return self.deny(conn, task, cap.id, code);
        }
        if let Err(field) = fits(&(cap.input)(), &input) {
            return self.pause_for_person(conn, task, cap, phase, input, 0, Ask::Correct { field, reason: "does_not_fit".into() }, true);
        }
        let output = match (cap.run, phase, approved) {
            (Run::Read(s), Phase::Read, _) => s.output,
            (Run::Change { prepare, .. }, Phase::Prepare, _) => prepare.output,
            // an apply always goes with the fingerprint of what it applies
            (Run::Change { apply, .. }, Phase::Apply, Some(_)) => apply.output,
            // the registry does not let a contract be like this
            _ => return self.close(conn, task, TaskState::Error, "run_does_not_fit", 0, error("run_does_not_fit", Next::TryLater)),
        };
        task.steps += 1;
        let n = task.steps;
        // written before it runs: if the app closes in the middle, the step stays «started» and is never repeated
        store::start_step(conn, &task.id, n, cap.id, phase, &hash, approved, now)?;
        // the id of the task goes with everything the run leaves behind; only the apply gets the connection exclusive
        let result = match cap.run {
            Run::Change { apply, .. } if phase == Phase::Apply => {
                (apply.call)(&mut Call::applying(conn, who.account, &task.id, approved.unwrap_or_default()), input.clone())
            }
            Run::Change { prepare: s, .. } | Run::Read(s) => (s.call)(&Call::new(conn, who.account, &task.id), input.clone()),
        };
        let value = match result {
            Ok(v) if fits(&output(), &v).is_ok() => v,
            Ok(_) => {
                self.ended(conn, task, n, cap.id, "failed")?;
                return self.close(conn, task, TaskState::Error, "bad_output", n, error("bad_output", Next::TryLater));
            }
            Err(CapabilityError::Invalid { field, reason }) => {
                self.ended(conn, task, n, cap.id, "invalid")?;
                return self.pause_for_person(conn, task, cap, phase, input, n, Ask::Correct { field: field.into(), reason: reason.into() }, true);
            }
            Err(CapabilityError::NeedsPerson { entity, fields }) => {
                let ids: Vec<String> = fields.iter().map(|f| format!("{entity}.{f}")).collect();
                // what it asks has to be in the dictionary and within its `data`, or it is outside its contract
                let Some(fields) = dictionary::asked(cap, self.modules, &ids, &[]) else {
                    self.ended(conn, task, n, cap.id, "failed")?;
                    return self.close(conn, task, TaskState::Error, "bad_output", n, error("bad_output", Next::TryLater));
                };
                self.ended(conn, task, n, cap.id, "needs_person")?;
                return self.pause_for_person(conn, task, cap, phase, input, n, Ask::Missing { fields }, false);
            }
            Err(CapabilityError::Stale) if phase == Phase::Apply => {
                self.ended(conn, task, n, cap.id, "stale")?;
                // what was approved no longer holds: prepared again, the person sees the datum as it is now
                return self.step(conn, who, task, cap, Phase::Prepare, input, Some(Again::DataChanged), None);
            }
            Err(CapabilityError::Failed { .. } | CapabilityError::Stale) => {
                self.ended(conn, task, n, cap.id, "failed")?;
                return self.close(conn, task, TaskState::Error, "capability_failed", n, error("capability_failed", Next::TryLater));
            }
        };
        // what the person approves: the kernel's proposal for a modification, the capability and its input otherwise
        let mut to_approve = hash.clone();
        if phase == Phase::Prepare && cap.effect == Effect::Modify {
            let Ok(proposal) = serde_json::from_value::<Proposal>(value.clone()) else {
                self.ended(conn, task, n, cap.id, "failed")?;
                return self.close(conn, task, TaskState::Error, "bad_output", n, error("bad_output", Next::TryLater));
            };
            if let Some(first) = proposal.invalid.first() {
                self.ended(conn, task, n, cap.id, "invalid")?;
                let ask = Ask::Correct { field: first.field.clone(), reason: first.reason.clone() };
                return self.pause_for_person(conn, task, cap, phase, input, n, ask, true);
            }
            if proposal.changes.is_empty() {
                self.ended(conn, task, n, cap.id, "ok")?;
                return self.close(conn, task, TaskState::Done, "nothing_to_change", n, Body::State { reason: "nothing_to_change".into() });
            }
            to_approve = proposal.fingerprint;
        }
        self.ended(conn, task, n, cap.id, "ok")?;
        match phase {
            Phase::Prepare if cap.approval == Approval::Required => {
                let now = self.clock.now_ms();
                // the pause that follows says it in the trail: paused, waiting for the approval
                store::ask_approval(conn, &task.id, &to_approve, &hash, cap.id, &input, &value, now)?;
                task.waiting = Some(Waiting { capability: cap.id.into(), phase: Phase::Apply, input, correction: false, asked: vec![], choices: vec![] });
                let body = Body::Proposal { capability: cap.id.into(), proposal: value, fingerprint: to_approve, again };
                self.pause(conn, task, Pause::Approval, "approval", n, body)
            }
            // a change that needs no approval is applied with the same input, tied to what was just prepared
            Phase::Prepare => self.step(conn, who, task, cap, Phase::Apply, input, None, Some(&to_approve)),
            Phase::Read | Phase::Apply => {
                // the permission once more, before handing it over
                let sees = match self.authorize(conn, who, cap) {
                    Ok(sees) => sees,
                    Err(code) => return self.deny(conn, task, cap.id, code),
                };
                // field by field, as the policy lets it go where it goes (ADR-004 §6): a notice, or the person
                let to = match task.channel {
                    Channel::Event { .. } => Destination::Notice,
                    Channel::Chat | Channel::Screen { .. } => Destination::Person { sees: &sees },
                };
                let Ok(fields) = dictionary::output_fields(cap, self.modules) else {
                    return self.close(conn, task, TaskState::Error, "bad_output", n, error("bad_output", Next::TryLater));
                };
                let (value, withheld) = policy::hand_over(cap.data, &fields, value, to);
                // a question of the chat is answered in words with its data (E1): what the person may see waits for
                // the operator that writes them, in `think`
                if task.channel == Channel::Chat && phase == Phase::Read && self.models.is_some() {
                    task.held = Some(Held { capability: cap.id.into(), step: n, value, withheld });
                    return self.running(conn, task, ANSWERING, n);
                }
                self.close(conn, task, TaskState::Done, "done", n, Body::Data { capability: cap.id.into(), value, withheld })
            }
        }
    }

    fn ended(&self, conn: &Connection, task: &Task, n: u32, capability: &'static str, result: &'static str) -> Result<(), RuntimeError> {
        store::end_step(conn, &task.id, n, result, self.clock.now_ms())?;
        self.trail(conn, task, TaskMoment::Step, Some(capability), Some(result), None)
    }

    /// The question of authorization, to the kernel (K2): the account as it is now, the organization and the
    /// permission. No account (the session closed, locked or of someone else) is no. Then the classes the capability
    /// touches, against those the account sees (ADR-004 §6, the execution): it gives those, for what is handed over.
    fn authorize(&self, conn: &Connection, who: &Who, cap: &Capability) -> Result<Vec<&'static str>, &'static str> {
        let account = self.accounts.now(conn, who).ok_or("no_session")?;
        kernel::authorize(self.modules, &account, who.organization, cap.permission).map_err(kernel::Denial::code)?;
        let sees = kernel::classes_seen(&account);
        if policy::may_touch(cap.data, &sees) {
            Ok(sees)
        } else {
            Err("class_not_seen")
        }
    }

    /// One moment of the life of the task, in the kernel's audit store (K9): ids and codes of the program only.
    fn trail(
        &self,
        conn: &Connection,
        task: &Task,
        moment: TaskMoment,
        capability: Option<&'static str>,
        outcome: Option<&'static str>,
        reason: Option<&'static str>,
    ) -> Result<(), RuntimeError> {
        kernel::record_task(conn, &TaskEvent { moment, task: &task.id, account: Some(&task.account), capability, outcome, reason })?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn pause_for_person(&self, conn: &Connection, task: &mut Task, cap: &Capability, phase: Phase, input: Value, n: u32, ask: Ask, correction: bool) -> Done {
        // a change never resumes at its apply: what the person adds changes the input, so it is prepared and
        // approved again (the blueprints §3.5)
        let phase = if phase == Phase::Apply { Phase::Prepare } else { phase };
        let asked = match &ask {
            Ask::Missing { fields } => fields.iter().map(|f| f.id.clone()).collect(),
            Ask::Correct { .. } | Ask::Choose { .. } => vec![],
        };
        task.waiting = Some(Waiting { capability: cap.id.into(), phase, input, correction, asked, choices: vec![] });
        self.pause(conn, task, Pause::Person, "needs_person", n, Body::Clarification { capability: cap.id.into(), ask })
    }

    /// The turn ends and the task waits for the person, until its pause runs out.
    fn pause(&self, conn: &Connection, task: &mut Task, pause: Pause, reason: &'static str, n: u32, body: Body) -> Done {
        let now = self.clock.now_ms();
        task.state = TaskState::Paused;
        task.pause = Some(pause);
        task.reason = Some(reason.into());
        task.pause_until_ms = Some(now + task.limits().pause_ms);
        self.trail(conn, task, TaskMoment::Closed, None, Some("paused"), Some(reason))?;
        self.respond(conn, task, n, body)
    }

    /// The kernel said no: the reason is its code (`access_denied`, `account_inactive`…).
    fn deny(&self, conn: &Connection, task: &mut Task, capability: &'static str, code: &'static str) -> Done {
        self.trail(conn, task, TaskMoment::Denied, Some(capability), None, Some(code))?;
        self.close(conn, task, TaskState::Denied, code, 0, error(code, Next::AskAdministrator))
    }

    fn limit(&self, conn: &Connection, task: &mut Task, reached: Reached) -> Done {
        self.close(conn, task, TaskState::Limit, reached.code(), 0, error(reached.code(), Next::StartAgain))
    }

    /// Every close is written with its reason, however the task ends (the blueprints §3.2).
    fn close(&self, conn: &Connection, task: &mut Task, state: TaskState, reason: &'static str, n: u32, body: Body) -> Done {
        let now = self.clock.now_ms();
        task.state = state;
        task.pause = None;
        task.reason = Some(reason.into());
        task.pause_until_ms = None;
        task.closed_ms = Some(now);
        task.waiting = None;
        task.held = None;
        self.trail(conn, task, TaskMoment::Closed, None, Some(state.as_str()), Some(reason))?;
        self.respond(conn, task, n, body)
    }

    /// The outcome, with the id of its task, kept as the last one of the task and handed over.
    fn respond(&self, conn: &Connection, task: &Task, step: u32, body: Body) -> Done {
        let outcome = self.outcome(task, step, body);
        store::save_task(conn, task, &outcome)?;
        self.deliver.deliver(&outcome);
        Ok(outcome)
    }

    /// An answer to a request that changed nothing: the task keeps what it said last.
    fn notice(&self, task: &Task, body: Body) -> Outcome {
        self.outcome(task, 0, body)
    }

    fn outcome(&self, task: &Task, step: u32, body: Body) -> Outcome {
        Outcome { task: task.id.clone(), step, channel: task.channel.clone(), state: task.state, at_ms: self.clock.now_ms(), body }
    }
}
