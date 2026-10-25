//! The executor (P7) and the path of a request through it: open or resume the task, run each step under its controls,
//! pause, close, and hand the outcome over. Before every invocation: the task may go on, the same step was not run
//! already, the permission and the classes of data the account sees, and the input fits its contract. After it: the
//! output fits its contract, the permission once more, and the policy hands it over field by field (ADR-004). What
//! the person writes in the chat meets the policy before any model. Every pause and every close leaves its code in the
//! trail.
//!
//! A modification is prepared and applied on the kernel (ERP-9): its `prepare` gives the kernel's proposal, the person
//! approves that proposal by its fingerprint, and the `apply` applies only that fingerprint.

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

use capability::{Approval, Call, Capability, CapabilityError, Effect, Run};
use kernel::{Proposal, TaskEvent, TaskMoment};

use super::dictionary;
use super::outcome::{Again, Ask, Body, Next, Outcome};
use super::policy::{self, Datum, Destination, Purpose};
use super::request::{Channel, Content, Request};
use super::store;
use super::task::{Pause, Phase, Reached, Task, TaskState, Waiting};
use super::{Runtime, RuntimeError, Who};

type Done = Result<Outcome, RuntimeError>;

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
    Body::Error { code: code.into(), next }
}

impl Runtime<'_> {
    /// Every request goes through here: a screen that names a capability, the chat, a fact of the calendar. It gets
    /// the connection exclusive, because the `apply` of a change hands it to the kernel for its transaction.
    pub fn submit(&self, conn: &mut Connection, who: &Who, request: Request) -> Done {
        let now = self.clock.now_ms();
        let Request { channel, task, content } = request;
        let mut task = match task {
            None => self.open(conn, who, channel, now)?,
            Some(id) => match store::load_task(conn, &id)? {
                Some(t) if t.account == who.account && t.organization == who.organization => t,
                // a task of another account is not touched: a new one says no
                _ => {
                    let mut t = self.open(conn, who, channel, now)?;
                    return self.close(conn, &mut t, TaskState::Denied, "not_your_task", 0, error("not_your_task", Next::StartAgain));
                }
            },
        };
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

    /// What the person wrote, before any model: the first point of the policy (ADR-004 §6). Without a provider, or
    /// with something in it that may not go to this one, it ends here; what may go is what the router will receive.
    fn chat(&self, conn: &Connection, task: &mut Task, text: String) -> Done {
        let Some(provider) = self.provider else {
            return self.close(conn, task, TaskState::Error, "without_ai", 0, error("without_ai", Next::UseScreen));
        };
        let to_router = policy::to_model(provider, Purpose::Route, vec![Datum::Text(text)]);
        if !to_router.left_out().is_empty() {
            self.trail(conn, task, TaskMoment::Denied, None, None, Some("not_for_this_provider"))?;
            return self.close(conn, task, TaskState::Denied, "not_for_this_provider", 0, error("not_for_this_provider", Next::UseScreen));
        }
        // the router comes with IA-4, and it receives `to_router`, never the text as it was written
        self.close(conn, task, TaskState::Error, "without_ai", 0, error("without_ai", Next::UseScreen))
    }

    fn open(&self, conn: &Connection, who: &Who, channel: Channel, now: i64) -> Result<Task, RuntimeError> {
        let task = Task::open(format!("task_{}", ulid::Ulid::generate()), who.organization, who.account, channel, now);
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
                task.waiting = Some(Waiting { capability: cap.id.into(), phase: Phase::Apply, input, correction: false, asked: vec![] });
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
            Ask::Correct { .. } => vec![],
        };
        task.waiting = Some(Waiting { capability: cap.id.into(), phase, input, correction, asked });
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
