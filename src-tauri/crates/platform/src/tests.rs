use std::cell::{Cell, RefCell};

use rusqlite::Connection;
use serde_json::{json, Value};

use super::outcome::{Again, Ask, Body, Next, Outcome};
use super::request::{Channel, Content, Request};
use super::task::{Limits, Phase, Reached, Task, TaskState};
use super::*;
use capability::Registry;
use kernel::Modules;

/// A module as any other would be, with the capabilities the tests need. It keeps a name in a table of its own, and
/// changes it on the kernel (ERP-9), like every module, and its part of the dictionary (K3) checks the name.
pub mod trial {
    use capability::{Call, CapabilityError, Limits};
    use kernel::{Applied, Changeable, Draft, Entity, Field, Kind, Origin, Proposal, Ref};
    use rusqlite::Connection;
    use serde_json::{json, Map, Value};

    pub const NAME: Entity = Entity { id: "name", fields: &[Field::new("name", Kind::Text, "public").mandatory().max_len(40)] };

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Nothing {}

    #[derive(serde::Serialize, schemars::JsonSchema)]
    pub struct Name {
        pub name: String,
    }

    #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
    pub struct Rename {
        pub name: String,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Count {
        pub n: i64,
    }

    fn db(_: rusqlite::Error) -> CapabilityError {
        CapabilityError::Failed { reason: "the database did not answer" }
    }

