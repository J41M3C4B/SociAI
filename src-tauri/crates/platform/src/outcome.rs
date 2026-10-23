//! The common contract of outcomes (the AI blueprint §6, P9): everything the platform hands back, to the chat, a
//! screen or a notice, has this shape and carries the id of its task. The design lane draws each kind of body.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::request::Channel;
use super::task::TaskState;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub task: String,
    /// The step it comes from; 0 when no capability ran.
    pub step: u32,
    pub channel: Channel,
    /// How the task is after this outcome.
    pub state: TaskState,
    /// When it was handed over, in milliseconds since 1970 (UTC).
    pub at_ms: i64,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    /// A text for the person and where each datum of it comes from. The operators of IA-4 write it.
    Answer { text: String, evidence: Vec<Evidence> },
    /// What a capability gave, for a screen.
    Data { capability: String, value: Value },
    /// The task paused: something to give or to correct.
    Clarification { capability: String, ask: Ask },
    /// A change prepared and waiting for the person, tied to its fingerprint.
    Proposal { capability: String, proposal: Value, fingerprint: String, again: Option<Again> },
    /// How the task is, with the code of why.
    State { reason: String },
    /// What went wrong, as a fixed code, and always a way to go on.
    Error { code: String, next: Next },
}

/// What a paused task asks of the person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Ask {
    /// Something only the person knows. With ERP-2 it names fields of the dictionary, with their options.
    Missing { what: String },
    /// A value to correct: which field and why.
    Correct { field: String, reason: String },
}

/// Why a proposal is asked for once more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Again {
    /// The input changed after it was shown: the earlier approval no longer counts.
    Changed,
    /// The decision was for a proposal that is no longer the one waiting.
    Outdated,
    /// The datum changed after it was approved: prepared again, it shows the datum as it is now.
    DataChanged,
}

/// The way to go on after an error (PAIR: an error never blocks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Next {
    /// Do it from its screen: the chat has no model yet.
    UseScreen,
    /// Someone with the permission has to do it, or grant it.
    AskAdministrator,
    /// Start a new task.
    StartAgain,
    /// See the history of the datum before trying again: a change may or may not have been made.
    CheckHistory,
    TryLater,
}

/// Where a datum of an answer comes from: what the person sees next to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub source: Source,
    /// What the person reads: «del acta, página 3».
    pub label: String,
    /// The id of the document, the change or the calculation, when there is one.
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Person,
    Document,
    History,
    Calculation,
}
