//! The door to the models (P11): the only way the platform calls one. A call takes an operator and what the policy
//! built for it (`ToModel`), never loose text, so nothing reaches a model that the policy did not let go. The app
//! opens the door with its providers, their pace and their prices; the tests, with a simulated model. The provider
//! and its conditions come from the same object that calls, so the policy never decides with one provider and calls
//! another.

use serde_json::Value;

use super::operators::Operator;
use super::policy::{Provider, ToModel};

/// What a model answered, and what it cost.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub value: Value,
    /// The model that answered: it may be a backup of the one asked.
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_mxn: f64,
}

/// Why a call to a model did not give an answer. None of them is the person's: each has its way to go on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No connection, the service is down or busy, or its allowance is used up.
    Unavailable,
    /// The monthly budget of the institution is used up: the AI waits, everything else works.
    Budget,
    /// The key is missing or was rejected: someone who administers has to see it.
    Key,
    /// The provider declined the request.
    Refused,
    /// The answer did not fit the schema of the operator, even after asking once more.
    BadOutput,
}

impl Failure {
    pub fn code(self) -> &'static str {
        match self {
            Failure::Unavailable => "model_unavailable",
            Failure::Budget => "model_budget",
            Failure::Key => "model_key",
            Failure::Refused => "model_refused",
            Failure::BadOutput => "model_bad_output",
        }
    }
}

pub trait Models {
    /// The provider these models belong to, with its conditions (ADR-004 §2).
    fn provider(&self) -> &Provider;

    /// One call: the operator says its prompt, the schema of its answer and its tier; the model reads only
    /// `to_model`. It blocks until the model answers: the runtime lets the database go meanwhile.
    fn call(&self, operator: &Operator, to_model: &ToModel) -> Result<Reply, Failure>;
}