    fn current(conn: &Connection) -> rusqlite::Result<(String, i64)> {
        conn.query_row("SELECT name, version FROM trial_name WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    /// The name of the trial, for the kernel: what it needs to prepare and apply one change of it.
    pub struct SetName {
        name: String,
        input: Value,
    }

    /// The change of the name that this input asks for.
    pub fn set_name(name: &str, input: &impl serde::Serialize) -> SetName {
        SetName { name: name.into(), input: serde_json::to_value(input).unwrap() }
    }

    fn fields(name: &str) -> Map<String, Value> {
        Map::from_iter([("name".to_string(), json!(name))])
    }

    impl Changeable for SetName {
        fn prepare(&self, conn: &Connection) -> rusqlite::Result<Proposal> {
            let (now, version) = current(conn)?;
            // the dictionary checks the name (ERP-2): nothing is left that only the module can check
            Ok(kernel::propose(
                &NAME,
                Draft { entity: Ref::new("trial", "name", "1"), version, before: Some(fields(&now)), after: fields(&self.name), invalid: vec![], input: &self.input },
            ))
        }

        fn version(&self, conn: &Connection) -> rusqlite::Result<i64> {
            Ok(current(conn)?.1)
        }

        fn write(&self, conn: &Connection, proposal: &Proposal) -> rusqlite::Result<()> {
            conn.execute(
                "UPDATE trial_name SET name = ?1, version = version + 1 WHERE id = 1 AND version = ?2",
                rusqlite::params![self.name, proposal.base_version],
            )?;
            Ok(())
        }
    }

    /// What happened to the name, for whoever reacts to it.
    pub const NAME_CHANGED: &str = "trial.name_changed";

    fn apply(call: &mut Call, change: SetName) -> Result<Applied, CapabilityError> {
        let origin = Origin::Task { task: call.task.into(), account: call.by.into() };
        kernel::apply(call.conn, &change, call.fingerprint, &origin, NAME_CHANGED).map_err(|e| CapabilityError::from_apply(e.code()))
    }

    pub fn read(call: &Call, _: Nothing) -> Result<Name, CapabilityError> {
        Ok(Name { name: current(call.conn).map_err(db)?.0 })
    }

    pub fn preview(call: &Call, ask: Rename) -> Result<Proposal, CapabilityError> {
        if ask.name.trim().is_empty() {
            return Err(CapabilityError::NeedsPerson { what: "the new name" });
        }
        set_name(&ask.name, &ask).prepare(call.conn).map_err(db)
    }

    pub fn rename(call: &mut Call, ask: Rename) -> Result<Applied, CapabilityError> {
        apply(call, set_name(&ask.name, &ask))
    }

    /// Never has enough until the count reaches 100: a task that does not advance.
    pub fn count(_: &Call, ask: Count) -> Result<Name, CapabilityError> {
        if ask.n < 100 {
            return Err(CapabilityError::NeedsPerson { what: "a larger count" });
        }
        Ok(Name { name: ask.n.to_string() })
    }

    pub fn broken(_: &Call, _: Nothing) -> Result<Name, CapabilityError> {
        Err(CapabilityError::Failed { reason: "always fails" })
    }

    #[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
    pub struct RenameWhy {
        pub name: String,
        pub why: Option<String>,
    }

    pub fn preview_why(call: &Call, ask: RenameWhy) -> Result<Proposal, CapabilityError> {
        set_name(&ask.name, &ask).prepare(call.conn).map_err(db)
    }

    /// Only when it applies does it find that something of the person is missing.
    pub fn rename_why(call: &mut Call, ask: RenameWhy) -> Result<Applied, CapabilityError> {
        if ask.why.is_none() {
            return Err(CapabilityError::NeedsPerson { what: "why the name changes" });
        }
        apply(call, set_name(&ask.name, &ask))
    }

    /// A modification that does not prepare the kernel's proposal: the app would not start with it.
    pub fn loose(_: &Call, ask: Rename) -> Result<Name, CapabilityError> {
        Ok(Name { name: ask.name })
    }

    pub fn loose_apply(_: &mut Call, ask: Rename) -> Result<Name, CapabilityError> {
        Ok(Name { name: ask.name })
    }

    capability::capability! {
        static LOOSE = "trial.loose" {
            description: "Un cambio que no prepara la propuesta del núcleo.",
            effect: Modify,
            permission: "kernel.usar",
            data: [Public],
            approval: Required,
            limits: Limits::CHANGE,
            run: change(loose, loose_apply),
        }
    }

    capability::capability! {
        static RENAME_WHY = "trial.rename_why" {
            description: "Cambia el nombre de prueba; al aplicar pide el motivo.",
            effect: Modify,
            permission: "kernel.usar",
            data: [Public],
            approval: Required,
            limits: Limits::CHANGE,
            run: change(preview_why, rename_why),
        }
    }

    capability::capability! {
        static READ = "trial.read" {
            description: "Lee el nombre de prueba.",
            effect: Query,
            permission: "kernel.usar",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(read),
        }
    }

    capability::capability! {
        static RENAME = "trial.rename" {
            description: "Cambia el nombre de prueba, con la aprobación de la persona.",
            effect: Modify,
            permission: "kernel.usar",
            data: [Public],
            approval: Required,
            limits: Limits::CHANGE,
            run: change(preview, rename),
        }
    }

    capability::capability! {
        static RENAME_NOW = "trial.rename_now" {
            description: "Cambia el nombre de prueba sin pedir aprobación.",
            effect: Modify,
            permission: "kernel.usar",
            data: [Public],
            approval: Never,
            limits: Limits::CHANGE,
            run: change(preview, rename),
        }
    }

    capability::capability! {
        static UNKNOWN = "trial.unknown" {
            description: "Pide un permiso que nadie define.",
            effect: Query,
            permission: "trial.nadie",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(read),
        }
    }

    capability::capability! {
        static ADMIN = "trial.admin" {
            description: "Una consulta solo para quien administra.",
            effect: Query,
            permission: "kernel.administrar",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(read),
        }
    }

    capability::capability! {
        static COUNT = "trial.count" {
            description: "Pide una cuenta mayor hasta llegar a 100.",
            effect: Query,
            permission: "kernel.usar",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(count),
        }
    }

    capability::capability! {
        static BROKEN = "trial.broken" {
            description: "Siempre falla.",
            effect: Query,
            permission: "kernel.usar",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(broken),
        }
    }
}

struct Clock(Cell<i64>);

impl super::Clock for Clock {
    fn now_ms(&self) -> i64 {
        // a millisecond passes on each look, so the trail keeps its order
        self.0.set(self.0.get() + 1);
        self.0.get()
    }
}

impl Clock {
    fn advance(&self, ms: i64) {
        self.0.set(self.0.get() + ms);
    }
}

/// The accounts of the tests: Ana has the session open, in the role of the direction (`manager`, which may use the
/// app and not administer it). Nobody else has a session. A test may change her account as an administrator would.
struct Accounts {
    role: Cell<&'static str>,
    active: Cell<bool>,
    organization: Cell<&'static str>,
}

impl super::Accounts for Accounts {
    fn now(&self, _: &Connection, who: &Who) -> Option<kernel::Account> {
        (who.account == ANA_ID).then(|| kernel::Account {
            id: ANA_ID.into(),
            organization: self.organization.get().into(),
            role: self.role.get().into(),
            active: self.active.get(),
            must_change_password: false,
        })
    }
}

#[derive(Default)]
struct Window(RefCell<Vec<Outcome>>);

impl Deliver for Window {
    fn deliver(&self, outcome: &Outcome) {
        self.0.borrow_mut().push(outcome.clone());
    }
}

struct World {
    conn: RefCell<Connection>,
    registry: Registry,
    modules: Modules,
    clock: Clock,
    accounts: Accounts,
    window: Window,
}

const ANA_ID: &str = "usr_01JABCDEFGHJKMNPQRSTVWXYZ0";
const LUIS_ID: &str = "usr_01JZZZZZZZZZZZZZZZZZZZZZZZ";
const ANA: Who = Who { account: ANA_ID, organization: "org" };

fn trial_table(conn: &Connection) {
    conn.execute_batch("CREATE TABLE trial_name (id INTEGER PRIMARY KEY, name TEXT NOT NULL, version INTEGER NOT NULL); INSERT INTO trial_name (id, name, version) VALUES (1, 'Casa Hogar', 1);")
        .unwrap();
}

/// The tables of the kernel (its audit store) and of the platform, as the app opens them.
fn migrated(conn: &mut Connection) {
    kernel::migrate(conn, &Modules::discover().unwrap()).unwrap();
    migrate(conn).unwrap();
}

fn world() -> World {
    let mut conn = Connection::open_in_memory().unwrap();
    migrated(&mut conn);
    trial_table(&conn);
    World::on(conn)
}

impl World {
    fn on(conn: Connection) -> World {
        World {
            conn: RefCell::new(conn),
            registry: Registry::discover().unwrap(),
            modules: Modules::discover().unwrap(),
            clock: Clock(Cell::new(1_000_000)),
            accounts: Accounts { role: Cell::new("manager"), active: Cell::new(true), organization: Cell::new("org") },
            window: Window::default(),
        }
    }

    fn runtime(&self) -> Runtime<'_> {
        Runtime { registry: &self.registry, modules: &self.modules, accounts: &self.accounts, clock: &self.clock, deliver: &self.window }
    }

    fn submit(&self, who: &Who, task: Option<&str>, content: Content) -> Outcome {
        let runtime = self.runtime();
        let request = Request { channel: Channel::Screen { place: "institution".into() }, task: task.map(String::from), content };
        runtime.submit(&mut self.conn.borrow_mut(), who, request).unwrap()
    }

    fn invoke(&self, task: Option<&str>, capability: &str, input: Value) -> Outcome {
        self.submit(&ANA, task, Content::Invoke { capability: capability.into(), input })
    }

    fn answer(&self, task: &str, values: Value) -> Outcome {
        self.submit(&ANA, Some(task), Content::Answer { values })
    }

    fn decide(&self, task: &str, fingerprint: &str, approve: bool) -> Outcome {
        self.submit(&ANA, Some(task), Content::Decision { fingerprint: fingerprint.into(), approve })
    }

    /// How many rows a table has.
    fn count(&self, table: &str) -> i64 {
        self.conn.borrow().query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    fn name(&self) -> String {
        self.conn.borrow().query_row("SELECT name FROM trial_name WHERE id = 1", [], |r| r.get(0)).unwrap()
    }

    fn task(&self, id: &str) -> Task {
        store::load_task(&self.conn.borrow(), id).unwrap().unwrap()
    }

    /// The trail of a task in the kernel's audit store: (moment, capability, outcome, reason).
    fn trail(&self, id: &str) -> Vec<Line> {
        self.conn
            .borrow()
            .prepare("SELECT event, subject, outcome, reason FROM kernel_audit WHERE correlation_id = ?1 ORDER BY rowid")
            .unwrap()
            .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

type Line = (String, Option<String>, Option<String>, Option<String>);

fn fingerprint_of(o: &Outcome) -> String {
    match &o.body {
        Body::Proposal { fingerprint, .. } => fingerprint.clone(),
        other => panic!("not a proposal: {other:?}"),
    }
}

fn line(moment: &str, capability: Option<&str>, outcome: Option<&str>, reason: Option<&str>) -> Line {
    (moment.into(), capability.map(String::from), outcome.map(String::from), reason.map(String::from))
}

/// The whole path of a change asked from a screen, with no router and no model: it is prepared, waits for the
/// approval of the person, is applied, and leaves its trail. Run it with `-- --nocapture` to read it.
#[test]
fn a_change_from_a_screen_walks_the_whole_path() {
    let w = world();
    println!("\nLa persona cambia el nombre en la pantalla y pulsa «Guardar».");
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Casa Hogar San José" }));
    let task = asked.task.clone();
    println!("  tarea {task}: {:?}, {:?}", asked.state, asked.body);
    assert_eq!(asked.state, TaskState::Paused);
    let Body::Proposal { proposal, again: None, .. } = &asked.body else { panic!("{asked:?}") };
    assert_eq!(proposal["base_version"], 1);
    assert_eq!(proposal["changes"], json!([{ "field": "name", "before": "Casa Hogar", "after": "Casa Hogar San José", "protected": false }]));
    assert_eq!(w.name(), "Casa Hogar", "nothing changes before the approval");

    println!("La persona revisa la propuesta y la aprueba.");
    let done = w.decide(&task, &fingerprint_of(&asked), true);
    println!("  tarea {}: {:?}, {:?}", done.task, done.state, done.body);
    assert_eq!(done.task, task, "the same task, resumed");
    assert_eq!(done.state, TaskState::Done);
    assert!(matches!(&done.body, Body::Data { capability, .. } if capability == "trial.rename"));
    assert_eq!(w.name(), "Casa Hogar San José");
    let (origin, by_task, by): (String, String, String) =
        w.conn.borrow().query_row("SELECT origin, task_id, account_id FROM kernel_change", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((origin.as_str(), by_task.as_str(), by.as_str()), ("task", task.as_str(), ANA_ID), "the change carries its task and who approved it");

    println!("Lo que quedó en la auditoría del núcleo para esa tarea:");
    let trail = w.trail(&task);
    for (moment, capability, outcome, reason) in &trail {
        let show = |s: &Option<String>| s.clone().unwrap_or_default();
        println!("  {moment:<14} {:<13} {:<7} {}", show(capability), show(outcome), show(reason));
    }
    assert_eq!(
        trail,
        vec![
            line("task.opened", None, Some("screen"), None),
            line("task.step", Some("trial.rename"), Some("ok"), None),
            line("task.closed", None, Some("paused"), Some("approval")),
            line("task.approved", Some("trial.rename"), None, None),
            line("task.step", Some("trial.rename"), Some("ok"), None),
            line("task.closed", None, Some("done"), Some("done")),
        ]
    );
    let account: String = w.conn.borrow().query_row("SELECT DISTINCT account_id FROM kernel_audit WHERE correlation_id = ?1", [&task], |r| r.get(0)).unwrap();
    assert_eq!(account, ANA_ID, "each moment says for whom the task runs");
    let delivered = w.window.0.borrow();
    assert_eq!(delivered.len(), 2, "each outcome is also handed to the window");
    assert!(delivered.iter().all(|o| o.task == task));
}

#[test]
fn a_query_hands_its_data_without_approval() {
    let w = world();
    let o = w.invoke(None, "trial.read", json!({}));
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(o.body, Body::Data { capability: "trial.read".into(), value: json!({ "name": "Casa Hogar" }) });
    assert_eq!(o.step, 1);
}

#[test]
fn a_change_that_needs_no_approval_is_prepared_and_applied_at_once() {
    let w = world();
    let o = w.invoke(None, "trial.rename_now", json!({ "name": "Otra" }));
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(w.name(), "Otra");
    assert_eq!(w.task(&o.task).steps, 2, "prepare and apply");
}

#[test]
fn every_close_is_recorded_with_its_reason_and_no_personal_data() {
    let w = world();
    let done = w.invoke(None, "trial.read", json!({}));
    let denied = w.invoke(None, "trial.admin", json!({}));
    let error = w.invoke(None, "trial.broken", json!({}));
    let paused = w.invoke(None, "trial.rename", json!({ "name": "Rosa Hernández López" }));
    let limit = w.invoke(None, "trial.count", json!({ "n": 1 }));
    let limit = w.answer(&limit.task, json!({ "n": 1 }));

    let closes: Vec<(TaskState, Option<String>)> = [&done, &denied, &error, &paused, &limit].iter().map(|o| {
        let t = w.task(&o.task);
        (t.state, t.reason)
    }).collect();
    assert_eq!(
        closes,
        vec![
            (TaskState::Done, Some("done".into())),
            (TaskState::Denied, Some("access_denied".into())),
            (TaskState::Error, Some("capability_failed".into())),
            (TaskState::Paused, Some("approval".into())),
            (TaskState::Limit, Some("limit_repeated".into())),
        ]
    );
    assert_eq!(denied.body, Body::Error { code: "access_denied".into(), next: Next::AskAdministrator });
    assert!(w.trail(&denied.task).contains(&line("task.denied", Some("trial.admin"), None, Some("access_denied"))));
    assert!(w.trail(&paused.task).contains(&line("task.closed", None, Some("paused"), Some("approval"))));

    // the trail holds ids and fixed codes: never the name the person wrote
    let trail: Vec<String> = w.conn
        .borrow()
        .prepare("SELECT event || ' ' || ifnull(subject, '') || ' ' || ifnull(outcome, '') || ' ' || ifnull(reason, '') FROM kernel_audit")
        .unwrap()
        .query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert!(trail.iter().all(|l| l.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c == '.' || c == ' ')), "{trail:?}");
    let everything: String = w.conn.borrow().query_row("SELECT group_concat(quote(at) || quote(event) || quote(correlation_id) || quote(account_id) || quote(subject) || quote(outcome) || quote(reason), ' ') FROM kernel_audit", [], |r| r.get(0)).unwrap();
    assert!(!everything.contains("Rosa"));
}

#[test]
fn a_task_that_does_not_advance_is_cut_by_its_steps() {
    let w = world();
    let mut o = w.invoke(None, "trial.count", json!({ "n": 1 }));
    let limit = Limits::of(&Channel::Screen { place: String::new() }).steps;
    for n in 2..=limit + 1 {
        assert_eq!(o.state, TaskState::Paused);
        o = w.answer(&o.task, json!({ "n": n }));
    }
    assert_eq!(o.state, TaskState::Limit);
    assert_eq!(o.body, Body::Error { code: Reached::Steps.code().into(), next: Next::StartAgain });
    assert_eq!(w.task(&o.task).steps, limit);
}

#[test]
fn the_same_capability_with_the_same_input_twice_closes_the_task() {
    let w = world();
    let o = w.invoke(None, "trial.count", json!({ "n": 5 }));
    let o = w.answer(&o.task, json!({}));
    assert_eq!((o.state, w.task(&o.task).reason), (TaskState::Limit, Some("limit_repeated".into())));
    assert_eq!(w.task(&o.task).steps, 1, "the repeated step never ran");
}

#[test]
fn corrections_have_a_limit() {
    let w = world();
    let long = |i: usize| format!("{}{i}", "x".repeat(50));
    let mut o = w.invoke(None, "trial.rename", json!({ "name": long(0) }));
    // what does not pass comes in the proposal: the person corrects it, with its field and its code
    assert_eq!(o.body, Body::Clarification { capability: "trial.rename".into(), ask: Ask::Correct { field: "name".into(), reason: "too_long".into() } });
    for i in 1..=3 {
        o = w.answer(&o.task, json!({ "name": long(i) }));
        assert_eq!(o.state, TaskState::Paused, "correction {i}");
    }
    let o = w.answer(&o.task, json!({ "name": long(4) }));
    assert_eq!((o.state, w.task(&o.task).reason), (TaskState::Limit, Some("limit_corrections".into())));
}

#[test]
fn an_input_that_does_not_fit_its_contract_never_runs() {
    let w = world();
    let o = w.invoke(None, "trial.rename", json!({ "nombre": "Otra" }));
    assert_eq!(o.body, Body::Clarification { capability: "trial.rename".into(), ask: Ask::Correct { field: "name".into(), reason: "does_not_fit".into() } });
    assert_eq!(w.task(&o.task).steps, 0);
    // corrected, it goes on
    let o = w.answer(&o.task, json!({ "name": "Otra" }));
    assert!(matches!(o.body, Body::Proposal { .. }));
}

#[test]
fn a_pause_runs_out() {
    let w = world();
    let o = w.invoke(None, "trial.rename", json!({ "name": "" }));
    assert_eq!(o.body, Body::Clarification { capability: "trial.rename".into(), ask: Ask::Missing { what: "the new name".into() } });
    w.clock.advance(Limits::of(&o.channel).pause_ms + 1);
    let o = w.answer(&o.task, json!({ "name": "Otra" }));
    assert_eq!((o.state, w.task(&o.task).reason), (TaskState::Limit, Some("limit_pause".into())));
    assert_eq!(w.name(), "Casa Hogar");
}

#[test]
fn a_turn_has_a_time_limit_and_spending_is_reserved_first() {
    let mut task = Task::open("t".into(), "org", "ana", Channel::Chat, 0);
    let limits = task.limits();
    assert_eq!(task.may_step(limits.turn_ms), Ok(()));
    assert_eq!(task.may_step(limits.turn_ms + 1), Err(Reached::Time));

    // two calls at once cannot pass the limit together: the second one is stopped by what the first reserved
    let half = limits.spend_mxn / 2.0;
    assert_eq!(task.reserve(half + 0.1), Ok(()));
    assert_eq!(task.reserve(half), Err(Reached::Spend));
    task.settle(half + 0.1, 0.3);
    assert_eq!(task.reserve(half), Ok(()));
    assert_eq!(task.reserve(limits.spend_mxn), Err(Reached::Spend));
    // a screen spends nothing on models
    let mut screen = Task::open("s".into(), "org", "ana", Channel::Screen { place: String::new() }, 0);
    assert_eq!(screen.reserve(0.01), Err(Reached::Spend));
}

#[test]
fn new_parameters_make_the_earlier_approval_stale() {
    let w = world();
    let first = w.invoke(None, "trial.rename", json!({ "name": "Uno" }));
    let task = first.task.clone();
    let second = w.invoke(Some(&task), "trial.rename", json!({ "name": "Dos" }));
    assert!(matches!(second.body, Body::Proposal { again: Some(Again::Changed), .. }), "{second:?}");

    // approving what was shown first does not apply it: the proposal waiting is shown again
    let o = w.decide(&task, &fingerprint_of(&first), true);
    assert!(matches!(o.body, Body::Proposal { again: Some(Again::Outdated), .. }), "{o:?}");
    assert_eq!(fingerprint_of(&o), fingerprint_of(&second));
    assert_eq!(w.name(), "Casa Hogar");

    let o = w.decide(&task, &fingerprint_of(&second), true);
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(w.name(), "Dos");
    let states: Vec<String> = w.conn.borrow().prepare("SELECT state FROM ai_approval ORDER BY asked_ms").unwrap()
        .query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(states, vec!["stale", "approved"]);
}

#[test]
fn the_same_proposal_asked_again_is_shown_again_without_running() {
    let w = world();
    let first = w.invoke(None, "trial.rename", json!({ "name": "Uno" }));
    let again = w.invoke(Some(&first.task), "trial.rename", json!({ "name": "Uno" }));
    assert_eq!(fingerprint_of(&again), fingerprint_of(&first));
    assert_eq!(w.task(&first.task).steps, 1);
}

#[test]
fn rejecting_a_proposal_changes_nothing() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let o = w.decide(&asked.task, &fingerprint_of(&asked), false);
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(o.body, Body::State { reason: "rejected".into() });
    assert_eq!(w.name(), "Casa Hogar");
    assert!(w.trail(&asked.task).contains(&line("task.closed", None, Some("done"), Some("rejected"))));
}

/// The kernel decides again right before applying: an account the administrator deactivated meanwhile applies nothing.
#[test]
fn the_kernel_is_asked_again_before_applying() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    w.accounts.active.set(false);
    let o = w.decide(&asked.task, &fingerprint_of(&asked), true);
    assert_eq!(o.state, TaskState::Denied);
    assert_eq!(o.body, Body::Error { code: "account_inactive".into(), next: Next::AskAdministrator });
    assert_eq!(w.name(), "Casa Hogar");
    assert!(w.trail(&asked.task).contains(&line("task.denied", Some("trial.rename"), None, Some("account_inactive"))));
}

/// What the kernel says no to, with its reason: another organization, a permission nobody defines, the role, and a
/// request with no session behind it.
#[test]
fn the_kernel_decides_and_says_why() {
    let w = world();
    let reason = |o: &Outcome| w.task(&o.task).reason;

    w.accounts.organization.set("otra");
    assert_eq!(reason(&w.invoke(None, "trial.read", json!({}))), Some("other_organization".into()));
    w.accounts.organization.set("org");

    assert_eq!(reason(&w.invoke(None, "trial.unknown", json!({}))), Some("unknown_permission".into()));
    assert_eq!(reason(&w.invoke(None, "trial.admin", json!({}))), Some("access_denied".into()));
    w.accounts.role.set("admin");
    assert_eq!(reason(&w.invoke(None, "trial.admin", json!({}))), Some("done".into()), "the administration may");

    let luis = Who { account: LUIS_ID, organization: "org" };
    let o = w.submit(&luis, None, Content::Invoke { capability: "trial.read".into(), input: json!({}) });
    assert_eq!(reason(&o), Some("no_session".into()));
}

/// A capability whose permission neither the kernel nor a module defines stops the app when it starts.
#[test]
fn a_permission_nobody_defines_is_found_before_starting() {
    let w = world();
    let unknown: Vec<&str> = unknown_permissions(&w.registry, &w.modules).iter().map(|e| e.id).collect();
    assert_eq!(unknown, vec!["trial.unknown"]);
}

/// The app closed while the change ran: the step stayed «started». The kernel says whether its fingerprint was
/// applied; applied, the task ends well, and the change is never made twice.
#[test]
fn an_apply_that_never_ended_and_was_applied_ends_well() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let fp = fingerprint_of(&asked);
    {
        let mut conn = w.conn.borrow_mut();
        store::start_step(&conn, &asked.task, 2, "trial.rename", Phase::Apply, "h", Some(&fp), 0).unwrap();
        let origin = kernel::Origin::Task { task: asked.task.clone(), account: ANA_ID.into() };
        kernel::apply(&mut conn, &trial::set_name("Otra", &json!({ "name": "Otra" })), Some(&fp), &origin, trial::NAME_CHANGED).unwrap();
    }
    let o = w.decide(&asked.task, &fp, true);
    assert_eq!(o.state, TaskState::Done);
    assert!(matches!(&o.body, Body::Data { capability, value } if capability == "trial.rename" && value["already_applied"] == true), "{o:?}");
    assert_eq!((w.name().as_str(), w.count("kernel_change")), ("Otra", 1), "it is never made twice");
    let result: String = w.conn.borrow().query_row("SELECT result FROM ai_step WHERE task_id = ?1 AND n = 2", [&asked.task], |r| r.get(0)).unwrap();
    assert_eq!(result, "ok");
}

/// Not applied, the task says so and applies nothing on its own: the kernel's transaction leaves no change half made.
#[test]
fn an_apply_that_never_ended_and_was_not_applied_is_not_run_again() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let fp = fingerprint_of(&asked);
    store::start_step(&w.conn.borrow(), &asked.task, 2, "trial.rename", Phase::Apply, "h", Some(&fp), 0).unwrap();
    let o = w.decide(&asked.task, &fp, true);
    assert_eq!(o.state, TaskState::Error);
    assert_eq!(o.body, Body::Error { code: "not_applied".into(), next: Next::StartAgain });
    assert_eq!((w.name().as_str(), w.count("kernel_change")), ("Casa Hogar", 0), "it is never repeated blindly");
    assert!(w.trail(&asked.task).contains(&line("task.step", Some("trial.rename"), Some("not_applied"), None)));
}

/// An apply with nothing to ask the kernel about (not a change of the kernel's) stays uncertain.
#[test]
fn an_apply_the_kernel_cannot_tell_about_stays_uncertain() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    store::start_step(&w.conn.borrow(), &asked.task, 2, "trial.rename", Phase::Apply, "h", None, 0).unwrap();
    let o = w.decide(&asked.task, &fingerprint_of(&asked), true);
    assert_eq!(o.body, Body::Error { code: "uncertain_change".into(), next: Next::CheckHistory });
    assert_eq!(w.name(), "Casa Hogar");
}

