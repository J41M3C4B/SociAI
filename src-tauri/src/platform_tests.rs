//! The AI platform with the modules the app links (ADR-003): the capabilities of a module, declared from its own crate
//! with a permission it defines, go through the runtime from end to end, and the kernel decides on each step (K2). A
//! change goes through the kernel (ERP-9), tied to the proposal the person approved.

use std::cell::Cell;

use capability::Registry;
use kernel::Modules;
use platform::outcome::{Ask, Body};
use platform::request::{Channel, Content};
use platform::task::TaskState;
use platform::{Accounts, Clock, Deliver, Outcome, Request, Runtime, Who};
use rusqlite::Connection;
use serde_json::json;

const ANA: &str = "usr_01JABCDEFGHJKMNPQRSTVWXYZ0";

/// Ana, with the session open and the role a test gives her.
struct Ana(Cell<&'static str>);

impl Accounts for Ana {
    fn now(&self, _: &Connection, who: &Who) -> Option<kernel::Account> {
        (who.account == ANA).then(|| kernel::Account {
            id: ANA.into(),
            organization: "org".into(),
            role: self.0.get().into(),
            active: true,
            must_change_password: false,
        })
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

/// The database as the app opens it: the kernel, its modules and the platform.
fn opened(modules: &Modules) -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    kernel::migrate(&mut conn, modules).unwrap();
    platform::migrate(&mut conn).unwrap();
    conn
}

fn submit(runtime: &Runtime, conn: &mut Connection, task: Option<&str>, content: Content) -> Outcome {
    let request = Request { channel: Channel::Screen { place: "muestra".into() }, task: task.map(String::from), content };
    runtime.submit(conn, &Who { account: ANA, organization: "org" }, request).unwrap()
}

/// The life of these tasks in the kernel's audit store: (moment, outcome, reason).
fn moments(conn: &Connection, tasks: &[&str]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut statement = conn.prepare("SELECT event, outcome, reason FROM kernel_audit WHERE correlation_id = ?1 ORDER BY rowid").unwrap();
    tasks
        .iter()
        .flat_map(|t| statement.query_map([t], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap())
        .collect()
}

/// Every capability the app links asks for a permission the kernel or a module defines, every modification prepares
/// the kernel's proposal, and every output names fields the dictionary has, within its `data`: the app starts.
#[test]
fn every_capability_asks_for_a_permission_that_exists() {
    let registry = Registry::discover().unwrap();
    let modules = Modules::discover().unwrap();
    assert!(registry.get("muestra.leer_nota").is_some(), "the sample module publishes from its own crate");
    assert!(registry.get("muestra.guardar_nota").is_some());
    assert_eq!(platform::unknown_permissions(&registry, &modules), vec![]);
    assert_eq!(platform::changes_without_proposal(&registry), vec![]);
    assert_eq!(platform::outputs_outside_dictionary(&registry, &modules), vec![]);
    assert_eq!(platform::capabilities_named_as_operators(&registry), vec![]);
}

#[test]
fn a_capability_of_a_module_runs_from_end_to_end() {
    let modules = Modules::discover().unwrap();
    let mut conn = opened(&modules);
    let registry = Registry::discover().unwrap();
    let ana = Ana(Cell::new("manager"));
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: None, answer: platform::Answer::Words };

    // a note the person saved on the screen, through the kernel (ERP-9)
    let id = kernel::new_id("nota");
    let note = muestra::contract::NoteInput { id: id.clone(), title: "Primera nota".into(), body: None, phone: Some("55 1234 5678".into()) };
    muestra::service::apply(&mut conn, &note, None, &kernel::Origin::Person { account: ANA.into() }).unwrap();
    let read = || Content::Invoke { capability: "muestra.leer_nota".into(), input: json!({ "id": id }) };

    // an account whose role the kernel does not know has no permission: the kernel says no
    ana.0.set("contador");
    let denied = submit(&runtime, &mut conn, None, read());
    assert_eq!(denied.state, TaskState::Denied);
    assert!(matches!(&denied.body, Body::Error { code, .. } if code == "unknown_role"), "{denied:?}");

    // the direction has the permissions of the modules (ERP-1): the module reads the note, without its protected phone
    ana.0.set("manager");
    let done = submit(&runtime, &mut conn, None, read());
    assert_eq!(done.state, TaskState::Done, "{done:?}");
    let Body::Data { capability, value, .. } = &done.body else { panic!("{done:?}") };
    assert_eq!(capability, "muestra.leer_nota");
    assert_eq!(value, &json!({ "id": id, "title": "Primera nota", "body": null, "version": 1 }));

    // and the kernel's audit store tells the life of both tasks, for whom and why
    let moments = moments(&conn, &[&denied.task, &done.task]);
    assert!(moments.contains(&("task.denied".into(), None, Some("unknown_role".into()))), "{moments:?}");
    assert!(moments.contains(&("task.step".into(), Some("ok".into()), None)), "{moments:?}");
}

/// A change of a module from end to end (IA-7): the kernel's proposal, the approval tied to its fingerprint, and the
/// note saved by the kernel with its history and the task as its origin.
#[test]
fn a_change_of_a_module_goes_through_the_kernel() {
    let modules = Modules::discover().unwrap();
    let mut conn = opened(&modules);
    let registry = Registry::discover().unwrap();
    let ana = Ana(Cell::new("manager"));
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: None, answer: platform::Answer::Words };
    let id = kernel::new_id("nota");
    let save = || Content::Invoke {
        capability: "muestra.guardar_nota".into(),
        input: json!({ "id": id, "title": "Primera nota", "body": null, "phone": "55 1234 5678" }),
    };

    // an account whose role the kernel does not know may not write notes
    ana.0.set("contador");
    let denied = submit(&runtime, &mut conn, None, save());
    assert!(matches!(&denied.body, Body::Error { code, .. } if code == "unknown_role"), "{denied:?}");

    // the administration sees the proposal, with the protected phone for her, and approves it
    ana.0.set("admin");
    let asked = submit(&runtime, &mut conn, None, save());
    let Body::Proposal { proposal, fingerprint, .. } = &asked.body else { panic!("{asked:?}") };
    assert_eq!((proposal["base_version"].clone(), proposal["fingerprint"].clone()), (json!(0), json!(fingerprint)));
    let fields: Vec<(&str, bool)> = proposal["changes"].as_array().unwrap().iter().map(|c| (c["field"].as_str().unwrap(), c["protected"] == true)).collect();
    assert_eq!(fields, [("phone", true), ("title", false)]);
    assert!(muestra::storage::find(&conn, &muestra::contract::NoteRef(id.clone())).unwrap().is_none(), "nothing before the approval");

    let done = submit(&runtime, &mut conn, Some(&asked.task), Content::Decision { fingerprint: fingerprint.clone(), approve: true });
    assert_eq!(done.state, TaskState::Done, "{done:?}");
    let note = muestra::storage::find(&conn, &muestra::contract::NoteRef(id.clone())).unwrap().unwrap();
    assert_eq!((note.title.as_str(), note.version), ("Primera nota", 1));

    // the history of the kernel says who and from which task, and that the phone changed, never its value
    let history = kernel::history(&conn, &muestra::service::note_ref(&id)).unwrap();
    let (origin, task, account): (String, String, String) =
        conn.query_row("SELECT origin, task_id, account_id FROM kernel_change WHERE fingerprint = ?1", [fingerprint], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((origin.as_str(), task.as_str(), account.as_str()), ("task", asked.task.as_str(), ANA));
    assert!(!format!("{history:?}").contains("1234"), "{history:?}");
    assert!(moments(&conn, &[&asked.task]).contains(&("task.approved".into(), None, None)));
}

/// The fields of the dictionary in the platform (IA-8): what a capability of a module gives names its fields, with
/// the class the catalog declares, and what it asks of the person is a field too, checked by the kernel.
#[test]
fn a_module_asks_and_gives_fields_of_the_dictionary() {
    let modules = Modules::discover().unwrap();
    let mut conn = opened(&modules);
    let registry = Registry::discover().unwrap();
    let ana = Ana(Cell::new("admin"));
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: None, answer: platform::Answer::Words };

    // the class of each datum of an output is the catalog's: the capability does not write it
    let classes = |id: &str| {
        let mut fields: Vec<(String, &str)> =
            platform::output_fields(registry.get(id).unwrap(), &modules).unwrap().into_iter().map(|f| (f.field, f.class)).collect();
        fields.sort();
        fields
    };
    assert_eq!(classes("muestra.leer_nota"), [("muestra.note.body".into(), "internal"), ("muestra.note.title".into(), "internal")]);
    assert_eq!(classes("muestra.contacto_de_nota"), [("muestra.note.phone".into(), "personal"), ("muestra.note.title".into(), "internal")]);

    // a note without a phone: the module says the phone is missing, and the platform asks for it with its kind
    let id = kernel::new_id("nota");
    let note = muestra::contract::NoteInput { id: id.clone(), title: "Llamar a la donante".into(), body: None, phone: None };
    muestra::service::apply(&mut conn, &note, None, &kernel::Origin::Person { account: ANA.into() }).unwrap();
    let asked = submit(&runtime, &mut conn, None, Content::Invoke { capability: "muestra.contacto_de_nota".into(), input: json!({ "id": id }) });
    let Body::Clarification { ask: Ask::Missing { fields }, .. } = &asked.body else { panic!("{asked:?}") };
    let asked_for: Vec<(&str, &serde_json::Value)> = fields.iter().map(|f| (f.id.as_str(), &f.field["kind"])).collect();
    assert_eq!(asked_for, [("muestra.note.phone", &json!("phone"))]);

    // what is not a phone does not pass the kernel, and is asked again with its code
    let again = submit(&runtime, &mut conn, Some(&asked.task), Content::Answer { values: json!({ "muestra.note.phone": "mañana" }) });
    let Body::Clarification { ask: Ask::Missing { fields }, .. } = &again.body else { panic!("{again:?}") };
    assert_eq!(fields[0].reason.as_deref(), Some("not_a_phone"));

    let done = submit(&runtime, &mut conn, Some(&asked.task), Content::Answer { values: json!({ "muestra.note.phone": "55 1234 5678" }) });
    assert_eq!(done.state, TaskState::Done, "{done:?}");
    assert_eq!(done.body, Body::Data { capability: "muestra.contacto_de_nota".into(), value: json!({ "title": "Llamar a la donante", "phone": "55 1234 5678" }), withheld: vec![] });
}

/// Mi institución on the platform (ERP-1): what is missing of the profile, saving a window with the kernel's proposal
/// and the approval of the direction, the name it cannot be saved without asked of the person as a field of the
/// dictionary, and a window read back with the class of each datum from the catalog.
#[test]
fn the_profile_of_the_institution_runs_on_the_platform() {
    let modules = Modules::discover().unwrap();
    let mut conn = opened(&modules);
    let registry = Registry::discover().unwrap();
    let ana = Ana(Cell::new("manager"));
    let runtime = Runtime { registry: &registry, modules: &modules, accounts: &ana, clock: &Ticks(Cell::new(0)), deliver: &Nowhere, models: None, answer: platform::Answer::Words };
    let invoke = |capability: &str, input: serde_json::Value| Content::Invoke { capability: capability.into(), input };

    // nothing yet: the name comes first, in its window
    let missing = submit(&runtime, &mut conn, None, invoke("mi_institucion.que_falta", json!({})));
    let Body::Data { value, .. } = &missing.body else { panic!("{missing:?}") };
    assert_eq!(value["missing"][0], json!({ "window": "identity", "field": "name" }));

    // a window without the name: the platform asks the person for it, with its kind
    let asked = submit(&runtime, &mut conn, None, invoke("mi_institucion.guardar_ventana", json!({ "window": "identity", "mission": "Cuidar." })));
    let Body::Clarification { ask: Ask::Missing { fields }, .. } = &asked.body else { panic!("{asked:?}") };
    let asked_for: Vec<(&str, &serde_json::Value)> = fields.iter().map(|f| (f.id.as_str(), &f.field["kind"])).collect();
    assert_eq!(asked_for, [("mi_institucion.profile.name", &json!("text"))]);

    // with the name, the kernel's proposal; approved, the window is saved with the task as its origin
    let proposed = submit(&runtime, &mut conn, Some(&asked.task), Content::Answer { values: json!({ "mi_institucion.profile.name": "Casa Ficticia" }) });
    let Body::Proposal { fingerprint, .. } = &proposed.body else { panic!("{proposed:?}") };
    assert_eq!(mi_institucion::service::profile(&conn).unwrap().version, 0, "nothing before the approval");
    let done = submit(&runtime, &mut conn, Some(&asked.task), Content::Decision { fingerprint: fingerprint.clone(), approve: true });
    assert_eq!(done.state, TaskState::Done, "{done:?}");
    let history = kernel::history(&conn, &mi_institucion::service::profile_ref()).unwrap();
    assert_eq!(history[0].origin, kernel::Origin::Task { task: asked.task.clone(), account: ANA.into() });

    // the window, back: its data named by their fields, each of the class the catalog declares
    let read = submit(&runtime, &mut conn, None, invoke("mi_institucion.ver_ventana", json!({ "window": "identity" })));
    let Body::Data { value, .. } = &read.body else { panic!("{read:?}") };
    assert_eq!((value["version"].clone(), value["profile"]["name"].clone(), value["profile"]["mission"].clone()), (json!(1), json!("Casa Ficticia"), json!("Cuidar.")));
    let fields = platform::output_fields(registry.get("mi_institucion.ver_ventana").unwrap(), &modules).unwrap();
    let class = |f: &str| fields.iter().find(|x| x.field == format!("mi_institucion.profile.{f}")).map(|x| x.class);
    assert_eq!((class("name"), class("legal_rfc"), class("legal_rep_name"), class("notes")), (Some("public"), Some("confidential"), Some("personal"), Some("internal")));
}
