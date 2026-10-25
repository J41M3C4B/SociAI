use std::time::Duration;

use rusqlite::Connection;
use serde_json::{json, Value};

use super::*;

/// A module as any other would be: it declares its capabilities next to its functions, and nobody lists them.
mod sample {
    use crate::{Call, CapabilityError, Limits};

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Window {
        pub window: String,
    }

    #[derive(serde::Serialize, schemars::JsonSchema)]
    pub struct Missing {
        pub fields: Vec<String>,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct Rename {
        pub name: String,
    }

    #[derive(serde::Serialize, schemars::JsonSchema)]
    pub struct Change {
        pub from: String,
        pub to: String,
    }

    pub fn missing(call: &Call, ask: Window) -> Result<Missing, CapabilityError> {
        let n: i64 = call.conn.query_row("SELECT 2", [], |r| r.get(0)).map_err(|_| CapabilityError::Failed { reason: "db" })?;
        Ok(Missing { fields: (0..n).map(|i| format!("{}.{i}", ask.window)).collect() })
    }

    pub fn preview(_: &Call, ask: Rename) -> Result<Change, CapabilityError> {
        if ask.name.trim().is_empty() {
            return Err(CapabilityError::NeedsPerson { what: "the new name" });
        }
        Ok(Change { from: "Old".into(), to: ask.name })
    }

    pub fn rename(call: &mut Call, ask: Rename) -> Result<Change, CapabilityError> {
        Ok(Change { from: call.by.into(), to: ask.name })
    }

    crate::capability! {
        static MISSING = "sample.missing" {
            description: "Qué datos le faltan a una ventana.",
            effect: Query,
            permission: "kernel.usar",
            data: [Internal],
            approval: Never,
            limits: Limits::READ,
            run: read(missing),
        }
    }