/// The person approves the kernel's proposal: its fingerprint takes the version of the datum, and the kernel applies
/// only that one.
#[test]
fn the_approval_is_tied_to_the_kernels_proposal() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let fp = fingerprint_of(&asked);
    assert_eq!(fp, kernel::fingerprint(&kernel::Ref::new("trial", "name", "1"), 1, &json!({ "name": "Otra" })));
    assert_ne!(fp, super::executor::fingerprint("trial.rename", &json!({ "name": "Otra" })));
    let done = w.decide(&asked.task, &fp, true);
    assert_eq!(done.state, TaskState::Done);
    let applied: String = w.conn.borrow().query_row("SELECT fingerprint FROM kernel_change", [], |r| r.get(0)).unwrap();
    assert_eq!(applied, fp);
}

/// Someone changed the datum after the person approved: the kernel does not apply it, and the person sees it as it is
/// now and approves it again.
#[test]
fn a_datum_that_changed_after_the_approval_is_prepared_and_approved_again() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    w.conn.borrow().execute("UPDATE trial_name SET name = 'Casa Hogar A.C.', version = version + 1", []).unwrap();

    let again = w.decide(&asked.task, &fingerprint_of(&asked), true);
    assert_eq!(again.state, TaskState::Paused);
    let Body::Proposal { proposal, again: Some(Again::DataChanged), .. } = &again.body else { panic!("{again:?}") };
    assert_eq!(proposal["changes"][0]["before"], "Casa Hogar A.C.", "the datum as it is now");
    assert_eq!(serde_json::to_value(&again).unwrap()["body"]["again"], "data_changed");
    assert_ne!(fingerprint_of(&again), fingerprint_of(&asked));
    assert_eq!(w.name(), "Casa Hogar A.C.", "nothing applied on the earlier approval");
    assert!(w.trail(&asked.task).contains(&line("task.step", Some("trial.rename"), Some("stale"), None)));

    let done = w.decide(&asked.task, &fingerprint_of(&again), true);
    assert_eq!(done.state, TaskState::Done);
    assert_eq!(w.name(), "Otra");
}

