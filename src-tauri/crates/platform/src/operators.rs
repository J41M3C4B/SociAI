//! The operators of the platform (ADR-003 A6): the steps where a model interprets one thing, declared here and only
//! here. Each has an id `ia.…`, what its data are for (the policy reads it), its tier of model, its prompt, versioned
//! in a file of its own, and the schema of its answer. The same executor invokes them, as steps of the task. No module
//! declares one, and no capability may take their name (`capabilities_named_as_operators`).

use serde_json::{json, Value};

use super::policy::Purpose;

/// The module part of the id of every operator.
pub const MODULE: &str = "ia";

/// The most capabilities the router is offered at once (P4: few options, never the whole list). Past it, the ones
/// left out are the last by id: the battery would show it.
pub const OFFERED_AT_MOST: usize = 32;

/// Which model an operator needs: the light one, or the one that reasons the most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Light,
    Strong,
}

#[derive(Debug)]
pub struct Operator {
    pub id: &'static str,
    pub purpose: Purpose,
    pub tier: Tier,
    /// Its fixed prompt; its version is the one in the name of its file.
    pub prompt: &'static str,
    pub version: u32,
    /// The JSON schema its answer must fit.
    pub schema: fn() -> Value,
    pub max_output_tokens: u32,
}

/// The router (P4): from what the person wrote and the capabilities offered to them, what they want. It needs the
/// intention, never who.
pub const ROUTE: Operator = Operator {
    id: "ia.enrutar",
    purpose: Purpose::Route,
    tier: Tier::Light,
    prompt: include_str!("../prompts/ia.enrutar.v1.md"),
    version: 1,
    schema: route_schema,
    max_output_tokens: 400,
};

/// Puts in words what a capability gave, for the person who asked (path A of E1).
pub const RESPOND: Operator = Operator {
    id: "ia.responder",
    purpose: Purpose::Answer,
    tier: Tier::Light,
    prompt: include_str!("../prompts/ia.responder.v1.md"),
    version: 1,
    schema: respond_schema,
    max_output_tokens: 600,
};

pub const ALL: &[&Operator] = &[&ROUTE, &RESPOND];

/// What the router answers. The input of the capability goes as a JSON text: the schema of each capability is its
/// own, and not every provider takes an object without properties.
fn route_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "intent": { "type": "string", "enum": ["invoke", "choose", "change", "out_of_scope"] },
            "capability": { "type": "string" },
            "options": { "type": "array", "items": { "type": "string" } },
            "input": { "type": "string" },
            "continues": { "type": "boolean" }
        },
        "required": ["intent", "continues"]
    })
}

fn respond_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "text": { "type": "string" } },
        "required": ["text"]
    })
}