    crate::capability! {
        static RENAME = "sample.rename" {
            description: "Cambia el nombre de la institución.",
            effect: Modify,
            permission: "kernel.usar",
            data: [Public, Internal],
            approval: Required,
            limits: Limits::CHANGE,
            run: change(preview, rename),
        }
    }
}

#[test]
fn the_registry_finds_what_a_module_declares_without_a_list() {
    let registry = Registry::discover().unwrap();
    let missing = registry.get("sample.missing").unwrap();
    assert_eq!(missing.module(), "sample");
    assert_eq!(missing.effect, Effect::Query);
    assert_eq!(missing.permission, "kernel.usar");
    assert_eq!(missing.data, &[DataClass::Internal]);
    let rename = registry.get("sample.rename").unwrap();
    assert_eq!(rename.approval, Approval::Required);
    assert!(matches!(rename.run, Run::Change { .. }));
    let ids: Vec<_> = registry.of_module("sample").map(|c| c.id).collect();
    assert_eq!(ids, ["sample.missing", "sample.rename"]);
    let changes: Vec<_> = registry.with_effect(Effect::Modify).map(|c| c.id).collect();
    assert!(changes.contains(&"sample.rename") && !changes.contains(&"sample.missing"));
}

/// Every capability of the program keeps the rules, so an invalid contract never reaches `main`.
#[test]
fn every_capability_of_the_program_keeps_the_rules() {
    if let Err(errors) = Registry::discover() {
        panic!("{}", errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"));
    }
}

#[test]
fn the_schemas_come_from_the_types() {
    let registry = Registry::discover().unwrap();
    let missing = registry.get("sample.missing").unwrap();
    let input = (missing.input)();
    assert_eq!(input["type"], "object");
    assert_eq!(input["required"], json!(["window"]));
    let Run::Read(step) = missing.run else { panic!("a query reads") };
    assert_eq!((step.output)()["properties"]["fields"]["type"], "array");
    let rename = registry.get("sample.rename").unwrap();
    assert_eq!((rename.input)()["required"], json!(["name"]));
}

#[test]
fn a_capability_runs_over_json_with_its_call() {
    let mut conn = Connection::open_in_memory().unwrap();
    let mut call = Call::new(&mut conn, "account-1", "task-1");
    let registry = Registry::discover().unwrap();

    let Run::Read(read) = registry.get("sample.missing").unwrap().run else { panic!() };
    assert_eq!((read.call)(&call, json!({ "window": "contact" })).unwrap(), json!({ "fields": ["contact.0", "contact.1"] }));

    let Run::Change { prepare, apply } = registry.get("sample.rename").unwrap().run else { panic!() };
    assert_eq!((prepare.call)(&call, json!({ "name": "Nuevo" })).unwrap(), json!({ "from": "Old", "to": "Nuevo" }));
    assert_eq!((apply.call)(&mut call, json!({ "name": "Nuevo" })).unwrap(), json!({ "from": "account-1", "to": "Nuevo" }));
    assert_eq!(
        (prepare.call)(&call, json!({ "name": " " })),
        Err(CapabilityError::NeedsPerson { what: "the new name" })
    );
}

/// What the kernel answers to an `apply`, by its code, is the same error for every module (ERP-9).
#[test]
fn the_answers_of_the_kernel_have_a_fixed_translation() {
    for code in ["prepare_again", "invalid", "nothing_to_change"] {
        assert_eq!(CapabilityError::from_apply(code), CapabilityError::Stale, "{code}");
    }
    for code in ["bad_origin", "bad_fact", "version_not_raised", "database"] {
        assert_eq!(CapabilityError::from_apply(code), CapabilityError::Failed { reason: code }, "{code}");
    }
}

#[test]
fn a_wrong_input_never_quotes_what_it_got() {
    let mut conn = Connection::open_in_memory().unwrap();
    let call = Call::new(&mut conn, "account-1", "task-1");
    let Run::Read(read) = Registry::discover().unwrap().get("sample.missing").unwrap().run else { panic!() };
    let err = (read.call)(&call, json!({ "window": 4455667788u64 })).unwrap_err();
    assert_eq!(err, CapabilityError::Invalid { field: "input", reason: "does not fit the schema of the capability" });
    assert!(!err.to_string().contains("4455667788"));
}

// contracts written by hand, as `capability!` never would, to see each rule refuse them

fn object() -> Value {
    json!({ "type": "object" })
}

fn text() -> Value {
    json!({ "type": "string" })
}

fn nothing(_: &Call, _: Value) -> Result<Value, CapabilityError> {
    Ok(Value::Null)
}

fn nothing_applied(_: &mut Call, _: Value) -> Result<Value, CapabilityError> {
    Ok(Value::Null)
}

const STEP: Step = Step { call: nothing, output: object };
const APPLY: Apply = Apply { call: nothing_applied, output: object };

const fn contract(id: &'static str) -> Capability {
    Capability {
        id,
        description: "Una capacidad de prueba.",
        effect: Effect::Query,
        permission: "kernel.usar",
        data: &[DataClass::Internal],
        approval: Approval::Never,
        limits: Limits::READ,
        input: object,
        run: Run::Read(STEP),
    }
}

fn faults_of(contracts: &'static [Capability]) -> Vec<Fault> {
    match Registry::of(contracts) {
        Ok(_) => vec![],
        Err(errors) => errors.into_iter().map(|e| e.fault).collect(),
    }
}

#[test]
fn a_whole_contract_is_accepted() {
    static GOOD: [Capability; 1] = [contract("probe.good")];
    assert_eq!(faults_of(&GOOD), []);
}

#[test]
fn a_permission_not_written_as_an_id_is_refused() {
    static BAD: [Capability; 3] = [
        Capability { permission: "usar", ..contract("probe.one_part") },
        Capability { permission: "Kernel.Usar", ..contract("probe.capitals") },
        Capability { permission: "contabilidad.polizas.registrar", ..contract("probe.three_parts") },
    ];
    assert_eq!(faults_of(&BAD), [Fault::BadPermission, Fault::BadPermission]);
}

#[test]
fn a_contract_without_a_data_class_is_refused() {
    static NO_CLASS: [Capability; 1] = [Capability { data: &[], ..contract("probe.no_class") }];
    assert_eq!(faults_of(&NO_CLASS), [Fault::NoDataClass]);
}

#[test]
fn a_sensitive_action_without_approval_is_refused() {
    static SENSITIVE: [Capability; 1] = [Capability {
        effect: Effect::Sensitive,
        limits: Limits::CHANGE,
        run: Run::Change { prepare: STEP, apply: APPLY },
        ..contract("probe.send")
    }];
    assert_eq!(faults_of(&SENSITIVE), [Fault::SensitiveWithoutApproval]);
}

#[test]
fn a_change_that_is_not_idempotent_is_not_retried() {
    static RETRIED: [Capability; 1] = [Capability {
        effect: Effect::Modify,
        limits: Limits { retries: 1, ..Limits::CHANGE },
        run: Run::Change { prepare: STEP, apply: APPLY },
        ..contract("probe.save")
    }];
    assert_eq!(faults_of(&RETRIED), [Fault::RetriesWithoutIdempotence]);
    // the same retries on a change that is idempotent are fine
    static IDEMPOTENT: [Capability; 1] = [Capability {
        effect: Effect::Modify,
        limits: Limits { retries: 1, idempotent: true, ..Limits::CHANGE },
        run: Run::Change { prepare: STEP, apply: APPLY },
        ..contract("probe.save")
    }];
    assert_eq!(faults_of(&IDEMPOTENT), []);
}

#[test]
fn the_run_must_fit_the_effect() {
    static QUERY_THAT_CHANGES: [Capability; 1] =
        [Capability { run: Run::Change { prepare: STEP, apply: APPLY }, ..contract("probe.query") }];
    assert_eq!(faults_of(&QUERY_THAT_CHANGES), [Fault::RunDoesNotFitEffect]);
    static CHANGE_THAT_READS: [Capability; 1] =
        [Capability { effect: Effect::Modify, limits: Limits::CHANGE, ..contract("probe.change") }];
    assert_eq!(faults_of(&CHANGE_THAT_READS), [Fault::RunDoesNotFitEffect]);
}

#[test]
fn the_other_rules() {
    static NO_TIMEOUT: [Capability; 1] =
        [Capability { limits: Limits { timeout: Duration::ZERO, ..Limits::READ }, ..contract("probe.slow") }];
    assert_eq!(faults_of(&NO_TIMEOUT), [Fault::NoTimeout]);
    static NO_DESCRIPTION: [Capability; 1] = [Capability { description: " ", ..contract("probe.mute") }];
    assert_eq!(faults_of(&NO_DESCRIPTION), [Fault::NoDescription]);
    static TEXT_INPUT: [Capability; 1] = [Capability { input: text, ..contract("probe.text") }];
    assert_eq!(faults_of(&TEXT_INPUT), [Fault::InputNotAnObject]);
    static REPEATED: [Capability; 2] = [contract("probe.twice"), contract("probe.twice")];
    assert_eq!(faults_of(&REPEATED), [Fault::Repeated]);
}

#[test]
fn an_id_is_module_dot_name() {
    let faults = |id: &str| {
        let id: &'static str = Box::leak(id.to_string().into_boxed_str());
        let one: &'static Capability = Box::leak(Box::new(contract(id)));
        Registry::of([one]).err().map(|errors| errors.iter().map(|e| e.fault).collect::<Vec<_>>())
    };
    for bad in ["missing", "Sample.missing", "sample.Missing", "sample.", ".missing", "sample.mis.sing", "sample.mis-sing", "1x.y"] {
        assert_eq!(faults(bad), Some(vec![Fault::BadId]), "{bad}");
    }
    for good in ["sample.missing", "projects.call_2", "hr.x"] {
        assert_eq!(faults(good), None, "{good}");
    }
}