#[test]
fn a_proposal_without_changes_asks_for_no_approval() {
    let w = world();
    let o = w.invoke(None, "trial.rename", json!({ "name": "Casa Hogar" }));
    assert_eq!((o.state, o.body.clone()), (TaskState::Done, Body::State { reason: "nothing_to_change".into() }));
    assert_eq!((w.count("ai_approval"), w.count("kernel_change")), (0, 0));
}

/// A modification whose `prepare` does not give the kernel's proposal stops the app when it starts.
#[test]
fn a_change_without_the_kernels_proposal_is_found_before_starting() {
    let w = world();
    let loose: Vec<&str> = changes_without_proposal(&w.registry).iter().map(|e| e.id).collect();
    assert_eq!(loose, vec!["trial.loose"]);
}

#[test]
fn a_task_of_another_account_is_not_touched() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let luis = Who { account: LUIS_ID, organization: "org" };
    let o = w.submit(&luis, Some(&asked.task), Content::Decision { fingerprint: fingerprint_of(&asked), approve: true });
    assert_ne!(o.task, asked.task);
    assert_eq!(o.state, TaskState::Denied);
    assert_eq!(w.task(&asked.task).state, TaskState::Paused);
    let runtime = w.runtime();
    assert!(runtime.task(&w.conn.borrow(), &luis, &asked.task).unwrap().is_none(), "nor can it be read");
    assert_eq!(runtime.task(&w.conn.borrow(), &ANA, &asked.task).unwrap(), Some(asked));
}

#[test]
fn a_closed_task_does_not_move() {
    let w = world();
    let done = w.invoke(None, "trial.read", json!({}));
    let again = w.invoke(Some(&done.task), "trial.rename_now", json!({ "name": "Otra" }));
    assert_eq!(again, done);
    assert_eq!(w.name(), "Casa Hogar");
}

#[test]
fn the_chat_without_a_model_says_how_to_go_on() {
    let w = world();
    let o = w.submit(&ANA, None, Content::Text { text: "¿Qué me falta?".into() });
    assert_eq!(o.body, Body::Error { code: "without_ai".into(), next: Next::UseScreen });
    assert!(o.state.is_closed());
}

#[test]
fn a_paused_task_survives_reopening_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let (task, fp) = {
        let mut conn = Connection::open(&path).unwrap();
        migrated(&mut conn);
        trial_table(&conn);
        let w = World::on(conn);
        let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
        (asked.task.clone(), fingerprint_of(&asked))
    };
    // the app opens again: a new connection, a new runtime, the same task
    let mut conn = Connection::open(&path).unwrap();
    migrated(&mut conn);
    let w = World::on(conn);
    assert_eq!(w.task(&task).state, TaskState::Paused);
    let o = w.decide(&task, &fp, true);
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(w.name(), "Otra");
}

#[test]
fn the_tables_of_the_platform_keep_the_shape_of_an_owner() {
    let mut conn = Connection::open_in_memory().unwrap();
    assert_eq!(kernel::conform(store::OWNER, store::MIGRATIONS, &mut conn), vec![]);
}

#[test]
fn the_fingerprint_does_not_depend_on_the_order_of_the_keys() {
    let a = super::executor::fingerprint("m.c", &json!({ "a": 1, "b": { "x": [1, 2], "y": null } }));
    let b = super::executor::fingerprint("m.c", &serde_json::from_str::<Value>(r#"{"b":{"y":null,"x":[1,2]},"a":1}"#).unwrap());
    assert_eq!(a, b);
    assert_ne!(a, super::executor::fingerprint("m.c", &json!({ "a": 2, "b": { "x": [1, 2], "y": null } })));
    assert_ne!(a, super::executor::fingerprint("m.d", &json!({ "a": 1, "b": { "x": [1, 2], "y": null } })));
}

#[test]
fn a_request_and_an_outcome_read_as_the_screens_write_them() {
    let r: Request = serde_json::from_value(json!({
        "channel": { "kind": "screen", "place": "institution" },
        "content": { "kind": "invoke", "capability": "trial.read", "input": {} }
    }))
    .unwrap();
    assert_eq!(r.channel, Channel::Screen { place: "institution".into() });
    assert!(r.task.is_none());
    let o = world().invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let v = serde_json::to_value(&o).unwrap();
    assert_eq!(v["state"], "paused");
    assert_eq!(v["body"]["kind"], "proposal");
    assert_eq!(v["body"]["again"], Value::Null);
    assert_eq!(v["channel"], json!({ "kind": "screen", "place": "institution" }));
}

#[test]
fn what_the_person_adds_to_an_approved_change_is_approved_again() {
    let w = world();
    let asked = w.invoke(None, "trial.rename_why", json!({ "name": "Otra" }));
    let o = w.decide(&asked.task, &fingerprint_of(&asked), true);
    assert_eq!(o.body, Body::Clarification { capability: "trial.rename_why".into(), ask: Ask::Missing { what: "why the name changes".into() } });

    // the answer changes the input: it is prepared and shown again, never applied on the earlier approval
    let o = w.answer(&asked.task, json!({ "why": "Cambio de razón social" }));
    assert!(matches!(&o.body, Body::Proposal { .. }), "{o:?}");
    assert_ne!(fingerprint_of(&o), fingerprint_of(&asked));
    assert_eq!(w.name(), "Casa Hogar");

    let o = w.decide(&asked.task, &fingerprint_of(&o), true);
    assert_eq!(o.state, TaskState::Done);
    assert_eq!(w.name(), "Otra");
}

#[test]
fn a_request_that_changes_nothing_keeps_what_the_task_said_last() {
    let w = world();
    let asked = w.invoke(None, "trial.rename", json!({ "name": "Otra" }));
    let o = w.answer(&asked.task, json!({ "name": "Tercera" }));
    assert_eq!(o.body, Body::Error { code: "not_waiting".into(), next: Next::StartAgain });
    let runtime = w.runtime();
    assert_eq!(runtime.task(&w.conn.borrow(), &ANA, &asked.task).unwrap(), Some(asked), "the proposal is still what waits");
}
